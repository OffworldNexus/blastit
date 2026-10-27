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

    // The response is credited back, byte for byte, as the client takes it.
    scheduler.respond(id, 201, &[], b"created!").await;
    assert_eq!(scheduler.window().await, (id, 8));
    let response = client.read_response().await;
    assert!(
        response.starts_with(b"HTTP/1.1 201 Created\r\n"),
        "{response:?}"
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
async fn a_response_is_granted_back_as_the_client_reads_it() {
    let rig = Rig::new(HttpConfig::default());
    let (mut client, _task) = rig.connect();
    let mut scheduler = rig.scheduler;
    let total = 3 * INITIAL_WINDOW as usize;

    client.write(b"GET /big HTTP/1.1\r\nHost: h\r\n\r\n").await;
    let (id, _) = scheduler.head().await;
    assert_eq!(scheduler.end().await, id);

    // The client reads everything it is sent, in the background.
    let reading = tokio::spawn(async move {
        let mut client = client;
        let response = client.read_until(b"\r\n0\r\n\r\n").await;
        (client, response)
    });

    scheduler.send(id, &Frame::RespHead(bare_head(200))).await;
    let frames = total / CHUNK;
    for _ in 0..frames {
        scheduler
            .send(id, &Frame::RespBody(vec![b'z'; CHUNK]))
            .await;
    }
    scheduler.send(id, &Frame::RespEnd).await;

    // Every body byte comes back as credit, batched.
    let mut granted = 0usize;
    let mut grants = 0;
    while granted < total {
        let (_, bytes) = scheduler.window().await;
        assert!(bytes > 0);
        if granted + bytes as usize <= total - CHUNK {
            assert!(
                bytes as usize >= 64 * 1024,
                "a grant of {bytes} bytes is below the batch size"
            );
        }
        granted += bytes as usize;
        grants += 1;
    }
    assert_eq!(granted, total, "the client took every byte offered");
    assert!(grants <= frames, "one grant per frame would be no batching");

    let (_client, response) = reading.await.expect("the reader task must finish");
    assert!(
        response.len() >= total,
        "the whole body should have reached the client, got {}",
        response.len()
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
    // less than the credit it is allowed, so nothing is failed: it is simply
    // granted nothing beyond what the socket has taken.
    let offered = INITIAL_WINDOW as usize + 512 * 1024;
    scheduler.send(id, &Frame::RespHead(bare_head(200))).await;
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

    // Reading a little more releases a little more.
    let _ = client.read_exact_n(512 * 1024).await;
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
