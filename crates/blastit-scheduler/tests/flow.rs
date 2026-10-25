//! Flow control: per-stream credit, the outbound caps, and what happens when a
//! peer sends more than it was granted (ARCHITECTURE.md §3.1).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use blastit_events::Outcome;
use blastit_frame::{Frame, INITIAL_WINDOW};
use common::{Options, Script, World, body_of, head, route, status_of};
use tokio::sync::Semaphore;

/// A body several windows long: three MiB, which is three `INITIAL_WINDOW`s.
fn three_mib() -> Vec<u8> {
    let mut body = Vec::with_capacity(3 * 1024 * 1024);
    let mut byte = 0u8;
    for _ in 0..3 * 1024 * 1024 {
        body.push(byte);
        byte = byte.wrapping_add(1);
    }
    body
}

#[tokio::test]
async fn a_large_upload_streams_through_with_credit() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(
        0,
        Script::Consume {
            body: b"stored".to_vec(),
        },
    ));
    let body = three_mib();

    // The client never sends more than its credit: with a 1 MiB window it has
    // to wait for the view to consume what it already sent, twice over.
    world
        .http
        .send_request(1, head("POST", "/upload"), &body)
        .await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"stored");

    // Every byte arrived, in order, and the view granted back exactly what it
    // read, window by window.
    let received = world.log.lock().unwrap().received_by(0);
    let mut uploaded = Vec::new();
    for (_, frame) in &received {
        if let Frame::ReqBody(chunk) = frame {
            uploaded.extend_from_slice(chunk);
        }
    }
    assert_eq!(uploaded.len(), body.len());
    assert_eq!(uploaded, body);
    assert_eq!(world.http.granted_bytes(1), body.len() as u64);
    assert_eq!(world.requests()[0].bytes_in(), body.len() as u64);
    assert_eq!(world.requests()[0].outcome(), Outcome::Ok);
}

#[tokio::test]
async fn a_queued_upload_does_not_delay_another_request() {
    tokio::time::pause();
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .manifest(&format!(
            "version = 1\n{}{}",
            route("slow", "/slow/"),
            route("app", ".*")
        ))
        .script(
            0,
            Script::Hold {
                body: b"held".to_vec(),
                gate: gate.clone(),
            },
        )
        .script(
            1,
            Script::Consume {
                body: b"ok".to_vec(),
            },
        );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 4;
    options.cfg.queue_timeout = Duration::from_secs(600);
    let mut world = World::start(options);

    // One request holds the only worker slot, so the upload behind it can only
    // send its first window; it then has to wait for credit, which is exactly
    // what bounds a queued body (§3.1).
    world.http.send_request(1, head("GET", "/slow/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;

    let upload: Vec<u8> = vec![b'u'; 2 * 1024 * 1024];
    world.http.send_head(2, head("POST", "/upload")).await;
    world
        .http
        .send_body(2, &upload[..INITIAL_WINDOW as usize])
        .await;
    world.advance(Duration::from_millis(5)).await;
    assert_eq!(world.http.upload_credit(2), 0, "the window is spent");

    // A request that arrives *after* the queued upload is still served:
    // nothing stopped reading `http.chan` (§3.1).
    world
        .http
        .send_request(3, head("GET", "/_blastit/health"), b"")
        .await;
    let health = world.http.response(3).await;
    assert_eq!(status_of(&health), 200);
    assert_eq!(body_of(&health), b"ok");
    assert!(
        world.log.lock().unwrap().request_ids(1).is_empty(),
        "the upload is queued, so its view has not seen it yet"
    );

    // Releasing the slow request lets the upload be dispatched; the view then
    // consumes what it has and grants credit for the rest.
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    world
        .http
        .send_body(2, &upload[INITIAL_WINDOW as usize..])
        .await;
    world.http.send(2, &Frame::ReqEnd).await;
    assert_eq!(body_of(&world.http.response(2).await), b"ok");

    let received = world.log.lock().unwrap().received_by(1);
    let mut uploaded = Vec::new();
    for (_, frame) in &received {
        if let Frame::ReqBody(chunk) = frame {
            uploaded.extend_from_slice(chunk);
        }
    }
    assert_eq!(uploaded, upload);
}

#[tokio::test]
async fn a_view_that_exceeds_its_response_credit_is_retired() {
    tokio::time::pause();
    // The view writes three half-megabyte frames with no credit granted: more
    // than one window, so it has broken §3.1.
    let mut world = World::start(Options::new().script(
        0,
        Script::OverCredit {
            chunk: 512 * 1024,
            count: 3,
        },
    ));
    world.http.send_request(1, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert!(common::aborted(&frames), "{frames:?}");

    world
        .until(|world| !world.log.lock().unwrap().retires.is_empty())
        .await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "credit_overflow".to_string())]
    );
    assert_eq!(world.requests()[0].outcome(), Outcome::ViewDied);
    assert!(matches!(
        world.retirements()[0].reason(),
        blastit_events::RetireReason::Other(reason) if reason == "credit_overflow"
    ));
    // The scheduler never forwarded more than the window it was allowed.
    let frames = world.http.take_all();
    let forwarded: usize = frames
        .iter()
        .filter_map(|(_, frame)| match frame {
            Frame::RespBody(body) => Some(body.len()),
            _ => None,
        })
        .sum();
    assert!(forwarded <= INITIAL_WINDOW as usize, "{forwarded} bytes");
}

#[tokio::test]
async fn a_client_that_exceeds_its_upload_credit_fails_the_stream() {
    tokio::time::pause();
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"held".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.request_timeout = Duration::from_secs(600);
    let mut world = World::start(options);

    // A `GET` with a body the view never grants credit for: one byte over the
    // initial window is a protocol violation (§3.1).
    world.http.send_head(1, head("GET", "/x")).await;
    world
        .http
        .send_body_forced(1, &vec![b'x'; INITIAL_WINDOW as usize + 1])
        .await;
    world.until(|world| !world.requests().is_empty()).await;

    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 502);
    assert_eq!(world.requests()[0].outcome(), Outcome::ViewDied);
    // The stream is cancelled on both sides: http stops reading this client's
    // socket, and the view stops working on a request nobody will answer.
    let frames = world.http.take_all();
    assert!(
        frames
            .iter()
            .any(|(id, frame)| *id == 1 && matches!(frame, Frame::Cancel)),
        "{frames:?}"
    );
    world
        .until(|world| {
            world
                .log
                .lock()
                .unwrap()
                .saw(0, |frame| matches!(frame, Frame::Cancel))
        })
        .await;
}

#[tokio::test]
async fn a_view_whose_outbound_queue_overflows_is_retired() {
    tokio::time::pause();
    // The view hands out more credit than it can possibly consume and then
    // stops reading, so the scheduler's queue towards it is what has to stay
    // bounded (§3.1). A `view.chan` that nobody drains must never grow a buffer
    // the scheduler cannot account for.
    let mut options = Options::new().script(
        0,
        Script::Deaf {
            window: 8 * 1024 * 1024,
        },
    );
    options.cfg.request_timeout = Duration::from_secs(600);
    options.cfg.queue_timeout = Duration::from_secs(600);
    let mut world = World::start(options);

    let body = vec![b'x'; 6 * 1024 * 1024];
    world.http.send_head(1, head("POST", "/x")).await;
    world.http.send_body(1, &body).await;
    world.http.send(1, &Frame::ReqEnd).await;

    world
        .until(|world| !world.log.lock().unwrap().retires.is_empty())
        .await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "credit_overflow".to_string())]
    );
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 502);
    assert_eq!(world.requests()[0].outcome(), Outcome::ViewDied);
}

#[tokio::test]
async fn credit_that_arrives_before_dispatch_is_not_lost() {
    tokio::time::pause();
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"held".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 4;
    let mut world = World::start(options);

    // Two requests: the first holds the slot, the second queues. The client
    // grants the *second* one more response credit while it is still waiting.
    world.http.send_request(1, head("GET", "/slow"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world
        .http
        .send_request(2, head("GET", "/queued"), b"")
        .await;
    world.advance(Duration::from_millis(1)).await;
    world.http.grant(2, 4096).await;
    world.advance(Duration::from_millis(1)).await;

    gate.add_permits(2);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    assert_eq!(body_of(&world.http.response(2).await), b"held");
    // The extra credit was handed to the view when the request was dispatched.
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    assert!(world.log.lock().unwrap().saw(
        0,
        |frame| matches!(frame, Frame::Window { bytes } if *bytes == 4096)
    ));
}
