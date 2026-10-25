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
use common::{Options, Script, World, body_of, head, route, status_of};
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
    world.advance(Duration::from_secs(30)).await;

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
