//! What each way of handling a request head costs in allocations.
//!
//! `http.chan` used to decode a `REQ_HEAD` into owned `Vec`s and then clone it
//! for the view; it now forwards the payload undecoded (see
//! `Scheduler::begin_raw`). The saving is the point of the change, so this
//! measures it: an 8-header head is handled both ways, exactly as far as the
//! scheduler would take it, and the two totals are compared with a counting
//! allocator.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use blastit_frame::{Decoder, Frame, Limits, ReqHead, ReqHeadView, Scheme};

/// A `System` allocator that counts allocations per thread while armed.
///
/// Counting per thread keeps a concurrently running test from polluting the
/// figure, and `ARMED` keeps the counter out of every allocation that is not
/// part of the measured section.
struct Counting;

/// Whether allocations are being counted.
static ARMED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Allocations on this thread since it was last armed.
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

// A global allocator has to be `unsafe impl`; the operations it wraps are the
// `System` ones, wrapped in blocks because `unsafe_op_in_unsafe_fn` is denied.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCS.with(|count| count.set(count.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCS.with(|count| count.set(count.get() + 1));
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCS.with(|count| count.set(count.get() + 1));
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Run `body` with counting on, and return how many allocations it made.
fn measured(body: impl FnOnce()) -> usize {
    ALLOCS.with(|count| count.set(0));
    ARMED.store(true, Ordering::Relaxed);
    body();
    ARMED.store(false, Ordering::Relaxed);
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
