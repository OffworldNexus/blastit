//! View process: an embedded CPython serving exactly one route over `view.chan`
//! (ARCHITECTURE.md §2.5, §3.1, §3.2, §9.3).
//!
//! A view process is one route of the manifest, running under that route's uid.
//! By the time [`serve`] is called, `init` has already initialised CPython,
//! imported the application, taken the GIL, forked and called
//! `PyOS_AfterFork_Child()` while holding it (§4, §9.3); [`serve`] is what the
//! forked child runs — on that thread, with the GIL still held. It therefore
//! **never initialises or finalises the interpreter**: the interpreter and the
//! application are inherited, and touching either here would break the fork
//! contract.
//!
//! # What it does
//!
//! ```text
//! READY ──▶ [ I/O thread: read frames, decode, dispatch ]   (GIL released)
//!              │ REQ_HEAD
//!              ▼
//!           worker thread ──▶ Python::attach (once) ──▶ environ ──▶ app(environ, start_response)
//!              │                                            │
//!              │                             RESP_HEAD ◀────┤ lazily, on the first
//!              │                             RESP_BODY ◀────┤ non-empty chunk
//!              │                             RESP_END/ABORT ◀┤ or at the end
//!              ▼                                            ▼
//!           DONE (after close(), with the GIL released) ──▶ park (GIL released)
//! ```
//!
//! * [`serve`] takes a [`Python`] token and detaches for the whole of its work
//!   (`Python::detach`), because its caller has the GIL: the I/O loop must not
//!   hold it, or the request threads it starts could never attach.
//! * The calling thread is the I/O thread. It reads frames until the channel's
//!   far end closes, and then waits for the workers still serving a request
//!   (§2.5). It never waits for a stream: every per-stream buffer is bounded by
//!   a credit window instead (§3.1's flow control).
//! * Every `REQ_HEAD` is handed to a **worker**, one OS thread that attaches to
//!   the interpreter once and then serves requests one after another. A worker
//!   parks — with the GIL released — while there is nothing to do, and is
//!   started only when no parked worker can take the request, so the stack
//!   grows with demand instead of holding `max_threads` threads up front. Each
//!   request still gets its own `wsgi.input` and its own response: a worker
//!   builds a PEP 3333 `environ`, calls the application, streams the response
//!   and runs the iterable's `close()`. `DONE` is sent after that, exactly once
//!   per `REQ_HEAD`, with the GIL released again — it is what releases the
//!   scheduler's worker slot, and it must not wait for Python.
//! * The request thread is the only place that can measure what a request
//!   cost, so it does (D5): the wall time of `serve_request` for that request —
//!   the WSGI call, the iteration of its iterable, the iterable's `close()` and
//!   the frames written back — and its own `CLOCK_THREAD_CPUTIME_ID` over the
//!   same span, both reported in the response's final frame
//!   (`RESP_END_COST`; the CPU time is absent, not zero, when the interpreter
//!   has no thread clock). The scheduler, not the view, decides what to do with
//!   the numbers.
//! * One writer owns the channel's write side, one frame at a time, so frames
//!   from concurrent requests never interleave mid-frame and no sender waits
//!   behind another stream's body.
//!
//! # Flow control (§3.1)
//!
//! * **Request bodies.** The scheduler may send `INITIAL_WINDOW` bytes without
//!   asking. The view sends `WINDOW` as the application consumes `wsgi.input`
//!   — in batches, and at the end of every read — so a slow application slows
//!   its own client down and nothing else. Body bytes beyond the credit are a
//!   protocol violation: [`serve`] returns [`ViewError::Protocol`] and the
//!   process is recycled.
//! * **Response bodies.** Every request starts with `INITIAL_WINDOW` of credit;
//!   `WINDOW` frames from the scheduler grant more. A request thread that runs
//!   out of credit blocks in its write — with the GIL released, on a condvar —
//!   exactly as it would on a slow client's socket. `CANCEL`, and the end of
//!   the channel, close the window so the thread wakes up and finishes.
//!
//! # Decisions the architecture leaves open
//!
//! * **`PATH_INFO` and `SCRIPT_NAME`.** The target's path is percent-decoded
//!   and read as latin-1, and `SCRIPT_NAME` is stripped from its front *only on
//!   a path boundary*: with `SCRIPT_NAME = /app`, `/app/list` becomes `/list`
//!   and `/application` is left alone. [`ViewConfig::validate`] refuses a mount
//!   point that could not be stripped unambiguously.
//! * **Header names with an underscore are dropped.** Otherwise `X_User` and
//!   `X-User` would both arrive as `HTTP_X_USER` and a client could smuggle a
//!   value past whatever validated the dashed spelling; the same rule drops an
//!   empty header name, which would become the bare `HTTP_`.
//! * **`CANCEL` makes reads raise `OSError`.** The thread itself keeps running
//!   — Python threads cannot be killed — but every further frame of that
//!   request is discarded, and `DONE` is still sent when the thread ends.
//! * **`max_threads` is a safety net.** A view that is at its limit answers a
//!   new `REQ_HEAD` with `503` + `RESP_END` + `DONE` from the I/O thread,
//!   without starting anything. The scheduler's budgets should never let this
//!   happen, and reuse must not weaken it: the limit counts *live* requests,
//!   not worker threads, so a parked worker cannot turn a `503` into a wait.
//! * **A worker that panics** still releases its slot: a drop guard removes it
//!   from the registry and sends a best-effort `DONE { Errored }`, and the
//!   worker retires instead of serving another request on a thread that
//!   unwound.
//! * **A worker's Python thread state outlives one request.** The worker
//!   attaches once and stays attached, so `threading.local` — and anything else
//!   an application keeps per thread — persists from one request to the next.
//!   That is what every threaded WSGI server does; it is the one user-visible
//!   consequence of reuse.
//!
//! # Workers and the GIL
//!
//! A worker attaches to the interpreter once, when its thread starts, and keeps
//! that attachment for as long as it lives, so the cost of a thread state is
//! paid once per worker and not once per request. It **releases the GIL**
//! (`Python::detach`) for every moment it is parked: a parked worker that held
//! the GIL would deadlock the whole process, because the I/O thread and every
//! other worker would wait on it. All of this goes through pyo3's public
//! [`Python::attach`]; nothing here initialises or finalises the interpreter,
//! which is inherited from the fork (§4, §9.3).
//!
//! # Errors
//!
//! [`serve`] returns `Err` only for a broken channel — an I/O error, bytes that
//! are not a frame stream, a frame a view may not receive, body bytes beyond
//! the credit — or a configuration it cannot serve. Whatever the application
//! does is its own request's problem and never reaches the caller. Before
//! returning, `serve` waits for every request thread it started.
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
mod flow;
mod input;
mod request;
mod response;
mod sync;
mod wire;

use std::collections::{HashMap, VecDeque};
use std::io::{ErrorKind, Read};
use std::os::unix::net::UnixStream;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use blastit_frame::{Decoder, DoneOutcome, Frame, HEADER_LEN, Limits, ReqHead, RespHead, TypeCode};
use pyo3::prelude::*;

pub use crate::config::ViewConfig;
pub use crate::error::ViewError;

use crate::input::BodyMsg;
use crate::request::{AppContext, RequestState, Worker};
use crate::sync::lock;
use crate::wire::Writer;

/// How much of the channel is read at once. One frame's worth or more, so a
/// full frame usually arrives in one syscall.
const READ_SIZE: usize = 64 * 1024;

/// How long a parked worker waits for another request before retiring.
///
/// Long enough that a view under steady traffic keeps its threads across the
/// gaps between requests, short enough that a view which has gone quiet gives
/// its stacks back. This is the "keep a small stack, not a whole pool" half of
/// §1's memory-frugal goal: a view idle for the keep-alive holds no worker at
/// all.
const IDLE_KEEPALIVE: Duration = Duration::from_secs(30);

/// The most workers that stay parked between requests.
///
/// A view keeps a few stacks warm — that is the point of reuse — but not as
/// many as its concurrency once reached: a burst of a hundred requests would
/// otherwise leave a hundred stacks behind. Workers beyond this retire as soon
/// as their request is done rather than park.
const IDLE_POOL_MAX: usize = 8;

/// Serve one view process on `chan` until the scheduler's end closes.
///
/// `py` is the calling thread's attachment to the interpreter: `init` holds it
/// when it calls this in a freshly forked child, right after
/// `PyOS_AfterFork_Child()` (§4). The whole call runs with the GIL released
/// (`Python::detach`), which is what lets the request threads this function
/// starts attach to the interpreter. The interpreter is never initialised or
/// finalised here.
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
/// frame stream, the scheduler sent a frame a view may not receive or more body
/// than its credit allowed, or the configuration cannot be served. The
/// interpreter is left as it was found, and the caller owns the process's exit.
pub fn serve(
    py: Python<'_>,
    chan: UnixStream,
    app: Py<PyAny>,
    cfg: ViewConfig,
) -> Result<(), ViewError> {
    // D5: the thread-CPU clock is resolved here, while the interpreter is
    // being held for the fork, and not per request.
    let clock = thread_clock(py);
    // The caller may be holding the GIL (init is, §4); nothing below needs it,
    // and the request threads do.
    py.detach(move || serve_detached(chan, app, cfg, clock))
}

/// `time.thread_time_ns`, the interpreter's name for
/// `CLOCK_THREAD_CPUTIME_ID` (D5).
///
/// The request thread measures its own CPU time with it. It is a plain Python
/// callable from the standard library, so measuring costs no new dependency and
/// no new syscall wrapper; `None` when the interpreter does not have it, in
/// which case views report no CPU time at all.
fn thread_clock(py: Python<'_>) -> Option<Py<PyAny>> {
    let module = py.import("time").ok()?;
    module.getattr("thread_time_ns").ok().map(Bound::unbind)
}

/// The body of [`serve`], with the GIL released.
fn serve_detached(
    chan: UnixStream,
    app: Py<PyAny>,
    cfg: ViewConfig,
    clock: Option<Py<PyAny>>,
) -> Result<(), ViewError> {
    cfg.validate().map_err(ViewError::Config)?;
    let limits = cfg.frame_limits;
    let mut chan = chan;
    // One writer for the whole process; the reading end stays here.
    let writer = Arc::new(Writer::new(chan.try_clone()?, limits));
    writer.write_frame(&Frame::Ready, 0)?;

    let mut view = View {
        pool: Pool::new(
            writer,
            Arc::new(Mutex::new(Registry {
                live: HashMap::new(),
            })),
            Arc::new(AppContext {
                app,
                route_name: cfg.route_name,
                script_name: cfg.script_name,
                clock,
            }),
        ),
        max_threads: cfg.max_threads,
    };

    let result = view.read_requests(&mut chan, limits);
    view.pool.finish();
    result
}

/// One request waiting for a worker, or already being served by one.
struct Job {
    /// The request's id on `view.chan`.
    id: u32,
    /// The head the scheduler sent.
    head: ReqHead,
    /// The far end of the request's `wsgi.input`.
    body: Receiver<BodyMsg>,
    /// What this request shares with the I/O thread.
    state: Arc<RequestState>,
}

/// One live request's handles, as the I/O thread knows them.
struct Live {
    /// The far end of the request's `wsgi.input`.
    tx: Sender<BodyMsg>,
    /// The request's shared state, including both windows.
    state: Arc<RequestState>,
}

/// Every request that is still running.
struct Registry {
    live: HashMap<u32, Live>,
}

/// The hand-off between the I/O thread and the parked workers.
struct Park {
    /// The jobs waiting for a worker, and the workers waiting for a job.
    inner: Mutex<PoolInner>,
    /// Wakes a parked worker when a job arrives, or when the view ends.
    wake: Condvar,
}

/// What the I/O thread and the parked workers share.
struct PoolInner {
    /// Jobs waiting for a worker. A job usually skips this queue — it is handed
    /// straight to a parked worker — so it is almost always empty.
    jobs: VecDeque<Job>,
    /// How many workers are parked right now.
    idle: usize,
    /// Set when the channel ends: no worker parks again, and every parked one
    /// wakes and retires.
    closing: bool,
}

impl PoolInner {
    /// Whether a parked worker is waiting and not already matched to a job.
    ///
    /// Each queued job will be taken by a parked worker, so the workers still
    /// available are the parked ones minus the queued jobs. When this is
    /// `false` every parked worker is already spoken for, and a new request
    /// must get a thread of its own: queueing it behind one of them would turn
    /// concurrency into a queue.
    fn has_free_worker(&self) -> bool {
        self.jobs.len() < self.idle
    }
}

/// The stack of OS threads a view reuses to serve requests (§2.5).
///
/// A worker attaches to the interpreter once and then serves requests one after
/// another: taking a job when one is waiting, parking as an idle worker when
/// none is, and retiring after [`IDLE_KEEPALIVE`]. Threads are started only
/// when a request arrives and no parked worker can take it, so a quiet view
/// holds no stack and demand is what grows the stack — never `max_threads`
/// threads allocated up front. At most [`IDLE_POOL_MAX`] workers stay parked:
/// a view between requests keeps a few stacks warm and lets the rest go.
///
/// Why reuse at all, when the cost of a thread was once considered the price of
/// "one OS thread per request": starting a thread, attaching it to the
/// interpreter and tearing its thread state down again costs more than a
/// trivial request does (§8, D5's profile). The application contract does not
/// change — still one request at a time on its thread, with its own
/// `wsgi.input` and its own response — but per-thread state the application
/// keeps (`threading.local`) now lives as long as the worker does.
struct Pool {
    /// The hand-off the workers park on.
    park: Arc<Park>,
    /// Every worker this process has started, joined when the channel ends.
    threads: Vec<JoinHandle<()>>,
    /// How many workers have been started, for naming their threads.
    started: u64,
    /// The channel's single writer, shared with every worker.
    writer: Arc<Writer>,
    /// Every request still running.
    registry: Arc<Mutex<Registry>>,
    /// The route's process-wide context.
    ctx: Arc<AppContext>,
}

/// The state of one view process between `READY` and the channel's end.
struct View {
    /// The worker stack, and the process-wide handles it shares.
    pool: Pool,
    /// The largest number of requests this process serves at once, or `None`
    /// for no limit.
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
                // A read that was interrupted or that hit a timeout says
                // nothing about the channel: keep reading. (The caller may have
                // set a timeout, and a signal can arrive at any time.)
                Err(err)
                    if matches!(
                        err.kind(),
                        ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                // The peer's end is gone: a reset is an EOF with an attitude.
                // There is nobody left to answer, so the loop ends the way it
                // does on a clean EOF and the request threads wind down.
                Err(err)
                    if matches!(
                        err.kind(),
                        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
                    ) =>
                {
                    break;
                }
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
            self.pool.reap();
        }
        result
    }

    /// Handle one frame from the scheduler.
    fn dispatch(&mut self, id: u32, frame: Frame) -> Result<(), ViewError> {
        match frame {
            Frame::ReqHead(head) => self.start(id, head),
            Frame::ReqBody(bytes) => self.deliver_body(id, bytes),
            Frame::Window { bytes } => {
                self.grant_response(id, bytes);
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

    /// Start a request for `head`, on a parked worker or a new one.
    ///
    /// Admission is decided here and nowhere else, so reuse cannot change it: a
    /// request that arrives with the process already at `max_threads` live
    /// requests is refused with `503` even if a worker is parked, and a refused
    /// request never waits for a worker.
    fn start(&mut self, id: u32, head: ReqHead) -> Result<(), ViewError> {
        if id == 0 {
            // A request without a stream could not be answered at all.
            return Err(ViewError::Protocol(
                "REQ_HEAD with id 0: a request needs a stream".to_string(),
            ));
        }
        self.pool.reap();

        let state = RequestState::new();
        let (tx, body) = channel();
        {
            let mut registry = lock(&self.pool.registry);
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
                    tx: tx.clone(),
                    state: Arc::clone(&state),
                },
            );
        }

        let job = Job {
            id,
            head,
            body,
            state,
        };
        if self.pool.assign(job).is_err() {
            // No worker could take the request and a new thread could not be
            // started: it has not been picked up by anyone, so release its slot
            // and answer it exactly as the cap does.
            lock(&self.pool.registry).live.remove(&id);
            return self.refuse(id, 503, "Service Unavailable");
        }
        Ok(())
    }

    /// Hand a message to the request's `wsgi.input`, spending its credit.
    ///
    /// Frames for a request that has already finished, been cancelled or been
    /// ended are dropped: nothing is waiting for them.
    fn deliver_body(&self, id: u32, bytes: Vec<u8>) -> Result<(), ViewError> {
        let Some((tx, state)) = self.live(id) else {
            return Ok(());
        };
        if state.output_closed() {
            return Ok(());
        }
        // The scheduler may not send more body than its credit: doing so is a
        // broken protocol, and the process is recycled (§3.1).
        if !state.flow().body_received(bytes.len()) {
            return Err(ViewError::Protocol(format!(
                "request {id} sent {} body bytes beyond its window",
                bytes.len()
            )));
        }
        let _ = tx.send(BodyMsg::Data(bytes));
        Ok(())
    }

    /// Hand one message to a live request's input.
    fn deliver(&self, id: u32, msg: BodyMsg) {
        if let Some((tx, _)) = self.live(id) {
            let _ = tx.send(msg);
        }
    }

    /// Grant a request's response body `bytes` more credit.
    ///
    /// A window for a request that is no longer running is stale scheduling
    /// bookkeeping, not a protocol error: the response it belongs to is over.
    fn grant_response(&self, id: u32, bytes: u32) {
        if let Some((_, state)) = self.live(id) {
            state.flow().response().grant(bytes);
        }
    }

    /// Cancel a request: its output is discarded from now on.
    ///
    /// The flow-control window is closed too, so a request thread blocked in a
    /// write wakes up instead of waiting for credit that will never arrive.
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
        self.pool.writer.write_frame(&Frame::RespHead(head), id)?;
        self.pool.writer.write_frame(&Frame::RespEnd, id)?;
        self.pool.writer.write_frame(
            &Frame::Done {
                outcome: DoneOutcome::Completed,
            },
            id,
        )?;
        Ok(())
    }

    /// The message route to a request that is still running.
    fn live(&self, id: u32) -> Option<(Sender<BodyMsg>, Arc<RequestState>)> {
        lock(&self.pool.registry)
            .live
            .get(&id)
            .map(|live| (live.tx.clone(), Arc::clone(&live.state)))
    }
}

impl Pool {
    /// A worker stack with nothing in it: workers are started as demand comes.
    fn new(writer: Arc<Writer>, registry: Arc<Mutex<Registry>>, ctx: Arc<AppContext>) -> Self {
        Self {
            park: Arc::new(Park {
                inner: Mutex::new(PoolInner {
                    jobs: VecDeque::new(),
                    idle: 0,
                    closing: false,
                }),
                wake: Condvar::new(),
            }),
            threads: Vec::new(),
            started: 0,
            writer,
            registry,
            ctx,
        }
    }

    /// Hand `job` to a parked worker that is not already committed, or start
    /// one for it.
    ///
    /// A parked worker is only free if no queued job is already waiting for it,
    /// which is what `jobs < idle` compares: the count of workers parked minus
    /// the count of jobs they have already been matched with is the number
    /// still available. Without that check a second concurrent request would
    /// queue behind the first on the one parked worker — reuse would quietly
    /// serialise requests that used to run in parallel.
    ///
    /// # Errors
    ///
    /// When no worker is free and a new thread could not be started. The job
    /// has not been picked up by anyone then, so the caller releases the
    /// request's slot and answers `503`; this never turns into a wait.
    fn assign(&mut self, job: Job) -> std::io::Result<()> {
        {
            let mut inner = lock(&self.park.inner);
            if inner.has_free_worker() {
                inner.jobs.push_back(job);
                self.park.wake.notify_one();
                return Ok(());
            }
        }
        self.spawn(job)
    }

    /// Start one worker that serves `first` before it parks.
    fn spawn(&mut self, first: Job) -> std::io::Result<()> {
        let park = Arc::clone(&self.park);
        let writer = Arc::clone(&self.writer);
        let registry = Arc::clone(&self.registry);
        let ctx = Arc::clone(&self.ctx);
        self.started += 1;
        let handle = std::thread::Builder::new()
            .name(format!("blastit-view-worker-{}", self.started))
            .spawn(move || worker_loop(park, writer, registry, ctx, first))?;
        self.threads.push(handle);
        Ok(())
    }

    /// Join the handles of workers that have already retired.
    ///
    /// A worker removes its own registry entry and stops parking on its own, but
    /// its `JoinHandle` stays here until it is reaped; without this a view that
    /// served a long burst would hold one handle per thread it ever started,
    /// finished or not.
    fn reap(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        let mut live = Vec::with_capacity(self.threads.len());
        for handle in self.threads.drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                live.push(handle);
            }
        }
        self.threads = live;
    }

    /// Wind the worker stack down after the channel is done with.
    ///
    /// Closing the pool is what retires the parked workers: they wake from their
    /// park and stop instead of waiting for a request that cannot come. The
    /// channel is over, so no request's body will grow any further and no
    /// response credit will ever arrive either: every live request is told both,
    /// which is what unblocks an application waiting for frames, or a worker
    /// waiting for a window, so that the join below can finish. A request that
    /// still has credit left finishes its current frames — the scheduler may
    /// still be reading, it just is not writing. The join is what makes
    /// [`serve`] wait for every request it accepted, exactly once each.
    fn finish(&mut self) {
        {
            let mut inner = lock(&self.park.inner);
            inner.closing = true;
        }
        self.park.wake.notify_all();

        let live: Vec<(Sender<BodyMsg>, Arc<RequestState>)> = lock(&self.registry)
            .live
            .values()
            .map(|live| (live.tx.clone(), Arc::clone(&live.state)))
            .collect();
        for (tx, state) in live {
            let _ = tx.send(BodyMsg::End);
            state.close_window();
        }
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Park {
    /// The next job for the calling worker, or `None` when the view is closing
    /// or the worker has been idle for longer than [`IDLE_KEEPALIVE`].
    ///
    /// This is the one place a worker blocks, and it is called with the GIL
    /// released; parking here while holding it would deadlock the process. The
    /// job queue is checked before the idle limit, so a worker that wakes to a
    /// job takes it even if it was about to retire.
    fn next_job(&self) -> Option<Job> {
        let mut inner = lock(&self.inner);
        loop {
            if let Some(job) = inner.jobs.pop_front() {
                return Some(job);
            }
            if inner.closing || inner.idle >= IDLE_POOL_MAX {
                return None;
            }
            inner.idle += 1;
            let (guard, timeout) = self
                .wake
                .wait_timeout(inner, IDLE_KEEPALIVE)
                .unwrap_or_else(PoisonError::into_inner);
            inner = guard;
            inner.idle -= 1;
            if timeout.timed_out() && inner.jobs.is_empty() && !inner.closing {
                return None;
            }
        }
    }
}

/// The body of one pooled worker thread.
///
/// The interpreter is attached once, here, for the whole life of the worker:
/// the Python thread state — and with it whatever the application keeps in
/// `threading.local` — survives from one request to the next, and a thread
/// state is no longer paid for on every request. The GIL is released
/// (`Python::detach`) for every moment the worker is parked; a parked worker
/// that held it would stop both the I/O thread and every other worker.
///
/// The attachment is pyo3's, never a second initialisation or finalisation of
/// the interpreter: the interpreter is inherited from the fork (§4, §9.3).
fn worker_loop(
    park: Arc<Park>,
    writer: Arc<Writer>,
    registry: Arc<Mutex<Registry>>,
    ctx: Arc<AppContext>,
    first: Job,
) {
    Python::attach(|py| {
        let mut next = Some(first);
        loop {
            let Some(job) = next else {
                break;
            };
            if serve_one(py, &writer, &registry, &ctx, job) {
                // The request panicked. The slot guard sent its `DONE` and
                // freed its slot while unwinding, but a thread that unwound is
                // not a thread to hand another request to.
                break;
            }
            // Park, with the GIL released, until there is another request or
            // the view ends.
            next = py.detach(|| park.next_job());
        }
    });
}

/// Serve one job on this worker, reporting whether the request panicked.
///
/// Takes the token the worker already holds: attaching once for the life of the
/// worker, rather than once per request, is where most of the saving is.
fn serve_one(
    py: Python<'_>,
    writer: &Arc<Writer>,
    registry: &Arc<Mutex<Registry>>,
    ctx: &Arc<AppContext>,
    job: Job,
) -> bool {
    let Job {
        id,
        head,
        body,
        state,
    } = job;
    let writer = Arc::clone(writer);
    let registry = Arc::clone(registry);
    let ctx = Arc::clone(ctx);
    let guard_state = Arc::clone(&state);
    let guard_writer = Arc::clone(&writer);
    let guard_registry = Arc::clone(&registry);
    guarded(id, guard_state, guard_writer, guard_registry, move || {
        let worker = Worker {
            id,
            head,
            body,
            writer,
            state,
            ctx,
        };
        worker.run(py)
    })
}

/// Run one request under its slot guard, and report whether the request panicked.
///
/// The guard is built *inside* the unwind boundary, so a panic anywhere in the
/// request — including the interpreter — drops it, and its [`Drop`] sends the
/// one `DONE { Errored }` that frees the scheduler's slot. The caller must
/// retire the worker when this returns `true`: the slot is released, but the
/// thread is not handed another request.
fn guarded<F>(
    id: u32,
    state: Arc<RequestState>,
    writer: Arc<Writer>,
    registry: Arc<Mutex<Registry>>,
    body: F,
) -> bool
where
    F: FnOnce() -> DoneOutcome,
{
    catch_unwind(AssertUnwindSafe(|| {
        let guard = SlotGuard {
            id,
            state: &state,
            writer: &writer,
            registry: &registry,
            done: false,
        };
        let outcome = body();
        guard.finish(outcome);
    }))
    .is_err()
}

/// Releases a request's slot however its thread ends, panics included.
struct SlotGuard<'a> {
    id: u32,
    state: &'a Arc<RequestState>,
    writer: &'a Arc<Writer>,
    registry: &'a Arc<Mutex<Registry>>,
    /// Set once `DONE` has been sent the normal way.
    done: bool,
}

impl SlotGuard<'_> {
    /// Report how the request ended and drop out of the registry.
    fn finish(mut self, outcome: DoneOutcome) {
        let _ = self.writer.write_frame(&Frame::Done { outcome }, self.id);
        self.done = true;
        lock(self.registry).live.remove(&self.id);
    }
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        // The thread is unwinding: the application never reported an outcome,
        // and the scheduler would wait for a slot forever, so say the worst and
        // let the scheduler recycle the process.
        let _ = self.writer.write_frame(
            &Frame::Done {
                outcome: DoneOutcome::Errored,
            },
            self.id,
        );
        self.state.cancel();
        lock(self.registry).live.remove(&self.id);
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
        assert_eq!(frame_kind(&Frame::Window { bytes: 1 }), "WINDOW");
    }

    /// Decode whatever the writer has already put on the channel.
    fn frames_from(peer: UnixStream, limits: Limits) -> Vec<(u32, Frame)> {
        let mut peer = peer;
        peer.set_nonblocking(true).expect("non-blocking");
        let mut decoder = Decoder::new(limits);
        let mut buf = [0u8; 256];
        loop {
            use std::io::Read as _;
            match peer.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => decoder.push(&buf[..n]),
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) => panic!("read failed: {err}"),
            }
        }
        let mut frames = Vec::new();
        while let Some(frame) = decoder.next_frame().expect("decode") {
            frames.push(frame);
        }
        frames
    }

    /// A registry holding one live request, as the I/O thread would have.
    fn registry_with(id: u32, state: &Arc<RequestState>) -> Arc<Mutex<Registry>> {
        let registry = Arc::new(Mutex::new(Registry {
            live: HashMap::new(),
        }));
        let (tx, rx) = channel();
        lock(&registry).live.insert(
            id,
            Live {
                tx,
                state: Arc::clone(state),
            },
        );
        // Keep the receiver alive for the test's duration.
        std::mem::forget(rx);
        registry
    }

    #[test]
    fn the_slot_guard_releases_a_request_that_never_reported() {
        let limits = Limits::default();
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, limits));
        let state = RequestState::new();
        let registry = registry_with(7, &state);

        // Dropped without `finish`, exactly as a panicking request thread does.
        {
            let _guard = SlotGuard {
                id: 7,
                state: &state,
                writer: &writer,
                registry: &registry,
                done: false,
            };
        }

        assert!(
            lock(&registry).live.is_empty(),
            "the guard must release the slot"
        );
        assert!(state.is_cancelled(), "and wake anything blocked on it");
        assert_eq!(
            frames_from(peer, limits),
            vec![(
                7,
                Frame::Done {
                    outcome: DoneOutcome::Errored
                }
            )]
        );
    }

    #[test]
    fn the_slot_guard_sends_nothing_extra_after_a_normal_finish() {
        let limits = Limits::default();
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, limits));
        let state = RequestState::new();
        let registry = registry_with(9, &state);

        let guard = SlotGuard {
            id: 9,
            state: &state,
            writer: &writer,
            registry: &registry,
            done: false,
        };
        guard.finish(DoneOutcome::Completed);

        assert!(lock(&registry).live.is_empty());
        assert!(!state.is_cancelled());
        assert_eq!(
            frames_from(peer, limits),
            vec![(
                9,
                Frame::Done {
                    outcome: DoneOutcome::Completed
                }
            )],
            "exactly one DONE, and no Errored after it"
        );
    }

    #[test]
    fn a_panicking_request_releases_its_slot_and_retires_the_worker() {
        // The pool runs every request under `guarded`, whose guard lives inside
        // the unwind boundary. A panic must still produce exactly one `DONE`
        // (the `Errored` the scheduler needs to free the slot), and `guarded`
        // must report it so the worker retires rather than serve again.
        let limits = Limits::default();
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, limits));
        let state = RequestState::new();
        let registry = registry_with(11, &state);

        let panicked = guarded(
            11,
            Arc::clone(&state),
            Arc::clone(&writer),
            Arc::clone(&registry),
            || panic!("the application blew the worker up"),
        );

        assert!(panicked, "a panic must retire the worker");
        assert!(
            lock(&registry).live.is_empty(),
            "and release the slot all the same"
        );
        assert!(state.is_cancelled(), "and wake anything blocked on it");
        assert_eq!(
            frames_from(peer, limits),
            vec![(
                11,
                Frame::Done {
                    outcome: DoneOutcome::Errored
                }
            )],
            "exactly one DONE, and it says Errored"
        );
    }

    #[test]
    fn a_served_request_keeps_the_worker() {
        // The other half of the panic test: a normal request reports no panic,
        // so the worker parks and can serve again.
        let limits = Limits::default();
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Writer::new(ours, limits));
        let state = RequestState::new();
        let registry = registry_with(13, &state);

        let panicked = guarded(
            13,
            Arc::clone(&state),
            Arc::clone(&writer),
            Arc::clone(&registry),
            || DoneOutcome::Completed,
        );

        assert!(!panicked, "a served request keeps its worker");
        assert!(lock(&registry).live.is_empty());
        assert_eq!(
            frames_from(peer, limits),
            vec![(
                13,
                Frame::Done {
                    outcome: DoneOutcome::Completed
                }
            )]
        );
    }

    /// A job for a request that does nothing, for the pool's bookkeeping tests.
    fn queued_job(id: u32) -> Job {
        let (_tx, body) = channel();
        Job {
            id,
            head: ReqHead {
                method: b"GET".to_vec(),
                target: b"/".to_vec(),
                headers: Vec::new(),
                remote_addr: "203.0.113.7".to_string(),
                scheme: blastit_frame::Scheme::Http,
                server_name: "view.test".to_string(),
                server_port: 80,
                peer_addr: "203.0.113.7:51000".to_string(),
            },
            body,
            state: RequestState::new(),
        }
    }

    #[test]
    fn a_parked_worker_committed_to_a_job_is_not_free_for_another() {
        // The pool only reuses a worker that no queued job is waiting for.
        // Getting this wrong would serialise two requests onto one thread.
        let mut inner = PoolInner {
            jobs: VecDeque::new(),
            idle: 0,
            closing: false,
        };
        assert!(!inner.has_free_worker(), "no worker at all");

        inner.idle = 1;
        assert!(inner.has_free_worker(), "one parked worker, nothing queued");

        inner.jobs.push_back(queued_job(1));
        assert!(
            !inner.has_free_worker(),
            "the one parked worker is already spoken for"
        );

        inner.idle = 2;
        assert!(inner.has_free_worker(), "a second parked worker is free");
    }
}
