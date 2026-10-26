//! Memory sizing: the derived view cap and high-watermark retirement
//! (ARCHITECTURE.md §8 Sizing, §9.2 #7 and #8).
//!
//! The probe is a fake in every test here — a real one reads the machine, and
//! these tests are about what the scheduler does with a number, not about how
//! the number is read (`blastit_sys::mem` covers that).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use blastit_events::RetireReason;
use blastit_scheduler::{MemoryProbe, derived_view_cap, mem_reserve};
use common::{Memory, Options, Script, World, body_of, head, route, status_of};
use tokio::sync::Semaphore;

/// One mebibyte, for arithmetic that reads the way §8 writes it.
const MIB: u64 = 1024 * 1024;

/// A two-route manifest: `/a/` and `/b/`, both answering.
fn two_routes() -> String {
    format!("version = 1\n{}{}", route("a", "/a/"), route("b", "/b/"))
}

/// A configurable world where both routes answer immediately.
fn answering() -> Options {
    Options::new()
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
        )
}

/// Gives `options` a probe that reads `usage` on every call, and a watermark.
fn watch(options: &mut Options, usage: &Arc<AtomicU64>, watermark: u64) {
    let usage = usage.clone();
    options.cfg.memory_usage = Some(MemoryProbe::new(move || {
        Some(usage.load(Ordering::Relaxed))
    }));
    options.cfg.mem_high_watermark_bytes = Some(watermark);
    options.cfg.memory_check_interval = Duration::from_secs(2);
}

#[tokio::test]
async fn an_idle_view_is_retired_above_the_watermark() {
    tokio::time::pause();
    let usage = Arc::new(AtomicU64::new(0));
    let mut options = answering();
    watch(&mut options, &usage, 100 * MIB);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");

    // Below the watermark, nothing happens however long we wait.
    usage.store(50 * MIB, Ordering::Relaxed);
    world.advance(Duration::from_secs(9)).await;
    assert!(world.log.lock().unwrap().retires.is_empty(), "well below");

    // Above it, the check retires the idle process (§8 Sizing).
    usage.store(101 * MIB, Ordering::Relaxed);
    world.advance(Duration::from_secs(3)).await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "memory_pressure".to_string())]
    );
    let retired = world.retirements();
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].reason(), &RetireReason::MemoryPressure);
}

#[tokio::test]
async fn memory_pressure_retires_one_view_per_check() {
    tokio::time::pause();
    let usage = Arc::new(AtomicU64::new(0));
    let mut options = answering();
    watch(&mut options, &usage, 100 * MIB);
    let mut world = World::start(options);

    // Two live views, and both are idle.
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"b");
    assert_eq!(world.log.lock().unwrap().spawns.len(), 2);

    usage.store(200 * MIB, Ordering::Relaxed);
    // One check: exactly one process goes, the one idle the longest.
    world.advance(Duration::from_secs(3)).await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "memory_pressure".to_string())]
    );
    // The next check takes the other one; after that there is nothing left to
    // retire and the scheduler does not invent work.
    world.advance(Duration::from_secs(3)).await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![
            (0, "memory_pressure".to_string()),
            (1, "memory_pressure".to_string()),
        ]
    );
    world.advance(Duration::from_secs(20)).await;
    assert_eq!(world.log.lock().unwrap().retires.len(), 2);
}

#[tokio::test]
async fn a_busy_view_is_never_retired_for_memory() {
    tokio::time::pause();
    let usage = Arc::new(AtomicU64::new(0));
    let gate = Arc::new(Semaphore::new(0));
    let mut options = answering().script(
        0,
        Script::Hold {
            body: b"a".to_vec(),
            gate: gate.clone(),
        },
    );
    watch(&mut options, &usage, 100 * MIB);
    let mut world = World::start(options);

    // The request is dispatched and held: the view has one request in flight.
    world
        .http
        .send_request(1, head("GET", "/a/"), b"body")
        .await;
    world
        .until(|world| world.log.lock().unwrap().spawns == vec![0])
        .await;
    usage.store(500 * MIB, Ordering::Relaxed);
    // Past several 2 s memory checks, and *inside* the 30 s request timeout: a
    // request in flight here is not one this test wants to see expire.
    world.advance(Duration::from_secs(25)).await;

    // §8 Sizing retires *idle* views: a request in flight is not one, and the
    // scheduler would rather be over the watermark than fail that request.
    assert!(world.log.lock().unwrap().retires.is_empty(), "still busy");

    // Once it is idle, the next check takes it.
    gate.add_permits(1);
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.advance(Duration::from_secs(3)).await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "memory_pressure".to_string())]
    );
}

#[tokio::test]
async fn an_unreadable_probe_is_not_a_retirement() {
    tokio::time::pause();
    let mut options = answering();
    options.cfg.memory_usage = Some(MemoryProbe::new(|| None));
    options.cfg.mem_high_watermark_bytes = Some(1);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.advance(Duration::from_secs(30)).await;
    assert!(
        world.log.lock().unwrap().retires.is_empty(),
        "a probe that cannot read must not guess"
    );
}

#[tokio::test]
async fn no_watermark_means_no_check() {
    tokio::time::pause();
    // A probe that would say "above the watermark" if it were ever called.
    let usage = Arc::new(AtomicU64::new(u64::MAX));
    let probe = usage.clone();
    let mut options = answering();
    options.cfg.memory_usage = Some(MemoryProbe::new(move || {
        Some(probe.load(Ordering::Relaxed))
    }));
    assert!(options.cfg.mem_high_watermark_bytes.is_none());
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.advance(Duration::from_secs(30)).await;
    assert!(world.log.lock().unwrap().retires.is_empty());
}

#[tokio::test]
async fn the_derived_cap_replaces_an_unset_max_view_processes() {
    tokio::time::pause();
    // 1 GiB total, 900 MiB in use: the reserve is 102.4 MiB and the estimate
    // 32 MiB, so §8's derived cap is 1.
    assert_eq!(derived_view_cap(1024 * MIB, 900 * MIB, 32 * MIB), 1);
    assert_eq!(mem_reserve(1024 * MIB), 1024 * MIB / 10);

    let usage = Arc::new(AtomicU64::new(900 * MIB));
    let probe = usage.clone();
    let mut options = answering();
    options.cfg.memory_usage = Some(MemoryProbe::new(move || {
        Some(probe.load(Ordering::Relaxed))
    }));
    options.cfg.mem_total_bytes = Some(1024 * MIB);
    options.cfg.view_mem_estimate = 32 * MIB;
    options.cfg.max_view_processes = None;
    let mut world = World::start(options);

    // Route 0 first, then route 1: the derived cap of one can only be met by
    // retiring the idle route 0, exactly like an explicit cap would (§8).
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
}

#[tokio::test]
async fn the_derived_cap_bounds_the_initial_prefork_burst() {
    tokio::time::pause();
    // Three preforked routes, and room for exactly one view process.
    let manifest = "version = 1\n\
        [[route]]\nname = \"a\"\npath = ['/a/']\nprefork = true\n\
        [[route]]\nname = \"b\"\npath = ['/b/']\nprefork = true\n\
        [[route]]\nname = \"c\"\npath = ['/c/']\nprefork = true\n";
    let usage = Arc::new(AtomicU64::new(900 * MIB));
    let probe = usage.clone();
    let mut options = Options::new().manifest(manifest);
    options.cfg.memory_usage = Some(MemoryProbe::new(move || {
        Some(probe.load(Ordering::Relaxed))
    }));
    options.cfg.mem_total_bytes = Some(1024 * MIB);
    options.cfg.view_mem_estimate = 32 * MIB;
    let world = World::start(options);

    // §8 Sizing: the cap is taken before the burst, so the two routes that do
    // not fit wait for a request instead of being forked at startup — the
    // scheduler is forked before init preloads the app, so nothing else could
    // bound this.
    world
        .until(|world| !world.log.lock().unwrap().spawns.is_empty())
        .await;
    world.advance(Duration::from_secs(5)).await;
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0],
        "one process, not three"
    );
    assert!(world.log.lock().unwrap().retires.is_empty());
}
#[tokio::test]
async fn an_explicit_cap_wins_over_the_derived_one() {
    tokio::time::pause();
    // A probe that would derive a cap of one if it were ever consulted.
    let usage = Arc::new(AtomicU64::new(900 * MIB));
    let probe = usage.clone();
    let mut options = answering();
    options.cfg.memory_usage = Some(MemoryProbe::new(move || {
        Some(probe.load(Ordering::Relaxed))
    }));
    options.cfg.mem_total_bytes = Some(1024 * MIB);
    options.cfg.view_mem_estimate = 32 * MIB;
    options.cfg.max_view_processes = Some(8);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    let frames = world.http.response(2).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"b");
    // Nothing was retired: `BLASTIT_MAX_VIEW_PROCESSES` was set, so §8's
    // derived default never applies.
    assert!(world.log.lock().unwrap().retires.is_empty());
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1]);
}

#[tokio::test]
async fn without_a_memory_total_the_cap_stays_unset() {
    tokio::time::pause();
    let usage = Arc::new(AtomicU64::new(900 * MIB));
    let probe = usage.clone();
    let mut options = answering();
    options.cfg.memory_usage = Some(MemoryProbe::new(move || {
        Some(probe.load(Ordering::Relaxed))
    }));
    options.cfg.mem_total_bytes = None;
    options.cfg.view_mem_estimate = 32 * MIB;
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"b");
    // §8: memory that cannot be read leaves the cap alone rather than guessing.
    assert!(world.log.lock().unwrap().retires.is_empty());
}

/// A three-route manifest: `/a/`, `/b/` and `/c/`, all answering.
fn three_routes() -> String {
    format!(
        "version = 1\n{}{}{}",
        route("a", "/a/"),
        route("b", "/b/"),
        route("c", "/c/")
    )
}

/// A world where every one of the three routes answers at once.
fn three_answering() -> Options {
    Options::new()
        .manifest(&three_routes())
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
        )
}

#[tokio::test]
async fn a_spawn_above_the_watermark_queues_instead_of_forking() {
    tokio::time::pause();
    // The machine is already over the watermark and there is nothing idle to
    // give back: §8 Sizing says wait, not fork a process the next check would
    // kill again. Before this rule the scheduler forked, the check retired, and
    // the request forked once more — retire-then-respawn thrash.
    let usage = Arc::new(AtomicU64::new(500 * MIB));
    let mut options = three_answering();
    watch(&mut options, &usage, 100 * MIB);
    options.cfg.max_view_processes = Some(2);
    options.cfg.queue_timeout = Duration::from_secs(30);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    world.advance(Duration::from_secs(9)).await;
    let spawns = world.log.lock().unwrap().spawns.clone();
    assert!(
        spawns.is_empty(),
        "no fork while over the watermark: {spawns:?}"
    );
    assert!(world.log.lock().unwrap().retires.is_empty());

    // Once the machine has room, the request that was waiting is served.
    usage.store(0, Ordering::Relaxed);
    world.advance(Duration::from_secs(3)).await;
    let frames = world.http.response(1).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"a");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0]);
}

#[tokio::test]
async fn a_view_retired_for_memory_is_not_respawned_straight_away() {
    tokio::time::pause();
    let usage = Arc::new(AtomicU64::new(0));
    let mut options = answering();
    watch(&mut options, &usage, 100 * MIB);
    options.cfg.spawn_backoff = Duration::from_secs(30);
    options.cfg.queue_timeout = Duration::from_secs(120);
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    usage.store(500 * MIB, Ordering::Relaxed);
    world.advance(Duration::from_secs(3)).await;
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "memory_pressure".to_string())]
    );

    // The machine has room again, but §8 Sizing still gives the route its
    // `spawn_backoff`: without it, the next request would copy the very heap
    // that was just given back, once per memory check.
    usage.store(0, Ordering::Relaxed);
    world.http.send_request(2, head("GET", "/a/"), b"").await;
    world.advance(Duration::from_secs(9)).await;
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0],
        "still backing off"
    );

    // Past the backoff, the same request gets its process and its answer.
    world.advance(Duration::from_secs(40)).await;
    let frames = world.http.response(2).await;
    assert_eq!(status_of(&frames), 200);
    assert_eq!(body_of(&frames), b"a");
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 0]);
}

#[tokio::test]
async fn the_measured_cost_per_view_replaces_the_configured_estimate() {
    tokio::time::pause();
    // 1 GiB total, 200 MiB in use, and a fork that copies 135 MiB of the
    // preloaded heap — §9.2 #2's real cost on CPython 3.14, where nothing is
    // immortalised, against a configured estimate of 32 MiB.
    let measured = 135 * MIB;
    let machine = Memory::new(200 * MIB, measured);
    let mut options = three_answering();
    options.cfg.mem_total_bytes = Some(1024 * MIB);
    options.cfg.view_mem_estimate = 32 * MIB;
    options.cfg.max_view_processes = None;
    let options = options.memory(machine.clone());
    let mut world = World::start(options);

    // §8 Sizing's derived cap, before anything is measured: the configured
    // estimate is the divisor, so it buys far more processes than fit.
    let usage_at_preload = 200 * MIB + measured;
    let naive = derived_view_cap(1024 * MIB, usage_at_preload, 32 * MIB);
    assert!(naive > 1);

    // Two routes up, one after the other: the rise between the two samples is
    // what one more view process costs.
    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"b");

    let expected = derived_view_cap(1024 * MIB, usage_at_preload, measured);
    assert_eq!(expected, 4);
    let caps = world.caps();
    assert_eq!(caps.len(), 1, "one event per change, not one per request");
    assert_eq!(caps[0].estimate_bytes(), measured);
    assert_eq!(caps[0].cap(), expected);
    assert!(
        caps[0].cap() < naive,
        "the measured estimate raised the denominator: {} vs {}",
        caps[0].cap(),
        naive
    );

    // A third process measures the same cost again: no new event, because
    // nothing changed.
    world.http.send_request(3, head("GET", "/c/"), b"").await;
    assert_eq!(body_of(&world.http.response(3).await), b"c");
    assert_eq!(world.caps(), caps);
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1, 2]);
}

#[tokio::test]
async fn a_machine_that_measures_nothing_keeps_the_configured_estimate() {
    tokio::time::pause();
    // Every fork reports the same usage: there is nothing to measure, so §8
    // Sizing's configured estimate stays the divisor and no cap event is
    // emitted at all.
    let machine = Memory::new(200 * MIB, 0);
    let mut options = three_answering();
    options.cfg.mem_total_bytes = Some(1024 * MIB);
    options.cfg.view_mem_estimate = 32 * MIB;
    options.cfg.max_view_processes = None;
    let options = options.memory(machine.clone());
    let mut world = World::start(options);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"b");
    world.http.send_request(3, head("GET", "/c/"), b"").await;
    assert_eq!(body_of(&world.http.response(3).await), b"c");

    assert!(world.caps().is_empty(), "{:?}", world.caps());
    assert!(world.log.lock().unwrap().retires.is_empty());
    assert_eq!(world.log.lock().unwrap().spawns, vec![0, 1, 2]);
}

#[tokio::test]
async fn the_measured_cap_is_the_one_that_bounds_the_processes() {
    tokio::time::pause();
    // A 512 MiB instance: 200 MiB in use, 135 MiB per fork, and §8's reserve
    // of `max(64 MiB, 10 %)`. The configured 32 MiB estimate buys three
    // processes; the measured cost buys exactly one, and that is the cap the
    // scheduler has to enforce from then on.
    let measured = 135 * MIB;
    let machine = Memory::new(200 * MIB, measured);
    let mut options = three_answering();
    options.cfg.mem_total_bytes = Some(512 * MIB);
    options.cfg.view_mem_estimate = 32 * MIB;
    options.cfg.max_view_processes = None;
    let options = options.memory(machine.clone());
    let mut world = World::start(options);

    let usage_at_preload = 200 * MIB + measured;
    assert_eq!(derived_view_cap(512 * MIB, usage_at_preload, 32 * MIB), 3);
    let measured_cap = derived_view_cap(512 * MIB, usage_at_preload, measured);
    assert_eq!(measured_cap, 1);

    world.http.send_request(1, head("GET", "/a/"), b"").await;
    assert_eq!(body_of(&world.http.response(1).await), b"a");
    world.http.send_request(2, head("GET", "/b/"), b"").await;
    assert_eq!(body_of(&world.http.response(2).await), b"b");
    assert_eq!(world.caps().len(), 1);
    assert_eq!(world.caps()[0].cap(), measured_cap);

    // Two processes are alive and the cap now says one, so the third route can
    // only be served by shedding the excess first — with the configured
    // estimate it would simply have forked a third and run the instance out of
    // memory.
    world.http.send_request(3, head("GET", "/c/"), b"").await;
    assert_eq!(body_of(&world.http.response(3).await), b"c");
    assert_eq!(
        world.log.lock().unwrap().spawns,
        vec![0, 1, 2],
        "one process per route, not one per request"
    );
    assert_eq!(
        world.log.lock().unwrap().retires,
        vec![(0, "lru_cap".to_string()), (1, "lru_cap".to_string()),],
        "the measured cap of one is what the scheduler holds itself to"
    );
}
