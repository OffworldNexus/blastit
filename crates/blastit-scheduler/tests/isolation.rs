//! Ids, isolation and body plumbing: what a view may and may not address
//! (ARCHITECTURE.md §3.1, §3.2).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use blastit_events::Outcome;
use blastit_frame::{Ctl, Frame};
use common::{Options, Script, World, body_of, head, route, status_of};
use std::os::fd::OwnedFd;
use std::time::Duration;
use tokio::net::UnixStream;

fn two_routes() -> String {
    format!("version = 1\n{}{}", route("a", "/a/"), route("b", "/b/"))
}

#[tokio::test]
async fn frames_for_unknown_request_ids_are_dropped() {
    tokio::time::pause();
    let mut world = World::start(
        Options::new()
            .manifest(&two_routes())
            .script(
                0,
                Script::ForeignId {
                    body: b"a".to_vec(),
                },
            )
            .script(
                1,
                Script::Answer {
                    body: b"b".to_vec(),
                },
            ),
    );

    // Route 0's view answers under a request id it was never given, next to
    // its real answer; route 1 is serving a different request at the same
    // time. The bogus frame must reach neither stream.
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200);
    assert_eq!(body_of(&first), b"a");
    assert_eq!(first.len(), 3, "only head, body and end: {first:?}");
    let second = world.http.response(2).await;
    assert_eq!(status_of(&second), 200);
    assert_eq!(body_of(&second), b"b");
    assert_eq!(second.len(), 3, "{second:?}");

    // Nothing else made it out of the scheduler.
    assert!(world.http.leftover().is_empty());
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1]);
    // The request ids are per view, and each view only ever saw its own.
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1]);
    assert_eq!(world.log.lock().unwrap().request_ids(1), vec![1]);
}

#[tokio::test]
async fn one_route_never_gets_a_second_process_while_the_first_lives() {
    tokio::time::pause();
    let mut world = World::start(Options::new().manifest(&two_routes()).script(
        0,
        Script::Answer {
            body: b"a".to_vec(),
        },
    ));
    for stream in 1..=4u32 {
        world
            .http
            .send_request(stream, head("GET", "/a/"), b"")
            .await;
        assert_eq!(body_of(&world.http.response(stream).await), b"a");
    }
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
    assert_eq!(
        world.log.lock().unwrap().request_ids(0),
        vec![1, 2, 3, 4],
        "each request gets its own id on the same view"
    );
}

#[tokio::test]
async fn a_body_is_forwarded_frame_by_frame_and_counted() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));
    let body: Vec<u8> = (0..70u32).map(|byte| byte as u8).collect();
    world
        .http
        .send_request(1, head("POST", "/upload"), &body)
        .await;
    assert_eq!(body_of(&world.http.response(1).await), b"ok");

    let event = &world.requests()[0];
    assert_eq!(event.bytes_in(), body.len() as u64);
    assert_eq!(event.bytes_out(), 2);
    assert_eq!(event.outcome(), blastit_events::Outcome::Ok);

    let received = world.log.lock().unwrap().received_by(0);
    let mut forwarded = Vec::new();
    for (_, frame) in received {
        if let Frame::ReqBody(chunk) = frame {
            forwarded.extend_from_slice(&chunk);
        }
    }
    assert_eq!(forwarded, body);
}

#[tokio::test]
async fn a_completed_response_is_never_answered_twice() {
    tokio::time::pause();
    // The view answers but never sends `DONE`, so its thread stays alive. When
    // the requests pass their timeout the scheduler must not write anything
    // more for them: their responses are over, and their outcome is final.
    let mut options = Options::new().script(
        0,
        Script::NoDone {
            body: b"done".to_vec(),
        },
    );
    options.cfg.max_workers = 8;
    options.cfg.stuck_threads = 1;
    options.cfg.request_timeout = Duration::from_secs(30);
    options.cfg.queue_timeout = Duration::from_secs(600);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"done");
    assert_eq!(body_of(&world.http.response(2).await), b"done");

    world.advance(Duration::from_secs(31)).await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    let leftover = world.http.leftover();
    assert!(
        leftover.is_empty(),
        "nothing more may be written: {leftover:?}"
    );

    let events = world.requests();
    assert_eq!(events.len(), 2);
    for event in &events {
        assert_eq!(event.outcome(), Outcome::Ok);
        assert_eq!(event.status(), 200);
    }
    // The threads are still alive, so the view is recycled for stuck threads —
    // that is what the overdue count is for.
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "stuck_threads".to_string())]
    );
}

#[tokio::test]
async fn a_duplicated_done_is_ignored() {
    tokio::time::pause();
    let mut options = Options::new().script(
        0,
        Script::DoubleDone {
            body: b"once".to_vec(),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.queue_size = 1;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"once");

    // The second `DONE` named a request that was already released, so it
    // released nothing: the slot count is still correct, and the next request
    // is served by the same process.
    world.http.send_request(2, head("GET", "/2"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"once");
    world.until(|world| world.requests().len() == 2).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::Ok);
    assert_eq!(world.requests()[1].outcome(), Outcome::Ok);
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
    let leftover = world.http.leftover();
    assert!(leftover.is_empty(), "{leftover:?}");
}

#[tokio::test]
async fn a_late_done_after_a_timeout_releases_the_slot() {
    tokio::time::pause();
    let mut options = Options::new().script(
        0,
        Script::DelayedDone {
            after: Duration::from_secs(20),
        },
    );
    options.cfg.max_workers = 1;
    options.cfg.request_timeout = Duration::from_secs(10);
    options.cfg.queue_timeout = Duration::from_secs(600);
    let mut world = World::start(options);

    // The client gets its `504` at T+10s.
    world.http.send_request(1, head("GET", "/1"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 504);
    assert_eq!(world.requests().len(), 1);
    assert_eq!(world.requests()[0].outcome(), Outcome::Timeout);

    // The thread is alive, so the second request waits for its slot...
    world.http.send_request(2, head("GET", "/2"), b"").await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1]);

    // ... until the thread exits at T+20s, which is what frees it.
    world.advance(Duration::from_secs(15)).await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    // The timed-out request still only has its one, final event.
    assert_eq!(world.requests().len(), 1);
}

#[tokio::test]
async fn an_unexpected_spawned_is_retired() {
    tokio::time::pause();
    let world = World::start(Options::new());
    // init hands the scheduler a process nobody asked for.
    let (for_scheduler, _view_end) = UnixStream::pair().expect("socketpair");
    let fd: OwnedFd = for_scheduler.into_std().unwrap().into();
    world.ctl_send_fd(
        Ctl::Spawned {
            route: 0,
            pid: 4242,
        },
        fd,
    );
    world
        .until(|world| !world.log.lock().unwrap().retires.is_empty())
        .await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "unexpected_spawn".to_string())]
    );
    assert_eq!(world.log.lock().unwrap().spawns.len(), 0);
}

#[tokio::test]
async fn a_reused_stream_id_is_fatal() {
    tokio::time::pause();
    // http's contract: a stream id is not reused while it is in flight.
    let mut world = World::start(Options::new().script(0, Script::Silent));
    world.http.send_head(7, head("GET", "/a")).await;
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;
    world.http.send_head(7, head("GET", "/b")).await;
    let error = world
        .finish()
        .await
        .expect_err("a reused stream id must end the run");
    assert!(
        format!("{error}").contains("reused stream id 7"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn bytes_that_are_not_frames_on_http_chan_are_fatal() {
    tokio::time::pause();
    let mut world = World::start(Options::new());
    // A frame header that claims four gigabytes: the decoder refuses it, and
    // http is blastit's own code, so the run must not paper over it.
    world
        .http
        .send_raw(&[0xff, 0xff, 0xff, 0xff, 0x01, 0, 0, 0, 0])
        .await;
    let error = world.finish().await.expect_err("a decode error is fatal");
    assert!(
        matches!(error, blastit_scheduler::SchedulerError::Frame(_)),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn a_completed_response_then_a_dead_view_frees_its_slot() {
    tokio::time::pause();
    // `NoDone` answers and keeps its thread; when the process dies the worker
    // slot is freed even though no `DONE` was ever sent (§2.4).
    let mut options = Options::new().script(
        0,
        Script::NoDone {
            body: b"done".to_vec(),
        },
    );
    options.cfg.max_workers = 1;
    let mut world = World::start(options);
    world.http.send_request(1, head("GET", "/1"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"done");

    // A second request cannot run: the only slot is held by a thread that is
    // still alive as far as the scheduler knows.
    world.http.send_request(2, head("GET", "/2"), b"").await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(world.log.lock().unwrap().request_ids(0), vec![1]);

    // The process exits; the request that was waiting gets the slot, and a new
    // process answers it.
    world.set_script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    let pid = world.log.lock().unwrap().pids[0].1;
    world.ctl_exit(0, pid);
    let frames = world.http.response(2).await;
    assert_eq!(body_of(&frames), b"ok");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 0]);
}

#[tokio::test]
async fn a_body_for_an_unknown_stream_is_ignored() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    ));
    // No `REQ_HEAD` ever announced stream 9.
    world.http.send(9, &Frame::ReqBody(b"junk".to_vec())).await;
    world.http.send(9, &Frame::ReqEnd).await;
    world.http.send_request(1, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"ok");
    // The junk never reached a view, and never produced a response.
    assert!(
        world
            .log
            .lock()
            .unwrap()
            .received_by(0)
            .iter()
            .all(|(id, _)| *id == 1)
    );
    assert!(world.http.leftover().is_empty());
    assert_eq!(world.requests().len(), 1);
}

#[tokio::test]
async fn a_response_abort_after_the_head_aborts_the_response() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(0, Script::AbortAfterHead));
    world.http.send_request(1, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert!(common::aborted(&frames), "{frames:?}");

    let event = world.requests()[0].clone();
    assert_eq!(event.outcome(), blastit_events::Outcome::ViewDied);
    assert_eq!(event.status(), 200);

    // `DONE` followed the abort, so the slot is free for the next request.
    world.http.send_request(2, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(2).await;
    assert!(common::aborted(&frames), "{frames:?}");
    world.until(|world| world.requests().len() == 2).await;
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
}

#[tokio::test]
async fn a_response_abort_without_a_head_becomes_a_502() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(0, Script::AbortWithoutHead));
    world.http.send_request(1, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 502, "{frames:?}");
    assert!(common::ended(&frames));
    assert_eq!(body_of(&frames), b"bad gateway\n");
    assert_eq!(
        world.requests()[0].outcome(),
        blastit_events::Outcome::ViewDied
    );
    assert_eq!(world.requests()[0].status(), 502);
    // `done` followed, so a second request is served by the same process.
    world.http.send_request(2, head("GET", "/x"), b"").await;
    assert_eq!(status_of(&world.http.awaited_response(2).await), 502);
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
}
