//! Requests run on their own OS threads, and one request never waits for
//! another (ARCHITECTURE.md §2.5). Requests that overlap still get distinct
//! threads; a worker is reused only once the request before it is done.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::{Duration, Instant};

use blastit_frame::{DoneOutcome, Frame, Limits};
use blastit_view::ViewConfig;
use support::{Harness, load_app, load_module};

/// Extract the body of one request's response from the frames collected for it.
fn body_of(frames: &[Frame]) -> Vec<u8> {
    let mut body = Vec::new();
    for frame in frames {
        if let Frame::RespBody(chunk) = frame {
            body.extend_from_slice(chunk);
        }
    }
    body
}

/// The status of one request's response, if it got one.
fn status_of(frames: &[Frame]) -> Option<u16> {
    frames.iter().find_map(|frame| match frame {
        Frame::RespHead(head) => Some(head.status),
        _ => None,
    })
}

#[test]
fn concurrent_requests_run_in_different_os_threads() {
    // Both requests wait on a barrier, so they are guaranteed to overlap: if
    // the view ever put them on one thread, the barrier would never fill.
    let module = load_module(
        "thread_idents",
        "import threading\n\
         idents = []\n\
         lock = threading.Lock()\n\
         barrier = threading.Barrier(2)\n\
         def app(environ, start_response):\n\
         \x20   ident = threading.get_ident()\n\
         \x20   with lock:\n\
         \x20       idents.append(ident)\n\
         \x20   barrier.wait(timeout=5)\n\
         \x20   start_response('200 OK', [])\n\
         \x20   return [str(ident).encode()]\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/one"), 1);
    harness.send(&Frame::ReqEnd, 1);
    harness.send(&support::request("GET", "/two"), 2);
    harness.send(&Frame::ReqEnd, 2);

    let frames = harness.drain_until_all_done(&[1, 2]);
    let first = String::from_utf8(body_of(&frames[&1])).expect("utf-8");
    let second = String::from_utf8(body_of(&frames[&2])).expect("utf-8");
    assert_ne!(
        first, second,
        "two requests that overlap run on different OS threads"
    );
    assert_eq!(status_of(&frames[&1]), Some(200));
    assert_eq!(status_of(&frames[&2]), Some(200));
    assert_eq!(
        support::global_len(&module, "idents"),
        2,
        "both requests ran the application"
    );
    assert!(harness.finish().is_ok());
}

#[test]
fn a_parked_worker_is_reused_for_the_next_request() {
    // The pool's point: a request that arrives after the previous one is done
    // runs on the same worker, with the same Python thread state. The cap is
    // one, so this also checks that a parked worker does not count as a live
    // request and does not turn a sequential request into a `503`.
    let cfg = ViewConfig {
        max_threads: Some(1),
        ..ViewConfig::for_route("web")
    };
    let mut harness = Harness::start(
        load_app(
            "reused_worker",
            "import threading\n\
             def app(environ, start_response):\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [str(threading.get_ident()).encode()]\n",
        ),
        cfg,
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/one"), 1);
    harness.send(&Frame::ReqEnd, 1);
    let first = harness.read_response(1);
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);

    // The worker has to park before the second request can find it idle; while
    // it does, the channel is quiet.
    assert!(
        harness.quiet(Duration::from_millis(100)),
        "the view answered after the first request was done"
    );

    harness.send(&support::request("GET", "/two"), 2);
    harness.send(&Frame::ReqEnd, 2);
    let second = harness.read_response(2);
    assert_eq!(second.status(), 200, "the reused worker still serves");
    assert_eq!(harness.expect_done(2), DoneOutcome::Completed);

    assert_eq!(
        first.body, second.body,
        "the second request ran on the first request's worker, not a new thread"
    );
    assert!(harness.finish().is_ok());
}

#[test]
fn a_sleeping_request_does_not_hold_up_another() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "slow_and_fast",
            "import time\n\
             def app(environ, start_response):\n\
             \x20   if environ['PATH_INFO'] == '/slow':\n\
             \x20       time.sleep(0.5)\n\
             \x20       body = b'slow'\n\
             \x20   else:\n\
             \x20       body = b'fast'\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [body]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let started = Instant::now();
    harness.send(&support::request("GET", "/slow"), 1);
    harness.send(&Frame::ReqEnd, 1);
    harness.send(&support::request("GET", "/fast"), 2);
    harness.send(&Frame::ReqEnd, 2);

    // The sleeping request is still asleep, so the first frame on the channel
    // belongs to the other one.
    let (id, frame) = harness.expect_frame();
    assert_eq!(id, 2, "the fast request answered first, but got {frame:?}");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(400),
        "the fast request waited for the slow one ({elapsed:?})"
    );

    let frames = harness.drain_until_all_done(&[1, 2]);
    assert_eq!(body_of(&frames[&1]), b"slow");
    assert_eq!(body_of(&frames[&2]), b"fast");
    assert!(harness.finish().is_ok());
}

#[test]
fn concurrent_responses_do_not_interleave_their_frames() {
    // Two requests stream large bodies at the same time, over a channel whose
    // frames are far smaller than the bodies. If the writer were not
    // serialising whole frames, the decoder would see nonsense.
    let limits = Limits::new(1024).unwrap();
    let cfg = ViewConfig {
        frame_limits: limits,
        ..ViewConfig::for_route("web")
    };
    let mut harness = Harness::start(
        load_app(
            "interleaved_bodies",
            "def app(environ, start_response):\n\
             \x20   fill = b'A' if environ['PATH_INFO'] == '/a' else b'B'\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [fill * 100000]\n",
        ),
        cfg,
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/a"), 1);
    harness.send(&Frame::ReqEnd, 1);
    harness.send(&support::request("GET", "/b"), 2);
    harness.send(&Frame::ReqEnd, 2);

    let frames = harness.drain_until_all_done(&[1, 2]);
    for (id, fill) in [(1u32, b'A'), (2u32, b'B')] {
        let body = body_of(&frames[&id]);
        assert_eq!(body.len(), 100_000, "request {id}");
        assert!(
            body.iter().all(|byte| *byte == fill),
            "request {id} got another request's bytes"
        );
    }
    assert!(harness.finish().is_ok());
}
