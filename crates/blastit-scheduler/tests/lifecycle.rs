//! View process lifecycle: lazy and eager spawn, retirement, recycling and
//! `DRAIN` (ARCHITECTURE.md §2.4, §4).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use blastit_events::{Outcome, RetireReason};
use blastit_frame::{Ctl, Frame};
use common::{Options, Script, World, body_of, head, route, status_of};
use tokio::sync::Semaphore;

/// A two-route manifest: `/a/` and everything else.
fn two_routes() -> String {
    format!("version = 1\n{}{}", route("a", "/a/"), route("b", ".*"))
}

#[tokio::test]
async fn lazy_spawn_then_dispatch_after_ready() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(
        0,
        Script::Answer {
            body: b"hello".to_vec(),
        },
    ));
    assert!(world.log.lock().unwrap().spawns.is_empty());
    world.http.send_request(7, head("GET", "/x"), b"body").await;
    let frames = world.http.response(7).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"hello");

    // Exactly one process, forked on demand (§2.4).
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
    let received = world.log.lock().unwrap().received_by(0);
    assert!(matches!(received[0].1, Frame::ReqHead(_)));
    assert!(matches!(received[1].1, Frame::ReqBody(ref body) if body == b"body"));
    assert!(matches!(received[2].1, Frame::ReqEnd));
    assert_eq!(world.requests()[0].outcome(), Outcome::Ok);
}

#[tokio::test]
async fn a_preforked_route_is_spawned_at_startup() {
    tokio::time::pause();
    let manifest = format!(
        "version = 1\n{}[[route]]\nname = \"b\"\npath = ['.*']\n",
        "[[route]]\nname = \"hot\"\npath = ['/hot/']\nprefork = true\n"
    );
    let world = World::start(Options::new().manifest(&manifest).script(
        0,
        Script::Answer {
            body: b"hot".to_vec(),
        },
    ));
    world
        .until(|world| !world.log.lock().unwrap().spawns.is_empty())
        .await;
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0],
        "only the preforked route"
    );
}

#[tokio::test]
async fn eager_mode_spawns_every_route_at_startup() {
    tokio::time::pause();
    let mut options = Options::new().manifest(&format!(
        "version = 1\n{}{}{}",
        route("a", "/a/"),
        route("b", "/b/"),
        route("c", ".*")
    ));
    options.cfg.eager_spawn = true;
    let world = World::start(options);
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 3)
        .await;
    let mut spawns = world.log.lock().unwrap().spawns.clone();
    spawns.sort_unstable();
    assert_eq!(spawns, vec![0, 1, 2]);
}

#[tokio::test]
async fn an_idle_view_is_retired_and_respawned_on_demand() {
    tokio::time::pause();
    let mut options = Options::new().script(
        0,
        Script::Answer {
            body: b"hi".to_vec(),
        },
    );
    options.cfg.idle_timeout = Duration::from_secs(60);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"hi");
    assert!(world.log.lock().unwrap().retires.is_empty());

    // Nothing is in flight, so after BLASTIT_VIEW_IDLE_TIMEOUT the process is
    // retired (§2.4).
    world.advance(Duration::from_secs(61)).await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "idle".to_string())]
    );
    let retired = world.retirements();
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].reason(), &RetireReason::Idle);

    // ... and the next request forks a fresh one.
    world.http.send_request(2, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"hi");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 0]);
}

#[tokio::test]
async fn a_view_is_retired_after_its_request_cap() {
    tokio::time::pause();
    let mut options = Options::new().script(
        0,
        Script::Answer {
            body: b"hi".to_vec(),
        },
    );
    options.cfg.max_requests = 2;
    options.cfg.idle_timeout = Duration::from_secs(600);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"hi");
    assert!(world.log.lock().unwrap().retires.is_empty(), "one of two");

    world.http.send_request(2, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"hi");
    // The cap is reached once the second thread is done: retired then, not
    // before (§2.4 keeps dispatching until `DONE`).
    world
        .until(|world| !world.log.lock().unwrap().retires.is_empty())
        .await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "max_requests".to_string())]
    );
    assert_eq!(world.retirements()[0].reason(), &RetireReason::MaxRequests);

    world.http.send_request(3, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(3).await), b"hi");
    assert_eq!(world.log.lock().unwrap().spawns.len(), 2);
}

#[tokio::test]
async fn the_view_cap_retires_the_least_recently_used_idle_view() {
    tokio::time::pause();
    let mut options = Options::new()
        .manifest(&two_routes())
        .script(
            0,
            Script::Answer {
                body: b"a".to_vec(),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"b".to_vec(),
            },
        );
    options.cfg.max_view_processes = Some(1);
    let mut world = World::start(options);

    // Route 0 first, then route 1: the cap can only be met by retiring the
    // idle route 0 (§2.4).
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"b");
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 2)
        .await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "lru_cap".to_string())]
    );
    assert_eq!(world.retirements()[0].reason(), &RetireReason::LruCap);
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1]);

    // Back to route 0: now route 1 is the least recently used idle view.
    world.http.send_request(3, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(3).await), b"a");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1, 0]);
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "lru_cap".to_string()), (1, "lru_cap".to_string())]
    );
}

#[tokio::test]
async fn the_view_cap_waits_when_nothing_is_idle() {
    tokio::time::pause();
    let gate = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .manifest(&two_routes())
        .script(
            0,
            Script::Hold {
                body: b"a".to_vec(),
                gate: gate.clone(),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"b".to_vec(),
            },
        );
    options.cfg.max_view_processes = Some(1);
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;
    // Route 1 cannot be spawned while route 0 is busy, so the request waits in
    // the queue and eventually times out (§2.4).
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    let frames = world.http.awaited_response(2).await;
    assert_eq!(status_of(&frames), 503);
    assert_eq!(world.requests()[0].outcome(), Outcome::QueueTimeout);
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0],
        "no second process"
    );
}

#[tokio::test]
async fn an_unused_preforked_view_is_retired_after_the_idle_timeout() {
    tokio::time::pause();
    let manifest = "version = 1\n[[route]]\nname = \"hot\"\npath = ['/hot/']\nprefork = true\n";
    let mut options = Options::new().manifest(manifest);
    options.cfg.idle_timeout = Duration::from_secs(60);
    let world = World::start(options);
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;

    // Nobody ever asked for it, so it is idle from the moment it is ready. The
    // first advance lets the `SPAWN`/`READY` round trip finish; the second one
    // puts the clock past the idle deadline that starts there.
    world.advance(Duration::from_secs(61)).await;
    world.advance(Duration::from_secs(61)).await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "idle".to_string())]
    );
    assert_eq!(world.retirements()[0].reason(), &RetireReason::Idle);
}

#[tokio::test]
async fn a_view_that_exits_mid_request_is_a_502_and_the_next_one_respawns() {
    tokio::time::pause();
    let mut world = World::start(Options::new().script(0, Script::ExitOnHead));

    world.http.send_request(1, head("GET", "/x"), b"").await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 502);
    assert!(common::ended(&frames));
    world.until(|world| !world.requests().is_empty()).await;
    assert_eq!(world.requests()[0].outcome(), Outcome::ViewDied);
    assert!(
        world.retirements().is_empty(),
        "it was not retired, it died"
    );

    // The route has no process again, so the next request forks one: this time
    // the script answers.
    world.set_script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    world.http.send_request(2, head("GET", "/x"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"ok");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 0]);
}

#[tokio::test]
async fn a_view_that_dies_before_ready_fails_its_waiters_and_backs_off() {
    tokio::time::pause();
    let mut options = Options::new().script(
        0,
        Script::NeverReady {
            after: Duration::from_millis(50),
        },
    );
    options.cfg.spawn_backoff = Duration::from_secs(1);
    options.cfg.queue_timeout = Duration::from_secs(300);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/x"), b"").await;
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 502, "the waiter gets a 502");
    assert_eq!(world.requests()[0].outcome(), Outcome::ViewDied);
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);

    // A second request during the backoff waits instead of spinning up another
    // fork, and is not failed: it is queued, not waiting for a dead process.
    world.set_script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    world.http.send_request(2, head("GET", "/x"), b"").await;
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0],
        "still backing off"
    );

    // After BLASTIT_VIEW_SPAWN's backoff the route is spawned again.
    world.advance(Duration::from_secs(2)).await;
    let frames = world.http.response(2).await;
    assert_eq!(body_of(&frames), b"ok");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 0]);
}

#[tokio::test]
async fn too_many_stuck_threads_recycle_the_view() {
    tokio::time::pause();
    let mut options = Options::new().script(0, Script::Silent);
    options.cfg.max_workers = 3;
    options.cfg.stuck_threads = 1;
    options.cfg.request_timeout = Duration::from_secs(30);
    options.cfg.queue_timeout = Duration::from_secs(300);
    let mut world = World::start(options);

    // A at T0, B at T+5s, C at T+20s: A and B expire together at T+35s, which
    // is one overdue thread too many, while C is still within its timeout.
    //
    // Each dispatch is confirmed before the clock moves, so the virtual time a
    // request lands at is exactly the one this test intends.
    world.http.send_request(1, head("GET", "/a"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    world.advance(Duration::from_secs(5)).await;

    world.http.send_request(2, head("GET", "/b"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    world.advance(Duration::from_secs(15)).await;

    world.http.send_request(3, head("GET", "/c"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 3)
        .await;
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
    world.advance(Duration::from_secs(15)).await;

    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 504);
    assert_eq!(world.requests()[0].outcome(), Outcome::Timeout);
    let second = world.http.response(2).await;
    assert_eq!(status_of(&second), 504);
    // C was in flight when the view was recycled: a 502 (§2.4).
    let third = world.http.response(3).await;
    assert_eq!(status_of(&third), 502);
    assert_eq!(world.requests()[2].outcome(), Outcome::ViewDied);
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "stuck_threads".to_string())]
    );
    assert_eq!(world.retirements()[0].reason(), &RetireReason::StuckThreads);

    // init reaps the process, which frees every slot the view held; the next
    // request forks a new process.
    world.until(|world| world.requests().len() == 3).await;
    world.set_script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    world.http.send_request(4, head("GET", "/d"), b"").await;
    assert_eq!(body_of(&world.http.response(4).await), b"ok");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 0]);
}

#[tokio::test]
async fn drain_finishes_the_in_flight_request_then_returns() {
    tokio::time::pause();
    let gate = Arc::new(Semaphore::new(0));
    let mut world = World::start(Options::new().script(
        0,
        Script::Hold {
            body: b"slow".to_vec(),
            gate: gate.clone(),
        },
    ));

    world.http.send_request(1, head("GET", "/x"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;
    world.ctl_send(Ctl::Drain);

    // New work is refused while draining: no admission, no new forks.
    world.http.send_request(2, head("GET", "/x"), b"").await;
    let refused = world.http.response(2).await;
    assert_eq!(status_of(&refused), 503);
    assert_eq!(world.requests()[0].outcome(), Outcome::QueueFull);
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);

    // The in-flight request still finishes, and then the run is over.
    gate.add_permits(1);
    let frames = world.http.response(1).await;
    assert_eq!(body_of(&frames), b"slow");
    world.finish().await.unwrap();
}

#[tokio::test]
async fn drain_gives_up_after_the_request_timeout() {
    tokio::time::pause();
    let mut options = Options::new().script(0, Script::Silent);
    options.cfg.request_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/x"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;
    world.ctl_send(Ctl::Drain);
    // The stuck request gets its 504 at its own deadline; its slot is never
    // released, so the drain deadline is what ends the run.
    world.finish().await.unwrap();
    let requests = world.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].status(), 504);
    assert_eq!(requests[0].outcome(), Outcome::Timeout);
}

#[tokio::test]
async fn closing_http_chan_ends_the_run() {
    tokio::time::pause();
    let mut world = World::start(Options::new());
    world.http.send_request(1, head("GET", "/x"), b"").await;
    assert_eq!(status_of(&world.http.response(1).await), 200);
    world.http.close().await;
    world.finish().await.unwrap();
}

#[tokio::test]
async fn an_unexpected_ctl_message_fails_the_run() {
    tokio::time::pause();
    let mut world = World::start(Options::new());
    world.ctl_send(Ctl::Spawn { route: 0 });
    let error = world
        .finish()
        .await
        .expect_err("the scheduler must refuse a SPAWN from init");
    assert!(
        format!("{error}").contains("unexpected SPAWN"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn a_cap_of_two_serves_three_routes_without_lru_churn() {
    tokio::time::pause();
    // Three routes, two processes: the third route's request can only be
    // served by retiring one of the first two, and all three must be answered.
    let manifest = format!(
        "version = 1\n{}{}{}",
        route("a", "/a/"),
        route("b", "/b/"),
        route("c", "/c/")
    );
    let mut options = Options::new()
        .manifest(&manifest)
        .script(
            0,
            Script::Answer {
                body: b"a".to_vec(),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"b".to_vec(),
            },
        )
        .script(
            2,
            Script::Answer {
                body: b"c".to_vec(),
            },
        );
    options.cfg.max_view_processes = Some(2);
    options.cfg.queue_timeout = Duration::from_secs(60);
    let mut world = World::start(options);

    // The three requests arrive together, before any process is ready. The
    // picker used to hand the cap a process that had been forked for one of
    // them but had not answered yet, which turned into thousands of
    // spawn/retire cycles a second and three `503`s.
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    world.http.send_request(3, head("GET", "/c/"), b"").await;

    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200);
    assert_eq!(body_of(&first), b"a");
    let second = world.http.response(2).await;
    assert_eq!(status_of(&second), 200);
    assert_eq!(body_of(&second), b"b");
    let third = world.http.response(3).await;
    assert_eq!(status_of(&third), 200, "the third route waited its turn");
    assert_eq!(body_of(&third), b"c");

    // Served sequentially behind a cap of two: a handful of forks, not a storm.
    let log = world.log.lock().unwrap();
    assert!(
        log.spawns.len() <= 4,
        "bounded spawns, got {:?}",
        log.spawns
    );
    assert!(
        log.retires.len() <= 2,
        "bounded retires, got {:?}",
        log.retires
    );
    assert!(
        log.retires.iter().all(|(_, reason)| reason == "lru_cap"),
        "only the cap retires here, got {:?}",
        log.retires
    );
    drop(log);
    assert!(
        world
            .requests()
            .iter()
            .all(|request| request.status() == 200),
        "no request was refused"
    );
}

#[tokio::test]
async fn a_view_that_is_not_ready_yet_is_never_a_victim() {
    tokio::time::pause();
    // Route 0's process is forked and never says `READY`. The cap of one is
    // full, but a process that has not served anything is not a candidate: the
    // request for route 1 waits for the queue timeout instead of killing it.
    let mut options = Options::new()
        .manifest(&two_routes())
        .script(
            0,
            Script::NeverReady {
                after: Duration::from_secs(3600),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"b".to_vec(),
            },
        );
    options.cfg.max_view_processes = Some(1);
    options.cfg.queue_timeout = Duration::from_secs(5);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    let frames = world.http.awaited_response(2).await;
    assert_eq!(status_of(&frames), 503);
    assert_eq!(world.requests()[0].outcome(), Outcome::QueueTimeout);
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0],
        "the not-ready process was never retired for a second one"
    );
    let retires = world.log.lock().unwrap().retires.clone();
    assert!(retires.is_empty(), "nothing was retired: {retires:?}");
}

#[tokio::test]
async fn a_view_whose_route_is_waiting_is_never_a_victim() {
    tokio::time::pause();
    // Route 0's process has served its one request (`max_requests = 1`, so it
    // is `stopping`) and route 0 has another request queued behind it. With the
    // cap of one full, route 1's request must not retire that process: it is
    // the only thing that can still answer route 0.
    let mut options = Options::new()
        .manifest(&two_routes())
        .script(
            0,
            Script::Hold {
                body: b"a".to_vec(),
                gate: Arc::new(Semaphore::new(0)),
            },
        )
        .script(
            1,
            Script::Answer {
                body: b"b".to_vec(),
            },
        );
    options.cfg.max_workers = 4;
    options.cfg.max_requests = 1;
    options.cfg.max_view_processes = Some(1);
    options.cfg.queue_timeout = Duration::from_secs(3600);
    let gate = match &options.scripts.lock().unwrap()[&0] {
        Script::Hold { gate, .. } => gate.clone(),
        _ => unreachable!("route 0 holds"),
    };
    let mut world = World::start(options);

    world
        .http
        .send_request(1, head("GET", "/a/"), b"body")
        .await;
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;
    // Both of these wait: route 0 is at its request cap, and route 1 has no
    // process and no room for one.
    world.http.send_request(2, head("GET", "/a/"), b"").await;
    world.http.send_request(3, head("GET", "/b/"), b"").await;
    gate.add_permits(2);

    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200);
    assert_eq!(body_of(&first), b"a");
    let second = world.http.response(2).await;
    assert_eq!(status_of(&second), 200, "route 0 was not starved");
    assert_eq!(body_of(&second), b"a");
    let third = world.http.response(3).await;
    assert_eq!(status_of(&third), 200);
    assert_eq!(body_of(&third), b"b");

    // The first retirement is the request cap, not the LRU picker reaching for
    // a process that still owed route 0 an answer.
    assert_eq!(
        world.log.lock().unwrap().retires[0],
        (0, "max_requests".to_string())
    );
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0, 0, 1],
        "route 0 was respawned for its own queued request before route 1's"
    );
}
