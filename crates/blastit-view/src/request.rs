//! What one request shares between its thread, the I/O thread and `wsgi.input`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Instant;

use blastit_frame::{DoneOutcome, ReqHead, RequestCost};
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
    /// `time.thread_time_ns`, which is `CLOCK_THREAD_CPUTIME_ID` (D5), or
    /// `None` when the interpreter did not have it.
    ///
    /// The clock is resolved once, before the fork, rather than per request:
    /// the per-request cost is measured by calling it, which is a plain
    /// function call, and the interpreter lookup would be the expensive part.
    pub(crate) clock: Option<Py<PyAny>>,
}

impl AppContext {
    /// The calling thread's CPU time in nanoseconds (D5).
    ///
    /// `None` when the clock is unavailable or its call failed: the view then
    /// reports no CPU time at all rather than a wrong one.
    pub(crate) fn cpu_ns(&self, py: Python<'_>) -> Option<u64> {
        let clock = self.clock.as_ref()?;
        clock.bind(py).call0().ok()?.extract().ok()
    }
}

/// The span of one request, measured two ways (D5).
///
/// Started when the request thread picks the request up and stopped when its
/// response ends, so it covers `serve_request`: the WSGI callable, the
/// iteration of its iterable, the iterable's `close()` and the frames written
/// back — which is what the scheduler charges. The scheduler clamps it against
/// its own measurements (the time to this view's first frame, and the time it
/// observed itself), so a view process that lies here cannot buy its clients a
/// bigger share of the queue.
pub(crate) struct Span {
    started: Instant,
    cpu_ns: Option<u64>,
}

impl Span {
    /// Start measuring.
    pub(crate) fn start(py: Python<'_>, ctx: &AppContext) -> Self {
        Self {
            started: Instant::now(),
            cpu_ns: ctx.cpu_ns(py),
        }
    }

    /// Stop measuring, and report the wall and thread-CPU times.
    ///
    /// `cpu_ns` is `None` when this thread could not measure its own CPU time —
    /// no `time.thread_time_ns`, or a reading that failed at one end of the
    /// span. Absent is not zero: zero is a measurement, and this is the absence
    /// of one ([`RequestCost::cpu_ns`]).
    pub(crate) fn stop(self, py: Python<'_>, ctx: &AppContext) -> RequestCost {
        let cpu_ns = match (self.cpu_ns, ctx.cpu_ns(py)) {
            // Both ends read: the difference is this thread's CPU time over
            // the span. The clock is monotonic, so it cannot go backwards;
            // `saturating_sub` is there for a platform that disagrees.
            (Some(before), Some(after)) => Some(after.saturating_sub(before)),
            _ => None,
        };
        RequestCost::new(
            u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            cpu_ns,
        )
    }
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
    // D5: the measurement starts with the request thread's own work and ends
    // when its response ends, so it covers `serve_request` — the WSGI call, the
    // iteration of its iterable, the iterable's `close()` and the frames this
    // function writes back.
    let span = Span::start(py, ctx);
    let resp = Arc::new(Response::new(Arc::clone(writer), id, Arc::clone(state)));
    let input = match Bound::new(
        py,
        Input::new(Arc::clone(state), body, id, Arc::clone(writer)),
    ) {
        Ok(input) => input,
        Err(err) => return failed(py, &resp, err, span.stop(py, ctx)),
    };
    let start_response = match Bound::new(py, StartResponse::new(Arc::clone(&resp))) {
        Ok(start_response) => start_response,
        Err(err) => return failed(py, &resp, err, span.stop(py, ctx)),
    };
    let environ = match environ::build(py, head, &ctx.route_name, &ctx.script_name, &input) {
        Ok(environ) => environ,
        Err(err) => return failed(py, &resp, err, span.stop(py, ctx)),
    };

    // The application runs the same way it would under any WSGI server: a
    // callable, the environ, and the start_response it was handed. It may have
    // written a body through `write()` before it raised, which is why the
    // failure is answered with `failed` and not directly with a `500`.
    let result = match ctx.app.bind(py).call1((environ, start_response)) {
        Ok(result) => result,
        Err(err) => return failed(py, &resp, err, span.stop(py, ctx)),
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
        // Everything the WSGI call does is behind us — the iteration, the
        // writes it made, and `close()` — so this is the cost (D5). Only the
        // frame that ends the response is still to come, and delivering it is
        // the channel's work, not the app's.
        let cost = span.stop(py, ctx);
        return match resp.finish(py, Some(cost)) {
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
                cost,
            ),
            Err(RespError::Channel(_)) => DoneOutcome::Errored,
        };
    };

    failed(py, &resp, err, span.stop(py, ctx))
}

/// Answer a failure of the application.
///
/// PEP 3333 splits the two cases at the response head: a failure before it can
/// still be answered with a short `500`, whereas a failure after it can only
/// abandon the response with `RESP_ABORT`. The traceback goes to stderr either
/// way (`wsgi.errors`, which is the same file).
fn failed(py: Python<'_>, resp: &Response, err: PyErr, cost: RequestCost) -> DoneOutcome {
    err.print(py);
    if resp.head_sent() {
        let _ = resp.abort(py, "application error after the response head was sent");
    } else {
        let _ = resp.send_internal_error(py, Some(cost));
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

    #[test]
    fn the_span_measures_wall_time_and_thread_cpu_time() {
        // D5: both numbers come from the same span, and the CPU one is this
        // thread's own clock. If the interpreter has no thread clock the view
        // reports *no* CPU time rather than a zero it did not measure — which is
        // why the assertions are conditional.
        Python::attach(|py| {
            let ctx = AppContext {
                app: py.None(),
                route_name: "web".to_string(),
                script_name: String::new(),
                clock: crate::thread_clock(py),
            };
            let raw = ctx.clock.is_some();
            let span = Span::start(py, &ctx);
            // Spend a little CPU time inside the span.
            let mut spin = 0u64;
            for step in 0..200_000u64 {
                spin = spin.wrapping_add(step);
            }
            let cost = span.stop(py, &ctx);
            assert!(spin > 0);
            assert!(cost.wall_ns > 0, "wall time is always measurable");
            if raw {
                let cpu_ns = cost.cpu_ns.expect("the interpreter has a thread clock");
                assert!(
                    cpu_ns <= cost.wall_ns,
                    "CPU time cannot exceed the wall time it happened in: {cost:?}"
                );
                assert!(cpu_ns > 0, "the thread ran Python: {cost:?}");
            } else {
                assert_eq!(cost.cpu_ns, None, "no clock, no CPU time");
            }

            // A context with no clock at all reports no CPU time, and still
            // reports the wall time.
            let blind = AppContext {
                app: py.None(),
                route_name: "web".to_string(),
                script_name: String::new(),
                clock: None,
            };
            assert_eq!(blind.cpu_ns(py), None);
            let span = Span::start(py, &blind);
            let cost = span.stop(py, &blind);
            assert_eq!(cost.cpu_ns, None);
            assert!(cost.wall_ns > 0, "wall time is measurable without a clock");

            // Half a clock — the start read, the end not — claims nothing about
            // the CPU either.
            let half = Span {
                started: std::time::Instant::now(),
                cpu_ns: Some(1_000),
            };
            assert_eq!(half.stop(py, &blind).cpu_ns, None);
        });
    }
}
