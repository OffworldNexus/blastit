//! The limits that keep one client from spoiling the server for the others:
//! the per-stream response buffer, the connection cap and the body deadline
//! (ARCHITECTURE.md §2.3, §8.2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use blastit_frame::{Frame, RespHead};
use blastit_http::HttpConfig;

use common::{Listening, QUIET, Rig};

/// A response head with nothing but a status.
fn bare_head(status: u16) -> RespHead {
    RespHead {
        status,
        reason: Vec::new(),
        headers: Vec::new(),
    }
}

#[tokio::test]
async fn a_client_that_stops_reading_does_not_delay_another() {
    // Room for 256 KiB of response per stream, so a client that never reads
    // reaches the limit quickly instead of after four megabytes.
    let rig = Rig::new(HttpConfig {
        stream_buffer_bytes: 256 * 1024,
        ..HttpConfig::default()
    });
    let (mut greedy, greedy_task) = rig.connect();
    let (mut polite, _polite_task) = rig.connect();
    let mut scheduler = rig.scheduler;

    greedy.write(b"GET /big HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (greedy_id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, greedy_id);
    polite
        .write(b"GET /small HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (polite_id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, polite_id);

    // Two megabytes of response for a client that never reads a byte of it.
    scheduler
        .send(greedy_id, &Frame::RespHead(bare_head(200)))
        .await;
    for _ in 0..32 {
        scheduler
            .send(greedy_id, &Frame::RespBody(vec![b'x'; 64 * 1024]))
            .await;
    }

    // The other client's answer arrives anyway, and quickly: the channel reader
    // never waited for the first one.
    let started = Instant::now();
    scheduler.respond(polite_id, 200, &[], b"small").await;
    let response = polite.read_response().await;
    assert!(
        response.ends_with(b"\r\n5\r\nsmall\r\n0\r\n\r\n"),
        "{response:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the small response waited for the greedy one"
    );

    // The greedy stream is failed on its own: the scheduler is told, and its
    // connection is closed rather than left writing to nobody.
    let cancelled = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // Body frames already queued for the failing stream arrive first.
            if let (id, Frame::Cancel) = scheduler.next().await {
                break id;
            }
        }
    })
    .await
    .expect("a CANCEL must arrive for the stream that overflowed");
    assert_eq!(cancelled, greedy_id);

    let outcome = tokio::time::timeout(Duration::from_secs(5), greedy_task)
        .await
        .expect("the greedy connection must end")
        .expect("the task must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn a_client_that_reads_its_large_response_is_not_disturbed() {
    // The same shape of response, but small enough to sit inside the stream
    // buffer: a client that keeps up never comes near the limit, and every byte
    // arrives. The limit counts bytes received but *not yet written*, so this
    // works even though the frames arrive as one burst.
    let rig = Rig::new(HttpConfig {
        stream_buffer_bytes: 1024 * 1024,
        ..HttpConfig::default()
    });
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client.write(b"GET /big HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);

    scheduler.send(id, &Frame::RespHead(bare_head(200))).await;
    let chunk = vec![b'y'; 64 * 1024];
    for _ in 0..8 {
        scheduler.send(id, &Frame::RespBody(chunk.clone())).await;
    }
    scheduler.send(id, &Frame::RespEnd).await;

    let response = client.read_until(b"\r\n0\r\n\r\n").await;
    assert!(
        response.starts_with(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"),
        "{:?}",
        String::from_utf8_lossy(&response[..40.min(response.len())])
    );
    assert!(
        response.len() >= chunk.len() * 8,
        "the whole body should have arrived, got {} bytes",
        response.len()
    );
}

#[tokio::test]
async fn connections_over_the_cap_are_answered_with_503() {
    let mut listening = Listening::start(HttpConfig {
        max_connections: 1,
        ..HttpConfig::default()
    })
    .await;

    // The first connection stays open: it finishes a request and then idles.
    let mut first = listening.connect().await;
    first.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = listening.scheduler.head().await;
    assert_eq!(listening.scheduler.end().await, id);
    listening.scheduler.respond(id, 200, &[], b"one").await;
    let _ = first.read_response().await;

    // The second is answered without ever touching `http.chan`.
    let mut second = listening.connect().await;
    second.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    assert_eq!(
        second.read_to_end().await,
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert!(
        listening.scheduler.next_within(QUIET).await.is_none(),
        "a refused connection never becomes a stream"
    );
    let rejections = listening.events.rejections();
    assert_eq!(rejections.len(), 1);
    assert_eq!(rejections[0].1, 503);
    assert_eq!(rejections[0].2, "too_many_connections");
    assert!(
        rejections[0].0.starts_with("127.0.0.1:"),
        "the socket peer is named: {}",
        rejections[0].0
    );

    // The first connection is unaffected, which is the point of answering
    // rather than dropping.
    first.write(b"GET /again HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, head) = listening.scheduler.head().await;
    assert_eq!(head.target, b"/again");
    assert_eq!(listening.scheduler.end().await, id);
    listening.scheduler.respond(id, 200, &[], b"two").await;
    let response = first.read_response().await;
    assert!(
        response.ends_with(b"\r\n3\r\ntwo\r\n0\r\n\r\n"),
        "{response:?}"
    );
}

#[tokio::test]
async fn a_body_that_takes_too_long_is_408_and_cancelled() {
    // A body deadline shorter than the idle bound, so the total bound is what
    // fires even though bytes keep arriving.
    let rig = Rig::new(HttpConfig {
        body_timeout: Duration::from_millis(120),
        keepalive_timeout: Duration::from_secs(30),
        ..HttpConfig::default()
    });
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client
        .write(b"POST /slow HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.body().await.1, b"abc");

    // Trickle a byte at a time: no single wait is long enough to look idle.
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if !client.write_or_gone(b"d").await {
            break;
        }
    }

    assert_eq!(
        client.read_to_end().await,
        b"HTTP/1.1 408 Request Timeout\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "an upload that cannot finish in time is refused with 408"
    );
    assert_eq!(scheduler.until_cancelled().await.0, id);
    assert_eq!(
        rig.events.rejections(),
        [(
            "203.0.113.7:54321".to_string(),
            408,
            "body_timeout".to_string()
        )]
    );
}

#[tokio::test]
async fn a_body_that_goes_quiet_still_hits_the_idle_bound() {
    // The other way round: a short idle bound inside a long body deadline, so
    // the client is treated as gone and gets no answer at all.
    let rig = Rig::new(HttpConfig {
        body_timeout: Duration::from_secs(30),
        keepalive_timeout: Duration::from_millis(60),
        ..HttpConfig::default()
    });
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.body().await.1, b"abc");
    assert_eq!(scheduler.until_cancelled().await.0, id);
    assert_eq!(
        client.read_within(Duration::from_secs(2)).await,
        Some(Vec::new()),
        "nothing is written for a client that went quiet"
    );
    assert!(rig.events.is_empty(), "an idle client is not a rejection");
}

#[tokio::test]
async fn a_scheduler_that_dies_mid_body_is_a_502() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client
        .write(b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nabc")
        .await;
    let (_id, _) = scheduler.head().await;
    assert_eq!(scheduler.body().await.1, b"abc");

    // The channel dies while the body is still arriving; nothing has been
    // written to the client, so it gets an answer rather than a hang.
    scheduler.close();
    assert_eq!(
        client.read_to_end().await,
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert!(rig.events.is_empty(), "a 502 is not an http.rejected event");
}

#[tokio::test]
async fn a_channel_that_dies_closes_an_idle_keep_alive_connection() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);
    scheduler.respond(id, 200, &[], b"ok").await;
    let _ = client.read_response().await;

    // With no scheduler there is nobody to route the next request to, so the
    // connection is dropped instead of waiting for one.
    scheduler.close();
    assert_eq!(
        client.read_within(Duration::from_secs(5)).await,
        Some(Vec::new()),
        "the idle connection is closed"
    );
    let outcome = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the connection must end")
        .expect("the task must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}
