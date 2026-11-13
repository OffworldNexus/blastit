//! Per-stream flow control (§3.1): request bodies are paced by the credit the
//! scheduler grants, and response bodies by the credit the front end grants
//! back as the client takes them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use blastit_frame::{Frame, INITIAL_WINDOW, MAX_FRAME_DEFAULT, RespHead};
use blastit_http::HttpConfig;

use common::{Harness, QUIET, Rig};

/// Response bytes one stream may have in flight before it is over its credit.
const CAP: usize = INITIAL_WINDOW as usize + MAX_FRAME_DEFAULT;

/// The body bytes one roomy frame carries in these tests.
const CHUNK: usize = 64 * 1024;

fn bare_head(status: u16) -> RespHead {
    RespHead {
        status,
        reason: Vec::new(),
        headers: Vec::new(),
    }
}

/// A head that announces `len` body bytes, so the bytes on the client's wire
/// are the body bytes and a test's accounting is exact.
fn length_head(status: u16, len: usize) -> RespHead {
    let len = len.to_string();
    RespHead {
        status,
        reason: Vec::new(),
        headers: common::pairs(&[("content-length", &len)]),
    }
}

/// Send `total` bytes of request body for `id`, and report how much of it the
/// front end forwarded.
///
/// Stops as soon as `want` bytes have arrived, so a test can look at the pause
/// without waiting for the timeout.
async fn collect_body(scheduler: &mut common::Scheduler, want: usize) -> usize {
    let mut seen = 0;
    while seen < want {
        let (_, chunk) = scheduler.body().await;
        seen += chunk.len();
    }
    seen
}

#[tokio::test]
async fn an_upload_is_paced_by_the_window() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;
    let total = 3 * INITIAL_WINDOW as usize;

    client
        .write(
            format!("POST /upload HTTP/1.1\r\nHost: h\r\nContent-Length: {total}\r\n\r\n")
                .as_bytes(),
        )
        .await;
    // The client keeps writing; the front end stops reading once its credit is
    // spent, so this task blocks part-way through.
    let body = vec![b'u'; total];
    let writing = tokio::spawn(async move {
        let sent = client.write_or_gone(&body).await;
        (client, sent)
    });

    let (id, _) = scheduler.head().await;

    // Exactly one window's worth goes out, and then it stops.
    let seen = collect_body(&mut scheduler, INITIAL_WINDOW as usize).await;
    assert_eq!(
        seen, INITIAL_WINDOW as usize,
        "the front end sent more than its credit"
    );
    assert!(
        scheduler
            .next_within(Duration::from_millis(300))
            .await
            .is_none(),
        "the upload must pause until the scheduler grants more"
    );

    // A grant releases exactly one more window.
    scheduler
        .send(
            id,
            &Frame::Window {
                bytes: INITIAL_WINDOW,
            },
        )
        .await;
    let seen = collect_body(&mut scheduler, INITIAL_WINDOW as usize).await;
    assert_eq!(seen, INITIAL_WINDOW as usize);

    // And the rest, so the whole body arrives.
    scheduler
        .send(
            id,
            &Frame::Window {
                bytes: INITIAL_WINDOW,
            },
        )
        .await;
    let rest = total - 2 * INITIAL_WINDOW as usize;
    let seen = collect_body(&mut scheduler, rest).await;
    assert_eq!(seen, rest);
    assert_eq!(scheduler.end().await, id, "REQ_END after the last byte");

    let (mut client, all_sent) = writing.await.expect("the writer task must finish");
    assert!(
        all_sent,
        "the client should have been able to send everything"
    );

    // A response far below the low-water mark is answered without a `WINDOW`:
    // there is nothing for the front end to grant back.
    scheduler.respond(id, 201, &[], b"created!").await;
    let response = client.read_response().await;
    assert!(
        response.starts_with(b"HTTP/1.1 201 Created\r\n"),
        "{response:?}"
    );
    assert_eq!(
        scheduler.window_within(QUIET).await,
        None,
        "a small response must not cost a WINDOW"
    );
}

#[tokio::test]
async fn a_chunked_upload_is_paced_by_the_window_too() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;
    let total = 2 * INITIAL_WINDOW as usize;

    client
        .write(b"POST /chunked HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n")
        .await;
    // A single chunk stream, written by a task that blocks once the front end
    // stops reading.
    let mut wire = Vec::new();
    wire.extend_from_slice(format!("{total:x}\r\n").as_bytes());
    wire.extend_from_slice(&vec![b'c'; total]);
    wire.extend_from_slice(b"\r\n0\r\n\r\n");
    let writing = tokio::spawn(async move {
        let sent = client.write_or_gone(&wire).await;
        (client, sent)
    });

    let (id, _) = scheduler.head().await;
    let seen = collect_body(&mut scheduler, INITIAL_WINDOW as usize).await;
    assert_eq!(seen, INITIAL_WINDOW as usize);
    assert!(
        scheduler
            .next_within(Duration::from_millis(300))
            .await
            .is_none(),
        "the chunked upload must pause too"
    );

    scheduler
        .send(
            id,
            &Frame::Window {
                bytes: INITIAL_WINDOW,
            },
        )
        .await;
    let seen = collect_body(&mut scheduler, INITIAL_WINDOW as usize).await;
    assert_eq!(seen, INITIAL_WINDOW as usize);
    assert_eq!(scheduler.end().await, id);

    let (_client, _sent) = writing.await.expect("the writer task must finish");
}

#[tokio::test]
async fn a_small_response_costs_no_window() {
    let mut h = Harness::start(HttpConfig::default());
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);

    // The whole body fits inside the credit the view already has, so the front
    // end owes it nothing: a `WINDOW` here would be a frame each way, and two
    // wakeups, for six bytes.
    h.scheduler.respond(id, 200, &[], b"hello!").await;
    let response = h.client.read_response().await;
    assert!(
        response.ends_with(b"\r\n6\r\nhello!\r\n0\r\n\r\n"),
        "{response:?}"
    );

    assert_eq!(
        h.scheduler.window_within(QUIET).await,
        None,
        "a body below the low-water mark must not cost a WINDOW"
    );
}

#[tokio::test]
async fn a_large_response_is_completed_by_its_grants() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;
    let total = 4 * INITIAL_WINDOW as usize;

    client.write(b"GET /big HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);

    // The client reads everything it is sent, in the background.
    let reading = tokio::spawn(async move {
        let mut client = client;
        let response = client.read_until(b"\r\n0\r\n\r\n").await;
        (client, response)
    });

    // A view that respects its window: it sends no more than its credit, and
    // waits for a `WINDOW` whenever it is about to run out. A body of several
    // windows can therefore only finish if the front end grants enough, and a
    // missing grant shows up as a hang here rather than a slow pass.
    scheduler.send(id, &Frame::RespHead(bare_head(200))).await;
    let frames = total / CHUNK;
    let mut credit = INITIAL_WINDOW as usize;
    let mut sent = 0usize;
    let mut granted = 0usize;
    let mut grants = 0usize;
    while sent < total {
        let take = CHUNK.min(total - sent);
        if take > credit {
            let (granted_id, bytes) = scheduler.window().await;
            assert_eq!(granted_id, id, "a grant for the wrong stream");
            assert!(
                bytes as usize > INITIAL_WINDOW as usize / 2,
                "a grant of {bytes} bytes is not the batched top-up"
            );
            credit += bytes as usize;
            granted += bytes as usize;
            grants += 1;
        }
        scheduler.send(id, &Frame::RespBody(vec![b'z'; take])).await;
        sent += take;
        credit -= take;
    }
    scheduler.send(id, &Frame::RespEnd).await;

    // The view never went short: what it started with plus every grant covers
    // the whole body.
    assert!(
        granted + INITIAL_WINDOW as usize >= total,
        "the view ran out of credit: {granted} granted for {total} bytes"
    );
    assert!(grants > 0, "a body past one window must be granted more");
    assert!(
        grants < frames,
        "{grants} grants for {frames} frames is no batching"
    );

    let (_client, response) = reading.await.expect("the reader task must finish");
    assert!(
        response.len() >= total,
        "the whole body should have reached the client, got {}",
        response.len()
    );
}

#[tokio::test]
async fn a_view_that_spends_its_credit_is_granted_before_the_connection_waits() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client.write(b"GET /big HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);

    let reading = tokio::spawn(async move {
        let mut client = client;
        let response = client.read_until(b"\r\n0\r\n\r\n").await;
        (client, response)
    });

    // The view sends exactly the window it starts with and then goes quiet: it
    // has no credit left, and a view blocked on credit sends nothing more, so no
    // frame will arrive to trigger a later grant.
    scheduler.send(id, &Frame::RespHead(bare_head(200))).await;
    for _ in 0..(INITIAL_WINDOW as usize / CHUNK) {
        scheduler
            .send(id, &Frame::RespBody(vec![b'z'; CHUNK]))
            .await;
    }

    // The grant must go out anyway, before the front end waits for a frame that
    // cannot come.
    let (granted_id, bytes) = scheduler.window().await;
    assert_eq!(granted_id, id);
    assert!(bytes > 0, "a window grants at least one byte");

    // With the credit it has been granted, the view finishes the response.
    scheduler
        .send(id, &Frame::RespBody(vec![b'z'; CHUNK]))
        .await;
    scheduler.send(id, &Frame::RespEnd).await;

    let (_client, response) = reading.await.expect("the reader task must finish");
    assert!(
        response.ends_with(b"\r\n0\r\n\r\n"),
        "the response must finish"
    );
}

#[tokio::test]
async fn a_client_that_stops_reading_stops_being_granted() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;

    client.write(b"GET /big HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);

    // Nobody is reading the client end of this connection, and the view sends
    // only as much as its credit allows, so nothing is failed: the front end
    // simply stops writing once the socket buffer is full, and its credit stops
    // there too. The body is announced by length, so the bytes on the wire are
    // the body bytes.
    let offered = 2 * INITIAL_WINDOW as usize;
    scheduler
        .send(id, &Frame::RespHead(length_head(200, offered)))
        .await;
    for _ in 0..(offered / CHUNK) {
        scheduler
            .send(id, &Frame::RespBody(vec![b'z'; CHUNK]))
            .await;
    }

    // The front end can only credit what it has written, and it stops writing
    // once the client's socket buffer is full.
    let mut granted = 0usize;
    while let Some((_, bytes)) = scheduler.window_within(Duration::from_millis(300)).await {
        granted += bytes as usize;
    }
    assert!(granted > 0, "the bytes it did write are credited");
    assert!(
        granted <= INITIAL_WINDOW as usize,
        "credit cannot run ahead of the writes: {granted}"
    );
    assert_eq!(
        scheduler.next_within(QUIET).await,
        None,
        "a client that reads nothing is not failed for it"
    );

    // Draining the whole body lets the front end write again, and the credit it
    // owes crosses the mark once more.
    client.read_exact_n(offered).await;
    let mut more = 0usize;
    while let Some((_, bytes)) = scheduler.window_within(Duration::from_millis(300)).await {
        more += bytes as usize;
    }
    assert!(more > 0, "reading from the client must release credit");
}

#[tokio::test]
async fn a_response_beyond_the_credit_aborts_that_stream_only() {
    // The default buffer is the window plus one frame: a peer that keeps
    // sending past its credit is over it, and only that stream fails.
    let rig = Rig::new(HttpConfig::default());
    let (mut greedy, greedy_task) = rig.connect_with_capacity(CHUNK);
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

    // Nobody reads the greedy client, and the view ignores its credit.
    scheduler
        .send(greedy_id, &Frame::RespHead(bare_head(200)))
        .await;
    for _ in 0..((CAP + INITIAL_WINDOW as usize) / CHUNK) {
        scheduler
            .send(greedy_id, &Frame::RespBody(vec![b'g'; CHUNK]))
            .await;
    }

    // The other stream is served throughout.
    scheduler.respond(polite_id, 200, &[], b"small").await;
    let response = polite.read_response().await;
    assert!(
        response.ends_with(b"\r\n5\r\nsmall\r\n0\r\n\r\n"),
        "{response:?}"
    );

    // The greedy stream is failed on its own.
    let outcome = tokio::time::timeout(Duration::from_secs(5), greedy_task)
        .await
        .expect("the greedy connection must end")
        .expect("the task must not panic");
    assert!(outcome.is_ok(), "{outcome:?}");
}

#[tokio::test]
async fn a_stream_waiting_for_credit_does_not_hold_up_another() {
    let rig = Rig::new(HttpConfig::default());
    let (mut uploading, _upload_task) = rig.connect();
    let (mut polite, _polite_task) = rig.connect();
    let mut scheduler = rig.scheduler;
    let total = 3 * INITIAL_WINDOW as usize;

    // The first client is uploading more than its credit allows and will not
    // be granted any more.
    uploading
        .write(
            format!("POST /upload HTTP/1.1\r\nHost: h\r\nContent-Length: {total}\r\n\r\n")
                .as_bytes(),
        )
        .await;
    let body = vec![b'u'; total];
    let writing = tokio::spawn(async move {
        let _ = uploading.write_or_gone(&body).await;
    });

    let (id, _) = scheduler.head().await;
    assert_eq!(
        collect_body(&mut scheduler, INITIAL_WINDOW as usize).await,
        INITIAL_WINDOW as usize
    );

    // A second connection is answered normally while the first is parked.
    polite
        .write(b"GET /small HTTP/1.1\r\nHost: h\r\n\r\n")
        .await;
    let (polite_id, head) = scheduler.head().await;
    assert_eq!(head.target, b"/small");
    assert_ne!(polite_id, id);
    assert_eq!(scheduler.end().await, polite_id);
    scheduler.respond(polite_id, 200, &[], b"small").await;
    let response = polite.read_response().await;
    assert!(
        response.ends_with(b"\r\n5\r\nsmall\r\n0\r\n\r\n"),
        "{response:?}"
    );

    // The parked upload is still parked, not broken.
    assert!(
        scheduler.next_within(QUIET).await.is_none(),
        "the parked upload stays parked"
    );
    writing.abort();
}

#[tokio::test]
async fn a_body_timeout_still_applies_while_waiting_for_credit() {
    let rig = Rig::new(HttpConfig {
        body_timeout: Duration::from_millis(120),
        keepalive_timeout: Duration::from_secs(30),
        ..HttpConfig::default()
    });
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;
    let total = 2 * INITIAL_WINDOW as usize;

    client
        .write(
            format!("POST /upload HTTP/1.1\r\nHost: h\r\nContent-Length: {total}\r\n\r\n")
                .as_bytes(),
        )
        .await;
    let body = vec![b'u'; total];
    let writing = tokio::spawn(async move {
        let _ = client.write_or_gone(&body).await;
    });

    let (id, _) = scheduler.head().await;
    assert_eq!(
        collect_body(&mut scheduler, INITIAL_WINDOW as usize).await,
        INITIAL_WINDOW as usize
    );

    // The upload is parked on credit, and the body deadline expires anyway.
    assert_eq!(scheduler.until_cancelled().await.0, id);
    assert_eq!(
        rig.events.rejections(),
        [("203.0.113.7".to_string(), 408, "body_timeout".to_string())],
        "the refusal names the peer's address, and not its port"
    );
    writing.abort();
}

#[tokio::test]
async fn credit_for_a_finished_stream_is_ignored() {
    // A `WINDOW` that arrives after the body is already dealt with must not
    // disturb anything, and must not be mistaken for a response frame.
    let mut h = Harness::start(HttpConfig::default());
    h.client.write(b"GET / HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = h.scheduler.head().await;
    assert_eq!(h.scheduler.end().await, id);

    h.scheduler.send(id, &Frame::Window { bytes: 4096 }).await;
    h.scheduler.send(id, &Frame::Window { bytes: 1 }).await;
    h.scheduler.respond(id, 200, &[], b"ok").await;

    let response = h.client.read_response().await;
    assert!(
        response.ends_with(b"\r\n2\r\nok\r\n0\r\n\r\n"),
        "{response:?}"
    );

    // The late grant is not a body: the response above is the whole answer.
    assert_eq!(h.events.rejections().len(), 0);
}
