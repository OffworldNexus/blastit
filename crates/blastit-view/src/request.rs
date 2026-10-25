//! What one request shares between its thread, the I/O thread and `wsgi.input`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use blastit_frame::{DoneOutcome, ReqHead};
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::environ;
use crate::flow::Flow;
use crate::input::{BodyMsg, Input};
use crate::response::{RespError, Response, StartResponse, resp_error_to_py};
use crate::wire::Writer;

/// The shared state of one request.
///
/// Three threads touch it: the I/O thread (cancels the request, meters the body
/// against its credit, grants credit for the response, delivers the frames),
/// the request thread (writes the response, credits the request body back) and,
/// through [`crate::input::Input`], whatever application threads read the body.
///
/// A single flag keeps the two possible writers of the response honest:
/// `terminal` — the one `RESP_END` or `RESP_ABORT` of this request has been
/// claimed. It is claimed exactly once, with [`RequestState::claim_terminal`],
/// so two paths racing to end the same response cannot both go out.
/// `cancelled` discards the response entirely, and closes the flow-control
/// window so a request thread blocked on credit wakes up.
#[derive(Debug)]
pub(crate) struct RequestState {
    cancelled: AtomicBool,
    terminal: AtomicBool,
    flow: Arc<Flow>,
}

impl RequestState {
    /// State for a request that has just started, with a fresh window in each
    /// direction (§3.1).
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            terminal: AtomicBool::new(false),
            flow: Flow::new(),
        })
    }

    /// The flow-control windows of this request.
    pub(crate) fn flow(&self) -> &Flow {
        &self.flow
    }

    /// Whether the scheduler cancelled this request.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Mark the request cancelled: its output is discarded from now on, and a
    /// thread blocked on response credit is woken to stop writing.
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.flow.response().close();
    }

    /// Whether the one terminal frame has been sent or claimed.
    pub(crate) fn is_terminal(&self) -> bool {
        self.terminal.load(Ordering::SeqCst)
    }

    /// Whether the response may not be written any more, one way or another.
    pub(crate) fn output_closed(&self) -> bool {
        self.is_cancelled() || self.is_terminal()
    }

    /// Claim the right to send this request's single terminal frame.
    ///
    /// Returns `true` for exactly one caller, ever: the first one to ask. Every
    /// other caller must stay quiet, because a request ends with either
    /// `RESP_END` or `RESP_ABORT` and never with both.
    pub(crate) fn claim_terminal(&self) -> bool {
        !self.terminal.swap(true, Ordering::SeqCst)
    }

    /// Stop waiting for response credit: no more is coming.
    ///
    /// Used when the channel ends with the request still running, so its thread
    /// can finish and be joined instead of blocking forever.
    pub(crate) fn close_window(&self) {
        self.flow.response().close();
    }
}

/// Everything a request thread needs that is the same for every request.
pub(crate) struct AppContext {
    /// The WSGI callable, imported once before the fork (§4) and the same in
    /// every request.
    pub(crate) app: Py<PyAny>,
    /// The route's name, for `blastit.route`.
    pub(crate) route_name: String,
    /// The route's mount point, for `SCRIPT_NAME`.
    pub(crate) script_name: String,
}

/// One request, as the OS thread that serves it sees it.
pub(crate) struct Worker {
    /// The request's id on `view.chan`.
    pub(crate) id: u32,
    /// The head the scheduler sent.
    pub(crate) head: ReqHead,
    /// The request body, fed by the I/O thread.
    pub(crate) body: Receiver<BodyMsg>,
    /// The channel's single writer.
    pub(crate) writer: Arc<Writer>,
    /// What this request shares with the I/O thread.
    pub(crate) state: Arc<RequestState>,
    /// The route's process-wide context.
    pub(crate) ctx: Arc<AppContext>,
}

impl Worker {
    /// Serve the request and report how it ended.
    ///
    /// The interpreter is attached for the duration of the call and released
    /// again before it returns: whatever reports the outcome — normally the
    /// thread's slot guard — does so only once nothing of the request still
    /// touches Python. There is exactly one `DONE` per `REQ_HEAD`, and the
    /// guard is what sends it.
    pub(crate) fn run(self) -> DoneOutcome {
        let Worker {
            id,
            head,
            body,
            writer,
            state,
            ctx,
        } = self;
        Python::attach(|py| serve_request(py, &head, &ctx, &state, &writer, body, id))
    }
}

/// The body of one request thread, with the GIL held.
fn serve_request(
    py: Python<'_>,
    head: &ReqHead,
    ctx: &AppContext,
    state: &Arc<RequestState>,
    writer: &Arc<Writer>,
    body: Receiver<BodyMsg>,
    id: u32,
) -> DoneOutcome {
    let resp = Arc::new(Response::new(Arc::clone(writer), id, Arc::clone(state)));
    let input = match Bound::new(
        py,
        Input::new(Arc::clone(state), body, id, Arc::clone(writer)),
    ) {
        Ok(input) => input,
        Err(err) => return failed(py, &resp, err),
    };
    let start_response = match Bound::new(py, StartResponse::new(Arc::clone(&resp))) {
        Ok(start_response) => start_response,
        Err(err) => return failed(py, &resp, err),
    };
    let environ = match environ::build(py, head, &ctx.route_name, &ctx.script_name, &input) {
        Ok(environ) => environ,
        Err(err) => return failed(py, &resp, err),
    };

    // The application runs the same way it would under any WSGI server: a
    // callable, the environ, and the start_response it was handed. It may have
    // written a body through `write()` before it raised, which is why the
    // failure is answered with `failed` and not directly with a `500`.
    let result = match ctx.app.bind(py).call1((environ, start_response)) {
        Ok(result) => result,
        Err(err) => return failed(py, &resp, err),
    };

    let mut failure = None;
    match result.try_iter() {
        Err(err) => failure = Some(err),
        Ok(mut iter) => {
            for item in iter.by_ref() {
                let item = match item {
                    Ok(item) => item,
                    Err(err) => {
                        failure = Some(err);
                        break;
                    }
                };
                let chunk = match chunk_bytes(&item) {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        failure = Some(err);
                        break;
                    }
                };
                if let Err(err) = resp.write_chunk(py, chunk) {
                    failure = Some(resp_error_to_py(err));
                    break;
                }
            }
        }
    }

    // `close()` runs before `RESP_END` and after everything else, error or not
    // (ARCHITECTURE.md §2.5): request_finished handlers and the database
    // connection are done before the worker slot is released. A close() that
    // raises is reported to stderr and marks the request errored, but it does
    // not change a response that was already streamed.
    let mut close_failed = false;
    if let Some(err) = call_close(&result) {
        err.print(py);
        close_failed = true;
    }

    // A cancelled request is over: nothing more is sent for it, and the
    // scheduler only needs to know the slot is free.
    if state.is_cancelled() {
        return DoneOutcome::Aborted;
    }

    let Some(err) = failure else {
        return match resp.finish(py) {
            Ok(()) => {
                if close_failed {
                    DoneOutcome::Errored
                } else {
                    DoneOutcome::Completed
                }
            }
            // Without a start_response there is no response at all.
            Err(RespError::NotStarted) => failed(
                py,
                &resp,
                PyRuntimeError::new_err("the application never called start_response"),
            ),
            Err(RespError::Channel(_)) => DoneOutcome::Errored,
        };
    };

    failed(py, &resp, err)
}

/// Answer a failure of the application.
///
/// PEP 3333 splits the two cases at the response head: a failure before it can
/// still be answered with a short `500`, whereas a failure after it can only
/// abandon the response with `RESP_ABORT`. The traceback goes to stderr either
/// way (`wsgi.errors`, which is the same file).
fn failed(py: Python<'_>, resp: &Response, err: PyErr) -> DoneOutcome {
    err.print(py);
    if resp.head_sent() {
        let _ = resp.abort(py, "application error after the response head was sent");
    } else {
        let _ = resp.send_internal_error(py);
    }
    DoneOutcome::Errored
}

/// The bytes of one item of a WSGI iterable, or the `TypeError` PEP 3333 wants
/// for anything else.
fn chunk_bytes<'a>(item: &'a Bound<'_, PyAny>) -> PyResult<&'a [u8]> {
    match item.cast::<PyBytes>() {
        Ok(bytes) => Ok(bytes.as_bytes()),
        Err(_) => Err(PyTypeError::new_err(
            "the application yielded something that is not bytes",
        )),
    }
}

/// Call the iterable's `close()`, if it has one.
///
/// A failing `close()` is reported to the caller so it can be logged and the
/// request marked as errored, but it does not change the response: whatever the
/// application streamed has already been streamed, and turning a complete
/// response into an aborted one because a cleanup handler raised would corrupt
/// it.
fn call_close(result: &Bound<'_, PyAny>) -> Option<PyErr> {
    match result.getattr_opt("close") {
        Ok(Some(close)) if close.is_callable() => close.call0().err(),
        Ok(_) => None,
        Err(err) => Some(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blastit_frame::INITIAL_WINDOW;

    #[test]
    fn the_terminal_frame_is_claimed_exactly_once() {
        let state = RequestState::new();
        assert!(!state.is_terminal());
        assert!(state.claim_terminal());
        assert!(state.is_terminal());
        assert!(!state.claim_terminal());
        assert!(!state.claim_terminal());
    }

    #[test]
    fn cancelling_closes_the_output_and_the_window() {
        let state = RequestState::new();
        assert!(!state.output_closed());
        // Spend the whole response window, then cancel: a writer with no credit
        // left must give up rather than wait for a grant that cannot come.
        let window = u64::from(INITIAL_WINDOW) as usize;
        assert_eq!(state.flow().response().take(window), window);
        state.cancel();
        assert!(state.is_cancelled());
        assert!(state.output_closed());
        assert!(!state.is_terminal());
        assert_eq!(state.flow().response().take(1), 0, "the window is closed");
    }

    #[test]
    fn every_request_has_its_own_window() {
        let first = RequestState::new();
        let second = RequestState::new();
        assert!(first.flow().body_received(10));
        assert!(second.flow().body_received(10));
        first.flow().body_granted(100);
        assert_eq!(
            first.flow().body_left(),
            i64::from(INITIAL_WINDOW) + 90,
            "grants belong to the request that granted them"
        );
        assert_eq!(
            second.flow().body_left(),
            i64::from(INITIAL_WINDOW) - 10,
            "and so do spends"
        );
    }
}
