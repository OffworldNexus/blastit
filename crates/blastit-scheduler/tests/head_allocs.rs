//! What each way of handling a request head costs in allocations.
//!
//! `http.chan` used to decode a `REQ_HEAD` into owned `Vec`s and then clone it
//! for the view; it now forwards the payload undecoded (see
//! `Scheduler::begin_raw`). The saving is the point of the change, so this
//! measures it: an 8-header head is handled both ways, exactly as far as the
//! scheduler would take it, and the two totals are compared with a counting
//! allocator.
//!
//! The same allocator answers the other half of `begin_raw`: a route with no
//! `[route.ingress]` may only hold an empty body, which the scheduler decides
//! from the policy alone — so it never copies the method or a header out of the
//! head. One route with `body = "none"` (the same plan, read from the head) and
//! the same route without a policy differ by exactly those three copies.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use blastit_events::EventKind;
use blastit_frame::{Decoder, Frame, Limits, ReqHead, ReqHeadView, Scheme};

use common::{Options, World, head};

/// A `System` allocator that counts allocations per thread while armed.
///
/// Counting per thread keeps a concurrently running test from polluting the
/// figure — and keeps one test's arming from disarming another's — so both the
/// counter and the flag are thread-local.
struct Counting;

thread_local! {
    /// Allocations on this thread since it was last armed.
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    /// Whether this thread is counting.
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

// A global allocator has to be `unsafe impl`; the operations it wraps are the
// `System` ones, wrapped in blocks because `unsafe_op_in_unsafe_fn` is denied.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.with(Cell::get) {
            ALLOCS.with(|count| count.set(count.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ARMED.with(Cell::get) {
            ALLOCS.with(|count| count.set(count.get() + 1));
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ARMED.with(Cell::get) {
            ALLOCS.with(|count| count.set(count.get() + 1));
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Run `body` with counting on, and return how many allocations it made.
fn measured(body: impl FnOnce()) -> usize {
    arm();
    body();
    disarm()
}

/// Start counting, discarding whatever the thread did before.
fn arm() {
    ALLOCS.with(|count| count.set(0));
    ARMED.with(|armed| armed.set(true));
}

/// Stop counting and return the allocations made since [`arm`].
fn disarm() -> usize {
    ARMED.with(|armed| armed.set(false));
    ALLOCS.with(Cell::get)
}

/// A request head with `header_count` headers, as http would frame one.
fn head_with(header_count: usize) -> ReqHead {
    let headers = (0..header_count)
        .map(|i| {
            (
                format!("x-test-{i}").into_bytes(),
                format!("value-{i}").into_bytes(),
            )
        })
        .collect();
    ReqHead {
        method: b"GET".to_vec(),
        target: b"/some/path?q=1".to_vec(),
        headers,
        remote_addr: "203.0.113.7".to_string(),
        scheme: Scheme::Http,
        server_name: "example.com".to_string(),
        server_port: 80,
        peer_addr: "203.0.113.7:51000".to_string(),
    }
}

#[test]
fn a_raw_head_is_forwarded_for_far_fewer_allocations_than_a_decoded_one() {
    const ITERATIONS: usize = 1_000;
    let limits = Limits::default();
    let head = head_with(8);
    let mut wire = Vec::new();
    limits
        .encode(&Frame::ReqHead(head.clone()), 7, &mut wire)
        .unwrap();

    // What the old path did: decode the head, clone it as dispatch did, and
    // encode the clone for the view.
    let decoded_total = measured(|| {
        for _ in 0..ITERATIONS {
            let mut decoder = Decoder::new(limits);
            decoder.push(&wire);
            let (id, frame) = decoder.next_frame().unwrap().unwrap();
            let Frame::ReqHead(head) = frame else {
                unreachable!("a REQ_HEAD decodes to a REQ_HEAD")
            };
            let mut out = Vec::new();
            limits
                .encode(&Frame::ReqHead(head.clone()), id, &mut out)
                .unwrap();
            std::hint::black_box(&out);
        }
    });

    // What the new path does: take the payload undecoded, read only what the
    // scheduler needs from it — and copy just those few fields out, the way
    // `begin_raw` does — then forward the payload with the id rewritten.
    let raw_total = measured(|| {
        for _ in 0..ITERATIONS {
            let mut decoder = Decoder::new(limits);
            decoder.push(&wire);
            let (_id, mut raw) = decoder.next_raw().unwrap().unwrap();
            let view = ReqHeadView::parse(&raw.payload).unwrap();
            let facts = (
                view.method().to_vec(),
                view.target().to_vec(),
                view.remote_addr().to_string(),
            );
            std::hint::black_box((facts, view.peer_addr(), view.headers().count()));
            let mut out = Vec::new();
            raw.set_id(7);
            raw.encode(&limits, &mut out).unwrap();
            std::hint::black_box(&out);
        }
    });

    let decoded = decoded_total as f64 / ITERATIONS as f64;
    let raw = raw_total as f64 / ITERATIONS as f64;
    println!("allocations per 8-header request: decoded {decoded}, raw {raw}");
    assert!(
        raw < decoded,
        "the raw path must allocate less than the decoded one ({raw} vs {decoded})"
    );
}

// ---------------------------------------------------------------------------
// `R12`: a route with no ingress policy never reads the head
// ---------------------------------------------------------------------------

/// One route, declaring no `[route.ingress]`: `plan_body` says
/// `AllowEmptyOnly` from the policy alone, so nothing in the head is read.
const PLAIN_ROUTE: &str = "\
version = 1
[[route]]
name = \"app\"
path = ['/app/']
";

/// The same route declaring `body = "none"`. Its plan is `AllowEmptyOnly` too,
/// so the only difference the two manifests can make is whether the scheduler
/// has a policy to consult — and therefore whether it reads the head.
const NONE_ROUTE: &str = "\
version = 1
[[route]]
name = \"app\"
path = ['/app/']
[route.ingress]
body = \"none\"
";

/// The head both worlds receive: a `POST` with a content type, a content
/// encoding and a zero length, so the policy path copies three `String`s out
/// of it and the policy-less path copies none.
fn ingress_head(target: &str) -> ReqHead {
    ReqHead {
        headers: vec![
            (b"content-type".to_vec(), b"application/json".to_vec()),
            (b"content-encoding".to_vec(), b"identity".to_vec()),
            (b"content-length".to_vec(), b"0".to_vec()),
        ],
        ..head("POST", target)
    }
}

/// Frames the view of `route` received, without cloning them.
fn heads_received(world: &World, route: usize) -> usize {
    world
        .log
        .lock()
        .expect("log lock")
        .received
        .iter()
        .filter(|(seen, _, frame)| *seen == route && matches!(frame, Frame::ReqHead(_)))
        .count()
}

/// Sends one warm-up head and waits for the view to take it, which pays for
/// the fork and the socketpair outside every measurement.
async fn warm_up(world: &mut World, target: &str) {
    world.http.send_head(1000, head("GET", target)).await;
    world.until(|world| heads_received(world, 0) > 0).await;
}

/// How many `request` events `path` has been given, without cloning one.
///
/// The event is emitted once the request is complete — the view's `DONE`
/// arrived — so it is the boundary that keeps one measurement from spilling
/// into the next.
fn completed(world: &World, path: &str) -> usize {
    world
        .events
        .lock()
        .expect("events lock")
        .iter()
        .filter(|event| matches!(event.kind(), EventKind::Request(info) if info.path() == path))
        .count()
}

/// The allocations one complete request costs, head to `request` event.
///
/// The scheduler runs on this thread (a current-thread runtime), so the
/// thread-local counter sees its allocations as well as the fake client's.
async fn per_head(world: &mut World, target: &str, iterations: usize) -> usize {
    const FIRST_ID: u32 = 2000;
    let head = ingress_head(target);
    let start = completed(world, target);
    arm();
    for iteration in 0..iterations {
        let want = start + iteration + 1;
        world
            .http
            .send_head(FIRST_ID + iteration as u32, head.clone())
            .await;
        world
            .until(move |world| completed(world, target) >= want)
            .await;
    }
    disarm()
}

/// A fresh world for `manifest`, settled by a warm-up and one discarded
/// measurement, then measured. Returns `(allocations, requests)`.
///
/// Two separate worlds make the two arms identical in history: the first
/// measured pass in a world allocates less than every later one (the client's
/// flow tables and the scheduler's maps are still growing), so only the second
/// pass is comparable across worlds.
async fn steady_state(manifest: &str) -> (usize, usize) {
    const ITERATIONS: usize = 100;
    let mut world = World::start(Options::new().manifest(manifest));
    warm_up(&mut world, "/app/").await;
    let _settle = per_head(&mut world, "/app/", ITERATIONS).await;
    (per_head(&mut world, "/app/", ITERATIONS).await, ITERATIONS)
}

#[tokio::test]
async fn a_policy_less_head_is_planned_without_copying_its_headers() {
    let (plain, iterations) = steady_state(PLAIN_ROUTE).await;
    let (ingress, _) = steady_state(NONE_ROUTE).await;
    println!(
        "allocations per head: no policy {}, body = \"none\" {}",
        plain as f64 / iterations as f64,
        ingress as f64 / iterations as f64,
    );
    // The declared route copies the method and the two headers it reads; the
    // policy-less one copies none of them, so the totals differ by exactly
    // three per request and by nothing else.
    assert_eq!(
        ingress,
        plain + 3 * iterations,
        "a route with no ingress policy must plan from the policy alone: {} vs {}",
        plain as f64 / iterations as f64,
        ingress as f64 / iterations as f64,
    );
}
