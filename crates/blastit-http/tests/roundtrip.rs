//! Round trips, keep-alive, pipelining and response encoding
//! (ARCHITECTURE.md §2.3, §3.2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use blastit_frame::Frame;
use blastit_http::HttpConfig;

use common::{
    Harness, QUIET, Rig, find, header_has_token, header_names, header_present, header_value,
};

/// A front end with the defaults, and one connection to it.
fn harness() -> Harness {
    Harness::start(HttpConfig::default())
}

#[tokio::test]
async fn a_simple_get_goes_out_and_comes_back_byte_for_byte() {
    let mut h = harness();
    h.client
        .write(b"GET /products/42?page=2 HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\n\r\n")
        .await;

    let (id, head) = h.scheduler.head().await;
    assert_ne!(id, 0, "stream ids start at one");
    assert_eq!(head.method, b"GET");
    assert_eq!(
        head.target, b"/products/42?page=2",
        "the query string is forwarded"
    );
    assert_eq!(
        head.remote_addr, "203.0.113.7:54321",
        "the TCP peer by default"
    );
    assert_eq!(head.server_name, "example.com");
    assert_eq!(head.server_port, 8080);
    assert_eq!(head.scheme, blastit_frame::Scheme::Http);
    let sent: Vec<String> = head
        .headers
        .iter()
        .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
        .collect();
    assert_eq!(sent, ["Host", "Accept"]);
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler
        .respond_reason(
            id,
            200,
            b"OK",
            &[("Content-Type", "text/plain"), ("Content-Length", "5")],
            b"hello",
        )
        .await;

    assert_eq!(
        h.client.read_response().await,
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello"
    );
    assert!(h.events.is_empty(), "nothing was rejected");
    assert!(h.client.read_within(QUIET).await.is_none(), "keep-alive");
}

#[tokio::test]
async fn a_response_without_a_length_is_chunked_for_http_1_1() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler
        .respond_chunks(
            id,
            200,
            &[("Content-Type", "text/plain")],
            &[b"he", b"ll", b"o"],
        )
        .await;

    assert_eq!(
        h.client.read_response().await,
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n2\r\nll\r\n1\r\no\r\n0\r\n\r\n"
    );
}

#[tokio::test]
async fn an_empty_chunk_is_not_a_terminator() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond_chunks(id, 200, &[], &[b"a", b"", b"b"])
        .await;
    assert_eq!(
        h.client.read_response().await,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n1\r\nb\r\n0\r\n\r\n"
    );
}

#[tokio::test]
async fn a_response_with_a_length_is_not_chunked() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(id, 201, &[("Content-Length", "3")], b"abc")
        .await;
    let response = h.client.read_response().await;
    assert_eq!(
        header_value(&response, "content-length").as_deref(),
        Some("3")
    );
    assert_eq!(
        response,
        b"HTTP/1.1 201 Created\r\nContent-Length: 3\r\n\r\nabc"
    );
    assert!(!header_present(&response, "transfer-encoding"));
}

#[tokio::test]
async fn a_longer_body_than_promised_is_truncated_and_the_connection_closes() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(id, 200, &[("Content-Length", "3")], b"abcdef")
        .await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc"
    );
}

#[tokio::test]
async fn a_shorter_body_than_promised_closes_without_a_terminator() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(id, 200, &[("Content-Length", "10")], b"hi")
        .await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhi"
    );
}

#[tokio::test]
async fn http_1_0_gets_a_close_delimited_body() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.0\r\nHost: h\r\n\r\n").await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"GET");
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"bye").await;

    let response = h.client.read_to_end().await;
    assert_eq!(
        response, b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nbye",
        "HTTP/1.0 never sees chunked framing"
    );
}

#[tokio::test]
async fn http_1_0_keep_alive_is_answered_explicitly() {
    let mut h = harness();
    h.client
        .write(b"GET / HTTP/1.0\r\nHost: h\r\nConnection: keep-alive\r\n\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(id, 200, &[("Content-Length", "2")], b"ok")
        .await;
    let response = h.client.read_response().await;
    assert_eq!(
        response,
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok"
    );

    // A second request on the same connection still works.
    h.client
        .write(b"GET /again HTTP/1.0\r\nHost: h\r\n\r\n")
        .await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/again");
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(id, 204, &[("Content-Length", "0")], b"")
        .await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
        "a 204 has no body, so it carries no length"
    );
}

#[tokio::test]
async fn two_requests_on_one_keep_alive_connection_use_different_streams() {
    let mut h = harness();

    h.client
        .write(b"GET /one HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (first, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/one");
    assert_eq!(h.scheduler.end().await, first);
    h.scheduler.respond(first, 200, &[], b"one").await;
    let response = h.client.read_response().await;
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));

    h.client
        .write(b"GET /two HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (second, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/two");
    assert_ne!(first, second, "every request gets its own stream id");
    assert_eq!(h.scheduler.end().await, second);
    h.scheduler.respond(second, 200, &[], b"two").await;

    let response = h.client.read_response().await;
    assert!(
        response.ends_with(b"\r\n3\r\ntwo\r\n0\r\n\r\n"),
        "{response:?}"
    );
}

#[tokio::test]
async fn pipelined_requests_are_answered_in_order() {
    let mut h = harness();
    h.client
        .write(b"GET /first HTTP/1.1\r\nHost: h\r\n\r\nGET /second HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;

    // Only the first request is forwarded until its response is done.
    let (first, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/first");
    assert_eq!(h.scheduler.end().await, first);
    assert!(
        h.scheduler.next_within(QUIET).await.is_none(),
        "the second request waits for the first response"
    );

    h.scheduler.respond(first, 200, &[], b"first").await;
    let (second, head) = h.scheduler.head().await;
    assert_eq!(head.target, b"/second");
    assert_ne!(first, second);
    assert_eq!(h.scheduler.end().await, second);
    h.scheduler.respond(second, 200, &[], b"second").await;

    let mut all = h.client.read_response().await;
    all.extend_from_slice(&h.client.read_response().await);
    let first_at = find(&all, b"first\r\n0\r\n\r\n").expect("first response");
    let second_at = find(&all, b"second\r\n0\r\n\r\n").expect("second response");
    assert!(first_at < second_at, "responses keep the request order");
}

#[tokio::test]
async fn head_requests_get_a_head_and_no_body() {
    let mut h = harness();
    h.client.write(b"HEAD / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, head) = h.scheduler.head().await;
    assert_eq!(head.method, b"HEAD");
    assert_eq!(h.scheduler.end().await, id);

    // The view sends a body anyway; a HEAD answer must not carry it.
    h.scheduler
        .respond(id, 200, &[("Content-Length", "5")], b"hello")
        .await;

    let response = h.client.read_head().await;
    assert_eq!(
        response, b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n",
        "the headers of the GET answer, without its body"
    );
    assert!(
        h.client.read_within(QUIET).await.is_none(),
        "no body byte follows a HEAD response"
    );

    // And the connection is still usable.
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"ok").await;
    assert!(
        h.client
            .read_response()
            .await
            .starts_with(b"HTTP/1.1 200 OK\r\n")
    );
}

#[tokio::test]
async fn a_304_keeps_its_length_and_a_204_loses_it() {
    // A 304 is describing the cached `GET`, so its length is meaningful; a 204
    // can never have a body, so a length on it is a lie.
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(
            id,
            304,
            &[("Content-Length", "42"), ("ETag", "\"v1\"")],
            b"",
        )
        .await;
    assert_eq!(
        h.client.read_head().await,
        b"HTTP/1.1 304 Not Modified\r\nContent-Length: 42\r\nETag: \"v1\"\r\n\r\n"
    );

    h.client
        .write(b"GET /again HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(id, 204, &[("Content-Length", "42")], b"")
        .await;
    assert_eq!(
        h.client.read_head().await,
        b"HTTP/1.1 204 No Content\r\n\r\n"
    );
}

#[tokio::test]
async fn an_interim_status_from_the_view_has_no_body() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);

    // A 1xx status is bodyless by definition, so the body frames are dropped
    // and the terminating frame is never named.
    h.scheduler.respond(id, 102, &[], b"ignored").await;
    let response = h.client.read_head().await;
    assert_eq!(response, b"HTTP/1.1 102 Processing\r\n\r\n");
    assert!(
        h.client.read_within(QUIET).await.is_none(),
        "no body byte follows an interim status"
    );
}

#[tokio::test]
async fn a_204_has_no_body_even_when_the_view_sends_one() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 204, &[], b"ignored").await;
    let response = h.client.read_head().await;
    assert_eq!(response, b"HTTP/1.1 204 No Content\r\n\r\n");
    assert!(h.client.read_within(QUIET).await.is_none());
}

#[tokio::test]
async fn hop_by_hop_headers_are_stripped_in_both_directions() {
    let mut h = harness();
    h.client
        .write(
            b"GET / HTTP/1.1\r\nHost: h\r\nConnection: keep-alive, x-hop\r\nx-hop: 1\r\nKeep-Alive: timeout=5\r\nTE: trailers\r\nTrailer: X-T\r\nProxy-Connection: keep-alive\r\nX-Keep: yes\r\n\r\n",
        )
        .await;
    let (id, head) = h.scheduler.head().await;
    let names: Vec<String> = head
        .headers
        .iter()
        .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
        .collect();
    assert_eq!(names, ["Host", "X-Keep"]);
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler
        .respond_chunks(
            id,
            200,
            &[
                ("Connection", "keep-alive"),
                ("Keep-Alive", "timeout=5"),
                ("Proxy-Connection", "keep-alive"),
                ("Transfer-Encoding", "chunked"),
                ("Upgrade", "h2c"),
                ("X-Keep", "yes"),
            ],
            &[b"body"],
        )
        .await;

    let response = h.client.read_response().await;
    let names: Vec<String> = header_names(&response);
    assert_eq!(names, ["X-Keep", "Transfer-Encoding"]);
    assert_eq!(
        response,
        b"HTTP/1.1 200 OK\r\nX-Keep: yes\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nbody\r\n0\r\n\r\n",
        "hop-by-hop headers must not reach the client"
    );
}

#[tokio::test]
async fn a_bad_header_from_the_view_becomes_a_502() {
    for (name, value) in [
        ("X Bad", "value"),
        ("X-Smuggled", "a\r\nX-Evil: 1"),
        ("X-Nul", "a\0b"),
        ("", "value"),
    ] {
        let mut h = harness();
        h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
        let (id, _) = h.scheduler.head().await;
        assert_eq!(h.scheduler.end().await, id);
        let head = blastit_frame::RespHead {
            status: 200,
            reason: Vec::new(),
            headers: vec![(name.as_bytes().to_vec(), value.as_bytes().to_vec())],
        };
        h.scheduler.send(id, &Frame::RespHead(head)).await;
        h.scheduler
            .send(id, &Frame::RespBody(b"body".to_vec()))
            .await;
        h.scheduler.send(id, &Frame::RespEnd).await;
        assert_eq!(
            h.client.read_to_end().await,
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "header {name:?}: {value:?}"
        );
    }
}

#[tokio::test]
async fn a_bad_reason_phrase_from_the_view_becomes_a_502() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond_reason(id, 200, b"OK\r\nX-Injected: 1", &[], b"")
        .await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
}

#[tokio::test]
async fn an_unknown_status_with_no_reason_gets_a_status_line() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 599, &[], b"").await;
    assert_eq!(
        h.client.read_response().await,
        b"HTTP/1.1 599 \r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
    );
}

#[tokio::test]
async fn resp_abort_before_the_head_is_a_502() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .send(
            id,
            &Frame::RespAbort {
                reason: "the view fell over".to_string(),
            },
        )
        .await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
}

#[tokio::test]
async fn resp_abort_after_the_head_closes_without_a_terminator() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler
        .send(
            id,
            &Frame::RespHead(blastit_frame::RespHead {
                status: 200,
                reason: Vec::new(),
                headers: Vec::new(),
            }),
        )
        .await;
    h.scheduler
        .send(id, &Frame::RespBody(b"partial".to_vec()))
        .await;
    h.scheduler
        .send(
            id,
            &Frame::RespAbort {
                reason: "halfway".to_string(),
            },
        )
        .await;

    let response = h.client.read_to_end().await;
    assert_eq!(
        response, b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\npartial\r\n",
        "the terminating chunk is never written after an abort"
    );
}

#[tokio::test]
async fn a_response_that_never_starts_a_head_is_a_502() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .send(id, &Frame::RespBody(b"orphan".to_vec()))
        .await;
    h.scheduler.send(id, &Frame::RespEnd).await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
}

#[tokio::test]
async fn the_view_can_close_the_connection() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond(
            id,
            200,
            &[("Connection", "close"), ("Content-Length", "2")],
            b"ok",
        )
        .await;
    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
    );
}

#[tokio::test]
async fn a_client_that_asks_to_close_gets_a_closing_response() {
    let mut h = harness();
    h.client
        .write(b"GET / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
        .await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"bye").await;
    let response = h.client.read_to_end().await;
    assert!(
        response.ends_with(b"\r\n0\r\n\r\n"),
        "the body is still framed: {response:?}"
    );
    assert!(header_has_token(&response, "connection", "close"));
}

#[tokio::test]
async fn a_view_that_sends_two_heads_closes_the_connection() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler
        .send(
            id,
            &Frame::RespHead(blastit_frame::RespHead {
                status: 200,
                reason: Vec::new(),
                headers: Vec::new(),
            }),
        )
        .await;
    h.scheduler
        .send(id, &Frame::RespBody(b"first".to_vec()))
        .await;
    // A second head would be a second response on the same request.
    h.scheduler
        .send(
            id,
            &Frame::RespHead(blastit_frame::RespHead {
                status: 500,
                reason: Vec::new(),
                headers: Vec::new(),
            }),
        )
        .await;
    h.scheduler.send(id, &Frame::RespEnd).await;

    assert_eq!(
        h.client.read_to_end().await,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nfirst\r\n",
        "the client keeps the first head and loses the connection"
    );
}

#[tokio::test]
async fn concurrent_connections_get_unique_stream_ids() {
    let rig = Rig::new(HttpConfig::default());
    let mut clients = Vec::new();
    for index in 0..4 {
        let (mut client, task) = rig.connect();
        client
            .write(format!("GET /c{index} HTTP/1.1\r\nHost: h\r\n\r\n").as_bytes())
            .await;
        clients.push((client, task));
    }

    let mut ids = Vec::new();
    let mut scheduler = rig.scheduler;
    while ids.len() < 4 {
        // Anything else is the `REQ_END` of an already counted request.
        if let (id, Frame::ReqHead(head)) = scheduler.next().await {
            let head: blastit_frame::ReqHead = head;
            assert!(head.target.starts_with(b"/c"), "unexpected {head:?}");
            ids.push(id);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4, "every connection gets its own stream id");
    assert!(ids.iter().all(|id| *id != 0));

    for (_, task) in clients {
        // Nothing answers, so each connection is still waiting; stopping the
        // tasks says so.
        task.abort();
        let _ = task.await;
    }
}

#[tokio::test]
async fn a_response_may_arrive_in_many_frames() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler
        .respond_chunks(id, 200, &[("Content-Length", "11")], &[b"hello ", b"world"])
        .await;
    assert_eq!(
        h.client.read_response().await,
        b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello world"
    );
}

#[tokio::test]
async fn an_empty_reason_uses_the_registered_phrase() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 404, &[], b"").await;
    let response = h.client.read_response().await;
    assert!(
        response.starts_with(b"HTTP/1.1 404 Not Found\r\n"),
        "{response:?}"
    );
}

#[tokio::test]
async fn the_client_can_disconnect_between_requests() {
    let mut h = harness();
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);
    h.scheduler.respond(id, 200, &[], b"ok").await;
    let _ = h.client.read_response().await;
    h.client.disconnect();

    // The connection task ends on its own, without an error.
    let outcome = tokio::time::timeout(Duration::from_secs(5), h.task)
        .await
        .expect("the connection task must end")
        .expect("the task must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}
