//! Shared test harness: a view on a socketpair, driven by a fake scheduler.
//!
//! Every test file uses this the same way: build a WSGI application from Python
//! source, start [`Harness::start`], play the scheduler on the other end of the
//! channel, and (usually) [`Harness::finish`] to check how `serve` returned.

#![allow(dead_code)]

use std::collections::HashMap;
use std::ffi::CString;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use blastit_frame::{Decoder, DoneOutcome, Frame, Limits, ReqHead, RespHead, Scheme, encode};
use blastit_view::{ViewConfig, ViewError, serve};
use pyo3::prelude::*;
use pyo3::types::PyModule;

/// How long a test waits for a frame before giving up.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The scheduler's end of a view channel, and the thread running `serve`.
pub struct Harness {
    /// The end the test writes requests on and reads responses from.
    pub peer: UnixStream,
    decoder: Decoder,
    limits: Limits,
    join: Option<JoinHandle<Result<(), ViewError>>>,
}

impl Harness {
    /// Start a view on a socketpair and hand back its other end.
    pub fn start(app: Py<PyAny>, cfg: ViewConfig) -> Self {
        let limits = cfg.frame_limits;
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        peer.set_read_timeout(Some(TIMEOUT)).expect("read timeout");
        let view_cfg = cfg.clone();
        let join = thread::spawn(move || serve(ours, app, view_cfg));
        Self {
            peer,
            decoder: Decoder::new(limits),
            limits,
            join: Some(join),
        }
    }

    /// Start a view whose only configuration is a route name.
    pub fn for_route(route: &str, app: Py<PyAny>) -> Self {
        Self::start(app, ViewConfig::for_route(route))
    }

    /// Send one frame.
    pub fn send(&mut self, frame: &Frame, id: u32) {
        let mut wire = Vec::new();
        encode(frame, id, &mut wire).expect("encode");
        self.peer.write_all(&wire).expect("write frame");
    }

    /// Send bytes that are not (necessarily) a frame.
    pub fn send_raw(&mut self, bytes: &[u8]) {
        self.peer.write_all(bytes).expect("write bytes");
    }

    /// Send a request body, split into frames like `http` would.
    pub fn send_body(&mut self, id: u32, body: &[u8]) {
        let mut wire = Vec::new();
        self.limits
            .encode_req_body(id, body, &mut wire)
            .expect("encode body");
        self.peer.write_all(&wire).expect("write body");
    }

    /// The next frame, or `None` after a quiet [`TIMEOUT`] or an EOF.
    pub fn next_frame(&mut self) -> Option<(u32, Frame)> {
        loop {
            match self.decoder.next_frame() {
                Ok(Some(frame)) => return Some(frame),
                Ok(None) => {}
                Err(err) => panic!("framing error: {err}"),
            }
            let mut buf = [0u8; 1 << 16];
            match self.peer.read(&mut buf) {
                Ok(0) => return None,
                Ok(n) => self.decoder.push(&buf[..n]),
                Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return None;
                }
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) => panic!("read failed: {err}"),
            }
        }
    }

    /// The next frame, failing the test if nothing arrives.
    pub fn expect_frame(&mut self) -> (u32, Frame) {
        self.next_frame().expect("a frame")
    }

    /// Read one request's response up to its `RESP_END` or `RESP_ABORT`.
    pub fn read_response(&mut self, id: u32) -> Response {
        let (start_id, frame) = self.expect_frame();
        assert_eq!(start_id, id, "response head for another request");
        let Frame::RespHead(head) = frame else {
            panic!("expected RESP_HEAD, got {frame:?}");
        };
        let mut body = Vec::new();
        loop {
            let (frame_id, frame) = self.expect_frame();
            assert_eq!(frame_id, id, "response frame for another request");
            match frame {
                Frame::RespBody(chunk) => body.extend_from_slice(&chunk),
                Frame::RespEnd => {
                    return Response {
                        head,
                        body,
                        end: End::End,
                        outcome: DoneOutcome::default(),
                    };
                }
                Frame::RespAbort { reason } => {
                    return Response {
                        head,
                        body,
                        end: End::Abort(reason),
                        outcome: DoneOutcome::default(),
                    };
                }
                other => panic!("unexpected frame in a response: {other:?}"),
            }
        }
    }

    /// Send a `GET`-style request with no body and read its response and `DONE`.
    pub fn get(&mut self, target: &str) -> Response {
        self.exchange(request("GET", target), &[], 1)
    }

    /// Send a head and a body, then read the response and its `DONE`.
    pub fn exchange(&mut self, head: Frame, body: &[u8], id: u32) -> Response {
        self.send(&head, id);
        if !body.is_empty() {
            self.send_body(id, body);
        }
        self.send(&Frame::ReqEnd, id);
        let mut response = self.read_response(id);
        response.outcome = self.expect_done(id);
        response
    }

    /// Whether nothing arrives within `within`.
    ///
    /// Shortens the read timeout for the check and puts it back afterwards, so
    /// a test can assert "the application has not answered yet" without waiting
    /// for the full timeout.
    pub fn quiet(&mut self, within: Duration) -> bool {
        let previous = self.peer.read_timeout().ok().flatten().unwrap_or(TIMEOUT);
        self.peer.set_read_timeout(Some(within)).expect("timeout");
        let frame = self.next_frame();
        self.peer.set_read_timeout(Some(previous)).expect("timeout");
        frame.is_none()
    }

    /// Send a request with a body and read its response and its `DONE`.
    pub fn post(&mut self, target: &str, body: &[u8]) -> Response {
        self.exchange(request("POST", target), body, 1)
    }

    /// The `DONE` frame of `id`, failing the test if anything else arrives.
    pub fn expect_done(&mut self, id: u32) -> DoneOutcome {
        let (frame_id, frame) = self.expect_frame();
        assert_eq!(frame_id, id, "DONE for another request");
        match frame {
            Frame::Done { outcome } => outcome,
            other => panic!("expected DONE, got {other:?}"),
        }
    }

    /// Every frame of `id` up to and including its `DONE`.
    pub fn drain_until_done(&mut self, id: u32) -> Vec<Frame> {
        let mut frames = Vec::new();
        loop {
            let (frame_id, frame) = self.expect_frame();
            assert_eq!(frame_id, id, "frame for another request");
            let done = matches!(frame, Frame::Done { .. });
            frames.push(frame);
            if done {
                return frames;
            }
        }
    }

    /// Every frame of each id in `ids`, until all of them are done.
    pub fn drain_until_all_done(&mut self, ids: &[u32]) -> HashMap<u32, Vec<Frame>> {
        let mut frames: HashMap<u32, Vec<Frame>> = ids.iter().map(|id| (*id, Vec::new())).collect();
        let mut done = 0;
        while done < ids.len() {
            let (id, frame) = self.expect_frame();
            assert!(ids.contains(&id), "frame for an unknown request {id}");
            if matches!(frame, Frame::Done { .. }) {
                done += 1;
            }
            frames.entry(id).or_default().push(frame);
        }
        frames
    }

    /// Close the scheduler's end and wait for `serve` to return.
    pub fn finish(mut self) -> Result<(), ViewError> {
        let join = self.join.take().expect("started");
        self.peer.shutdown(std::net::Shutdown::Both).ok();
        drop(self.peer);
        join.join().expect("the serve thread panicked")
    }
}

/// A response as the test sees it.
#[derive(Debug)]
pub struct Response {
    /// The head the view sent.
    pub head: RespHead,
    /// The body frames, concatenated.
    pub body: Vec<u8>,
    /// How the response ended.
    pub end: End,
    /// How the request's thread ended, when the test read its `DONE`.
    pub outcome: DoneOutcome,
}

/// How a response ended.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// `RESP_END`: a complete response.
    End,
    /// `RESP_ABORT { reason }`.
    Abort(String),
}

impl Response {
    /// The status of the head.
    pub fn status(&self) -> u16 {
        self.head.status
    }

    /// The body as text, for assertions on the parts of it that are ASCII.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The value of `key` in a body of `key=value` lines (see [`echo_app`]).
    pub fn field(&self, key: &str) -> String {
        for line in self.text().lines() {
            if let Some((name, value)) = line.split_once('=')
                && name == key
            {
                return value.to_string();
            }
        }
        panic!("{key} is not in the response body: {}", self.text());
    }

    /// Whether the body contains `needle`, compared as bytes.
    pub fn contains(&self, needle: &[u8]) -> bool {
        if needle.is_empty() {
            return true;
        }
        self.body
            .windows(needle.len())
            .any(|window| window == needle)
    }
}

/// A request head as the scheduler would build it.
pub fn request(method: &str, target: &str) -> Frame {
    Frame::ReqHead(head(method, target, Vec::new()))
}

/// A request head with the given headers.
pub fn head(method: &str, target: &str, headers: Vec<(&str, &str)>) -> ReqHead {
    ReqHead {
        method: method.as_bytes().to_vec(),
        target: target.as_bytes().to_vec(),
        headers: headers
            .into_iter()
            .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
            .collect(),
        remote_addr: "203.0.113.7:51000".to_string(),
        scheme: Scheme::Http,
        server_name: "view.test".to_string(),
        server_port: 80,
    }
}

/// Import a Python module from source, under a name unique to one test.
pub fn load_module(name: &str, source: &str) -> Py<PyModule> {
    Python::attach(|py| {
        let code = CString::new(source).expect("no NUL in the source");
        let file = CString::new(format!("{name}.py")).expect("no NUL in the file name");
        let module = CString::new(name).expect("no NUL in the module name");
        PyModule::from_code(py, &code, &file, &module)
            .unwrap_or_else(|err| panic!("{name} does not import: {err}"))
            .unbind()
    })
}

/// The `app` callable of a module.
pub fn app_of(module: &Py<PyModule>) -> Py<PyAny> {
    Python::attach(|py| {
        module
            .bind(py)
            .getattr("app")
            .expect("the module defines app")
            .unbind()
    })
}

/// Import a module and hand back its `app`.
pub fn load_app(name: &str, source: &str) -> Py<PyAny> {
    app_of(&load_module(name, source))
}

/// A module global as an integer, for tests that assert on a side effect.
pub fn global_int(module: &Py<PyModule>, name: &str) -> i64 {
    Python::attach(|py| {
        module
            .bind(py)
            .getattr(name)
            .unwrap_or_else(|err| panic!("{name} is not a module global: {err}"))
            .extract()
            .unwrap_or_else(|err| panic!("{name} is not an int: {err}"))
    })
}

/// A module global as a string, for tests that assert on a side effect.
pub fn global_str(module: &Py<PyModule>, name: &str) -> String {
    Python::attach(|py| {
        module
            .bind(py)
            .getattr(name)
            .unwrap_or_else(|err| panic!("{name} is not a module global: {err}"))
            .extract()
            .unwrap_or_else(|err| panic!("{name} is not a str: {err}"))
    })
}

/// The length of a module global, for tests that assert on a side effect.
pub fn global_len(module: &Py<PyModule>, name: &str) -> usize {
    Python::attach(|py| {
        module
            .bind(py)
            .getattr(name)
            .unwrap_or_else(|err| panic!("{name} is not a module global: {err}"))
            .len()
            .unwrap_or_else(|err| panic!("{name} has no length: {err}"))
    })
}

/// A WSGI application that answers `200` with `body`.
pub fn hello_app(body: &str) -> String {
    format!(
        "def app(environ, start_response):\n\
         \x20   start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\")])\n\
         \x20   return [{body:?}.encode(\"latin-1\")]\n"
    )
}

/// A WSGI application that answers `200` with `key=value` lines.
///
/// The values are `str(...)`-ed and latin-1 encoded, so the body carries a
/// string-valued `environ` entry byte for byte (which is what the path and
/// header tests need) while numbers and tuples stay readable.
pub fn echo_app(keys: &[&str]) -> String {
    let keys = keys
        .iter()
        .map(|key| format!("{key:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "def app(environ, start_response):\n\
         \x20   keys = [{keys}]\n\
         \x20   out = \"\\n\".join(\"{{}}={{}}\".format(k, environ.get(k)) for k in keys)\n\
         \x20   start_response(\"200 OK\", [(\"Content-Type\", \"text/plain\")])\n\
         \x20   return [out.encode(\"latin-1\")]\n"
    )
}

/// An application that sleeps for `seconds` before answering `200`.
pub fn sleeping_app(seconds: f64) -> String {
    format!(
        "import time\n\
         def app(environ, start_response):\n\
         \x20   time.sleep({seconds})\n\
         \x20   start_response(\"200 OK\", [])\n\
         \x20   return [b\"slept\"]\n"
    )
}
