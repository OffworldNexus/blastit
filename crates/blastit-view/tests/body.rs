//! `wsgi.input`: streaming, partial reads, blocking and the metered queue.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use blastit_frame::{DoneOutcome, Frame};

use support::{End, Harness, load_app, load_module};

/// The body the piecewise test reads, in the order it reads it.
const PIECES: &[u8] = b"abcdef\ngh\nij";

#[test]
fn the_body_can_be_read_in_pieces_across_frames() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_pieces",
            "def app(environ, start_response):\n\
             \x20   stream = environ['wsgi.input']\n\
             \x20   chunks = [stream.read(2), stream.read(3), stream.readline(), stream.read(), stream.read(), stream.readline()]\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b','.join(chunks)]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("POST", "/"), 1);
    // The body arrives in three frames, cut in the middle of a read.
    harness.send_body(1, &PIECES[..3]);
    harness.send_body(1, &PIECES[3..7]);
    harness.send_body(1, &PIECES[7..]);
    harness.send(&Frame::ReqEnd, 1);

    let response = harness.read_response(1);
    assert_eq!(
        response.body, b"ab,cde,f\n,gh\nij,,",
        "read(2), read(3), readline(), read(), read() at EOF, readline() at EOF"
    );
    assert_eq!(response.end, End::End);
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn the_body_iterates_line_by_line() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_lines",
            "def app(environ, start_response):\n\
             \x20   stream = environ['wsgi.input']\n\
             \x20   lines = list(stream)\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b','.join(lines) + b';' + str(len(lines)).encode()]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.post("/", b"one\ntwo\nthree");
    assert_eq!(
        response.body, b"one\n,two\n,three;3",
        "iteration yields lines, and the last one needs no newline"
    );
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn readlines_stops_after_its_hint() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_readlines",
            "def app(environ, start_response):\n\
             \x20   stream = environ['wsgi.input']\n\
             \x20   first = stream.readlines(2)\n\
             \x20   rest = stream.read()\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b';'.join(first + [rest])]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.post("/", b"a\nb\nc\n");
    assert_eq!(
        response.body, b"a\n;b\nc\n",
        "readlines(2) stops as soon as it has two bytes of lines, like io.BytesIO"
    );
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn read_of_zero_bytes_returns_at_once() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_read_zero",
            "def app(environ, start_response):\n\
             \x20   empty = environ['wsgi.input'].read(0)\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b'len=' + str(len(empty)).encode()]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // No body and no REQ_END: the application must not wait for either.
    harness.send(&support::request("POST", "/"), 1);
    let response = harness.read_response(1);
    assert_eq!(response.body, b"len=0");
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_read_blocks_until_the_body_arrives() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_blocks",
            "def app(environ, start_response):\n\
             \x20   body = environ['wsgi.input'].read(100)\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b'len=' + str(len(body)).encode() + b':' + body]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("POST", "/"), 1);
    assert!(
        harness.quiet(Duration::from_millis(200)),
        "the application answered before it had read its body"
    );

    // Half of what `read(100)` asked for: still not enough to return.
    harness.send_body(1, &[b'a'; 50]);
    assert!(
        harness.quiet(Duration::from_millis(200)),
        "read(100) returned a short read"
    );

    harness.send_body(1, &[b'b'; 50]);
    harness.send(&Frame::ReqEnd, 1);
    let response = harness.read_response(1);
    assert_eq!(
        response.body.len(),
        8 + 100,
        "the prefix and the whole body"
    );
    assert_eq!(&response.body[..8], b"len=100:");
    assert_eq!(&response.body[8..58], &[b'a'; 50][..]);
    assert_eq!(&response.body[58..], &[b'b'; 50][..]);
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_read_after_the_end_of_the_body_returns_nothing() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_eof",
            "def app(environ, start_response):\n\
             \x20   stream = environ['wsgi.input']\n\
             \x20   first = stream.read()\n\
             \x20   second = stream.read()\n\
             \x20   third = stream.readline()\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [first + b'|' + second + b'|' + third + b'|']\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.post("/", b"once");
    assert_eq!(response.body, b"once|||");
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_cancelled_request_makes_reads_raise() {
    let module = load_module(
        "body_cancel",
        "caught = ''\n\
         def app(environ, start_response):\n\
         \x20   global caught\n\
         \x20   try:\n\
         \x20       environ['wsgi.input'].read()\n\
         \x20   except OSError as err:\n\
         \x20       caught = str(err)\n\
         \x20   start_response('200 OK', [])\n\
         \x20   return [b'after the cancel']\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // The application blocks on a body that never arrives, and is cancelled.
    harness.send(&support::request("POST", "/"), 1);
    assert!(harness.quiet(Duration::from_millis(200)));
    harness.send(&Frame::Cancel, 1);

    let frames = harness.drain_until_done(1);
    assert_eq!(
        frames,
        vec![Frame::Done {
            outcome: DoneOutcome::Aborted
        }],
        "a cancelled request sends nothing but DONE"
    );
    let caught = support::global_str(&module, "caught");
    assert!(
        caught.contains("cancel"),
        "the read reported the cancel: {caught}"
    );
    assert!(harness.finish().is_ok());
}

#[test]
fn a_none_size_means_read_everything() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_none_sizes",
            "def app(environ, start_response):\n\
             \x20   stream = environ['wsgi.input']\n\
             \x20   parts = [stream.readline(None), stream.read(None), stream.read(None)]\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b','.join(parts)]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.post("/", b"one\ntwo");
    assert_eq!(
        response.body, b"one\n,two,",
        "None reads mean the same as no argument"
    );
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn readlines_accepts_none_and_zero_hints() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_none_hint",
            "def app(environ, start_response):\n\
             \x20   stream = environ['wsgi.input']\n\
             \x20   first = stream.readline(0)\n\
             \x20   rest = stream.readlines(None)\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b';'.join([first] + rest) + b';' + str(len(rest)).encode()]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.post("/", b"a\nb\nc\n");
    assert_eq!(
        response.body, b";a\n;b\n;c\n;3",
        "readline(0) is empty, and readlines(None) takes everything"
    );
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn an_absurd_size_is_clamped_rather_than_waited_for() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "body_huge_size",
            "def app(environ, start_response):\n\
             \x20   body = environ['wsgi.input'].read(10 ** 18)\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [str(len(body)).encode()]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // The read is clamped, and the end of the body ends it long before the
    // clamp matters.
    let response = harness.post("/", b"small body");
    assert_eq!(response.body, b"10");
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}
