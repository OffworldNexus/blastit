//! `serve` must work when its caller is holding the GIL.
//!
//! That is exactly init's call site: a freshly forked child, right after
//! `PyOS_AfterFork_Child()`, on the main thread with the GIL held (§4, §9.3).
//! [`serve`] detaches for the whole of its work, so the request threads it
//! starts can attach — this test would hang if it did not.
//!
//! It runs in its own test binary because it serves on the test's own thread
//! rather than on a background one: the point is *which* thread calls it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use blastit_frame::{Decoder, DoneOutcome, Frame, Limits, encode};
use blastit_view::{ViewConfig, serve};
use pyo3::prelude::*;
use support::{load_app, request};

/// What the scheduler side of this test saw.
#[derive(Debug)]
struct Seen {
    status: u16,
    body: Vec<u8>,
    outcome: Option<DoneOutcome>,
}

#[test]
fn serve_runs_with_the_gil_held_by_its_caller() {
    let app = load_app(
        "gil_held",
        "import threading\n\
         def app(environ, start_response):\n\
         \x20   start_response('200 OK', [('Content-Type', 'text/plain')])\n\
         \x20   return [(environ['PATH_INFO'] + ':' + str(threading.get_ident())).encode()]\n",
    );
    let limits = Limits::default();
    let (ours, peer) = UnixStream::pair().expect("socketpair");
    peer.set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");

    // The scheduler runs on another thread, because this one serves.
    let scheduler: JoinHandle<Seen> = thread::spawn(move || {
        let mut peer = peer;
        let mut decoder = Decoder::new(limits);
        let mut seen = Seen {
            status: 0,
            body: Vec::new(),
            outcome: None,
        };

        assert_eq!(
            read_frame(&mut peer, &mut decoder),
            (0, Frame::Ready),
            "READY comes first"
        );

        let mut buf = Vec::new();
        encode(&request("GET", "/gil"), 1, &mut buf).expect("encode");
        encode(&Frame::ReqEnd, 1, &mut buf).expect("encode");
        peer.write_all(&buf).expect("write request");

        loop {
            match read_frame(&mut peer, &mut decoder) {
                (1, Frame::RespHead(head)) => seen.status = head.status,
                (1, Frame::RespBody(chunk)) => seen.body.extend_from_slice(&chunk),
                (1, Frame::RespEnd) => {}
                (1, Frame::Done { outcome }) => {
                    seen.outcome = Some(outcome);
                    break;
                }
                (id, other) => panic!("request {id}: unexpected frame {other:?}"),
            }
        }
        // Close the channel: `serve` returns once it sees the EOF.
        peer.shutdown(std::net::Shutdown::Both).ok();
        drop(peer);
        seen
    });

    // The GIL is held here, exactly as init holds it, and the request threads
    // `serve` starts still get to run Python.
    let result = Python::attach(|py| serve(py, ours, app, ViewConfig::for_route("web")));
    let seen = scheduler.join().expect("the scheduler thread panicked");

    assert!(result.is_ok(), "serve returned {result:?}");
    assert_eq!(seen.status, 200);
    let body = String::from_utf8(seen.body).expect("utf-8");
    let (path, ident) = body.split_once(':').expect("path and thread id");
    assert_eq!(path, "/gil");
    assert!(!ident.is_empty(), "the application ran on a request thread");
    assert_eq!(seen.outcome, Some(DoneOutcome::Completed));
}

/// Read one frame, blocking until it arrives.
fn read_frame(peer: &mut UnixStream, decoder: &mut Decoder) -> (u32, Frame) {
    loop {
        if let Some(frame) = decoder.next_frame().expect("decode") {
            return frame;
        }
        let mut buf = [0u8; 4096];
        let n = peer.read(&mut buf).expect("read");
        assert!(n > 0, "the view closed the channel");
        decoder.push(&buf[..n]);
    }
}
