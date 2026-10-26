//! The view's frame protocol and request lifecycle, driven from a socketpair.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::{Duration, Instant};

use blastit_frame::{DoneOutcome, Frame, Limits, RespHead};
use blastit_view::{ViewConfig, ViewError};
use support::{End, Harness, global_int, hello_app};

#[test]
fn ready_is_the_first_frame() {
    let mut harness = Harness::for_route("web", support::load_app("ready", &hello_app("hi")));
    let (id, frame) = harness.expect_frame();
    assert_eq!(id, 0, "READY has no stream");
    assert_eq!(frame, Frame::Ready);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_get_serves_head_body_end_and_done() {
    let mut harness =
        Harness::for_route("web", support::load_app("hello", &hello_app("hello world")));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);

    let response = harness.read_response(1);
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"hello world");
    assert_eq!(response.end, End::End);
    assert_eq!(
        response
            .head
            .headers
            .iter()
            .map(|(name, _)| String::from_utf8_lossy(name).into_owned())
            .collect::<Vec<_>>(),
        vec!["Content-Type".to_string()]
    );
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn the_returned_write_callable_sends_the_head_immediately() {
    let module = support::load_module(
        "write_callable",
        "def app(environ, start_response):\n\
         \x20   write = start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\")])\n\
         \x20   write(b\"one \")\n\
         \x20   write(b\"\")\n\
         \x20   write(b\"two\")\n\
         \x20   return []\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"one two", "empty chunks are ignored");
    assert_eq!(response.end, End::End);
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn close_runs_before_done() {
    let module = support::load_module(
        "close_before_done",
        "closed = 0\n\
         class Body:\n\
         \x20   def __iter__(self):\n\
         \x20       yield b\"one\"\n\
         \x20       yield b\"two\"\n\
         \x20   def close(self):\n\
         \x20       global closed\n\
         \x20       closed += 1\n\
         def app(environ, start_response):\n\
         \x20   start_response(\"200 OK\", [])\n\
         \x20   return Body()\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);
    let response = harness.read_response(1);
    assert_eq!(response.body, b"onetwo");
    assert_eq!(response.end, End::End);
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);

    // `DONE` is sent after `close()` returned, so by the time the scheduler
    // sees it the side effect is already there (ARCHITECTURE.md §2.5).
    assert_eq!(global_int(&module, "closed"), 1);
    assert!(harness.finish().is_ok());
}

#[test]
fn close_also_runs_when_the_iterable_raises() {
    let module = support::load_module(
        "close_after_exception",
        "closed = 0\n\
         class Body:\n\
         \x20   def __iter__(self):\n\
         \x20       raise RuntimeError(\"iterable boom\")\n\
         \x20   def close(self):\n\
         \x20       global closed\n\
         \x20       closed += 1\n\
         def app(environ, start_response):\n\
         \x20   start_response(\"200 OK\", [])\n\
         \x20   return Body()\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);
    let response = harness.read_response(1);
    assert_eq!(response.status(), 500, "nothing had been written yet");
    assert_eq!(response.end, End::End);
    assert_eq!(harness.expect_done(1), DoneOutcome::Errored);
    assert_eq!(global_int(&module, "closed"), 1);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_failing_application_becomes_a_500_before_the_head() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "raises_early",
            "def app(environ, start_response):\n\
             \x20   raise RuntimeError(\"boom\")\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 500);
    assert!(!response.body.is_empty(), "a 500 carries a short body");
    assert_eq!(response.end, End::End);
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_failing_application_aborts_after_the_head() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "raises_late",
            "def app(environ, start_response):\n\
             \x20   write = start_response(\"200 OK\", [])\n\
             \x20   write(b\"partial\")\n\
             \x20   raise RuntimeError(\"late boom\")\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"partial");
    assert!(
        matches!(response.end, End::Abort(_)),
        "a response that already started is abandoned, not ended: {:?}",
        response.end
    );
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_chunk_that_is_not_bytes_is_an_error() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "not_bytes",
            "def app(environ, start_response):\n\
             \x20   start_response(\"200 OK\", [])\n\
             \x20   return [\"a str, not bytes\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(
        response.status(),
        500,
        "the head had not been sent, so the failure is still answerable"
    );
    assert_eq!(response.end, End::End);
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn an_application_without_start_response_becomes_a_500() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "never_started",
            "def app(environ, start_response):\n\
             \x20   return [b\"no head\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 500);
    assert_eq!(response.end, End::End);
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_bad_status_line_is_refused_by_start_response() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "bad_status",
            "def app(environ, start_response):\n\
             \x20   start_response(\"nonsense\", [])\n\
             \x20   return [b\"never\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 500, "the ValueError left the app early");
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn cancel_discards_the_rest_of_a_response_but_still_sends_done() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "cancel_me",
            "import time\n\
             def app(environ, start_response):\n\
             \x20   write = start_response(\"200 OK\", [])\n\
             \x20   write(b\"first\")\n\
             \x20   time.sleep(0.4)\n\
             \x20   write(b\"second\")\n\
             \x20   return [b\"third\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);
    // The head and the first chunk are out before the scheduler gives up.
    let (id, frame) = harness.expect_frame();
    assert_eq!(id, 1);
    let Frame::RespHead(head) = frame else {
        panic!("expected RESP_HEAD, got {frame:?}");
    };
    assert_eq!(head.status, 200);
    assert_eq!(
        harness.expect_frame(),
        (1, Frame::RespBody(b"first".to_vec()))
    );

    // The application is still sleeping when the cancel arrives, so nothing it
    // writes afterwards may reach the scheduler.
    harness.send(&Frame::Cancel, 1);
    let frames = harness.drain_until_done(1);
    assert_eq!(
        frames,
        vec![Frame::Done {
            outcome: DoneOutcome::Aborted
        }],
        "only DONE follows a cancel"
    );
    assert!(harness.finish().is_ok());
}

#[test]
fn max_threads_refuses_a_request_with_503() {
    let cfg = ViewConfig {
        max_threads: Some(1),
        ..ViewConfig::for_route("web")
    };
    let mut harness = Harness::start(support::load_app("busy", &support::sleeping_app(0.4)), cfg);
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // The first request is still sleeping in its thread when the second
    // arrives, so the safety net answers it instead of the application.
    harness.send(&support::request("GET", "/one"), 1);
    harness.send(&Frame::ReqEnd, 1);
    harness.send(&support::request("GET", "/two"), 2);
    harness.send(&Frame::ReqEnd, 2);

    let refused = harness.read_response(2);
    assert_eq!(refused.status(), 503);
    assert_eq!(refused.end, End::End);
    assert_eq!(harness.expect_done(2), DoneOutcome::Completed);

    let served = harness.read_response(1);
    assert_eq!(served.status(), 200);
    assert_eq!(served.body, b"slept");
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    let ended = harness.finish();
    assert!(ended.is_ok(), "serve returned {ended:?}");
}

#[test]
fn a_large_response_is_split_into_frames_that_fit() {
    let limits = Limits::new(256).unwrap();
    let cfg = ViewConfig {
        frame_limits: limits,
        ..ViewConfig::for_route("web")
    };
    let module = support::load_module(
        "large_body",
        "def app(environ, start_response):\n\
         \x20   start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\")])\n\
         \x20   return [b\"x\" * 1000]\n",
    );
    let mut harness = Harness::start(support::app_of(&module), cfg);
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);
    let mut chunks = 0;
    let mut body = Vec::new();
    loop {
        let (id, frame) = harness.expect_frame();
        assert_eq!(id, 1);
        match frame {
            Frame::RespHead(head) => assert_eq!(head.status, 200),
            Frame::RespBody(chunk) => {
                assert!(
                    chunk.len() <= limits.max_payload(),
                    "a body frame carried {} bytes, over the payload limit of {}",
                    chunk.len(),
                    limits.max_payload()
                );
                chunks += 1;
                body.extend_from_slice(&chunk);
            }
            Frame::RespEnd | Frame::RespEndCost { .. } => break,
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(body, vec![b'x'; 1000]);
    assert_eq!(chunks, 5, "1000 bytes at 247 payload bytes each");
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_body_that_is_never_read_stalls_instead_of_aborting() {
    // The window is the whole safety net now: the scheduler may send one
    // window's worth and no more, and nothing is aborted or buffered.
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "ignores_body",
            "import time\n\
             def app(environ, start_response):\n\
             \x20   time.sleep(0.4)\n\
             \x20   start_response(\"200 OK\", [])\n\
             \x20   return [b\"ignored it all\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("POST", "/"), 1);
    harness.upload(1, &vec![b'a'; support::INITIAL_CREDIT]);
    assert_eq!(
        harness.upload_credit(1),
        0,
        "an application that reads nothing credits nothing back"
    );

    harness.send(&Frame::ReqEnd, 1);
    let response = harness.read_response(1);
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"ignored it all");
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_second_req_head_for_a_live_request_is_a_protocol_error() {
    let mut harness = Harness::start(
        support::load_app("slow_head", &support::sleeping_app(0.3)),
        ViewConfig::for_route("web"),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/one"), 1);
    harness.send(&support::request("GET", "/two"), 1);

    let err = harness
        .finish()
        .expect_err("serve must refuse the second head");
    assert!(
        matches!(err, ViewError::Protocol(_)),
        "expected a protocol error, got {err:?}"
    );
}

#[test]
fn a_frame_the_view_may_not_receive_is_a_protocol_error() {
    let mut harness = Harness::for_route("web", support::load_app("stray", &hello_app("hi")));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // A response frame on the request direction, and a READY that belongs to
    // the other end of the channel: neither is part of a view's vocabulary.
    harness.send(
        &Frame::RespHead(RespHead {
            status: 200,
            reason: Vec::new(),
            headers: Vec::new(),
        }),
        1,
    );
    let err = harness.finish().expect_err("serve must refuse it");
    assert!(
        matches!(err, ViewError::Protocol(_)),
        "expected a protocol error, got {err:?}"
    );

    let mut harness = Harness::for_route("web", support::load_app("stray_ready", &hello_app("hi")));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));
    harness.send(&Frame::Ready, 0);
    let err = harness.finish().expect_err("serve must refuse it");
    assert!(
        matches!(err, ViewError::Protocol(_)),
        "expected a protocol error, got {err:?}"
    );
}

#[test]
fn bytes_that_are_not_frames_are_a_framing_error() {
    let mut harness = Harness::for_route("web", support::load_app("garbage", &hello_app("hi")));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // Type code 0x00 is not part of the vocabulary, so the decoder refuses it.
    harness.send_raw(&[5, 0, 0, 0, 0x00, 0, 0, 0, 0]);
    let err = harness.finish().expect_err("serve must refuse it");
    assert!(
        matches!(err, ViewError::Frame(_)),
        "expected a framing error, got {err:?}"
    );
}

#[test]
fn an_eof_waits_for_the_request_threads() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app("sleeps_then_answers", &support::sleeping_app(1.0)),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);
    // The scheduler stops writing — no more requests — but keeps reading.
    harness
        .peer
        .shutdown(std::net::Shutdown::Write)
        .expect("half close");

    let started = Instant::now();
    let response = harness.read_response(1);
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"slept");
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(
        started.elapsed() >= Duration::from_millis(500),
        "the request thread ran to completion before DONE"
    );
    assert!(
        harness.finish().is_ok(),
        "EOF after all threads finished is not an error"
    );
}

#[test]
fn an_empty_channel_closes_cleanly() {
    let mut harness =
        Harness::for_route("web", support::load_app("immediate_eof", &hello_app("hi")));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));
    assert!(harness.finish().is_ok());
}

#[test]
fn a_request_that_ends_without_reading_its_body_still_ends() {
    // The application answers and returns while body frames are still arriving:
    // they are dropped, and the request ends normally.
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "leaves_body",
            "def app(environ, start_response):\n\
             \x20   start_response(\"200 OK\", [])\n\
             \x20   return [b\"ignored the body\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("POST", "/"), 1);
    let response = harness.read_response(1);
    assert_eq!(response.body, b"ignored the body");
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);

    harness.send_body(1, b"this body is nobody's");
    harness.send(&Frame::ReqEnd, 1);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_req_head_without_a_stream_is_a_protocol_error() {
    let mut harness = Harness::for_route("web", support::load_app("id_zero", &hello_app("hi")));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 0);
    let err = harness.finish().expect_err("serve must refuse id 0");
    assert!(
        matches!(err, ViewError::Protocol(_)),
        "expected a protocol error, got {err:?}"
    );
}

#[test]
fn a_second_start_response_without_exc_info_is_an_error() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "start_twice",
            "def app(environ, start_response):\n\
             \x20   start_response(\"200 OK\", [])\n\
             \x20   start_response(\"418 Teapot\", [])\n\
             \x20   return [b\"unreachable\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(
        response.status(),
        500,
        "the second call raised before anything was sent"
    );
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_second_start_response_with_exc_info_replaces_an_unsent_head() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "replace_head",
            "import sys\n\
             def app(environ, start_response):\n\
             \x20   start_response(\"200 OK\", [])\n\
             \x20   try:\n\
             \x20       raise ValueError(\"recoverable\")\n\
             \x20   except ValueError:\n\
             \x20       write = start_response(\"418 Teapot\", [], sys.exc_info())\n\
             \x20   write(b\"teapot\")\n\
             \x20   return []\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(
        response.status(),
        418,
        "PEP 3333 lets the application replace a head it has not sent"
    );
    assert_eq!(response.body, b"teapot");
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn an_exc_info_tuple_is_re_raised_once_the_head_is_sent() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "exc_info_tuple",
            "import sys\n\
             def app(environ, start_response):\n\
             \x20   write = start_response(\"200 OK\", [])\n\
             \x20   write(b\"partial\")\n\
             \x20   try:\n\
             \x20       raise ValueError(\"the real failure\")\n\
             \x20   except ValueError:\n\
             \x20       start_response(\"500 Nope\", [], sys.exc_info())\n\
             \x20   return [b\"unreachable\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"partial");
    assert!(
        matches!(response.end, End::Abort(_)),
        "the re-raised exception abandons the rest: {:?}",
        response.end
    );
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn an_exception_instance_is_re_raised_like_an_exc_info_tuple() {
    let mut harness = Harness::for_route(
        "web",
        support::load_app(
            "exc_info_instance",
            "def app(environ, start_response):\n\
             \x20   write = start_response(\"200 OK\", [])\n\
             \x20   write(b\"partial\")\n\
             \x20   start_response(\"500 Nope\", [], ValueError(\"passed straight in\"))\n\
             \x20   return [b\"unreachable\"]\n",
        ),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"partial");
    assert!(matches!(response.end, End::Abort(_)));
    assert_eq!(response.outcome, DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_raising_close_marks_the_request_errored_but_finishes_it() {
    let module = support::load_module(
        "close_raises",
        "class Body:\n\
         \x20   def __iter__(self):\n\
         \x20       yield b\"all there is\"\n\
         \x20   def close(self):\n\
         \x20       raise RuntimeError(\"close failed\")\n\
         def app(environ, start_response):\n\
         \x20   start_response(\"200 OK\", [])\n\
         \x20   return Body()\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    let response = harness.get("/");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.body, b"all there is",
        "a cleanup handler cannot un-stream a response that was already sent"
    );
    assert_eq!(response.end, End::End);
    assert_eq!(
        response.outcome,
        DoneOutcome::Errored,
        "but the scheduler hears that it failed"
    );
    assert!(harness.finish().is_ok());
}

#[test]
fn an_unservable_script_name_is_refused_before_ready() {
    let cfg = ViewConfig {
        script_name: "/app/".to_string(),
        ..ViewConfig::for_route("web")
    };
    let harness = Harness::start(support::load_app("bad_script_name", &hello_app("hi")), cfg);
    let err = harness.finish().expect_err("the mount point is not usable");
    assert!(
        matches!(err, ViewError::Config(_)),
        "expected a configuration error, got {err:?}"
    );

    // Nothing was served, so READY never went out either.
    let cfg = ViewConfig {
        script_name: "app".to_string(),
        ..ViewConfig::for_route("web")
    };
    let mut harness = Harness::start(
        support::load_app("bad_script_name_2", &hello_app("hi")),
        cfg,
    );
    assert!(harness.next_frame().is_none(), "no READY for a bad config");
    assert!(matches!(
        harness.finish().expect_err("still refused"),
        ViewError::Config(_)
    ));
}

#[test]
fn a_read_timeout_on_the_channel_is_retried() {
    // The channel may be handed over with a read timeout (a supervisor's, a
    // signal's): a timed-out read says nothing about the channel, so the view
    // must keep reading rather than give up.
    let mut harness = Harness::start_reading_with_timeout(
        support::load_app("slow_start", &hello_app("after a pause")),
        ViewConfig::for_route("web"),
        Duration::from_millis(50),
    );
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    // Long enough for the I/O thread to hit its timeout more than once.
    std::thread::sleep(Duration::from_millis(200));
    let response = harness.get("/");
    assert_eq!(response.status(), 200);
    assert_eq!(response.body, b"after a pause");
    assert_eq!(response.outcome, DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_response_reports_what_the_request_cost_the_view() {
    // D5: the request thread is the only place that can measure the cost, so
    // it reports both numbers — the wall time of the WSGI call through
    // `serve_request`, and its own thread CPU time — in the last frame of the
    // response.
    let mut harness = Harness::for_route("web", support::load_app("cost", &hello_app("measured")));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);

    let (response, cost) = support::read_response_and_cost(&mut harness, 1);
    assert_eq!(response.body, b"measured");
    assert_eq!(response.end, End::End);
    let cost = cost.expect("the view measured the request");
    assert!(cost.wall_ns > 0, "the wall time is always measurable");
    let cpu_ns = cost
        .cpu_ns
        .expect("CPython's time.thread_time_ns is CLOCK_THREAD_CPUTIME_ID");
    assert!(cpu_ns > 0, "and this request ran Python");
    assert!(
        cpu_ns <= cost.wall_ns,
        "CPU time cannot exceed wall time: {cost:?}"
    );
    assert_eq!(harness.expect_done(1), DoneOutcome::Completed);
    assert!(harness.finish().is_ok());
}

#[test]
fn a_view_that_fails_after_the_head_still_reports_nothing_and_aborts() {
    // The cancel path has no response to end, so there is nothing to report:
    // the scheduler falls back to the time it observed (D5).
    let module = support::load_module(
        "abort_cost",
        "def app(environ, start_response):\n\
         \x20   start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\")])\n\
         \x20   yield b\"part\"\n\
         \x20   raise RuntimeError(\"boom\")\n",
    );
    let mut harness = Harness::for_route("web", support::app_of(&module));
    assert_eq!(harness.expect_frame(), (0, Frame::Ready));

    harness.send(&support::request("GET", "/"), 1);
    harness.send(&Frame::ReqEnd, 1);

    let response = harness.read_response(1);
    assert_eq!(response.body, b"part");
    assert!(matches!(response.end, End::Abort(_)), "{:?}", response.end);
    assert_eq!(harness.expect_done(1), DoneOutcome::Errored);
    assert!(harness.finish().is_ok());
}
