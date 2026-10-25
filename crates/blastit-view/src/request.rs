//! What one request shares between its thread, the I/O thread and `wsgi.input`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use blastit_frame::{DoneOutcome, Frame, ReqHead};
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::environ;
use crate::input::{BodyMsg, Input};
use crate::response::{RespError, Response, StartResponse, resp_error_to_py};
use crate::wire::Writer;

/// The shared state of one request.
///
/// Three threads touch it: the I/O thread (cancels the request, meters the
/// body, aborts it when the body queue overflows, delivers the frames), the
/// request thread (writes the response) and, through [`crate::input::Input`],
/// whatever application threads read the body.
///
/// Two flags keep the two writers of the response honest:
///
/// * `cancelled` — the scheduler sent `CANCEL`. Every later frame of that
///   request is discarded (nothing is sent), reads raise `OSError`, and `DONE`
///   still goes out with [`DoneOutcome::Aborted`](blastit_frame::DoneOutcome).
/// * `terminal` — the one `RESP_END` or `RESP_ABORT` of this request has been
///   claimed. It is claimed exactly once, with [`RequestState::claim_terminal`],
///   which is what stops the I/O thread's abort and the request thread's
///   `RESP_END` from both going out when a body overflows right as the
///   application finishes.
#[derive(Debug)]
pub(crate) struct RequestState {
    cancelled: AtomicBool,
    terminal: AtomicBool,
    overflowed: AtomicBool,
    pending: AtomicUsize,
}

impl RequestState {
    /// State for a request that has just started.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            terminal: AtomicBool::new(false),
            overflowed: AtomicBool::new(false),
            pending: AtomicUsize::new(0),
        })
    }

    /// Whether the scheduler cancelled this request.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Mark the request cancelled, discarding every further response frame.
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
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

    /// Give up on the request because its body queue overflowed, returning
    /// whether this call should be the one to send the `RESP_ABORT`.
    ///
    /// The request stays in its failed state for every later read, and `DONE`
    /// reports it as aborted even when the terminal frame was already claimed
    /// by the application's own `RESP_END`.
    pub(crate) fn overflow(&self) -> bool {
        self.overflowed.store(true, Ordering::SeqCst);
        self.claim_terminal()
    }

    /// Whether the body queue overflowed (see [`crate::BODY_BUFFER_LIMIT`]).
    pub(crate) fn is_overflowed(&self) -> bool {
        self.overflowed.load(Ordering::SeqCst)
    }

    /// Add body bytes the I/O thread has just queued for the application.
    ///
    /// Returns the new total, which the caller compares against
    /// [`crate::BODY_BUFFER_LIMIT`].
    pub(crate) fn add_pending(&self, bytes: usize) -> usize {
        self.pending.fetch_add(bytes, Ordering::SeqCst) + bytes
    }

    /// Note that the application consumed `bytes` body bytes.
    pub(crate) fn consume_pending(&self, bytes: usize) {
        // Saturating: the counter is a safety net, and a wrong zero there must
        // never wrap around into "the queue is enormous".
        let mut current = self.pending.load(Ordering::SeqCst);
        while current != 0 {
            let next = current.saturating_sub(bytes);
            match self.pending.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }

    /// Body bytes queued but not yet read by the application.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }
}

/// Take a lock, treating a poisoned mutex as usable (see [`crate::wire`]).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
    /// Serve the request, then report how it ended.
    ///
    /// The interpreter is attached for the duration of the call and released
    /// again before `DONE` is sent: the scheduler is told the slot is free only
    /// once nothing of the request is still touching Python. `DONE` goes out
    /// exactly once, whatever happened to the application — including a
    /// cancelled request and a channel that cannot be written to any more.
    pub(crate) fn run(self) -> DoneOutcome {
        let Worker {
            id,
            head,
            body,
            writer,
            state,
            ctx,
        } = self;
        let outcome =
            Python::attach(|py| serve_request(py, &head, &ctx, &state, &writer, body, id));
        // Best effort: if the channel is gone the I/O thread has already found
        // out, and there is nobody left to tell anyway.
        let _ = writer.write_frame(&Frame::Done { outcome }, id);
        outcome
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
    let input = match Bound::new(py, Input::new(Arc::clone(state), body)) {
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
    // connection are done before the worker slot is released.
    if let Some(err) = call_close(&result) {
        err.print(py);
    }

    // A cancelled request is over: nothing more is sent for it, and the
    // scheduler only needs to know the slot is free.
    if state.is_cancelled() {
        return DoneOutcome::Aborted;
    }
    // The body queue overflowed, and the I/O thread has already answered the
    // request with RESP_ABORT.
    if state.is_overflowed() {
        return DoneOutcome::Aborted;
    }

    let Some(err) = failure else {
        return match resp.finish(py) {
            Ok(()) => DoneOutcome::Completed,
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
    fn cancelling_closes_the_output_but_not_the_claim() {
        let state = RequestState::new();
        assert!(!state.output_closed());
        state.cancel();
        assert!(state.is_cancelled());
        assert!(state.output_closed());
        assert!(!state.is_terminal());
    }

    #[test]
    fn an_overflow_claims_the_terminal_frame_exactly_once() {
        let state = RequestState::new();
        assert!(!state.is_overflowed());
        assert!(state.overflow());
        assert!(state.is_overflowed());
        assert!(!state.overflow(), "the terminal frame was claimed already");
        assert!(state.is_overflowed());
    }

    #[test]
    fn the_second_terminal_claim_loses_even_after_an_abort() {
        let state = RequestState::new();
        assert!(state.overflow());
        // A request thread finishing at the same moment must not add RESP_END.
        assert!(!state.claim_terminal());
        assert!(state.output_closed());
    }

    #[test]
    fn pending_bytes_are_metered_and_never_underflow() {
        let state = RequestState::new();
        assert_eq!(state.pending(), 0);
        assert_eq!(state.add_pending(10), 10);
        assert_eq!(state.add_pending(5), 15);
        state.consume_pending(7);
        assert_eq!(state.pending(), 8);
        state.consume_pending(100);
        assert_eq!(state.pending(), 0);
        assert_eq!(state.add_pending(1), 1);
    }
}
