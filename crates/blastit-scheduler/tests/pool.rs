//! A route's pool of view processes: how many forks it costs, which worker a
//! request goes to, and what a reload does to it (`ADR-0015`).
//!
//! Every test here drives the real scheduler over the fake init and the fake
//! `view.chan` tasks, so the assertions are about `SPAWN`/`SPAWNED`/`READY` and
//! the frames a worker saw — never about internals the code could fake.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use blastit_events::EventKind;
use blastit_frame::{Ctl, Frame};
use common::{Options, Script, World, body_of, head, route, status_of};
use tokio::sync::Semaphore;

/// Two routes: `a` (index 0) and `b` (index 1).
fn two_routes() -> String {
    format!("{}{}", route("a", "/a/"), route("b", "/b/"))
}

/// The `manifest.warning` sentences the scheduler emitted.
fn warnings(world: &World) -> Vec<String> {
    world
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event.kind() {
            EventKind::ManifestWarning(warning) => Some(warning.message().to_owned()),
            _ => None,
        })
        .collect()
}

/// R1: a route whose provider runs one request per process forks one process
/// per concurrent request, and each request is answered by the process it was
/// dispatched to.
#[tokio::test]
async fn per_process_cap_one_forks_per_request() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut world = World::start(
        Options::new()
            .blastfile(&two_routes())
            .cap_route(0, 1)
            .pool_route(0, true)
            .script_for_spawn(
                0,
                0,
                Script::Hold {
                    body: b"first".to_vec(),
                    gate: hold.clone(),
                },
            )
            .script_for_spawn(
                0,
                1,
                Script::Answer {
                    body: b"second".to_vec(),
                },
            ),
    );

    // The first request occupies the only slot of worker A, so the second one
    // cannot be served by it: it has to fork a sibling.
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world.http.send_request(2, head("GET", "/a/"), b"").await;

    let second = world.http.response(2).await;
    assert_eq!(status_of(&second), 200, "{second:?}");
    assert_eq!(
        body_of(&second),
        b"second",
        "the second request is answered by the sibling that was forked for it"
    );

    let (first_pid, second_pid) = {
        let log = world.log.lock().unwrap();
        assert_eq!(log.spawns, vec![0, 0], "one fork per concurrent request");
        assert_eq!(log.pids.len(), 2);
        let first_pid = log.pids[0].1;
        let second_pid = log.pids[1].1;
        // Each process was given exactly one request, with the id it started
        // from: a per-process id space, not a route-wide one.
        assert_eq!(log.request_ids_of(first_pid), vec![1]);
        assert_eq!(log.request_ids_of(second_pid), vec![1]);
        (first_pid, second_pid)
    };
    assert_ne!(first_pid, second_pid);

    // Worker A finally answers, with its own body: the two workers were never
    // interchangeable for attribution.
    hold.add_permits(1);
    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200, "{first:?}");
    assert_eq!(body_of(&first), b"first");
    assert!(world.http.leftover().is_empty());
}

/// R1's negative half: the threaded shape serves the same load from **one**
/// process, whatever `per_process_cap` would allow.
#[tokio::test]
async fn a_threaded_route_serves_both_requests_from_one_process() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut world = World::start(
        Options::new()
            .blastfile(&two_routes())
            .cap_route(0, 1)
            .pool_route(0, false)
            .script(
                0,
                Script::Hold {
                    body: b"both".to_vec(),
                    gate: hold.clone(),
                },
            ),
    );

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world.http.send_request(2, head("GET", "/a/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 1)
        .await;
    {
        let log = world.log.lock().unwrap();
        assert_eq!(
            log.spawns,
            vec![0],
            "a threaded route never forks a sibling"
        );
        assert_eq!(log.pids.len(), 1);
    }
    hold.add_permits(2);
    for stream in 1..=2u32 {
        assert_eq!(status_of(&world.http.response(stream).await), 200);
    }
    assert_eq!(world.log.lock().unwrap().pids.len(), 1);
}

/// R2: the worker a request goes to is the least-recently-used one that admits
/// it — and one at its ceiling is skipped even when it is the coldest.
#[tokio::test]
async fn lru_picks_the_coldest_admitting_worker() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut world = World::start(
        Options::new()
            .blastfile(&two_routes())
            .cap_route(0, 1)
            .pool_route(0, true)
            .script(
                0,
                Script::Hold {
                    body: b"held".to_vec(),
                    gate: hold.clone(),
                },
            ),
    );

    // Two concurrent requests fill one process each, so the route holds two
    // workers: the first request lands on the first fork (the pid tie-break of
    // a cold pool), the second one on the sibling forked for it.
    for stream in 1..=2u32 {
        world
            .http
            .send_request(stream, head("GET", "/a/"), b"")
            .await;
    }
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 2)
        .await;
    let pids: Vec<u32> = world
        .log
        .lock()
        .unwrap()
        .pids
        .iter()
        .map(|(_, pid)| *pid)
        .collect();
    assert_eq!(pids.len(), 2, "one worker per concurrent request: {pids:?}");
    let first = pids[0];
    let second = pids[1];
    world
        .until(|world| world.log.lock().unwrap().request_ids(0).len() == 2)
        .await;
    {
        let log = world.log.lock().unwrap();
        assert_eq!(
            log.request_ids_of(first),
            vec![1],
            "the first request went to the first worker"
        );
        assert_eq!(
            log.request_ids_of(second),
            vec![1],
            "the second request went to the sibling"
        );
    }

    // Both requests finish, so both workers admit again. They were used in
    // order, so the first one has waited longest: the next request must go to
    // it and not to the one used most recently.
    hold.add_permits(2);
    for stream in 1..=2u32 {
        assert_eq!(status_of(&world.http.response(stream).await), 200);
    }
    world.http.send_request(3, head("GET", "/a/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids_of(first).len() == 2)
        .await;
    assert_eq!(
        world.log.lock().unwrap().request_ids_of(first),
        vec![1, 2],
        "the coldest admitting worker took the third request"
    );

    // Negative half: that worker is at its ceiling again, so the fourth
    // request is never handed to it — the sibling takes it, no fork needed.
    world.http.send_request(4, head("GET", "/a/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids_of(second).len() == 2)
        .await;
    {
        let log = world.log.lock().unwrap();
        assert_eq!(
            log.request_ids_of(first),
            vec![1, 2],
            "a worker at its ceiling is skipped even though it is the coldest"
        );
        assert_eq!(log.pids.len(), 2, "no third process was needed");
    }

    hold.add_permits(8);
    for stream in 3..=4u32 {
        assert_eq!(status_of(&world.http.response(stream).await), 200);
    }
}
/// R15: siblings never answer each other's requests — a frame under a foreign
/// id is dropped, and the sibling's own stream still completes.
#[tokio::test]
async fn siblings_never_answer_each_others_requests() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut world = World::start(
        Options::new()
            .cap_route(0, 1)
            .pool_route(0, true)
            .script_for_spawn(
                0,
                0,
                Script::Hold {
                    body: b"b".to_vec(),
                    gate: hold.clone(),
                },
            )
            .script_for_spawn(
                0,
                1,
                Script::ForeignId {
                    body: b"a".to_vec(),
                },
            ),
    );

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 2)
        .await;

    // The sibling that writes a frame under a foreign id must not reach the
    // first worker's stream, which is still open.
    let second = world.http.response(2).await;
    assert_eq!(status_of(&second), 200, "{second:?}");
    assert_eq!(body_of(&second), b"a");
    assert_eq!(second.len(), 3, "only head, body and end: {second:?}");
    hold.add_permits(1);
    let first = world.http.response(1).await;
    assert_eq!(status_of(&first), 200, "{first:?}");
    assert_eq!(body_of(&first), b"b");
    assert_eq!(first.len(), 3, "{first:?}");
    assert!(world.http.leftover().is_empty());
    assert_eq!(world.requests().len(), 2);
}

/// The measured regression `MB.26.8` pins: requests queued behind a
/// request-per-process worker that retires must be served by a fresh worker,
/// never failed with `502 view_died`.
#[tokio::test]
async fn a_queued_request_survives_the_worker_it_queued_behind() {
    tokio::time::pause();
    let mut options = Options::new().cap_route(0, 1).pool_route(0, true).script(
        0,
        Script::Answer {
            body: b"ok".to_vec(),
        },
    );
    options.cfg.max_workers = 8;
    options.cfg.max_requests = 1;
    options.cfg.queue_size = 16;
    let mut world = World::start(options);

    // Eight concurrent requests behind one one-slot route: each worker serves
    // one and retires, and every request must still be answered.
    for stream in 1..=8u32 {
        world.http.send_request(stream, head("GET", "/"), b"").await;
    }
    for stream in 1..=8u32 {
        let frames = world.http.awaited_response(stream).await;
        assert_eq!(
            status_of(&frames),
            200,
            "stream {stream} was failed instead of served: {frames:?}"
        );
        assert_eq!(body_of(&frames), b"ok");
    }
    let events = world.requests();
    assert_eq!(events.len(), 8);
    assert!(
        events
            .iter()
            .all(|event| event.outcome() == blastit_events::Outcome::Ok),
        "no request died with its worker"
    );
    let log = world.log.lock().unwrap();
    assert!(
        log.pids.len() >= 2,
        "the queue was served by the workers that replaced the retired ones"
    );
    assert!(
        log.retired_pids_of(0)
            .iter()
            .all(|(_, reason)| reason == "max_requests"),
        "{:?}",
        log.retired_pids
    );
}

/// R3: one `SPAWN` in flight per route, and — until the first memory
/// measurement — growth waits for the fork it already sent to be `READY`.
#[tokio::test]
async fn one_fork_at_a_time() {
    tokio::time::pause();
    let park = Arc::new(Semaphore::new(0));
    let hold = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .blastfile(&two_routes())
        .cap_route(0, 1)
        .pool_route(0, true)
        .script(
            0,
            Script::Hold {
                body: b"ok".to_vec(),
                gate: hold.clone(),
            },
        )
        .park_spawned(park.clone());
    options.cfg.queue_size = 16;
    let mut world = World::start(options);

    // Four cold requests: one fork, because the route may only have one in
    // flight. No memory probe is attached, so the derived cap is unmeasured and
    // growth also waits for each `READY`.
    for stream in 1..=4u32 {
        world
            .http
            .send_request(stream, head("GET", "/a/"), b"")
            .await;
    }
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 1)
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.log.lock().unwrap().spawns.len(),
        1,
        "a second SPAWN must wait for the first one's answer"
    );
    assert!(world.log.lock().unwrap().spawned_sent.is_empty(), "parked");

    // Each delivered `SPAWNED` is a `READY` (the worker is not parked), and
    // only then does the next fork go out: one fork per `READY`, four in all.
    for expected in 2..=4usize {
        park.add_permits(1);
        world
            .until(|world| world.log.lock().unwrap().spawns.len() == expected)
            .await;
        assert!(
            world.log.lock().unwrap().spawned_sent.len() < expected,
            "spawned_sent must never run ahead of the forks"
        );
    }
    park.add_permits(4);
    world
        .until(|world| world.log.lock().unwrap().spawned_sent.len() == 4)
        .await;
    assert_eq!(world.log.lock().unwrap().pids.len(), 4);

    hold.add_permits(4);
    for stream in 1..=4u32 {
        assert_eq!(status_of(&world.http.response(stream).await), 200);
    }
}

/// R3's other half: the fork that was already reported still counts as a live
/// heap copy, so growth waits for its `READY` — the cap is re-derived between
/// two forks instead of being priced from the estimate it understates.
#[tokio::test]
async fn a_second_fork_waits_for_the_first_ready() {
    tokio::time::pause();
    let mut options = Options::new().cap_route(0, 1).pool_route(0, true).script(
        0,
        Script::ReadyAfter {
            after: Duration::from_millis(5),
        },
    );
    options.cfg.max_workers = 4;
    options.cfg.queue_size = 16;
    let mut world = World::start(options);

    // `SPAWNED` arrives straight away, so the route's spawn slot is free — but
    // the worker has not sent `READY`, so it is still an unmeasured copy.
    world.http.send_request(1, head("GET", "/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawned_sent.len() == 1)
        .await;
    world.http.send_request(2, head("GET", "/"), b"").await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.log.lock().unwrap().spawns.len(),
        1,
        "growth waits for the fork already promised"
    );

    // Its `READY` releases the gate, and the queued request's fork goes out.
    world.advance(Duration::from_millis(10)).await;
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 2)
        .await;
}

/// R4: a `SPAWNED` with nothing outstanding names a process nobody wanted, so
/// it is retired rather than adopted.
#[tokio::test]
async fn spawned_consumes_the_slot() {
    tokio::time::pause();
    let world = World::start(Options::new());
    let (for_scheduler, _view_end) = tokio::net::UnixStream::pair().expect("socketpair");
    let fd: std::os::fd::OwnedFd = for_scheduler.into_std().unwrap().into();
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
        world.log.lock().unwrap().retired_pids_of(0),
        vec![(4242, "unexpected_spawn".to_string())]
    );
    assert!(world.log.lock().unwrap().spawns.is_empty());
    assert!(world.requests().is_empty());
}

/// R4, the colliding half: two processes must never share one `ViewKey`, so a
/// `SPAWNED` naming a pid the pool holds retires the held entry (failing its
/// request) and the newcomer is killed by the same message; the route still
/// grows a fresh worker afterwards.
#[tokio::test]
async fn a_colliding_spawned_retires_the_held_pid() {
    tokio::time::pause();
    let park = Arc::new(Semaphore::new(0));
    let hold = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .cap_route(0, 1)
        .pool_route(0, true)
        .script(
            0,
            Script::Hold {
                body: b"held".to_vec(),
                gate: hold.clone(),
            },
        )
        .park_spawned(park.clone());
    options.cfg.queue_size = 8;
    let mut world = World::start(options);

    // One worker exists and holds the request it was forked for.
    world.http.send_request(1, head("GET", "/1"), b"").await;
    park.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 1)
        .await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0) == vec![1])
        .await;
    let held_pid = world.log.lock().unwrap().pids[0].1;

    // A second request makes the route's one outstanding SPAWN, which the fake
    // init parks: the slot is open while that newcomer is still owed.
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 2)
        .await;

    // init answers that SPAWN with the pid the pool already holds.
    let (for_scheduler, _newcomer) = tokio::net::UnixStream::pair().expect("socketpair");
    let fd: std::os::fd::OwnedFd = for_scheduler.into_std().unwrap().into();
    world.ctl_send_fd(
        Ctl::Spawned {
            route: 0,
            pid: held_pid,
        },
        fd,
    );

    // The dead entry's request is failed with a `502`: its process is gone.
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 502, "{frames:?}");
    world
        .until(|world| !world.log.lock().unwrap().retired_pids_of(0).is_empty())
        .await;
    assert_eq!(
        world.log.lock().unwrap().retired_pids_of(0),
        vec![(held_pid, "duplicate_pid".to_string())],
        "the newcomer owns that pid now, so the one message retires it"
    );

    // The slot was consumed by that answer, so the route grows again for the
    // request that is still queued, and the worker the queue gets is a real
    // one: a fresh pid, never the retired entry's.
    park.add_permits(8);
    world
        .until(|world| world.log.lock().unwrap().pids.len() >= 3)
        .await;
    hold.add_permits(8);
    let frames = world.http.response(2).await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
    assert_eq!(
        world.log.lock().unwrap().request_ids_of(held_pid),
        vec![1],
        "the retired pid can never be given another request"
    );
}

/// R5: the memory-derived cap counts live **processes** — the fork in flight
/// R5: the memory-derived cap counts live **processes**, the fork each route was
/// already promised included.
///
/// The shape is the **boot burst** ([`Scheduler::start`]'s eager spawn), not a
/// request-driven one: at boot every eager route has its own `SPAWN` in flight
/// at once, so only the cap can bound the fleet — a request-driven burst is
/// bounded by its route's single slot instead (`one_fork_at_a_time`). Dropping
/// the promised fork from the count would let all four routes fork here.
#[tokio::test]
async fn growth_counts_a_promise_not_a_process() {
    tokio::time::pause();
    let park = Arc::new(Semaphore::new(0));
    let manifest = format!(
        "{}{}{}{}",
        route("a", "/a/"),
        route("b", "/b/"),
        route("c", "/c/"),
        route("d", "/d/")
    );
    let mut options = Options::new()
        .blastfile(&manifest)
        .park_spawned(park.clone());
    options.cfg.max_workers = 4;
    options.cfg.max_view_processes = Some(2);
    options.cfg.eager_spawn = true;
    let world = World::start(options);

    // `start` asks for one process per eager route in one go. Two promises fill
    // the cap, so two `SPAWN`s may go out and no more: each promise is a live
    // process before its `SPAWNED` arrives.
    for _ in 0..128 {
        tokio::task::yield_now().await;
    }
    let spawns = world.log.lock().unwrap().spawns.clone();
    assert_eq!(
        spawns,
        vec![0, 1],
        "the cap counts the promised forks, so only two routes may fork: {spawns:?}"
    );

    // Delivering them keeps it at two: they *are* the two processes the cap
    // allows, so nothing else is forked for the routes that came after.
    park.add_permits(4);
    world
        .until(|world| world.log.lock().unwrap().spawned_sent.len() == 2)
        .await;
    for _ in 0..128 {
        tokio::task::yield_now().await;
    }
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1]);
}

/// R6: a route that has requests and **no process at all** is served before a
/// neighbour grows. The neighbour here is the process cap: route 0's first
/// process does not fit yet, so the room that frees up must go to it and not to
/// the busy route's third worker.
#[tokio::test]
async fn growth_yields_to_a_starved_route() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .blastfile(&two_routes())
        .cap_route(0, 1)
        .pool_route(0, true)
        .cap_route(1, 1)
        .pool_route(1, true)
        .script(
            1,
            Script::Hold {
                body: b"b".to_vec(),
                gate: hold.clone(),
            },
        );
    options.cfg.max_workers = 4;
    options.cfg.max_view_processes = Some(2);
    options.cfg.queue_size = 16;
    let mut world = World::start(options);

    // Route 1 fills the whole process cap with two busy workers.
    world.http.send_request(1, head("GET", "/b/"), b"").await;
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(1).len() == 2)
        .await;
    assert_eq!(world.log.lock().unwrap().spawns, vec![1, 1]);
    // One more request for each route. Route 0 cannot be forked: the cap is
    // full of route 1's workers, and neither is idle.
    world.http.send_request(3, head("GET", "/b/"), b"").await;
    world.http.send_request(4, head("GET", "/a/"), b"").await;
    for _ in 0..128 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![1, 1],
        "a full cap forks nobody"
    );

    // One of route 1's processes goes away, so the cap has room for exactly one
    // more. The room must go to the route that has no process at all.
    let second = world.log.lock().unwrap().pids[1].1;
    world.ctl_exit(1, second);
    world
        .until(|world| world.log.lock().unwrap().spawns.contains(&0))
        .await;
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![1, 1, 0],
        "the starved route gets the freed process, not the busy one"
    );
}

/// R6's tie-break: two cold routes never defer to each other for ever. The
/// lowest-indexed one forks first, and the other forks straight after it.
#[tokio::test]
async fn two_cold_routes_each_get_their_first_process() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let manifest = format!(
        "{}{}{}",
        route("a", "/a/"),
        route("b", "/b/"),
        route("c", "/c/")
    );
    let mut options = Options::new()
        .blastfile(&manifest)
        .cap_route(2, 1)
        .pool_route(2, true)
        .script(
            2,
            Script::Hold {
                body: b"c".to_vec(),
                gate: hold.clone(),
            },
        );
    options.cfg.max_workers = 2;
    options.cfg.queue_size = 16;
    let mut world = World::start(options);

    // Route 2 holds both admission slots, so the two cold routes' requests
    // queue up instead of being visited one at a time.
    world.http.send_request(1, head("GET", "/c/"), b"").await;
    world.http.send_request(2, head("GET", "/c/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(2).len() == 2)
        .await;
    world.http.send_request(3, head("GET", "/a/"), b"").await;
    world.http.send_request(4, head("GET", "/b/"), b"").await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(world.log.lock().unwrap().spawns, vec![2, 2]);

    // Both slots free at once. Neither cold route may wait for the other.
    hold.add_permits(2);
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 4)
        .await;
    let spawns = world.log.lock().unwrap().spawns.clone();
    assert_eq!(
        &spawns[..2],
        &[2, 2],
        "the routes that already had workers are untouched"
    );
    assert!(
        spawns[2..].contains(&0) && spawns[2..].contains(&1),
        "both cold routes got their first process: {spawns:?}"
    );
}
/// R8: a reload retires every idle worker of every route, lets an in-flight one
/// finish and retires it at `DONE`, and sends nothing new to a stale worker.
#[tokio::test]
async fn reload_retires_every_idle_worker_of_every_route() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut world = World::start(
        Options::new()
            .blastfile(&two_routes())
            .cap_route(0, 1)
            .pool_route(0, true)
            .cap_route(1, 1)
            .pool_route(1, true)
            .script(
                0,
                Script::Answer {
                    body: b"a".to_vec(),
                },
            )
            .script(
                1,
                Script::Hold {
                    body: b"b".to_vec(),
                    gate: hold.clone(),
                },
            ),
    );

    // Route 0 is idle after one request; route 1 is busy with one.
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(1) == vec![1])
        .await;
    let idle_pid = world.log.lock().unwrap().pids[0].1;
    let busy_pid = world.log.lock().unwrap().pids[1].1;

    world.ctl_send(Ctl::Reload);
    // The idle worker goes at once; the busy one is stale, not killed.
    world
        .until(|world| {
            world.log.lock().unwrap().retired_pids_of(0) == vec![(idle_pid, "reload".to_string())]
        })
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(
        world.log.lock().unwrap().retired_pids_of(1).is_empty(),
        "an in-flight worker finishes its request first"
    );

    // Nothing new reaches the stale worker: a fresh request forks a sibling.
    world.http.send_request(3, head("GET", "/b/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 3)
        .await;
    let fresh_pid = world.log.lock().unwrap().pids[2].1;
    assert_ne!(fresh_pid, busy_pid, "a stale worker takes no new request");
    assert_eq!(
        world.log.lock().unwrap().request_ids_of(busy_pid),
        vec![1],
        "the stale worker never saw a second request"
    );

    // Its request ends, and then it retires with the reload's reason.
    hold.add_permits(1);
    assert_eq!(status_of(&world.http.response(2).await), 200);
    world
        .until(|world| {
            world
                .log
                .lock()
                .unwrap()
                .retired_pids_of(1)
                .contains(&(busy_pid, "reload".to_string()))
        })
        .await;
}

/// R8, the birth half: a worker forked for a `SPAWN` sent before the reload is
/// retired on arrival, and its release lets the route grow again.
#[tokio::test]
async fn a_spawn_sent_before_a_reload_is_retired_on_arrival() {
    tokio::time::pause();
    let park = Arc::new(Semaphore::new(0));
    let hold = Arc::new(Semaphore::new(0));
    let mut world = World::start(
        Options::new()
            .cap_route(0, 1)
            .pool_route(0, true)
            .script(
                0,
                Script::Hold {
                    body: b"ok".to_vec(),
                    gate: hold.clone(),
                },
            )
            .park_spawned(park.clone()),
    );

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 1)
        .await;
    // The fork init made predates the reload: it holds the old code.
    world.ctl_send(Ctl::Reload);
    park.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().retired_pids.len() == 1)
        .await;
    let (pid, reason) = {
        let log = world.log.lock().unwrap();
        (log.retired_pids[0].1, log.retired_pids[0].2.clone())
    };
    assert_eq!(reason, "reload");
    assert!(
        world.log.lock().unwrap().request_ids(0).is_empty(),
        "a stale-at-birth worker never serves"
    );

    // Its `pending_ready` was released, so the route forks again instead of
    // waiting for a heap copy that no longer exists.
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 2)
        .await;
    park.add_permits(1);
    world
        .until(|world| world.log.lock().unwrap().request_ids(0) == vec![1])
        .await;
    {
        let log = world.log.lock().unwrap();
        assert_eq!(log.pids.len(), 2);
        assert_ne!(log.pids[1].1, pid, "a fresh worker serves the request");
    }
    hold.add_permits(1);
    assert_eq!(status_of(&world.http.response(1).await), 200);
}

/// R9: a fork that never reaches `READY` costs its route one backoff and
/// touches nothing else — no sibling is retired, no request is failed.
#[tokio::test]
async fn a_failed_spawn_backs_off_growth_only() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut options = Options::new()
        .cap_route(0, 1)
        .pool_route(0, true)
        .script_for_spawn(
            0,
            0,
            Script::Hold {
                body: b"served".to_vec(),
                gate: hold.clone(),
            },
        )
        .script_for_spawn(
            0,
            1,
            Script::NeverReady {
                after: Duration::from_millis(1),
            },
        );
    options.cfg.queue_size = 8;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().request_ids(0) == vec![1])
        .await;
    // The second request asks for a sibling, and that fork dies before READY.
    world.http.send_request(2, head("GET", "/2"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 2)
        .await;
    // `NeverReady` closes after its own delay, which is a timer.
    world.advance(Duration::from_millis(2)).await;
    world
        .until(|world| !world.log.lock().unwrap().closed_by_process.is_empty())
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert!(
        world.log.lock().unwrap().retires.is_empty(),
        "a failed fork retires no sibling: {:?}",
        world.log.lock().unwrap().retires
    );
    assert!(
        world.requests().iter().all(|event| event.status() != 502),
        "the queue is not failed while a sibling can serve it"
    );

    // Growth is cooled for exactly one backoff, then the queued request gets
    // its worker.
    assert_eq!(world.log.lock().unwrap().spawns.len(), 2);
    world.advance(Duration::from_millis(1500)).await;
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 3)
        .await;
    hold.add_permits(2);
    assert_eq!(status_of(&world.http.response(1).await), 200);
    assert_eq!(status_of(&world.http.response(2).await), 200);
}

/// R9, the refused half: a `SPAWN` init answers with `pid: 0` frees the slot,
/// warns once, and the route grows again after its backoff.
#[tokio::test]
async fn a_refused_spawn_is_one_backoff() {
    tokio::time::pause();
    let park = Arc::new(Semaphore::new(0));
    let hold = Arc::new(Semaphore::new(0));
    let mut world = World::start(
        Options::new()
            .cap_route(0, 1)
            .pool_route(0, true)
            .script(
                0,
                Script::Hold {
                    body: b"ok".to_vec(),
                    gate: hold.clone(),
                },
            )
            .park_spawned(park.clone()),
    );

    world.http.send_request(1, head("GET", "/1"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 1)
        .await;
    // init could not fork: no process exists for that SPAWN.
    world.ctl_send(Ctl::Exited {
        route: 0,
        pid: 0,
        status: blastit_frame::ExitStatus::Code(1),
    });
    world.until(|world| !warnings(world).is_empty()).await;
    assert_eq!(warnings(&world).len(), 1, "{:?}", warnings(&world));
    assert!(world.log.lock().unwrap().retires.is_empty());

    // The slot is free, but the route is cooled: no fork for one backoff.
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(world.log.lock().unwrap().spawns.len(), 1);
    world.advance(Duration::from_millis(1500)).await;
    world
        .until(|world| world.log.lock().unwrap().spawns.len() == 2)
        .await;

    // The parked `SPAWNED` is now unexpected; the request is answered by the
    // worker the retry forked.
    park.add_permits(2);
    hold.add_permits(1);
    let frames = world.http.awaited_response(1).await;
    assert_eq!(status_of(&frames), 200, "{frames:?}");
}

/// R11: a request-per-process route never puts two requests into one process,
/// whatever the load.
#[tokio::test]
async fn a_php_route_never_threads() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut options = Options::new().cap_route(0, 1).pool_route(0, true).script(
        0,
        Script::Hold {
            body: b"ok".to_vec(),
            gate: hold.clone(),
        },
    );
    options.cfg.max_workers = 4;
    let mut world = World::start(options);

    for stream in 1..=3u32 {
        world.http.send_request(stream, head("GET", "/"), b"").await;
    }
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 3)
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    {
        let log = world.log.lock().unwrap();
        assert_eq!(log.spawns.len(), 3, "one process per request");
        // No worker ever holds a second request while its first is live.
        for (_, pid) in &log.pids {
            let mut open = 0i32;
            for (_, worker, _, frame) in &log.by_process {
                if worker != pid {
                    continue;
                }
                match frame {
                    Frame::ReqHead(_) => {
                        open += 1;
                        assert!(open <= 1, "worker {pid} was given two live requests");
                    }
                    Frame::Done { .. } => open -= 1,
                    _ => {}
                }
            }
        }
    }
    hold.add_permits(3);
    for stream in 1..=3u32 {
        assert_eq!(status_of(&world.http.response(stream).await), 200);
    }
}

/// R13: a route never exceeds the budget that is its process limit, and the
/// requests it cannot serve wait instead of failing.
#[tokio::test]
async fn a_route_never_exceeds_its_process_limit() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut options = Options::new().cap_route(0, 1).pool_route(0, true).script(
        0,
        Script::Hold {
            body: b"ok".to_vec(),
            gate: hold.clone(),
        },
    );
    options.cfg.max_workers = 2;
    options.cfg.queue_size = 16;
    let mut world = World::start(options);

    for stream in 1..=4u32 {
        world.http.send_request(stream, head("GET", "/"), b"").await;
    }
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 2)
        .await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    {
        let log = world.log.lock().unwrap();
        assert_eq!(log.spawns, vec![0, 0], "at most `max_workers` processes");
        assert_eq!(log.pids.len(), 2);
    }
    assert!(
        world.requests().iter().all(|event| event.status() != 502),
        "the requests that wait are not failed"
    );

    hold.add_permits(8);
    for stream in 1..=4u32 {
        assert_eq!(status_of(&world.http.response(stream).await), 200);
    }
}

/// R13: a burst's extras retire on `BLASTIT_VIEW_IDLE_TIMEOUT`, so a burst
/// leaves at most what the prefork floor asked for, never a new floor.
#[tokio::test]
async fn burst_pool_shrinks_on_idle_timeout() {
    tokio::time::pause();
    let hold = Arc::new(Semaphore::new(0));
    let mut options = Options::new().cap_route(0, 1).pool_route(0, true).script(
        0,
        Script::Hold {
            body: b"ok".to_vec(),
            gate: hold.clone(),
        },
    );
    options.cfg.max_workers = 4;
    options.cfg.idle_timeout = Duration::from_secs(10);
    let mut world = World::start(options);

    for stream in 1..=3u32 {
        world.http.send_request(stream, head("GET", "/"), b"").await;
    }
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 3)
        .await;
    let burst: Vec<u32> = world
        .log
        .lock()
        .unwrap()
        .pids
        .iter()
        .map(|(_, pid)| *pid)
        .collect();
    hold.add_permits(3);
    for stream in 1..=3u32 {
        assert_eq!(status_of(&world.http.response(stream).await), 200);
    }

    world.advance(Duration::from_secs(11)).await;
    world
        .until(|world| world.log.lock().unwrap().retired_pids.len() == 3)
        .await;
    let retired = world.log.lock().unwrap().retired_pids_of(0);
    let mut retired_pids: Vec<u32> = retired.iter().map(|(pid, _)| *pid).collect();
    retired_pids.sort_unstable();
    let mut expected = burst.clone();
    expected.sort_unstable();
    assert_eq!(retired_pids, expected, "every burst pid was retired");
    assert!(
        retired.iter().all(|(_, reason)| reason == "idle"),
        "{retired:?}"
    );

    // A follow-up request gets a fresh process.
    world.http.send_request(4, head("GET", "/"), b"").await;
    world
        .until(|world| world.log.lock().unwrap().pids.len() == 4)
        .await;
    let fresh = world.log.lock().unwrap().pids[3].1;
    assert!(!burst.contains(&fresh));
    hold.add_permits(1);
    assert_eq!(status_of(&world.http.response(4).await), 200);
}
