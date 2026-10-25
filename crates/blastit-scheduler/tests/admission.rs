//! Admission: the global worker budget, the per-view cap, the queue, request
//! timeouts and client cancels (ARCHITECTURE.md §2.4, §3.2).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use blastit_events::Outcome;
use blastit_frame::Frame;
use common::{Options, Script, World, body_of, ended, head, status_of};
use tokio::sync::{Notify, Semaphore};

#[tokio::test]
async fn the_budget_is_global_and_the_queue_is_fifo() {
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
    options.cfg.queue_size = 2;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    // One slot is taken, so these two wait in the queue, and the fourth finds
    // it full: that is what proves they were queued before the clock moves.
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.http.send_request(3, head("GET", "/3"), b"").await;
    world.http.send_request(4, head("GET", "/4"), b"").await;
    assert_eq!(status_of(&world.http.response(4).await), 503);
    world.advance(Duration::from_millis(5)).await;
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1]);

    // Releasing one thread at a time dispatches the queue in arrival order.
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(2).await), b"held");
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 3)
        .await;
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(3).await), b"held");

    for event in world.requests() {
        if event.path() == "/4" {
            assert_eq!(event.outcome(), Outcome::QueueFull);
            continue;
        }
        assert_eq!(event.outcome(), Outcome::Ok, "{:?}", event.path());
    }
    let second = world.event_by_path("/2").unwrap();
    let third = world.event_by_path("/3").unwrap();
    assert!(
        second.queued_ms() > 0.0 && third.queued_ms() >= second.queued_ms(),
        "the queue is FIFO: /2 {:?}ms then /3 {:?}ms",
        second.queued_ms(),
        third.queued_ms()
    );
}

#[tokio::test]
async fn a_full_queue_answers_503_queue_full() {
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
    options.cfg.queue_size = 1;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.http.send_request(3, head("GET", "/3"), b"").await;

    let refused = world.http.response(3).await;
    assert_eq!(status_of(&refused), 503);
    assert!(ended(&refused));
    let event = world.requests().last().cloned().unwrap();
    assert_eq!(event.outcome(), Outcome::QueueFull);
    assert_eq!(event.status(), 503);
    // The queue still holds the one request it is allowed.
    gate.add_permits(2);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    assert_eq!(body_of(&world.http.response(2).await), b"held");
}

#[tokio::test]
async fn a_queue_wait_over_the_limit_answers_503_queue_timeout() {
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
    options.cfg.queue_timeout = Duration::from_secs(10);
    options.cfg.request_timeout = Duration::from_secs(120);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    let refused = world.http.awaited_response(2).await;
    assert_eq!(status_of(&refused), 503);
    let event = world.requests().last().cloned().unwrap();
    assert_eq!(event.outcome(), Outcome::QueueTimeout);
    assert!(event.queued_ms() >= 10_000.0, "{:?}", event.queued_ms());

    // The first request was never affected.
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
}

#[tokio::test]
async fn the_per_view_cap_queues_even_with_budget_to_spare() {
    tokio::time::pause();
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new().script(
        0,
        Script::Hold {
            body: b"held".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.max_workers = 8;
    options.cfg.view_max_workers = Some(1);
    options.cfg.queue_size = 1;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.log.lock().unwrap().request_ids(0),
        vec![1],
        "the per-view cap holds the second request back"
    );
    // Queue size is one, so a third request is refused: the second is queued.
    world.http.send_request(3, head("GET", "/3"), b"").await;
    assert_eq!(status_of(&world.http.response(3).await), 503);

    gate.add_permits(2);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    assert_eq!(body_of(&world.http.response(2).await), b"held");
}

#[tokio::test]
async fn a_slot_is_only_released_by_done() {
    tokio::time::pause();
    // The view answers in full but never sends DONE, so its thread is still
    // alive as far as the scheduler knows (§2.4).
    let mut options = Options::new().script(
        0,
        Script::NoDone {
            body: b"answer".to_vec(),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 1;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"answer");
    assert_eq!(world.requests()[0].outcome(), Outcome::Ok);

    // The response is complete, but the slot is still taken: a second request
    // queues behind it, and a third finds the queue full.
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.http.send_request(3, head("GET", "/3"), b"").await;
    assert_eq!(status_of(&world.http.response(3).await), 503);
    assert_eq!(
        world.requests().last().unwrap().outcome(),
        Outcome::QueueFull
    );
}

#[tokio::test]
async fn a_late_done_releases_the_slot() {
    tokio::time::pause();
    let gate = Arc::new(Notify::new());
    let mut options = Options::new().script(
        0,
        Script::Gated {
            body: b"answer".to_vec(),
            gate: gate.clone(),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 1;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"answer");
    world.http.send_request(2, head("GET", "/2"), b"").await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1]);

    gate.notify_one();
    assert_eq!(body_of(&world.http.response(2).await), b"answer");
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
}

#[tokio::test]
async fn a_slow_request_gets_504_and_its_slot_stays_taken() {
    tokio::time::pause();
    let mut options = Options::new().script(0, Script::HeadOnly);
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 1;
    options.cfg.request_timeout = Duration::from_secs(30);
    options.cfg.queue_timeout = Duration::from_secs(300);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    // The head went out, so the timeout aborts the response rather than
    // sending a second head.
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert!(common::aborted(&frames), "{frames:?}");
    let event = world.requests()[0].clone();
    assert_eq!(event.outcome(), Outcome::Timeout);
    assert_eq!(event.status(), 200);

    // The view was told to cancel, and the slot is still held: a queued
    // request cannot run, and the queue is full after one more.
    world
        .until(|world| {
            world
                .log
                .lock()
                .unwrap()
                .saw(0, |frame| matches!(frame, Frame::Cancel))
        })
        .await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.http.send_request(3, head("GET", "/3"), b"").await;
    assert_eq!(status_of(&world.http.response(3).await), 503);
}

#[tokio::test]
async fn a_silent_request_gets_a_504_body() {
    tokio::time::pause();
    let mut options = Options::new().script(0, Script::Silent);
    options.cfg.request_timeout = Duration::from_secs(30);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 504);
    assert!(ended(&frames));
    assert_eq!(body_of(&frames), b"gateway timeout\n");
    assert_eq!(world.requests()[0].outcome(), Outcome::Timeout);
    assert_eq!(world.requests()[0].status(), 504);
}

#[tokio::test]
async fn a_queued_request_cancelled_by_the_client_leaves_the_queue() {
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
    options.cfg.queue_size = 2;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.http.send(2, &Frame::Cancel).await;
    world.until(|world| world.requests().len() == 1).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::ClientGone);
    assert_eq!(
        world.log.lock().unwrap().request_ids(0),
        vec![1],
        "a cancelled request never reaches the view"
    );

    // It left the queue: two more requests fit exactly into queue_size = 2.
    world.http.send_request(3, head("GET", "/3"), b"").await;
    world.http.send_request(4, head("GET", "/4"), b"").await;
    world.advance(Duration::from_millis(5)).await;
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1]);

    gate.add_permits(3);
    assert_eq!(body_of(&world.http.response(1).await), b"held");
    assert_eq!(body_of(&world.http.response(3).await), b"held");
    assert_eq!(body_of(&world.http.response(4).await), b"held");
}

#[tokio::test]
async fn a_dispatched_request_cancelled_by_the_client_keeps_its_slot() {
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
    options.cfg.queue_size = 1;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world.http.send(1, &Frame::Cancel).await;
    // The view is told, and the request is over as far as the client is
    // concerned...
    world
        .until(|world| {
            world
                .log
                .lock()
                .unwrap()
                .saw(0, |frame| matches!(frame, Frame::Cancel))
        })
        .await;
    world.until(|world| world.requests().len() == 1).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::ClientGone);

    // ... but the thread is still alive, so the slot is held until DONE (§2.4):
    // one request fits in the queue, the next one does not.
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world.http.send_request(3, head("GET", "/3"), b"").await;
    assert_eq!(status_of(&world.http.response(3).await), 503);

    gate.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(2).await), b"held");
}

#[tokio::test]
async fn a_cancelled_request_tells_its_view_and_then_releases_the_slot() {
    tokio::time::pause();
    let mut options = Options::new().script(0, Script::WaitCancel);
    options.cfg.max_workers = 1;
    // One request per process, so the next request forks a fresh view instead
    // of landing in the process that only knows how to wait for a cancel.
    options.cfg.max_requests = 1;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world.http.send(1, &Frame::Cancel).await;

    // The view answers the cancel with DONE, which frees the slot.
    world.until(|world| world.requests().len() == 1).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::ClientGone);
    world.set_script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    world.http.send_request(2, head("GET", "/2"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"ok");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 0]);
}
