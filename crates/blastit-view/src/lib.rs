//! View process: an embedded CPython serving exactly one route over `view.chan`
//! (ARCHITECTURE.md §2.5, §3.1, §9.3).
//!
//! A view process is one route of the manifest, running under that route's uid.
//! By the time [`serve`] is called, `init` has already initialised CPython,
//! imported the application, taken the GIL, forked and called
//! `PyOS_AfterFork_Child()` while holding it (§4, §9.3); [`serve`] is what the
//! forked child runs. It therefore **never initialises or finalises the
//! interpreter** — the interpreter and the application are inherited, and
//! touching either here would break the fork contract.
//!
//! # What it does
//!
//! ```text
//! READY ──▶ [ I/O thread: read frames, decode, dispatch ]
//!              │ REQ_HEAD
//!              ▼
//!           std::thread ──▶ Python::attach ──▶ environ ──▶ app(environ, start_response)
//!              │                                            │
//!              │                             RESP_HEAD ◀────┤ lazily, on the first
//!              │                             RESP_BODY ◀────┤ non-empty chunk
//!              │                             RESP_END/ABORT ◀┤ or at the end
//!              ▼                                            ▼
//!           DONE (after close(), with the GIL released)
//! ```
//!
//! * The calling thread is the I/O thread. It holds no GIL, reads frames until
//!   the channel's far end closes, and then waits for the request threads it
//!   started (§2.5's "one OS thread per request": threads, not a pool, because
//!   the application is not written for cooperative scheduling).
//! * Every `REQ_HEAD` starts a thread that attaches to the interpreter, builds
//!   a PEP 3333 `environ`, calls the application, streams the response and runs
//!   the iterable's `close()`. `DONE` is sent after that, exactly once per
//!   `REQ_HEAD`, with the GIL released again — it is what releases the
//!   scheduler's worker slot, and it must not wait for Python.
//! * One writer owns the channel's write side, so frames from concurrent
//!   requests never interleave mid-frame.
//!
//! # Decisions the architecture leaves open
//!
//! * **`PATH_INFO` and `SCRIPT_NAME`.** The target's path is percent-decoded and
//!   read as latin-1, and `SCRIPT_NAME` is stripped from its front *only on a
//!   path boundary*: with `SCRIPT_NAME = /app`, `/app/list` becomes `/list` and
//!   `/application` is left alone.
//! * **Header names with an underscore are dropped.** Otherwise `X_User` and
//!   `X-User` would both arrive as `HTTP_X_USER` and a client could smuggle a
//!   value past whatever validated the dashed spelling; the same rule drops an
//!   empty header name, which would become the bare `HTTP_`.
//! * **`wsgi.input` is metered, not bounded.** Body frames are queued for the
//!   application on an unbounded queue with a byte cap; a bounded channel would
//!   block the I/O thread — and so every other request in the process — while
//!   an application that does not read holds it up. Past
//!   [`BODY_BUFFER_LIMIT`] the view stops queueing, answers the request with
//!   `RESP_ABORT`, and makes `wsgi.input` raise `OSError`.
//! * **`CANCEL` makes reads raise `OSError`.** The thread itself keeps running
//!   — Python threads cannot be killed — but every further frame of that
//!   request is discarded, and `DONE` is still sent when the thread ends.
//! * **`max_threads` is a safety net.** A view that is at its limit answers a
//!   new `REQ_HEAD` with `503` + `RESP_END` + `DONE` from the I/O thread,
//!   without starting anything. The scheduler's budgets should never let this
//!   happen.
//!
//! # Errors
//!
//! [`serve`] returns `Err` only for a broken channel: an I/O error, bytes that
//! are not a frame stream, or a frame a view may not receive. Whatever the
//! application does is its own request's problem and never reaches the caller.
//! Before returning, `serve` waits for every request thread it started.
//!
//! # Code
//!
//! The crate is `#![forbid(unsafe_code)]`: everything that touches the C API
//! goes through pyo3's safe wrappers, and nothing here starts or stops the
//! interpreter.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![forbid(unsafe_code)]

mod config;
mod environ;
mod error;
mod input;
mod request;
mod response;
mod wire;

use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use blastit_frame::{Decoder, DoneOutcome, Frame, HEADER_LEN, Limits, ReqHead, RespHead, TypeCode};
use pyo3::prelude::*;

pub use crate::config::{BODY_BUFFER_LIMIT, ViewConfig};
pub use crate::error::ViewError;

use crate::input::BodyMsg;
use crate::request::{AppContext, RequestState, Worker, lock};
use crate::wire::Writer;

/// How much of the channel is read at once. One frame's worth or more, so a
/// full frame usually arrives in one syscall.
const READ_SIZE: usize = 64 * 1024;

/// Why an overflowing body queue aborts a request.
const OVERFLOW_REASON: &str =
    "request body exceeded the view's buffer limit before the application read it";

/// Serve one view process on `chan` until the scheduler's end closes.
///
/// `app` is the WSGI callable every request is dispatched to; it must have been
/// imported by the caller, before the fork, and `app: Py<PyAny>` keeps it alive
/// for the lifetime of the process. `cfg` describes the route this process
/// serves.
///
/// The call blocks. It sends `READY` (id 0) before reading anything, so the
/// scheduler knows the process is ready for requests, and it returns only after
/// every request thread it started has finished — see the module documentation
/// for the whole sequence.
///
/// # Errors
///
/// [`ViewError`]: the channel could not be read, the bytes on it are not a
/// frame stream, or the scheduler sent a frame a view may not receive. The
/// interpreter is not touched: it stays initialised, and the caller owns its
/// exit.
pub fn serve(chan: UnixStream, app: Py<PyAny>, cfg: ViewConfig) -> Result<(), ViewError> {
    let limits = cfg.frame_limits;
    let mut chan = chan;
    // One writer for the whole process; the reading end stays here.
    let writer = Arc::new(Writer::new(chan.try_clone()?, limits));
    writer.write_frame(&Frame::Ready, 0)?;

    let mut view = View {
        writer,
        ctx: Arc::new(AppContext {
            app,
            route_name: cfg.route_name,
            script_name: cfg.script_name,
        }),
        registry: Arc::new(Mutex::new(Registry {
            live: HashMap::new(),
        })),
        threads: Vec::new(),
        max_threads: cfg.max_threads,
    };

    let result = view.read_requests(&mut chan, limits);
    view.finish();
    result
}

/// One live request's handles, as the I/O thread knows them.
struct Live {
    /// The far end of the request's `wsgi.input`.
    tx: Sender<BodyMsg>,
    /// The request's shared state.
    state: Arc<RequestState>,
}

/// Every request that is still running.
struct Registry {
    live: HashMap<u32, Live>,
}

/// The state of one view process between `READY` and the channel's end.
struct View {
    writer: Arc<Writer>,
    ctx: Arc<AppContext>,
    registry: Arc<Mutex<Registry>>,
    threads: Vec<JoinHandle<()>>,
    max_threads: Option<u32>,
}

impl View {
    /// Read and dispatch frames until the scheduler's end closes.
    fn read_requests(&mut self, chan: &mut UnixStream, limits: Limits) -> Result<(), ViewError> {
        let mut decoder = Decoder::with_capacity(limits, limits.max_frame() + HEADER_LEN);
        let mut buf = vec![0u8; READ_SIZE];
        let mut result = Ok(());

        'read: loop {
            match chan.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => decoder.push(&buf[..n]),
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) => {
                    result = Err(ViewError::Io(err));
                    break;
                }
            }
            loop {
                match decoder.next_frame() {
                    Ok(None) => break,
                    Ok(Some((id, frame))) => {
                        if let Err(err) = self.dispatch(id, frame) {
                            result = Err(err);
                            break 'read;
                        }
                    }
                    Err(err) => {
                        result = Err(ViewError::Frame(err));
                        break 'read;
                    }
                }
            }
        }
        result
    }

    /// Handle one frame from the scheduler.
    fn dispatch(&mut self, id: u32, frame: Frame) -> Result<(), ViewError> {
        match frame {
            Frame::ReqHead(head) => self.start(id, head),
            Frame::ReqBody(bytes) => {
                self.deliver_body(id, bytes);
                Ok(())
            }
            Frame::ReqEnd => {
                self.deliver(id, BodyMsg::End);
                Ok(())
            }
            Frame::Cancel => {
                self.cancel(id);
                Ok(())
            }
            other => Err(ViewError::Protocol(format!(
                "a view may not receive {} frames",
                frame_kind(&other)
            ))),
        }
    }

    /// Start a request thread for `head`.
    fn start(&mut self, id: u32, head: ReqHead) -> Result<(), ViewError> {
        let state = RequestState::new();
        let (tx, body) = channel();
        {
            let mut registry = lock(&self.registry);
            if registry.live.contains_key(&id) {
                return Err(ViewError::Protocol(format!(
                    "REQ_HEAD for request {id}, which is already running"
                )));
            }
            if let Some(max) = self.max_threads
                && registry.live.len() as u32 >= max
            {
                drop(registry);
                return self.refuse(id, 503, "Service Unavailable");
            }
            registry.live.insert(
                id,
                Live {
                    tx,
                    state: Arc::clone(&state),
                },
            );
        }

        let worker = Worker {
            id,
            head,
            body,
            writer: Arc::clone(&self.writer),
            state,
            ctx: Arc::clone(&self.ctx),
        };
        let registry = Arc::clone(&self.registry);
        let spawned = std::thread::Builder::new()
            .name(format!("blastit-view-req-{id}"))
            .spawn(move || {
                worker.run();
                // The slot is free again (and the input's sender is dropped)
                // once the thread is done with everything else.
                lock(&registry).live.remove(&id);
            });
        match spawned {
            Ok(handle) => {
                self.threads.push(handle);
                Ok(())
            }
            Err(_) => {
                lock(&self.registry).live.remove(&id);
                self.refuse(id, 503, "Service Unavailable")
            }
        }
    }

    /// Hand a message to the request's `wsgi.input`, metering its body.
    ///
    /// Frames for a request that has already finished, been cancelled or been
    /// aborted are dropped: nothing is waiting for them.
    fn deliver_body(&self, id: u32, bytes: Vec<u8>) {
        let Some((tx, state)) = self.live(id) else {
            return;
        };
        if state.output_closed() {
            return;
        }
        if state.add_pending(bytes.len()) > BODY_BUFFER_LIMIT {
            // The application is not keeping up, so nothing may be queued: the
            // request is over, and the scheduler is told so at once rather than
            // waiting for a thread that may never read again.
            if state.overflow() {
                let _ = self.writer.write_frame(
                    &Frame::RespAbort {
                        reason: OVERFLOW_REASON.to_string(),
                    },
                    id,
                );
            }
            let _ = tx.send(BodyMsg::Overflow);
            return;
        }
        let _ = tx.send(BodyMsg::Data(bytes));
    }

    /// Hand one message to a live request's input.
    fn deliver(&self, id: u32, msg: BodyMsg) {
        if let Some((tx, _)) = self.live(id) {
            let _ = tx.send(msg);
        }
    }

    /// Cancel a request: its output is discarded from now on.
    fn cancel(&self, id: u32) {
        if let Some((tx, state)) = self.live(id) {
            state.cancel();
            let _ = tx.send(BodyMsg::Cancelled);
        }
    }

    /// Answer a request the process will not run.
    fn refuse(&self, id: u32, status: u16, reason: &str) -> Result<(), ViewError> {
        let head = RespHead {
            status,
            reason: reason.as_bytes().to_vec(),
            headers: vec![(
                b"Content-Type".to_vec(),
                b"text/plain; charset=utf-8".to_vec(),
            )],
        };
        self.writer.write_frame(&Frame::RespHead(head), id)?;
        self.writer.write_frame(&Frame::RespEnd, id)?;
        self.writer.write_frame(
            &Frame::Done {
                outcome: DoneOutcome::Completed,
            },
            id,
        )?;
        Ok(())
    }

    /// The message route to a request that is still running.
    fn live(&self, id: u32) -> Option<(Sender<BodyMsg>, Arc<RequestState>)> {
        lock(&self.registry)
            .live
            .get(&id)
            .map(|live| (live.tx.clone(), Arc::clone(&live.state)))
    }

    /// Wind the process down after the channel is done with.
    ///
    /// The channel is over, so no request's body will grow any further: telling
    /// each live request that its body has ended unblocks an application that
    /// is waiting for frames that will never come, which is what lets the join
    /// below finish. A request that is still producing output finishes it —
    /// the scheduler may still be reading, it just is not writing.
    fn finish(&mut self) {
        let live: Vec<Sender<BodyMsg>> = lock(&self.registry)
            .live
            .values()
            .map(|live| live.tx.clone())
            .collect();
        for tx in live {
            let _ = tx.send(BodyMsg::End);
        }
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

/// The name of a frame's kind, for a protocol error.
fn frame_kind(frame: &Frame) -> &'static str {
    TypeCode::from_code(frame.code())
        .map(TypeCode::name)
        .unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_kinds_are_named_as_in_the_architecture() {
        assert_eq!(frame_kind(&Frame::Ready), "READY");
        assert_eq!(frame_kind(&Frame::RespBody(vec![1])), "RESP_BODY");
        assert_eq!(
            frame_kind(&Frame::Done {
                outcome: DoneOutcome::Completed
            }),
            "DONE"
        );
    }
}
