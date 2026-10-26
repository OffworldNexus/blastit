//! Flow control: request- and response-body windows (§3.1).
//!
//! The harness plays a well-behaved http, so these tests exercise the whole
//! credit loop in both directions: uploads far larger than one window, and
//! responses that only complete because the scheduler granted more.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use blastit_frame::{DoneOutcome, Frame};
use blastit_view::ViewError;
use support::{End, Harness, INITIAL_CREDIT, load_app};

/// Three mebibytes, with a byte pattern that makes a lost or reordered chunk
/// visible.
fn big_body(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

#[test]
fn a_three_mib_upload_is_read_with_read() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "flow_read_all",
            "def app(environ, start_response):\n\
             \x20   body = environ['wsgi.input'].read()\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [str(len(body)).encode() + b'|' + body[:8] + b'|' + body[-8:]]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let body = big_body(3 * 1024 * 1024);
    assert!(body.len() > INITIAL_CREDIT, "the window must actually bind");
    let response = harness.post("/", &body);

    assert_eq!(response.status(), 200);
    let parts: Vec<&[u8]> = response.body.split(|byte| *byte == b'|').collect();
    assert_eq!(parts.len(), 3, "length|prefix|suffix");
    assert_eq!(parts[0], body.len().to_string().as_bytes());
    assert_eq!(parts[1], &body[..8]);
    assert_eq!(parts[2], &body[body.len() - 8..]);
    assert_eq!(response.end, End::End);
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(
        harness.upload_credit(1) >= INITIAL_CREDIT,
        "the view credited the body back as it was read"
    );
    assert!(harness.finish().is_ok());
}

#[test]
fn a_three_mib_upload_is_read_in_chunks() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "flow_read_chunks",
            "def app(environ, start_response):\n\
             \x20   stream = environ['wsgi.input']\n\
             \x20   total = 0\n\
             \x20   first = b''\n\
             \x20   while True:\n\
             \x20       chunk = stream.read(8192)\n\
             \x20       if not chunk:\n\
             \x20           break\n\
             \x20       total += len(chunk)\n\
             \x20       if len(first) < 8:\n\
             \x20           first += chunk[:8 - len(first)]\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [str(total).encode() + b'|' + first]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let body = big_body(3 * 1024 * 1024 + 17);
    let response = harness.post("/", &body);
    assert_eq!(response.status(), 200);
    let parts: Vec<&[u8]> = response.body.split(|byte| *byte == b'|').collect();
    assert_eq!(parts.len(), 2, "length|prefix");
    assert_eq!(parts[0], body.len().to_string().as_bytes());
    assert_eq!(
        parts[1],
        &body[..8],
        "8192-byte reads reassemble the body in order"
    );
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_three_mib_upload_is_iterated_line_by_line() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "flow_lines",
            "def app(environ, start_response):\n\
             \x20   lines = list(environ['wsgi.input'])\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [str(len(lines)).encode() + b'|' + str(sum(len(l) for l in lines)).encode()]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let line = b"0123456789abcdef\n";
    let body: Vec<u8> = line
        .iter()
        .copied()
        .cycle()
        .take(line.len() * 200_000)
        .collect();
    let response = harness.post("/", &body);
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.text(),
        format!("200000|{}", body.len()),
        "every line arrived, and only lines"
    );
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_response_larger_than_the_window_waits_for_credit() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "flow_big_response",
            "def app(environ, start_response):\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b'z' * (1048576 + 5000)]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);

    // The head, and exactly one window's worth of body: the view may not send
    // more than it has credit for.
    let mut body = 0;
    loop {
        let (id, frame) = harness.expect_frame();
        assert_eq!(id, 1);
        match frame {
            Frame::RespHead(head) => assert_eq!(head.status, 200),
            Frame::RespBody(chunk) => body += chunk.len(),
            other => panic!("unexpected frame {other:?}"),
        }
        if body >= INITIAL_CREDIT {
            break;
        }
    }
    assert_eq!(body, INITIAL_CREDIT, "the window, and not a byte more");
    assert!(
        harness.quiet(Duration::from_millis(300)),
        "the view kept writing past its credit"
    );

    // http wrote those bytes to the client, so it grants more.
    harness.grant_response(1, 5000);
    let mut rest = 0;
    loop {
        let (id, frame) = harness.expect_frame();
        assert_eq!(id, 1);
        match frame {
            Frame::RespBody(chunk) => rest += chunk.len(),
            Frame::RespEnd | Frame::RespEndCost { .. } => break,
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(rest, 5000, "the rest arrived once there was credit");
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn the_io_thread_keeps_serving_while_a_request_waits_for_credit() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "flow_two_requests",
            "def app(environ, start_response):\n\
             \x20   if environ['PATH_INFO'] == '/big':\n\
             \x20       body = b'b' * (1048576 + 100)\n\
             \x20   else:\n\
             \x20       body = b'small'\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [body]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/big"), 1);
    harness.send(&Frame::ReqEnd, 1);
    // Read the big response's head and its window, so its thread is parked on
    // credit.
    let mut body = 0;
    loop {
        let (id, frame) = harness.expect_frame();
        assert_eq!(id, 1);
        match frame {
            Frame::RespHead(_) => {}
            Frame::RespBody(chunk) => body += chunk.len(),
            other => panic!("unexpected frame {other:?}"),
        }
        if body >= INITIAL_CREDIT {
            break;
        }
    }

    // A second request is served in full while the first waits: the I/O thread
    // never blocks on a stream, and neither does another request thread.
    harness.send(&support::request("GET", "/small"), 2);
    harness.send(&Frame::ReqEnd, 2);
    let small = harness.read_response(2);
    assert_eq!(small.status(), 200);
    assert_eq!(small.body, b"small");
    assert_eq!(small.end, End::End);
    assert_eq!(harness.expect_done(2), DoneOutcome::Completed);

    // Now let the first one finish as well.
    harness.grant_response(1, 100);
    let mut rest = 0;
    loop {
        let (id, frame) = harness.expect_frame();
        assert_eq!(id, 1);
        match frame {
            Frame::RespBody(chunk) => rest += chunk.len(),
            Frame::RespEnd | Frame::RespEndCost { .. } => break,
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(rest, 100);
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_body_that_is_never_read_stops_at_the_window() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "flow_unread_body",
            "import time\n\
             def app(environ, start_response):\n\
             \x20   time.sleep(0.4)\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b'ignored it all']\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("POST", "/"), 1);
    // The application reads nothing, so this is all the scheduler may send: a
    // window's worth, and no `WINDOW` comes back for it.
    let body = vec![b'a'; INITIAL_CREDIT];
    harness.upload(1, &body);
    assert_eq!(
        harness.upload_credit(1),
        0,
        "nothing was consumed, so nothing was credited back"
    );

    harness.send(&Frame::ReqEnd, 1);
    let response = harness.read_response(1);
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"ignored it all");
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn body_bytes_beyond_the_window_are_a_protocol_error() {
    let mut harness = Harness::for_route(
        "web",
        load_app(
            "flow_over_credit",
            "import time\n\
             def app(environ, start_response):\n\
             \x20   time.sleep(0.4)\n\
             \x20   start_response('200 OK', [])\n\
             \x20   return [b'never reads']\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("POST", "/"), 1);
    // One byte past the credit: §3.1 says the process is recycled, so `serve`
    // has to come back with an error rather than carry on.
    harness.send_body(1, &vec![b'a'; INITIAL_CREDIT]);
    harness.send_body(1, b"!");

    let err = harness.finish().expect_err("serve must refuse the overrun");
    assert!(
        matches!(err, ViewError::Protocol(_)),
        "expected a protocol error, got {err:?}"
    );
}
