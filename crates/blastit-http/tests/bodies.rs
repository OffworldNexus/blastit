//! Request bodies: `Content-Length`, chunked, `100-continue` and cancellation
//! (ARCHITECTURE.md §2.3, §3.1).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use blastit_frame::{Frame, Limits};
use blastit_http::HttpConfig;

use common::{Harness, PEER, QUIET, Rig};

/// A front end whose channel frames hold about 190 bytes of body, so a body of
/// any size arrives as several frames.
fn small_frames() -> HttpConfig {
    HttpConfig {
        frame_limits: Limits::new(200).expect("200 is a usable frame size"),
        ..HttpConfig::default()
    }
}

#[tokio::test]
async fn a_content_length_body_arrives_as_several_frames() {
    let mut h = Harness::start(small_frames());
    let body: Vec<u8> = (0..1000u32).map(|index| (index % 251) as u8).collect();
    let mut request = b"POST /upload HTTP/1.1\r\nHost: h\r\nContent-Length: 1000\r\n\r\n".to_vec();
    request.extend_from_slice(&body);
    h.client.write(&request).await;

    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"POST");
    assert_eq!(head.target, b"/upload");
    assert_eq!(head.server_name, "h");
    assert_eq!(
        head.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(b"content-length"))
            .map(|(_, value)| value.clone()),
        Some(b"1000".to_vec()),
        "Content-Length is forwarded"
    );

    let limit = small_frames().frame_limits.max_payload();
    let mut rebuilt = Vec::new();
    let mut frames = 0;
    loop {
        match h.scheduler.next().await {
            (_, Frame::ReqBody(chunk)) => {
                assert!(
                    chunk.len() <= limit,
                    "a body frame of {} bytes",
                    chunk.len()
                );
                assert!(!chunk.is_empty(), "an empty body frame is noise");
                frames += 1;
                rebuilt.extend_from_slice(&chunk);
            }
            (ended, Frame::ReqEnd) => {
                assert_eq!(ended, id);
                break;
            }
            other => panic!("expected body frames, got {other:?}"),
        }
    }
    assert!(frames > 1, "1000 bytes did not fit one small frame");
    assert_eq!(rebuilt, body);

    h.scheduler.respond(id, 200, &[], b"ok").await;
    let response = h.client.read_response().await;
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"), "{response:?}");
}

#[tokio::test]
async fn a_body_is_forwarded_before_it_has_all_arrived() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST /upload HTTP/1.1\r\nHost: h\r\nContent-Length: 5000\r\n\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;

    let first = vec![b'a'; 2000];
    h.client.write(&first).await;
    let seen = h.scheduler.whole_body_prefix(id, 2000).await;
    assert_eq!(
        seen, first,
        "the front end does not wait for the whole body"
    );

    let rest = vec![b'b'; 3000];
    h.client.write(&rest).await;
    let seen = h.scheduler.whole_body_prefix(id, 3000).await;
    assert_eq!(seen, rest);
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler.respond(id, 200, &[], b"ok").await;
    let _ = h.client.read_response().await;
    assert!(h.events.is_empty());
}

#[tokio::test]
async fn a_content_length_of_zero_sends_no_body_frame() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST /empty HTTP/1.1\r\nHost: h\r\nContent-Length: 0\r\n\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id, "REQ_END without a body frame");
    h.scheduler
        .respond(id, 204, &[("Content-Length", "0")], b"")
        .await;
    assert_eq!(
        h.client.read_head().await,
        b"HTTP/1.1 204 No Content\r\n\r\n",
        "a 204 cannot carry a body, so its length is dropped"
    );
}

#[tokio::test]
async fn a_chunked_body_is_dechunked_with_extensions_and_trailers_dropped() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(
            b"POST /chunked HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n\
              5;name=value\r\nhello\r\n\
              0001\r\n \r\n\
              5\r\nworld\r\n\
              0;final\r\nX-Trailer: 1\r\nAnother: 2\r\n\r\n",
        )
        .await;

    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"POST");
    assert!(
        !head
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(b"transfer-encoding")),
        "the chunked framing is hop-by-hop"
    );
    assert_eq!(h.scheduler.whole_body(id).await, b"hello world");
    assert!(h.events.is_empty());

    h.scheduler.respond(id, 200, &[], b"done").await;
    let response = h.client.read_response().await;
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"), "{response:?}");
}

#[tokio::test]
async fn a_chunked_body_is_forwarded_as_it_arrives() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nfirst")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(
        h.scheduler.body().await.1,
        b"first",
        "a chunk that is still incomplete is forwarded all the same"
    );

    h.client.write(b"!\r\n").await;
    assert_eq!(h.scheduler.body().await.1, b"!");
    h.client.write(b"0\r\n\r\n").await;
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler.respond(id, 200, &[], b"ok").await;
    let _ = h.client.read_response().await;
}

#[tokio::test]
async fn a_malformed_chunked_body_is_refused_and_cancelled() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nabc")
        .await;
    let (id, _) = h.scheduler.head().await;
    // The chunk promises four bytes and then stops with a bare LF.
    h.client.write(b"\n0\r\n\r\n").await;

    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert_eq!(
        h.scheduler.until_cancelled().await.0,
        id,
        "the stream is cancelled"
    );
    assert_eq!(
        h.events.rejections(),
        [(PEER.to_string(), 400, "malformed_request".to_string())]
    );
}

#[tokio::test]
async fn a_chunked_body_over_the_limit_is_413_and_cancelled() {
    let mut h = Harness::start(HttpConfig {
        max_body_bytes: 8,
        ..HttpConfig::default()
    });
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;
    h.client.write(b"10\r\n0123456789abcdef\r\n0\r\n\r\n").await;

    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert_eq!(h.scheduler.until_cancelled().await.0, id);
    assert_eq!(
        h.events.rejections(),
        [(PEER.to_string(), 413, "body_too_large".to_string())]
    );
}

#[tokio::test]
async fn a_declared_body_over_the_limit_never_reaches_the_scheduler() {
    let mut h = Harness::start(HttpConfig {
        max_body_bytes: 10,
        ..HttpConfig::default()
    });
    h.client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 11\r\n\r\n")
        .await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert!(
        h.scheduler.next_within(QUIET).await.is_none(),
        "a refused request never becomes a stream"
    );
    assert_eq!(
        h.events.rejections(),
        [(PEER.to_string(), 413, "body_too_large".to_string())]
    );
}

#[tokio::test]
async fn expect_100_continue_is_answered_before_the_body_is_read() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(b"POST /upload HTTP/1.1\r\nHost: h\r\nExpect: 100-continue\r\nContent-Length: 5\r\n\r\n")
        .await;

    // The client has not sent a body byte, and the answer is already there.
    let interim = h.client.read_head().await;
    assert_eq!(interim, b"HTTP/1.1 100 Continue\r\n\r\n");

    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"POST");
    assert!(
        head.headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(b"expect")),
        "the expectation is still visible to the app"
    );

    h.client.write(b"hello").await;
    assert_eq!(h.scheduler.whole_body(id).await, b"hello");

    h.scheduler.respond(id, 200, &[], b"thanks").await;
    let response = h.client.read_response().await;
    assert_eq!(
        response,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\nthanks\r\n0\r\n\r\n"
    );
}

#[tokio::test]
async fn a_100_continue_is_not_sent_for_a_request_without_a_body() {
    let mut h = Harness::start(HttpConfig::default());
    h.client
        .write(
            b"POST /empty HTTP/1.1\r\nHost: h\r\nExpect: 100-continue\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"ok").await;

    let response = h.client.read_response().await;
    assert!(
        response.starts_with(b"HTTP/1.1 200 OK\r\n"),
        "a request with no body gets no interim answer: {response:?}"
    );
}

#[tokio::test]
async fn a_client_that_vanishes_mid_body_cancels_the_stream() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.body().await.1, b"abc");

    // The client goes away rather than sending the rest of the body.
    drop(client);
    assert_eq!(scheduler.cancel().await, id);
    assert!(
        scheduler.next_within(QUIET).await.is_none(),
        "nothing else is sent for a cancelled stream"
    );
}

#[tokio::test]
async fn a_body_that_stops_arriving_times_out_and_cancels() {
    let rig_cfg = HttpConfig {
        keepalive_timeout: Duration::from_millis(50),
        ..HttpConfig::default()
    };
    let rig = Rig::new(rig_cfg);
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    let started = Instant::now();
    client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.body().await.1, b"abc");
    assert_eq!(scheduler.cancel().await, id);
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "the idle bound was actually waited for"
    );
}

#[tokio::test]
async fn a_big_upload_does_not_need_a_big_buffer_in_the_front_end() {
    // 512 KiB through the front end, with the channel framing it into pieces.
    let mut h = Harness::start(small_frames());
    let body: Vec<u8> = (0..512 * 1024u32)
        .map(|index| (index % 253) as u8)
        .collect();
    let mut request = format!(
        "POST /big HTTP/1.1\r\nHost: h\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(&body);
    h.client.write(&request).await;

    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.whole_body(id).await, body);
    h.scheduler.respond(id, 201, &[], b"").await;
    let response = h.client.read_response().await;
    assert!(
        response.starts_with(b"HTTP/1.1 201 Created\r\n"),
        "{response:?}"
    );
}
