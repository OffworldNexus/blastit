//! The scheduler itself: one task, one state machine (ARCHITECTURE.md §2.4).
//!
//! [`run`] wires up `http.chan`, `ctl.chan` and one reader/writer pair per
//! `view.chan`, then loops over the four things that can happen: a frame from
//! http, a frame from a view, a message from init, or a timer. Everything that
//! decides what happens next — routing, admission, dispatch, timeouts,
//! retirement — lives in [`Scheduler`] and runs on that one task, so there are
//! no locks anywhere.
//!
//! # The loop never waits for a peer
//!
//! A shared channel carries many streams, so nothing on the scheduler's task
//! may wait for one slow stream, one slow client or one slow view to catch up
//! (§3.1, "Flow control"). Two rules follow, and both hold here:
//!
//! * **Every peer has its own writer task**, fed by a queue that
//!   [`crate::task::Outbound::push`] fills without awaiting. The byte
//!   accounting keeps that queue bounded: a view whose queue passes
//!   `4 × INITIAL_WINDOW` plus slack is *recycled* instead of buffered, and
//!   `http.chan` passing 64 MiB is fatal for the scheduler.
//! * **Every reader always keeps reading.** Bodies are bounded by per-stream
//!   credit instead of by stopping a channel: each body starts with
//!   `INITIAL_WINDOW` bytes of credit, `WINDOW` frames extend it, and a peer
//!   that sends beyond its credit has broken the protocol.
//!
//! The [`Emit`] callback is the one thing that runs inline: it must not block.
//!
//! # Where the decisions differ from §2.4, and why
//!
//! * The `request` event's `path` is the routed `PATH_INFO`: percent-decoded,
//!   `SCRIPT_NAME` stripped, and never a query string (§8.1). A request the
//!   scheduler answers before routing (a `400`) has no `PATH_INFO`, so it
//!   reports the target with its query string removed instead.
//! * `request_timeout` is measured from dispatch, not from arrival: a request
//!   that waited in the queue then gets its full time on a worker thread, and
//!   the wait itself is reported separately in `queued_ms`.
//! * Percent-decoding, control-byte and `SCRIPT_NAME` failures reuse the
//!   `unmatched` outcome: no view was involved, and §8.1's outcome list is
//!   closed.
//! * The health path is matched against the decoded path *before*
//!   `BLASTIT_SCRIPT_NAME` is stripped, so it is the absolute path the
//!   operator configured, and only `GET` and `HEAD` reach it.
//! * A view that exceeds its response credit, or one whose outbound queue
//!   overflows, is retired with `credit_overflow`, a
//!   [`RetireReason::Other`] (§8.1's reasons are init's: idle, max_requests,
//!   stuck_threads, memory_pressure, lru_cap).
//! * A `RESP_ABORT` from a view is reported with the `view_died` outcome, and
//!   a `DRAIN` rejects new requests with `503`/`queue_full` (the closest of the
//!   seven outcomes, which §8.1 closes).
//! * A scheduler-written body is sent even for a `HEAD` request: deciding not
//!   to is http's business, not the scheduler's (§2.3).
//! * §8 Sizing's derived `BLASTIT_MAX_VIEW_PROCESSES` default is taken twice:
//!   provisionally at startup, so the eager or preforked burst obeys it, and
//!   again at the first `SPAWNED`, which is the first moment §8's "usage after
//!   preload" exists (init forks every view from its supervision loop, which
//!   begins only once the app is preloaded, §4). From then on it is re-derived
//!   from the cost per view that is *measured* as views come up (see
//!   [`Scheduler::measure_view_memory`]) and reported as `process.cap` when it
//!   changes.
//! * The LRU cap only ever retires a view that is `READY`, has nothing in
//!   flight and has no request waiting in the queue for its own route
//!   ([`Scheduler::evictable_view`]). Anything else and a request would fail
//!   only because the cap was busy killing the very process that would have
//!   served it — a real instance did that thousands of times a second, and
//!   answered every request `503`. With no evictable view the request keeps
//!   waiting for a worker slot, bounded by `BLASTIT_QUEUE_TIMEOUT` as usual.
//! * Spawning is also refused while aggregate usage is above
//!   `BLASTIT_MEM_HIGH_WATERMARK` unless an evictable view can be retired
//!   first, so memory pressure never becomes retire-then-respawn thrash. A view
//!   retired for memory pressure cools its route down for
//!   [`SchedulerConfig::spawn_backoff`], as does one retired for the cap before
//!   it ever served anything.
//! * Two settings exist that §8.2 does not name, because §2.4 asks for the
//!   behaviour without giving it a variable: [`SchedulerConfig::spawn_backoff`]
//!   ("don't spin") and the per-peer byte caps in this module.
//!
//! # Fair queuing (D5)
//!
//! With `BLASTIT_FAIR_QUEUING=on` the admission queue is ordered by
//! [`crate::fair`] instead of by arrival: the two-level flow with the least
//! budget spent goes first, within a route and across routes. Off — the
//! default — is exactly the FIFO §2.4 describes, including its one subtlety:
//! a request at the head that cannot be served must not hold up a later one
//! whose route has a ready view, so the walk skips it.
//!
//! The queue holds stream ids either way, and every path that removes a
//! request (dispatch, timeout, cancel, release) goes through the same
//! [`Admission::remove`], so the two orderings cannot drift apart.
//!
//! The cost a fair flow is charged comes from the view process — the only
//! place that can measure it — in the `RESP_END_COST` frame, which is the
//! response's last frame in its second wire form: the wall time of
//! `serve_request` (the WSGI call through the iterable's `close()`, and the
//! frames written back) and the thread's CPU time over the same span. The
//! report is untrusted: a view is sandboxed and may be compromised, so it is
//! clamped **between** the time up to the view's first response frame — which
//! it cannot shorten without delaying its own client — **and** the wall time
//! the scheduler itself observed between dispatch and completion; a request
//! whose end frame carries no report (an older build, a timeout, a
//! cancellation, a view that died) charges the observed time instead. The
//! event keeps both numbers as `cost_ns` (what was charged) and `cpu_ns` (what
//! the worker reported, `null` when it had no thread clock to read), and it is
//! still emitted when the scheduler learns the request ended — at `RESP_END`
//! for a response, at release for everything else.

use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use blastit_events::{
    Component, Event, EventKind, Outcome, ProcessCap, ProcessRetired, RequestInfo, RetireReason,
};
use blastit_frame::{
    Ctl, DoneOutcome, Frame, FrameError, INITIAL_WINDOW, Limits, ReqHead, RequestCost, RespHead,
};
use blastit_manifest::RoutingTable;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::config::{FAIR_GC_INTERVAL, SchedulerConfig, derived_view_cap, nanos};
use crate::ctl::CtlChannel;
use crate::error::SchedulerError;
use crate::fair::{Cost, FairQueue, FlowRef, NextFlow};
use crate::path::{
    is_control_byte, normalize_script_name, path_of_target, percent_decode, strip_script_name,
};
use crate::route::RouteInfo;
use crate::task::{
    OutFrame, Outbound, PushError, ctl_frame_size, read_frames, release, write_peer,
};

/// How many `http.chan` frames may pile up before its reader task waits.
const HTTP_IN_CAPACITY: usize = 64;
/// How many frames may be queued towards one view before its reader waits.
const VIEW_IN_CAPACITY: usize = 256;
/// How many `ctl.chan` messages may pile up before its task waits.
const CTL_IN_CAPACITY: usize = 64;
/// Bytes that may be queued towards http before the scheduler stops: nothing
/// on this path may buffer more than a handful of response windows.
const HTTP_OUTBOUND_CAP: usize = 64 * 1024 * 1024;
/// Bytes that may be queued towards one view: four windows plus room for a
/// head and a few small frames. Past it the view is recycled, not buffered.
const VIEW_OUTBOUND_CAP: usize = 4 * INITIAL_WINDOW as usize + 64 * 1024;
/// Bytes that may be queued towards init. `ctl.chan` traffic is tiny; a queue
/// this deep means init stopped reading, which is fatal anyway.
const CTL_OUTBOUND_CAP: usize = 1024 * 1024;
/// The `reason` a view is retired with when it breaks flow control, or when
/// its outbound queue overflows (§8.1's reasons are init's).
const CREDIT_OVERFLOW: &str = "credit_overflow";

/// Where a `request` or `process.retired` event goes.
///
/// init owns stdout (§8.1): the scheduler hands every event to this closure,
/// which writes it to the events pipe. **It must not block**: it is called
/// inline from the scheduler's loop, and a blocking callback would stop every
/// stream on every channel.
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

/// A frame from `http.chan`, or the end of that channel.
enum HttpIn {
    /// One frame, muxed by the stream id http allocated.
    Frame { id: u32, frame: Frame },
    /// http closed its end of the channel.
    Closed,
    /// http sent bytes that are not a frame. This is blastit's own code on the
    /// other end (§3.1), so it is fatal rather than a view's problem.
    Failed(FrameError),
}

/// Which view process a message came from: the route index plus the pid init
/// gave it.
///
/// The pid is what tells a replaced instance of the same route apart from its
/// predecessor: requests are keyed by `(ViewKey, req_id)`, so a frame from a
/// retired process can never touch the requests of the one that replaced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ViewKey {
    route: usize,
    pid: u32,
}

/// A frame from one `view.chan`, or the end of that channel.
enum ViewIn {
    /// One frame, muxed by the request id the scheduler allocated for that view.
    Frame { key: ViewKey, id: u32, frame: Frame },
    /// The view's end of the channel closed (or its process died).
    Closed { key: ViewKey },
}

/// A message from `ctl.chan`, or its failure.
enum CtlIn {
    /// One message and the descriptor that travelled with it, if any.
    Message(Ctl, Option<OwnedFd>),
    /// `ctl.chan` failed: the scheduler cannot run without init.
    Failed(SchedulerError),
}

/// One live view process (or one whose `SPAWNED` has not arrived yet).
struct View {
    key: ViewKey,
    /// The view sent `READY`; only then may it be dispatched to (§4).
    ready: bool,
    /// `SPAWN` was sent and `SPAWNED` has not arrived.
    spawning: bool,
    /// `BLASTIT_VIEW_MAX_REQUESTS` is reached: dispatch nothing more to it.
    stopping: bool,
    /// Request threads currently alive in this process (§2.4: the budget counts
    /// threads, and a thread lives until `DONE`).
    inflight: u32,
    /// Requests ever dispatched to this process.
    sent: u64,
    /// Next request id to hand out; ids are unique among in-flight requests.
    next_request_id: u32,
    /// When the last in-flight request finished, for idle retirement.
    idle_since: Option<Instant>,
    /// Last time this view was dispatched to, for the LRU cap.
    last_used: u64,
    /// Frames towards this view, with its own writer task behind them.
    out: Option<Outbound>,
}

impl View {
    /// A placeholder for a route whose `SPAWN` is in flight.
    fn spawning(route: usize) -> Self {
        Self {
            key: ViewKey { route, pid: 0 },
            ready: false,
            spawning: true,
            stopping: false,
            inflight: 0,
            sent: 0,
            next_request_id: 0,
            idle_since: None,
            last_used: 0,
            out: None,
        }
    }

    /// The next free request id for this view.
    fn allocate_request_id(&mut self) -> u32 {
        self.next_request_id = self.next_request_id.wrapping_add(1);
        if self.next_request_id == 0 {
            self.next_request_id = 1;
        }
        self.next_request_id
    }
}

/// The admission queue: plain FIFO (§2.4), or the fair queue (D5).
///
/// Both hold the stream ids of the requests that are waiting for a worker
/// slot; only the order they hand them back differs.
enum Admission {
    /// §2.4's bounded FIFO, in arrival order.
    Fifo(VecDeque<u32>),
    /// The two-level fair queue, in budget order.
    Fair(Box<FairQueue>),
}

impl Admission {
    /// The queue the configuration asks for.
    fn new(fair_queuing: bool, cfg: crate::config::FairConfig) -> Self {
        if fair_queuing {
            Self::Fair(Box::new(FairQueue::new(cfg)))
        } else {
            Self::Fifo(VecDeque::new())
        }
    }

    /// The fair queue, when the configuration asked for one.
    fn fair(&self) -> Option<&FairQueue> {
        match self {
            Self::Fifo(_) => None,
            Self::Fair(queue) => Some(queue),
        }
    }

    /// The fair queue, when the configuration asked for one.
    fn fair_mut(&mut self) -> Option<&mut FairQueue> {
        match self {
            Self::Fifo(_) => None,
            Self::Fair(queue) => Some(queue),
        }
    }

    /// Whether fair queuing is on.
    fn is_fair(&self) -> bool {
        self.fair().is_some()
    }

    /// The flow of a new request, counting it as admitted (D5).
    ///
    /// A FIFO queue has no flows, so this is `None` there and the caller
    /// carries nothing it will not need.
    ///
    /// `now` is the scheduler's clock, converted once: the queue stamps
    /// `last_active` from the same clock its eviction rules read, so an idle
    /// flow's debt ages against the clock the scheduler actually uses (a paused
    /// one in a test, the wall clock in production) instead of against
    /// [`std::time::Instant::now`] behind its back.
    fn resolve(
        &mut self,
        peer: &str,
        headers: &[(Vec<u8>, Vec<u8>)],
        now: Instant,
    ) -> Option<FlowRef> {
        self.fair_mut()
            .map(|queue| queue.resolve(peer, headers, now.into_std()))
    }

    /// Queue a request that is waiting for `route`.
    fn push(&mut self, stream_id: u32, route: u32, flow: Option<FlowRef>, now: Instant) {
        match self {
            Self::Fifo(queue) => queue.push_back(stream_id),
            Self::Fair(queue) => queue.push(
                stream_id,
                route,
                flow.unwrap_or_else(FairQueue::overflow_flow),
                now.into_std(),
            ),
        }
    }

    /// Forget a request: it was dispatched, timed out, or was cancelled.
    ///
    /// Idempotent: a stream id that is not queued is not an error.
    fn remove(&mut self, stream_id: u32) {
        match self {
            Self::Fifo(queue) => queue.retain(|queued| *queued != stream_id),
            Self::Fair(queue) => queue.remove(stream_id),
        }
    }

    /// Whether a request is still waiting.
    fn contains(&self, stream_id: u32) -> bool {
        match self {
            Self::Fifo(queue) => queue.contains(&stream_id),
            Self::Fair(queue) => queue.contains(stream_id),
        }
    }

    /// How many requests are waiting.
    fn len(&self) -> usize {
        match self {
            Self::Fifo(queue) => queue.len(),
            Self::Fair(queue) => queue.len(),
        }
    }

    /// Whether nothing is waiting.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every waiting request, in no particular order.
    ///
    /// The queue timeouts and the shutdown check walk all of them, so the
    /// order they come back in does not matter.
    fn ids(&self) -> Vec<u32> {
        match self {
            Self::Fifo(queue) => queue.iter().copied().collect(),
            Self::Fair(queue) => queue.ids(),
        }
    }

    /// Every route that has something waiting (D5: empty for the FIFO, whose
    /// walk reads the routes off the requests themselves).
    fn routes(&self) -> Vec<u32> {
        match self {
            Self::Fifo(_) => Vec::new(),
            Self::Fair(queue) => queue.routes(),
        }
    }

    /// The request that goes next for `route`, and the flow it belongs to
    /// (D5).
    fn peek(&self, route: u32) -> Option<NextFlow> {
        self.fair()?.peek(route)
    }

    /// Take the next request for `route` (D5).
    fn pop(&mut self, route: u32) -> Option<u32> {
        self.fair_mut()?.pop(route)
    }

    /// Charge a flow for a request (D5).
    fn debit(&mut self, flow: &FlowRef, charged_ns: u64, now: Instant) {
        if let Some(queue) = self.fair_mut() {
            queue.debit(flow, Cost(charged_ns), now.into_std());
        }
    }

    /// Let a flow go once the request that held it is over (D5).
    fn release_flow(&mut self, flow: FlowRef, now: Instant) {
        if let Some(queue) = self.fair_mut() {
            queue.release(flow, now.into_std());
        }
    }

    /// Collect the flows that may be collected now (D5).
    fn gc(&mut self, now: Instant) {
        if let Some(queue) = self.fair_mut() {
            queue.gc(now.into_std());
        }
    }
}
/// One §8 Sizing memory sample: the aggregate usage, and how many views had
/// sent `READY` when it was taken.
///
/// The difference between two samples is the memory one more view process
/// cost, which is the divisor §8's derived cap should have used all along.
#[derive(Debug, Clone, Copy)]
struct MemSample {
    /// Aggregate usage in bytes.
    used: u64,
    /// Views that had sent `READY`.
    views: u32,
}

/// One request, from its `REQ_HEAD` to the `DONE` that releases its slot.
struct Request {
    /// http's stream id, for the response frames.
    stream_id: u32,
    /// The id reported as `request_id` in events (§8.1).
    request_id: String,
    /// The head as it arrived; the view rebuilds its own `environ` from it.
    head: ReqHead,
    /// The route this request was routed to, if any (`None` for the requests
    /// the scheduler answers itself: health, `400`, `404`).
    route: Option<usize>,
    /// Index of the matching path regex in the route (§8.1 `rule`).
    rule: Option<u32>,
    /// The routed `PATH_INFO`: decoded and `SCRIPT_NAME`-stripped (§8.1).
    /// `None` when the target could not be decoded at all.
    path_info: Option<String>,
    arrived: Instant,
    /// Set while the request waits in the admission queue.
    queued_since: Option<Instant>,
    /// Set when the request was handed to a view; the timeout runs from here.
    dispatched_at: Option<Instant>,
    /// `dispatched_at + BLASTIT_REQUEST_TIMEOUT`.
    deadline: Option<Instant>,
    status: u16,
    outcome: Outcome,
    bytes_in: u64,
    bytes_out: u64,
    /// Body chunks held while the request is queued. Credit bounds them: http
    /// may not send more than [`Request::upload_credit`] allows (§3.1).
    body: Vec<Vec<u8>>,
    /// `REQ_END` arrived.
    end_seen: bool,
    /// A `CANCEL` arrived from http.
    cancelled: bool,
    /// The view and request id this request was dispatched under.
    view: Option<(ViewKey, u32)>,
    /// The fair-queuing flow this request belongs to (D5), when fair queuing
    /// is on. It is resolved once, when the request is admitted, and returned
    /// once, when the request is released.
    flow: Option<FlowRef>,
    /// The view's own measurement of what this request cost it (D5), from the
    /// `RESP_END_COST` frame. Untrusted: [`Scheduler::charge`] clamps it
    /// between the view's first frame and the time the scheduler observed.
    report: Option<RequestCost>,
    /// What was charged to the flow for this request, in nanoseconds: the
    /// reported cost clamped between the view's first frame and the observed
    /// time, or the observed time when there was no report. This is the
    /// `cost_ns` of the `request` event (§8.1).
    charged_ns: u64,
    /// The flow has been charged for this request. Exactly one of the response
    /// end and the release does it (D5).
    charged: bool,
    /// When the response ended — the last byte, the abort or the failure —
    /// which is where `duration_ms` stops even though the event waits for the
    /// `DONE` that carries the cost report (D5).
    finished_at: Option<Instant>,
    /// When the view sent its first response frame (D5): the floor under what
    /// this request may be charged, because it is the one part of the request
    /// the view cannot shorten without making its own client wait.
    head_at: Option<Instant>,
    /// A `RESP_HEAD` was forwarded to http.
    head_sent: bool,
    /// The response to http is over (`RESP_END`, `RESP_ABORT` or a status the
    /// scheduler sent itself).
    complete: bool,
    /// The `request` event has been emitted.
    emitted: bool,
    /// The request passed `BLASTIT_REQUEST_TIMEOUT`.
    timed_out: bool,
    /// This request holds one of the global worker slots.
    slots: bool,
    /// Request-body bytes http may still send before the view grants more.
    upload_credit: u32,
    /// Response-body bytes the view may still send before http grants more.
    download_credit: u32,
}

/// What routing a request target produced.
enum Resolution {
    /// The target cannot be routed: a `400` (§2.4).
    Bad,
    /// The health path, answered by the scheduler itself.
    Health,
    /// Nothing matched: a `404` (§2.4).
    Unrouted,
    /// A route, one of its rules, and the `PATH_INFO` that matched.
    Routed {
        /// Route index.
        route: usize,
        /// Index of the matching path regex.
        rule: u32,
        /// The decoded, stripped path.
        path_info: String,
    },
}

/// The scheduler's whole state.
struct Scheduler {
    cfg: SchedulerConfig,
    routes: Vec<RouteInfo>,
    table: RoutingTable,
    emit: Emit,
    /// Frames towards http, with its own writer task behind them.
    http_out: Outbound,
    /// The sender every view reader task clones.
    view_in: mpsc::Sender<ViewIn>,
    /// `ctl.chan` messages towards init, with its own task behind them.
    ctl_out: Outbound,
    /// The normalised `BLASTIT_SCRIPT_NAME` mount prefix (§6), if any.
    script_name: Option<String>,
    started: Instant,
    counter: u64,
    /// Every request that has not been released yet, by http stream id.
    requests: HashMap<u32, Request>,
    /// Requests waiting for a worker slot, oldest first (§2.4's bounded FIFO)
    /// or in fair order (D5).
    queue: Admission,
    /// The live view per route: at most one (§2.4).
    views: HashMap<usize, View>,
    /// `(view, request id)` to http stream id: what a frame from a view means.
    dispatch: HashMap<(ViewKey, u32), u32>,
    /// Worker slots in use: dispatched requests whose `DONE` has not arrived.
    slots: u32,
    /// Routes whose last spawn failed, and when they may be spawned again.
    backoff: HashMap<usize, Instant>,
    /// Monotonic counter behind the LRU cap.
    lru: u64,
    /// A `DRAIN` arrived: no new admission, no new spawns.
    draining: bool,
    /// When draining gives up on the requests that are left.
    drain_deadline: Option<Instant>,
    /// When the fair queue's idle flows are collected next (D5). `None` while
    /// fair queuing is off, because a FIFO queue has no flows to collect.
    next_gc: Option<Instant>,
    /// §8 Sizing's derived view process cap, once init has proved the app is
    /// preloaded (see [`Scheduler::derive_view_cap`]).
    derived_view_cap: Option<u32>,
    /// Whether `derived_view_cap` came from a reading taken *after* the
    /// preload, which is the one §8 asks for.
    derived_after_preload: bool,
    /// §8 Sizing: the usage init reported once the app was preloaded, kept as
    /// the `used` of every later derivation of the cap.
    preload_used: Option<u64>,
    /// §8 Sizing: the last `(usage, ready views)` sample, which is what turns
    /// consecutive samples into a measured cost per view process.
    mem_sample: Option<MemSample>,
    /// §8 Sizing: the measured cost of one view process, a moving average
    /// clamped to at least `BLASTIT_VIEW_MEM_ESTIMATE`.
    measured_view_mem: Option<u64>,
    /// §8 Sizing: the `(estimate, cap)` pair the last `process.cap` event
    /// reported, so a change — and only a change — is logged.
    reported_cap: Option<(u64, u32)>,
    /// The most recent aggregate reading, with when it was taken. A spawn
    /// decision must not read the probe once per pump, so this is what
    /// [`Scheduler::memory`] serves until it is
    /// [`SchedulerConfig::memory_check_interval`] old.
    mem_reading: Option<(Instant, Option<u64>)>,
    /// When the memory high watermark is checked next (`None`: not watched).
    memory_check_at: Option<Instant>,
    /// http closed `http.chan`.
    http_closed: bool,
    /// Frames dropped because they named an unknown stream or request, or
    /// because a peer sent one the protocol does not allow there.
    dropped: u64,
}

impl Scheduler {
    /// A scheduler with nothing in flight.
    #[allow(clippy::too_many_arguments)]
    fn new(
        cfg: SchedulerConfig,
        routes: Vec<RouteInfo>,
        table: RoutingTable,
        emit: Emit,
        http_out: Outbound,
        view_in: mpsc::Sender<ViewIn>,
        ctl_out: Outbound,
    ) -> Self {
        let script_name = cfg.script_name.as_deref().and_then(normalize_script_name);
        let fair_queuing = cfg.fair_queuing;
        let queue = Admission::new(fair_queuing, cfg.fair.clone());
        // §8 Sizing: the watermark is watched on a timer, so the scheduler only
        // schedules it when it has something to watch.
        let memory_check_at = cfg
            .watches_memory()
            .then(|| Instant::now() + cfg.memory_check_interval);
        Self {
            cfg,
            routes,
            table,
            emit,
            http_out,
            view_in,
            ctl_out,
            script_name,
            started: Instant::now(),
            counter: 0,
            requests: HashMap::new(),
            queue,
            views: HashMap::new(),
            dispatch: HashMap::new(),
            slots: 0,
            backoff: HashMap::new(),
            lru: 0,
            draining: false,
            drain_deadline: None,
            next_gc: fair_queuing.then(|| Instant::now() + FAIR_GC_INTERVAL),
            derived_view_cap: None,
            derived_after_preload: false,
            preload_used: None,
            mem_sample: None,
            measured_view_mem: None,
            reported_cap: None,
            mem_reading: None,
            memory_check_at,
            http_closed: false,
            dropped: 0,
        }
    }

    /// A fresh `request_id`: sixteen hex characters, the low 32 bits of the
    /// milliseconds since the scheduler started plus the low 32 bits of a
    /// request counter (§8.1 leaves the format open; this is unique for the
    /// life of the process).
    fn next_request_id(&mut self) -> String {
        self.counter = self.counter.wrapping_add(1);
        let millis = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        format!(
            "{:08x}{:08x}",
            (millis & 0xffff_ffff) as u32,
            (self.counter & 0xffff_ffff) as u32
        )
    }

    /// Spawns the views that must exist at startup: every preforked route, and
    /// every route when `BLASTIT_VIEW_SPAWN=eager` (§2.4, §6).
    ///
    /// §8 Sizing: the derived cap is taken first, so this burst obeys it too —
    /// init has not finished preloading yet, which is why
    /// [`Scheduler::derive_view_cap`] takes it again at the first `SPAWNED`.
    fn start(&mut self) -> Result<(), SchedulerError> {
        self.derive_view_cap(false);
        let wanted: Vec<usize> = (0..self.routes.len())
            .filter(|route| self.cfg.eager_spawn || self.routes[*route].prefork)
            .collect();
        for route in wanted {
            self.try_spawn(route)?;
        }
        Ok(())
    }

    /// Whether the run is over.
    fn done(&self) -> bool {
        if self.http_closed {
            return true;
        }
        if !self.draining {
            return false;
        }
        let idle = self.slots == 0 && self.queue.is_empty();
        let expired = match self.drain_deadline {
            Some(deadline) => Instant::now() >= deadline,
            None => true,
        };
        idle || expired
    }

    /// When the scheduler must wake up next, if ever.
    fn next_deadline(&self) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        let mut take = |at: Instant| {
            next = Some(match next {
                Some(current) => current.min(at),
                None => at,
            });
        };
        for stream_id in self.queue.ids() {
            if let Some(since) = self
                .requests
                .get(&stream_id)
                .and_then(|request| request.queued_since)
            {
                take(since + self.cfg.queue_timeout);
            }
        }
        for request in self.requests.values() {
            if request.slots
                && !request.timed_out
                && let Some(deadline) = request.deadline
            {
                take(deadline);
            }
        }
        if !self.cfg.idle_timeout.is_zero() {
            for view in self.views.values() {
                if view.ready
                    && view.inflight == 0
                    && let Some(since) = view.idle_since
                {
                    take(since + self.cfg.idle_timeout);
                }
            }
        }
        for until in self.backoff.values() {
            take(*until);
        }
        if self.draining
            && let Some(deadline) = self.drain_deadline
        {
            take(deadline);
        }
        // §8 Sizing: the memory watermark is checked on its own cadence.
        if let Some(at) = self.memory_check_at {
            take(at);
        }
        // D5: so is the fair queue's collection of idle flows.
        if let Some(at) = self.next_gc {
            take(at);
        }
        next
    }

    // ---------------------------------------------------------------- events

    /// Everything that is time-based, after any event: queue timeouts, work to
    /// dispatch, request timeouts, and retirements.
    fn pump(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        self.backoff.retain(|_, until| *until > now);
        self.expire_queue()?;
        self.dispatch_queued()?;
        self.expire_requests()?;
        self.retire_finished_views()?;
        self.check_memory()?;
        self.collect_flows(now);
        Ok(())
    }

    /// Collects the fair queue's idle flows (D5), at most once per
    /// [`FAIR_GC_INTERVAL`].
    ///
    /// A flow's own request end collects it, so this is only the sweep for a
    /// flow whose last request ended while the queue stayed busy: without it,
    /// the tables would grow with the number of clients that have ever
    /// connected.
    fn collect_flows(&mut self, now: Instant) {
        let Some(at) = self.next_gc else {
            return;
        };
        if now < at {
            return;
        }
        // Count from now, so a scheduler that was busy for several intervals
        // cannot sweep several times in a row.
        self.next_gc = Some(now + FAIR_GC_INTERVAL);
        self.queue.gc(now);
    }

    /// Handles one `http.chan` frame.
    fn on_http(&mut self, event: HttpIn) -> Result<(), SchedulerError> {
        let (id, frame) = match event {
            HttpIn::Closed => {
                self.http_closed = true;
                return Ok(());
            }
            // http is blastit's own code: bytes that are not frames mean the
            // build is broken, not that a peer misbehaved (§3.1).
            HttpIn::Failed(error) => return Err(SchedulerError::Frame(error)),
            HttpIn::Frame { id, frame } => (id, frame),
        };
        match frame {
            Frame::ReqHead(head) => self.begin(id, head),
            Frame::ReqBody(body) => self.request_body(id, body),
            Frame::ReqEnd => self.request_end(id),
            Frame::Cancel => self.client_cancel(id),
            Frame::Window { bytes } => self.client_window(id, bytes),
            // http never sends a response frame; count it and move on.
            _ => {
                self.dropped += 1;
                Ok(())
            }
        }
    }

    /// Handles one `view.chan` frame, or the loss of a view process.
    fn on_view(&mut self, event: ViewIn) -> Result<(), SchedulerError> {
        let (key, id, frame) = match event {
            ViewIn::Closed { key } => {
                self.view_gone(key);
                return Ok(());
            }
            ViewIn::Frame { key, id, frame } => (key, id, frame),
        };
        match frame {
            Frame::Ready => {
                self.view_ready(key);
                Ok(())
            }
            Frame::Done { outcome } => self.view_done(key, id, outcome),
            Frame::RespEnd => self.view_end(key, id, None),
            Frame::RespEndCost { cost } => self.view_end(key, id, Some(cost)),
            Frame::RespHead(head) => self.view_head(key, id, head),
            Frame::RespBody(body) => self.view_body(key, id, body),
            Frame::RespAbort { reason } => self.view_abort(key, id, &reason),
            Frame::Window { bytes } => self.view_window(key, id, bytes),
            // A view may only address its own requests: drop anything else.
            _ => {
                self.dropped += 1;
                Ok(())
            }
        }
    }

    /// Handles one `ctl.chan` message from init.
    fn on_ctl(&mut self, message: Ctl, fd: Option<OwnedFd>) -> Result<(), SchedulerError> {
        match message {
            Ctl::Spawned { route, pid } => self.view_spawned(route as usize, pid, fd),
            Ctl::Exited { route, pid, .. } => {
                self.view_gone(ViewKey {
                    route: route as usize,
                    pid,
                });
                Ok(())
            }
            Ctl::Drain => {
                self.draining = true;
                self.drain_deadline = Some(Instant::now() + self.cfg.request_timeout);
                Ok(())
            }
            // The scheduler is the only sender of these.
            other => Err(SchedulerError::protocol(format!(
                "unexpected {} on ctl.chan",
                other.type_code().name()
            ))),
        }
    }

    // --------------------------------------------------------------- pushes

    /// Queues a frame towards http.
    fn http_push(&mut self, id: u32, frame: Frame) -> Result<(), SchedulerError> {
        match self.http_out.push(id, frame) {
            Ok(()) => Ok(()),
            Err(PushError::Overflow { bytes, cap }) => {
                Err(SchedulerError::overflow("http.chan", bytes, cap))
            }
            Err(PushError::Closed) => {
                // The writer is gone: http has closed its end.
                self.http_closed = true;
                Ok(())
            }
            Err(PushError::Frame(error)) => Err(SchedulerError::Frame(error)),
        }
    }

    /// Queues a control message towards init.
    fn ctl_push(&mut self, message: Ctl) -> Result<(), SchedulerError> {
        match self.ctl_out.push(0, Frame::Ctl(message)) {
            Ok(()) => Ok(()),
            Err(PushError::Overflow { bytes, cap }) => {
                Err(SchedulerError::overflow("ctl.chan", bytes, cap))
            }
            Err(PushError::Closed) => Err(SchedulerError::closed()),
            Err(PushError::Frame(error)) => Err(SchedulerError::Frame(error)),
        }
    }

    /// Queues a frame towards one view.
    ///
    /// A view whose queue is full is not worth waiting for: it is recycled, and
    /// its requests fail, rather than letting one process hold up the loop.
    fn view_push(&mut self, key: ViewKey, id: u32, frame: Frame) -> Result<(), SchedulerError> {
        let Some(out) = self.view_out(key) else {
            return Ok(());
        };
        match out.push(id, frame) {
            Ok(()) => Ok(()),
            Err(PushError::Closed) => {
                self.view_gone(key);
                Ok(())
            }
            Err(PushError::Overflow { .. }) | Err(PushError::Frame(_)) => {
                self.retire(key, RetireReason::Other(CREDIT_OVERFLOW.to_string()))
            }
        }
    }

    // --------------------------------------------------------------- request

    /// Routes a new request and either dispatches it or queues it (§2.4).
    fn begin(&mut self, stream_id: u32, head: ReqHead) -> Result<(), SchedulerError> {
        if self.requests.contains_key(&stream_id) {
            // http's contract is that a stream id is not reused while it is in
            // flight; anything else would let one request inherit another's
            // worker slot, so it is fatal rather than guessed at.
            return Err(SchedulerError::protocol(format!(
                "http.chan reused stream id {stream_id} while it was still in flight"
            )));
        }
        let request_id = self.next_request_id();
        // The event's one clock: `arrived` and the flow the queue stamps are
        // read from it, so a request cannot age against two clocks at once.
        let now = Instant::now();
        let mut request = Request {
            stream_id,
            request_id,
            head,
            route: None,
            rule: None,
            path_info: None,
            arrived: now,
            queued_since: None,
            dispatched_at: None,
            deadline: None,
            status: 0,
            outcome: Outcome::Ok,
            bytes_in: 0,
            bytes_out: 0,
            body: Vec::new(),
            end_seen: false,
            cancelled: false,
            view: None,
            flow: None,
            report: None,
            charged_ns: 0,
            charged: false,
            finished_at: None,
            head_at: None,
            head_sent: false,
            complete: false,
            emitted: false,
            timed_out: false,
            slots: false,
            upload_credit: INITIAL_WINDOW,
            download_credit: INITIAL_WINDOW,
        };
        let resolution = self.resolve(&request);
        match resolution {
            Resolution::Bad => {
                self.requests.insert(stream_id, request);
                self.reject(
                    stream_id,
                    400,
                    "Bad Request",
                    "bad request\n",
                    Outcome::Unmatched,
                    None,
                )
            }
            Resolution::Health => {
                self.requests.insert(stream_id, request);
                self.respond(stream_id, 200, "OK", "ok", None)?;
                self.finish_immediately(stream_id, Outcome::Ok)
            }
            Resolution::Unrouted => {
                self.requests.insert(stream_id, request);
                self.reject(
                    stream_id,
                    404,
                    "Not Found",
                    "not found\n",
                    Outcome::Unmatched,
                    None,
                )
            }
            Resolution::Routed {
                route,
                rule,
                path_info,
            } => {
                request.route = Some(route);
                request.rule = Some(rule);
                request.path_info = Some(path_info);
                // D5: the flow is resolved once per request, from the raw peer
                // and the headers, and it is the *same* resolution the front
                // end used for `REMOTE_ADDR`. The queue is given this event's
                // clock, so what it stamps and what it later compares are one
                // clock.
                request.flow =
                    self.queue
                        .resolve(&request.head.peer_addr, &request.head.headers, now);
                self.requests.insert(stream_id, request);
                let retry_after = retry_after_seconds(self.cfg.queue_timeout);
                if self.draining {
                    self.reject(
                        stream_id,
                        503,
                        "Service Unavailable",
                        "draining\n",
                        Outcome::QueueFull,
                        Some(retry_after),
                    )
                } else if self.can_dispatch(route) {
                    self.dispatch(stream_id)
                } else if self.queue.len() >= self.cfg.queue_size {
                    self.reject(
                        stream_id,
                        503,
                        "Service Unavailable",
                        "queue full\n",
                        Outcome::QueueFull,
                        Some(retry_after),
                    )
                } else {
                    self.enqueue(stream_id);
                    Ok(())
                }
            }
        }
    }

    /// Turns the request target into a routing decision (§2.4, §6).
    fn resolve(&self, request: &Request) -> Resolution {
        let path = path_of_target(&request.head.target);
        let Some(decoded) = percent_decode(path) else {
            return Resolution::Bad;
        };
        if !decoded.starts_with(b"/") {
            return Resolution::Bad;
        }
        // NUL and the C0 controls cannot be routed on (and must never reach an
        // event), so they are a `400` like any other unusable target.
        if decoded.iter().any(|byte| is_control_byte(*byte)) {
            return Resolution::Bad;
        }
        let Ok(path) = std::str::from_utf8(&decoded) else {
            return Resolution::Bad;
        };
        let Ok(method) = std::str::from_utf8(&request.head.method) else {
            return Resolution::Bad;
        };
        // Only a GET or a HEAD is a health check; anything else is a request
        // the app may well want to see (§2.4).
        if path == self.cfg.health_path && (method == "GET" || method == "HEAD") {
            return Resolution::Health;
        }
        let path = match self.script_name.as_deref() {
            Some(prefix) => match strip_script_name(path, prefix) {
                Some(rest) => rest,
                None => return Resolution::Unrouted,
            },
            None => path,
        };
        match self.table.route(method, path) {
            Some(matched) => Resolution::Routed {
                route: matched.route,
                rule: u32::try_from(matched.rule).unwrap_or(u32::MAX),
                path_info: path.to_string(),
            },
            None => Resolution::Unrouted,
        }
    }

    /// Puts a request in the admission queue: at the back of the FIFO, or into
    /// its flow (D5).
    fn enqueue(&mut self, stream_id: u32) {
        let now = Instant::now();
        let route = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.route)
            .and_then(|route| u32::try_from(route).ok())
            .unwrap_or(u32::MAX);
        let flow = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.flow);
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.queued_since = Some(now);
        }
        self.queue.push(stream_id, route, flow, now);
    }

    /// Records request body bytes: forwards them when the request is
    /// dispatched, holds them while it waits.
    ///
    /// Credit is what bounds the held bytes: http may not send more than the
    /// view has granted (§3.1), so a queued request can hold at most
    /// `INITIAL_WINDOW` bytes and other streams keep flowing.
    fn request_body(&mut self, stream_id: u32, body: Vec<u8>) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        let view = request.view;
        let allowed = request.upload_credit as u64;
        if body.len() as u64 > allowed {
            return self.http_credit_violation(stream_id);
        }
        let count = body.len() as u64;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.bytes_in += count;
            request.upload_credit -= body.len() as u32;
        }
        match view {
            Some((key, id)) => self.view_push(key, id, Frame::ReqBody(body)),
            None => {
                if let Some(request) = self.requests.get_mut(&stream_id) {
                    request.body.push(body);
                }
                Ok(())
            }
        }
    }

    /// Forwards `REQ_END`, or remembers it for a queued request.
    fn request_end(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get_mut(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        request.end_seen = true;
        let view = request.view;
        match view {
            Some((key, id)) => self.view_push(key, id, Frame::ReqEnd),
            None => Ok(()),
        }
    }

    /// The client went away (§3.2): forward `CANCEL` and keep the slot.
    ///
    /// The request reports the status it had reached — `0` when the response
    /// never started — with the `client_gone` outcome (§8.1).
    fn client_cancel(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        let view = request.view;
        if let Some((key, id)) = view {
            self.view_push(key, id, Frame::Cancel)?;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.cancelled = true;
            request.outcome = Outcome::ClientGone;
            request.complete = true;
        }
        self.mark_finished(stream_id);
        self.emit_request(stream_id);
        if view.is_none() {
            // Never dispatched: it leaves the queue, and no slot was ever held.
            self.release(stream_id);
        }
        Ok(())
    }

    /// http granted the stream more response-body credit (§3.1): the view gets
    /// to send that much more.
    fn client_window(&mut self, stream_id: u32, bytes: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        let view = request.view;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.download_credit = request.download_credit.saturating_add(bytes);
        }
        match view {
            Some((key, id)) => self.view_push(key, id, Frame::Window { bytes }),
            None => Ok(()),
        }
    }

    /// http sent more request-body bytes than its credit allowed (§3.1): the
    /// stream is failed on both sides.
    fn http_credit_violation(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let view = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.view);
        if let Some((key, id)) = view {
            self.view_push(key, id, Frame::Cancel)?;
        }
        self.fail_request(stream_id, Outcome::ViewDied, 502)?;
        // The stream is over on both sides: http stops reading this client's
        // socket for a stream the scheduler will not answer.
        self.http_push(stream_id, Frame::Cancel)
    }

    // -------------------------------------------------------------- dispatch

    /// Whether a route can take a request right now.
    fn can_dispatch(&self, route: usize) -> bool {
        if self.slots >= self.cfg.max_workers {
            return false;
        }
        match self.views.get(&route) {
            Some(view) => {
                view.ready
                    && !view.spawning
                    && !view.stopping
                    && view.inflight < self.cfg.view_worker_cap()
                    && view.out.as_ref().map(Outbound::is_open).unwrap_or(false)
            }
            None => false,
        }
    }

    /// Dispatches every queued request it can, spawning views as needed.
    ///
    /// With fair queuing off (the default) this is §2.4's FIFO walk: the
    /// requests are considered in arrival order, and one that cannot be served
    /// — its route's view is not ready — is skipped rather than allowed to hold
    /// up the ones behind it.
    fn dispatch_queued(&mut self) -> Result<(), SchedulerError> {
        if self.queue.is_fair() {
            return self.dispatch_queued_fair();
        }
        let queued: Vec<u32> = self.queue.ids();
        for stream_id in queued {
            if !self.queue.contains(stream_id) {
                continue;
            }
            let Some(route) = self
                .requests
                .get(&stream_id)
                .and_then(|request| request.route)
            else {
                continue;
            };
            if self.slots >= self.cfg.max_workers {
                return Ok(());
            }
            if self.can_dispatch(route) {
                self.dispatch(stream_id)?;
                continue;
            }
            if !self.views.contains_key(&route) && !self.draining {
                self.try_spawn(route)?;
            }
        }
        Ok(())
    }

    /// The fair walk (D5): dispatch the queued request whose flow has spent the
    /// least, among the routes that can take one right now.
    ///
    /// A route whose view is not ready — or not spawned yet — never holds up
    /// another route: the choice is made among the routes that *can* dispatch,
    /// and a route whose spawn is in flight is simply not one of them yet.
    ///
    /// The candidates are compared by their flow's two-level virtual time, in
    /// the same order the route's own backlog uses: the outer flow's `used`
    /// first, then the inner flow's. An outer flow belongs to the *queue*, not
    /// to a route — one client's budget is one number however many routes it has
    /// work for — so `(outer.used, inner.used)` is the same key in-route and
    /// across routes, and the route index is only the last tiebreak, where the
    /// in-route order breaks ties on the flow keys.
    fn dispatch_queued_fair(&mut self) -> Result<(), SchedulerError> {
        loop {
            if self.slots >= self.cfg.max_workers {
                return Ok(());
            }
            let mut best: Option<(NextFlow, u32)> = None;
            for route in self.queue.routes() {
                if !self.can_dispatch(route as usize) {
                    if !self.views.contains_key(&(route as usize)) && !self.draining {
                        self.try_spawn(route as usize)?;
                    }
                    continue;
                }
                let Some(next) = self.queue.peek(route) else {
                    continue;
                };
                let better = match &best {
                    Some((best, best_route)) => {
                        (next.outer, next.inner, route) < (best.outer, best.inner, *best_route)
                    }
                    None => true,
                };
                if better {
                    best = Some((next, route));
                }
            }
            let Some((_, route)) = best else {
                return Ok(());
            };
            // Take it out of the order, and advance the queue's clock to it:
            // that is what makes a later request from the same flow wait its
            // turn, and what lets an idle flow catch up and be collected.
            let Some(stream_id) = self.queue.pop(route) else {
                continue;
            };
            let slots = self.slots;
            self.dispatch(stream_id)?;
            if self.slots == slots {
                // Its view could not take it after all, which the walk above
                // just said it could: put the request back rather than lose it,
                // and stop, because nothing about the order has changed.
                self.requeue(stream_id);
                return Ok(());
            }
        }
    }

    /// Puts a request that was taken out of the queue but not dispatched back.
    ///
    /// Its `queued_since` is untouched, so a request that this happens to still
    /// has the queue timeout it always had. The flow's order puts it at the
    /// back of its own queue, which is where a request of that flow that took
    /// no time belongs.
    fn requeue(&mut self, stream_id: u32) {
        let now = Instant::now();
        let route = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.route)
            .and_then(|route| u32::try_from(route).ok())
            .unwrap_or(u32::MAX);
        let flow = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.flow);
        self.queue.push(stream_id, route, flow, now);
    }

    /// Hands one request to its view, rewriting its id (§3.1).
    fn dispatch(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some((key, id, _out)) = self.reserve(stream_id) else {
            return Ok(());
        };
        let now = Instant::now();
        let (head, body, end, extra_credit) = {
            let Some(request) = self.requests.get_mut(&stream_id) else {
                return Ok(());
            };
            request.queued_since = None;
            request.dispatched_at = Some(now);
            request.deadline = Some(now + self.cfg.request_timeout);
            request.view = Some((key, id));
            request.slots = true;
            (
                request.head.clone(),
                std::mem::take(&mut request.body),
                request.end_seen,
                // A view starts with `INITIAL_WINDOW` of response credit, so
                // anything http granted on top of that has to be handed over.
                request.download_credit.saturating_sub(INITIAL_WINDOW),
            )
        };
        self.queue.remove(stream_id);
        self.dispatch.insert((key, id), stream_id);
        self.slots += 1;

        self.view_push(key, id, Frame::ReqHead(head))?;
        if extra_credit > 0 {
            self.view_push(
                key,
                id,
                Frame::Window {
                    bytes: extra_credit,
                },
            )?;
        }
        for chunk in body {
            self.view_push(key, id, Frame::ReqBody(chunk))?;
        }
        if end {
            self.view_push(key, id, Frame::ReqEnd)?;
        }
        Ok(())
    }

    /// Takes a worker slot and a request id for `stream_id`, if its view can
    /// take one.
    fn reserve(&mut self, stream_id: u32) -> Option<(ViewKey, u32, Outbound)> {
        let route = self.requests.get(&stream_id)?.route?;
        let cap = self.cfg.view_worker_cap();
        let last_used = self.lru;
        let view = self.views.get_mut(&route)?;
        if !view.ready || view.spawning || view.stopping || view.inflight >= cap {
            return None;
        }
        let out = view.out.clone()?;
        if !out.is_open() {
            return None;
        }
        let id = view.allocate_request_id();
        view.inflight += 1;
        view.sent += 1;
        view.idle_since = None;
        view.last_used = last_used;
        if self.cfg.max_requests > 0 && view.sent >= self.cfg.max_requests {
            view.stopping = true;
        }
        self.lru = last_used.wrapping_add(1);
        Some((view.key, id, out))
    }

    /// The queue towards a view, if that view is still the live one.
    fn view_out(&self, key: ViewKey) -> Option<Outbound> {
        self.views
            .get(&key.route)
            .filter(|view| view.key == key)
            .and_then(|view| view.out.clone())
    }

    // ---------------------------------------------------------------- timers

    /// Rejects queued requests whose wait exceeded `BLASTIT_QUEUE_TIMEOUT`.
    fn expire_queue(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let timeout = self.cfg.queue_timeout;
        let retry_after = retry_after_seconds(timeout);
        let expired: Vec<u32> = self
            .queue
            .ids()
            .into_iter()
            .filter(|stream_id| {
                self.requests
                    .get(stream_id)
                    .and_then(|request| request.queued_since)
                    .map(|since| now.duration_since(since) >= timeout)
                    .unwrap_or(false)
            })
            .collect();
        for stream_id in expired {
            self.queue.remove(stream_id);
            self.reject(
                stream_id,
                503,
                "Service Unavailable",
                "queue timeout\n",
                Outcome::QueueTimeout,
                Some(retry_after),
            )?;
        }
        Ok(())
    }

    /// Gives a `504` to every dispatched request past its timeout, and recycles
    /// a view with too many overdue threads (§2.4).
    ///
    /// A request whose response is already over — answered or cancelled — is
    /// only *marked* overdue: its answer has been sent, its outcome is final,
    /// and writing another frame for it would corrupt the stream. It still
    /// counts towards the view's stuck threads, because its thread is still
    /// alive.
    fn expire_requests(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let expired: Vec<(u32, Option<(ViewKey, u32)>)> = self
            .requests
            .values()
            .filter_map(|request| {
                if !request.slots || request.timed_out {
                    return None;
                }
                if now < request.deadline? {
                    return None;
                }
                Some((request.stream_id, request.view))
            })
            .collect();
        let mut affected: Vec<ViewKey> = Vec::new();
        for (stream_id, view) in expired {
            let (complete, head_sent) = match self.requests.get(&stream_id) {
                Some(request) => (request.complete, request.head_sent),
                None => continue,
            };
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.timed_out = true;
            }
            if let Some((key, _)) = view {
                affected.push(key);
            }
            if complete {
                continue;
            }
            if head_sent {
                self.http_push(
                    stream_id,
                    Frame::RespAbort {
                        reason: "request timeout".to_string(),
                    },
                )?;
            } else {
                self.respond(stream_id, 504, "Gateway Timeout", "gateway timeout\n", None)?;
            }
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.complete = true;
                request.outcome = Outcome::Timeout;
            }
            self.mark_finished(stream_id);
            self.emit_request(stream_id);
            if let Some((key, id)) = view {
                self.view_push(key, id, Frame::Cancel)?;
            }
        }
        for key in affected {
            let overdue = self
                .requests
                .values()
                .filter(|request| {
                    request.view.map(|(view, _)| view) == Some(key) && request.timed_out
                })
                .count() as u32;
            if overdue > self.cfg.stuck_threads {
                self.retire(key, RetireReason::StuckThreads)?;
            }
        }
        Ok(())
    }

    /// Retires what is finished: a view over `BLASTIT_VIEW_MAX_REQUESTS` with
    /// nothing in flight, or one idle for `BLASTIT_VIEW_IDLE_TIMEOUT` (§2.4).
    fn retire_finished_views(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let mut victims: Vec<(ViewKey, RetireReason)> = Vec::new();
        for view in self.views.values() {
            if !view.ready || view.inflight > 0 {
                continue;
            }
            if view.stopping {
                victims.push((view.key, RetireReason::MaxRequests));
                continue;
            }
            if !self.cfg.idle_timeout.is_zero()
                && let Some(since) = view.idle_since
                && now.duration_since(since) >= self.cfg.idle_timeout
            {
                victims.push((view.key, RetireReason::Idle));
            }
        }
        for (key, reason) in victims {
            self.retire(key, reason)?;
        }
        Ok(())
    }

    /// Retires one idle view when aggregate memory is above
    /// `BLASTIT_MEM_HIGH_WATERMARK` (§8 Sizing, §9.2 #8).
    ///
    /// One process per check and never one with a request in flight: the point
    /// is to give memory back, not to fail a request. With nothing idle the
    /// check does nothing, and the next one — `memory_check_interval` later —
    /// tries again.
    fn check_memory(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let Some(at) = self.memory_check_at else {
            return Ok(());
        };
        if now < at {
            return Ok(());
        }
        // Count from now, so a scheduler that was busy for several intervals
        // cannot burst through a backlog and retire several views in a row.
        self.memory_check_at = Some(now + self.cfg.memory_check_interval);
        let Some(watermark) = self.cfg.mem_high_watermark_bytes else {
            return Ok(());
        };
        let Some(usage) = self.memory_now() else {
            return Ok(());
        };
        if usage <= watermark {
            return Ok(());
        }
        let Some(victim) = self.oldest_idle_view() else {
            return Ok(());
        };
        self.retire(victim, RetireReason::MemoryPressure)
    }

    /// The idle view that has gone unused the longest, which is the one memory
    /// pressure retires first (§8 Sizing: "oldest-first").
    ///
    /// A view with a request in flight is never a candidate, and neither is one
    /// whose process has not been forked yet.
    fn oldest_idle_view(&self) -> Option<ViewKey> {
        self.views
            .values()
            .filter(|view| view.ready && !view.spawning && view.key.pid != 0 && view.inflight == 0)
            .min_by_key(|view| view.last_used)
            .map(|view| view.key)
    }

    /// Reads the machine's aggregate memory usage, if it can be read (§9.2 #8).
    fn read_memory(&self) -> Option<u64> {
        self.cfg
            .memory_usage
            .as_ref()
            .and_then(|probe| probe.read())
    }

    /// Reads the probe now, and keeps the reading for the next
    /// [`Scheduler::memory`] call.
    fn memory_now(&mut self) -> Option<u64> {
        let usage = self.read_memory();
        self.mem_reading = Some((Instant::now(), usage));
        usage
    }

    /// The aggregate usage in bytes, re-read only when the cached reading is
    /// older than `memory_check_interval`.
    ///
    /// §8 Sizing samples memory on that cadence anyway, and a spawn that is
    /// refused for memory is retried on every pump: without the cache a busy
    /// scheduler would read the probe per event rather than per check.
    fn memory(&mut self) -> Option<u64> {
        if let Some((at, usage)) = self.mem_reading
            && at.elapsed() < self.cfg.memory_check_interval
        {
            return usage;
        }
        self.memory_now()
    }

    /// Whether aggregate usage is above `BLASTIT_MEM_HIGH_WATERMARK` (§8
    /// Sizing). Memory that cannot be read is not pressure.
    fn above_watermark(&mut self) -> bool {
        let Some(watermark) = self.cfg.mem_high_watermark_bytes else {
            return false;
        };
        self.memory().is_some_and(|usage| usage > watermark)
    }

    /// Takes §8 Sizing's derived view cap (§8 Sizing, §9.2 #8).
    ///
    /// §8 asks for the cap to come from the usage *after preload*, and that
    /// moment is not when the scheduler starts: init forks every view from its
    /// supervision loop, which begins only once `BLASTIT_APP`,
    /// `BLASTIT_PRELOAD` and `BLASTIT_WARMUP` have been imported (§4), and the
    /// scheduler itself is forked before CPython exists. So the cap is taken
    /// twice:
    ///
    /// * `after_preload: false`, from [`Scheduler::start`], so the eager or
    ///   preforked burst obeys the cap it can already see rather than forking
    ///   everything the manifest names;
    /// * `after_preload: true`, at the first `SPAWNED` — the first proof the
    ///   preloaded heap is part of the reading — which replaces it, and which
    ///   also gives [`Scheduler::measure_view_memory`] the baseline it counts
    ///   the measured cost per view from.
    ///
    /// A reading that cannot be taken leaves the cap unset, which §8 asks for.
    fn derive_view_cap(&mut self, after_preload: bool) {
        if self.cfg.max_view_processes.is_some()
            || self.measured_view_mem.is_some()
            || (self.derived_view_cap.is_some() && (self.derived_after_preload || !after_preload))
        {
            return;
        }
        let Some(total) = self.cfg.mem_total_bytes else {
            return;
        };
        let Some(used) = self.memory_now() else {
            return;
        };
        if after_preload {
            // §8 Sizing: this reading is the `used` every later derivation
            // starts from, and the baseline the measured cost is counted
            // against.
            self.preload_used = Some(used);
            self.mem_sample = Some(MemSample { used, views: 0 });
        }
        self.derived_view_cap = Some(derived_view_cap(total, used, self.cfg.view_mem_estimate));
        self.derived_after_preload = after_preload;
    }

    /// §8 Sizing: measures what one more view process costs, from the rise in
    /// aggregate usage per view that came up, and re-derives the cap from it.
    ///
    /// Measured cost is the number §8 Sizing's default was always missing: each
    /// view copies most of the preloaded heap (about 135 MiB under gVisor on
    /// CPython 3.14, against a `BLASTIT_VIEW_MEM_ESTIMATE` of 32 MiB), so a cap
    /// derived from the configured estimate bounds nothing at all. The estimate
    /// is a moving average clamped to at least the configured one: a machine
    /// that measures nothing keeps the operator's number, and one that measures
    /// more than that gets the cap that fits.
    ///
    /// Nothing happens when `BLASTIT_MAX_VIEW_PROCESSES` is set (§8 only
    /// derives the default), when the machine's memory cannot be read, or
    /// before the first `SPAWNED` gave a baseline to measure against.
    fn measure_view_memory(&mut self) {
        if self.cfg.max_view_processes.is_some() || self.cfg.mem_total_bytes.is_none() {
            return;
        }
        let Some(usage) = self.memory_now() else {
            return;
        };
        let views = self.ready_view_count();
        let Some(previous) = self.mem_sample else {
            self.mem_sample = Some(MemSample { used: usage, views });
            return;
        };
        if views <= previous.views || usage <= previous.used {
            // Nothing to learn: no view came up since the last sample, or the
            // machine gave memory back rather than spending it. The sample
            // point still moves forward.
            self.mem_sample = Some(MemSample {
                used: usage.max(previous.used),
                views: views.max(previous.views),
            });
            return;
        }
        let added = u64::from(views - previous.views);
        let per_view = ((usage - previous.used) / added).max(self.cfg.view_mem_estimate);
        self.measured_view_mem = Some(match self.measured_view_mem {
            Some(previous) => (previous + per_view) / 2,
            None => per_view,
        });
        self.mem_sample = Some(MemSample { used: usage, views });
        self.derive_measured_cap();
    }

    /// The views that have sent `READY`, which is how many heap copies exist.
    fn ready_view_count(&self) -> u32 {
        u32::try_from(self.views.values().filter(|view| view.ready).count()).unwrap_or(u32::MAX)
    }

    /// Re-derives §8 Sizing's cap from the measured cost per view, and reports
    /// the pair when it changed.
    fn derive_measured_cap(&mut self) {
        let (Some(total), Some(used), Some(estimate)) = (
            self.cfg.mem_total_bytes,
            self.preload_used,
            self.measured_view_mem,
        ) else {
            return;
        };
        let cap = derived_view_cap(total, used, estimate);
        self.derived_view_cap = Some(cap);
        self.derived_after_preload = true;
        if self.reported_cap != Some((estimate, cap)) {
            self.reported_cap = Some((estimate, cap));
            self.emit_cap(estimate, cap);
        }
    }

    /// Emits `process.cap` (§8.1): the measured cost per view and the cap it
    /// buys. Only ever emitted when one of the two changes.
    fn emit_cap(&mut self, estimate: u64, cap: u32) {
        let event = Event::now(
            Component::Scheduler,
            EventKind::ProcessCap(ProcessCap::new(estimate, cap)),
        );
        (self.emit)(event);
    }

    /// The live view process cap in force: `BLASTIT_MAX_VIEW_PROCESSES`, or the
    /// §8 derived default once it has been measured.
    fn view_cap(&self) -> Option<u32> {
        self.cfg.max_view_processes.or(self.derived_view_cap)
    }

    // ------------------------------------------------------------- lifecycle

    /// Asks init for a view process, respecting the LRU cap and the memory high
    /// watermark (§2.4, §8 Sizing).
    ///
    /// When the cap is full, or usage is above the watermark, a view has to be
    /// retired first. Only [`Scheduler::evictable_view`] qualifies for that,
    /// and with no evictable view the spawn does not happen at all: the request
    /// that wanted it waits in the admission queue, where
    /// `BLASTIT_QUEUE_TIMEOUT` bounds the wait, instead of the cap forking and
    /// killing in a loop.
    fn try_spawn(&mut self, route: usize) -> Result<(), SchedulerError> {
        if self.views.contains_key(&route) {
            return Ok(());
        }
        if let Some(until) = self.backoff.get(&route).copied() {
            if Instant::now() < until {
                return Ok(());
            }
            self.backoff.remove(&route);
        }
        if !self.make_room(route)? {
            return Ok(());
        }
        // `make_room` retires at most one process, and the cap it retired for
        // may still be exceeded — a measured cap can land below the number of
        // processes that are already live. Then the spawn waits for the next
        // round of shedding rather than adding to the overshoot.
        if let Some(cap) = self.view_cap()
            && self.views.len() as u32 >= cap
        {
            return Ok(());
        }
        self.views.insert(route, View::spawning(route));
        let route_id = u32::try_from(route).unwrap_or(u32::MAX);
        if let Err(error) = self.ctl_push(Ctl::Spawn { route: route_id }) {
            self.views.remove(&route);
            return Err(error);
        }
        Ok(())
    }

    /// Makes room for a new view process for `route`, or says the spawn has to
    /// wait (§2.4, §8 Sizing).
    ///
    /// A full cap and a reading above `BLASTIT_MEM_HIGH_WATERMARK` both mean a
    /// process has to go first, and both use the same victim: the
    /// least-recently-used view that has nothing in flight and owes nothing to
    /// its own route's queue. Retiring one for memory pressure while the cap
    /// has room is deliberate — §8 Sizing would rather give a process back than
    /// add one to a machine that is already over the watermark.
    ///
    /// Returns `false` when the spawn must not happen: the caller leaves the
    /// request queued.
    fn make_room(&mut self, route: usize) -> Result<bool, SchedulerError> {
        let cap_full = self
            .view_cap()
            .is_some_and(|cap| self.views.len() as u32 >= cap);
        let over_watermark = self.above_watermark();
        if !cap_full && !over_watermark {
            return Ok(true);
        }
        let Some(victim) = self.evictable_view(route) else {
            return Ok(false);
        };
        let reason = if cap_full {
            RetireReason::LruCap
        } else {
            RetireReason::MemoryPressure
        };
        self.retire(victim, reason)?;
        Ok(true)
    }

    /// The view that may be retired to make room for `route`.
    ///
    /// It must be live, `READY`, have nothing in flight, and have no request
    /// waiting in the admission queue for its own route. A process that has not
    /// finished the request it was forked for — or one that is still waiting
    /// for its first `READY` — is never the victim: choosing one would kill the
    /// answer to a request that is about to be dispatched, and doing that once
    /// per pump is exactly the livelock this predicate exists to stop. The
    /// least recently used of the candidates wins.
    fn evictable_view(&self, route: usize) -> Option<ViewKey> {
        self.views
            .values()
            .filter(|view| {
                view.key.route != route
                    && view.ready
                    && !view.spawning
                    && view.key.pid != 0
                    && view.inflight == 0
                    && !self.route_waiting(view.key.route)
            })
            .min_by_key(|view| view.last_used)
            .map(|view| view.key)
    }

    /// Whether any request still in the admission queue is routed to `route`.
    fn route_waiting(&self, route: usize) -> bool {
        if let Some(queue) = self.queue.fair() {
            return queue.waiting_for_route(u32::try_from(route).unwrap_or(u32::MAX));
        }
        self.queue.ids().into_iter().any(|stream_id| {
            self.requests
                .get(&stream_id)
                .and_then(|request| request.route)
                == Some(route)
        })
    }

    /// Retires one view process: fail what it owes, ask init to kill it, and
    /// keep its slots until it is gone (§2.4).
    fn retire(&mut self, key: ViewKey, reason: RetireReason) -> Result<(), SchedulerError> {
        if self
            .views
            .get(&key.route)
            .map(|view| view.key != key)
            .unwrap_or(true)
        {
            return Ok(());
        }
        let affected: Vec<u32> = self
            .dispatch
            .iter()
            .filter(|((view, _), _)| *view == key)
            .map(|(_, stream_id)| *stream_id)
            .collect();
        for stream_id in &affected {
            self.fail_request(*stream_id, Outcome::ViewDied, 502)?;
        }
        let Some(view) = self.views.remove(&key.route) else {
            return Ok(());
        };
        // Dropping the queue ends the writer task, which closes the
        // scheduler's end of this view's channel.
        drop(view.out);
        // §2.4/§8 Sizing: a route must not be forked again immediately after a
        // retirement that was not about its own work. A view retired for the
        // cap before it ever served anything is a fork that bought nothing, so
        // its route waits `spawn_backoff` like a route whose spawn failed; one
        // retired for memory pressure would otherwise be respawned by the very
        // next request and copy the same heap back, which is the
        // retire-then-respawn thrash §8 Sizing warns about. Retiring a view
        // that has served is not rate-limited: that is ordinary LRU work.
        if matches!(reason, RetireReason::MemoryPressure)
            || (matches!(reason, RetireReason::LruCap) && view.sent == 0)
        {
            self.backoff
                .insert(key.route, Instant::now() + self.cfg.spawn_backoff);
        }
        let route_id = u32::try_from(key.route).unwrap_or(u32::MAX);
        self.ctl_push(Ctl::Retire {
            route: route_id,
            pid: key.pid,
            reason: reason.as_str().to_string(),
        })?;
        self.emit_retired(key, &reason);
        Ok(())
    }

    /// init answered a `SPAWN` with a pid and the `view.chan` descriptor (§4).
    fn view_spawned(
        &mut self,
        route: usize,
        pid: u32,
        fd: Option<OwnedFd>,
    ) -> Result<(), SchedulerError> {
        // §8 Sizing: this is the first moment the preloaded heap exists (see
        // `derive_view_cap`), so it is where the cap is measured for good.
        self.derive_view_cap(true);
        let Some(fd) = fd else {
            return Err(SchedulerError::protocol(
                "SPAWNED arrived without a view.chan descriptor",
            ));
        };
        let expecting = self
            .views
            .get(&route)
            .map(|view| view.spawning)
            .unwrap_or(false);
        if !expecting {
            // Nothing asked for this process: close our end and have init reap
            // it, so an unknown view cannot linger holding a uid.
            drop(fd);
            self.dropped += 1;
            return self.ctl_push(Ctl::Retire {
                route: u32::try_from(route).unwrap_or(u32::MAX),
                pid,
                reason: RetireReason::Other("unexpected_spawn".to_string())
                    .as_str()
                    .to_string(),
            });
        }
        let std_stream = std::os::unix::net::UnixStream::from(fd);
        std_stream
            .set_nonblocking(true)
            .map_err(SchedulerError::io)?;
        let stream = UnixStream::from_std(std_stream).map_err(SchedulerError::io)?;
        let (read, write) = stream.into_split();
        let key = ViewKey { route, pid };
        let (out, outgoing, pending) = Outbound::new(self.cfg.frame_limits, VIEW_OUTBOUND_CAP);
        let limits = self.cfg.frame_limits;
        tokio::spawn(read_frames(
            read,
            limits,
            self.view_in.clone(),
            move |id, frame| ViewIn::Frame { key, id, frame },
            move |_| ViewIn::Closed { key },
        ));
        tokio::spawn(write_peer(write, limits, outgoing, pending));
        if let Some(view) = self.views.get_mut(&route) {
            view.key = key;
            view.spawning = false;
            view.out = Some(out);
        }
        Ok(())
    }

    /// A view is ready to serve (§4).
    fn view_ready(&mut self, key: ViewKey) {
        let mut marked = false;
        if let Some(view) = self.views.get_mut(&key.route)
            && view.key == key
        {
            view.ready = true;
            view.spawning = false;
            // A view that has never served anything is idle from now on, so
            // even a preforked process that nobody asked for is retired by
            // `BLASTIT_VIEW_IDLE_TIMEOUT` (§2.4).
            if view.inflight == 0 {
                view.idle_since = Some(Instant::now());
            }
            marked = true;
        }
        if marked {
            // §8 Sizing: this is the first moment the view's share of the
            // preloaded heap exists in the aggregate reading.
            self.measure_view_memory();
        }
    }

    /// A view process is gone: fail its requests, free their slots, and let
    /// the next request for its route spawn a new one (§2.4).
    fn view_gone(&mut self, key: ViewKey) {
        // Whether this was still the live process for its route. A retired
        // view's channel also closes, and that is not a spawn failure.
        let live = self
            .views
            .get(&key.route)
            .map(|view| view.key == key)
            .unwrap_or(false);
        let ready = live
            && self
                .views
                .get(&key.route)
                .map(|view| view.ready)
                .unwrap_or(false);
        if live {
            self.views.remove(&key.route);
        }
        let affected: Vec<u32> = self
            .dispatch
            .iter()
            .filter(|((view, _), _)| *view == key)
            .map(|(_, stream_id)| *stream_id)
            .collect();
        for stream_id in affected {
            let _ = self.fail_request(stream_id, Outcome::ViewDied, 502);
            // The threads died with the process, so their slots are free even
            // though no `DONE` will ever arrive.
            self.release(stream_id);
        }
        if live && !ready {
            // It never served anything: whatever was waiting for it gets a
            // `502`, and the route backs off before it is spawned again.
            let waiting: Vec<u32> = self
                .queue
                .ids()
                .into_iter()
                .filter(|stream_id| {
                    self.requests
                        .get(stream_id)
                        .and_then(|request| request.route)
                        == Some(key.route)
                })
                .collect();
            for stream_id in waiting {
                self.queue.remove(stream_id);
                let _ = self.fail_request(stream_id, Outcome::ViewDied, 502);
                self.release(stream_id);
            }
            self.backoff
                .insert(key.route, Instant::now() + self.cfg.spawn_backoff);
        }
    }

    // --------------------------------------------------------------- replies

    /// Sends a small `text/plain` response the scheduler wrote itself.
    ///
    /// `retry_after` is §8's `Retry-After` in seconds, which every `503` the
    /// scheduler writes carries: a client that is turned away because the
    /// server is busy should be told when to come back, and §2.4's queue
    /// timeout is the number the server can actually promise.
    fn respond(
        &mut self,
        stream_id: u32,
        status: u16,
        reason: &str,
        body: &str,
        retry_after: Option<u64>,
    ) -> Result<(), SchedulerError> {
        let mut headers = vec![
            (
                b"Content-Type".to_vec(),
                b"text/plain; charset=utf-8".to_vec(),
            ),
            (
                b"Content-Length".to_vec(),
                body.len().to_string().into_bytes(),
            ),
        ];
        if let Some(seconds) = retry_after {
            headers.push((b"Retry-After".to_vec(), seconds.to_string().into_bytes()));
        }
        let head = RespHead {
            status,
            reason: reason.as_bytes().to_vec(),
            headers,
        };
        self.http_push(stream_id, Frame::RespHead(head))?;
        if !body.is_empty() {
            self.http_push(stream_id, Frame::RespBody(body.as_bytes().to_vec()))?;
        }
        self.http_push(stream_id, Frame::RespEnd)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.head_sent = true;
            request.status = status;
            request.bytes_out = body.len() as u64;
        }
        self.mark_finished(stream_id);
        Ok(())
    }

    /// Records where the response ended, which is where `duration_ms` stops
    /// (§8.1) even though the `request` event may wait for the view's `DONE`.
    fn mark_finished(&mut self, stream_id: u32) {
        if let Some(request) = self.requests.get_mut(&stream_id)
            && request.finished_at.is_none()
        {
            request.finished_at = Some(Instant::now());
        }
    }

    /// Answers a request the scheduler refused, and logs it.
    fn reject(
        &mut self,
        stream_id: u32,
        status: u16,
        reason: &str,
        body: &str,
        outcome: Outcome,
        retry_after: Option<u64>,
    ) -> Result<(), SchedulerError> {
        self.respond(stream_id, status, reason, body, retry_after)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.outcome = outcome;
            request.complete = true;
        }
        self.mark_finished(stream_id);
        self.emit_request(stream_id);
        self.release(stream_id);
        Ok(())
    }

    /// Marks an answer the scheduler just wrote as complete, and logs it.
    fn finish_immediately(
        &mut self,
        stream_id: u32,
        outcome: Outcome,
    ) -> Result<(), SchedulerError> {
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.outcome = outcome;
            request.complete = true;
        }
        self.mark_finished(stream_id);
        self.emit_request(stream_id);
        self.release(stream_id);
        Ok(())
    }

    /// Fails a request to http: a `502` when nothing was sent yet, a
    /// `RESP_ABORT` once the head is out.
    fn fail_request(
        &mut self,
        stream_id: u32,
        outcome: Outcome,
        status: u16,
    ) -> Result<(), SchedulerError> {
        let (head_sent, complete) = match self.requests.get(&stream_id) {
            Some(request) => (request.head_sent, request.complete),
            None => return Ok(()),
        };
        if complete {
            return Ok(());
        }
        if head_sent {
            self.http_push(
                stream_id,
                Frame::RespAbort {
                    reason: outcome.as_str().to_string(),
                },
            )?;
        } else {
            let (reason, body) = canned(status);
            // A `502` is not a `503`: the server is not busy, the view died.
            self.respond(stream_id, status, reason, body, None)?;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = outcome;
        }
        self.mark_finished(stream_id);
        self.emit_request(stream_id);
        Ok(())
    }

    /// Releases a request: its worker slot, its view id and its memory.
    fn release(&mut self, stream_id: u32) {
        self.charge(stream_id);
        self.emit_request(stream_id);
        let Some(request) = self.requests.remove(&stream_id) else {
            return;
        };
        if let Some((key, id)) = request.view {
            self.dispatch.remove(&(key, id));
            if let Some(view) = self.views.get_mut(&key.route)
                && view.key == key
            {
                view.inflight = view.inflight.saturating_sub(1);
                if view.inflight == 0 {
                    view.idle_since = Some(Instant::now());
                }
            }
        }
        if request.slots {
            self.slots = self.slots.saturating_sub(1);
        }
        self.queue.remove(stream_id);
        if let Some(flow) = request.flow {
            self.queue.release_flow(flow, Instant::now());
        }
    }

    /// What this request cost, and what the flow is charged for it (D5).
    ///
    /// The cost is the view's own report when there is one, **clamped** into
    /// `[floor, observed]`, where `observed` is the time the scheduler saw
    /// between dispatch and completion and `floor` the time up to the view's
    /// first response frame. Both bounds are the scheduler's own measurements,
    /// and both are needed because a view process is sandboxed and may be
    /// compromised: a report that claims more time than the request could
    /// possibly have taken would let one client buy the whole queue, and a
    /// report of nothing would let it keep its place at the front for free —
    /// while the floor is exactly the time the view cannot hide, because
    /// shortening it means delaying its own client's response.
    ///
    /// With no report — the view crashed, timed out, was cancelled, the client
    /// went away, or the view was built before D5 — the observed time is charged
    /// instead.
    ///
    /// It is idempotent, and it is called from two places: at the response end
    /// — with the report when the view sent one, with the observed time when it
    /// did not — and at release, for a request whose response never ends (a
    /// `504`, a cancellation, a view that died).
    fn charge(&mut self, stream_id: u32) {
        let Some(request) = self.requests.get(&stream_id) else {
            return;
        };
        if request.charged {
            return;
        }
        let now = Instant::now();
        let observed = request
            .dispatched_at
            .map(|since| now.duration_since(since))
            .map(nanos)
            .unwrap_or(0);
        let floor = match (request.dispatched_at, request.head_at) {
            // Dispatch to the view's first frame: the view's own work on the
            // request, which it cannot hide without delaying its client.
            (Some(dispatched), Some(head)) => head.duration_since(dispatched),
            _ => Duration::ZERO,
        };
        let floor = nanos(floor)
            // A clock that cannot go backwards still cannot make the floor the
            // ceiling: `clamp` would panic on `floor > observed`.
            .min(observed);
        let charged = match request.report {
            Some(report) => report.wall_ns.clamp(floor, observed),
            None => observed,
        };
        let flow = request.flow;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.charged_ns = charged;
            request.charged = true;
        }
        if let Some(flow) = flow {
            self.queue.debit(&flow, charged, Instant::now());
        }
    }

    /// Emits the one `request` event a request gets, once (§8.1).
    fn emit_request(&mut self, stream_id: u32) {
        let Some(request) = self.requests.get(&stream_id) else {
            return;
        };
        if request.emitted {
            return;
        }
        let now = Instant::now();
        let queued_until = request.dispatched_at.unwrap_or(now);
        let queued_ms = millis(queued_until.duration_since(request.arrived));
        // §8.1: `duration_ms` is the time to the last response byte, which is
        // where `finished_at` was set. The event itself may wait for the view's
        // `DONE` (D5), which is where the cost report travels.
        let duration_ms = millis(
            request
                .finished_at
                .unwrap_or(now)
                .duration_since(request.arrived),
        );
        // §8.1: the `path` is the routed `PATH_INFO`, and never a query
        // string. A target the scheduler could not decode has no `PATH_INFO`
        // at all, so it reports the target with its query string removed.
        let path = match &request.path_info {
            Some(path_info) => path_info.clone(),
            None => String::from_utf8_lossy(path_of_target(&request.head.target)).into_owned(),
        };
        let mut info = RequestInfo::from_target(
            String::from_utf8_lossy(&request.head.method).into_owned(),
            &path,
        )
        .with_status(request.status)
        .with_outcome(request.outcome)
        .with_timings(queued_ms, duration_ms)
        .with_bytes(request.bytes_in, request.bytes_out)
        .with_remote_addr(request.head.remote_addr.clone())
        .with_cost(
            request.charged_ns,
            // The view's own CPU measurement, when it could take one: absent is
            // not the same as zero (§8.1).
            request.report.and_then(|report| report.cpu_ns),
        );
        if let Some(rule) = request.rule {
            info = info.with_rule(rule);
        }
        let mut event = Event::now(Component::Scheduler, EventKind::Request(info))
            .with_request_id(request.request_id.clone());
        if let Some(route) = request.route
            && let Some(route) = self.routes.get(route)
        {
            event = event.with_route(route.name.clone());
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.emitted = true;
        }
        (self.emit)(event);
    }

    /// Emits `process.retired` for a view the scheduler retired (§8.1).
    fn emit_retired(&mut self, key: ViewKey, reason: &RetireReason) {
        let uid = self
            .routes
            .get(key.route)
            .map(|route| route.uid)
            .unwrap_or(0);
        let mut event = Event::now(
            Component::Scheduler,
            EventKind::ProcessRetired(ProcessRetired::new(key.pid, uid, reason.clone())),
        );
        if let Some(route) = self.routes.get(key.route) {
            event = event.with_route(route.name.clone());
        }
        (self.emit)(event);
    }

    // ----------------------------------------------------------- view frames

    /// `DONE` from a view: the thread exited, so the slot is free (§2.4).
    ///
    /// A `DONE` that arrives twice, or one that arrives after the response was
    /// already answered by a timeout, finds no stream to release the first
    /// time and is simply dropped the second.
    fn view_done(
        &mut self,
        key: ViewKey,
        id: u32,
        outcome: DoneOutcome,
    ) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let (complete, head_sent, cancelled, status) = match self.requests.get(&stream_id) {
            Some(request) => (
                request.complete,
                request.head_sent,
                request.cancelled,
                request.status,
            ),
            None => return Ok(()),
        };
        if !complete {
            match outcome {
                DoneOutcome::Completed if head_sent => {
                    // The thread finished; only its `RESP_END` is missing.
                    self.http_push(stream_id, Frame::RespEnd)?;
                    if let Some(request) = self.requests.get_mut(&stream_id) {
                        request.complete = true;
                        if !request.cancelled {
                            request.outcome = Outcome::Ok;
                        }
                    }
                    self.mark_finished(stream_id);
                }
                DoneOutcome::Completed | DoneOutcome::Errored => {
                    self.fail_request(stream_id, Outcome::ViewDied, 502)?;
                }
                DoneOutcome::Aborted => {
                    let outcome = if cancelled {
                        Outcome::ClientGone
                    } else {
                        Outcome::ViewDied
                    };
                    let status = if head_sent { status } else { 502 };
                    self.fail_request(stream_id, outcome, status)?;
                }
            }
        }
        self.release(stream_id);
        Ok(())
    }

    /// `RESP_HEAD` from a view.
    fn view_head(&mut self, key: ViewKey, id: u32, head: RespHead) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let forward = match self.requests.get(&stream_id) {
            Some(request) => !request.complete && !request.head_sent,
            None => false,
        };
        if !forward {
            return Ok(());
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.status = head.status;
            request.head_sent = true;
            // D5: the first frame the view produced is the floor of what its
            // request may be charged — the view cannot shorten that without
            // delaying its own response.
            request.head_at = Some(Instant::now());
        }
        self.http_push(stream_id, Frame::RespHead(head))
    }

    /// `RESP_BODY` from a view, which may not exceed the credit http granted.
    fn view_body(&mut self, key: ViewKey, id: u32, body: Vec<u8>) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let (forward, credit) = match self.requests.get(&stream_id) {
            Some(request) => (
                !request.complete && request.head_sent,
                request.download_credit,
            ),
            None => (false, 0),
        };
        if !forward {
            return Ok(());
        }
        if body.len() as u64 > credit as u64 {
            // The view sent more than http allowed (§3.1): recycle it.
            return self.retire(key, RetireReason::Other(CREDIT_OVERFLOW.to_string()));
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.download_credit -= body.len() as u32;
            request.bytes_out += body.len() as u64;
        }
        self.http_push(stream_id, Frame::RespBody(body))
    }

    /// `RESP_END` from a view: the response is complete, the slot is not.
    ///
    /// `cost` is the view's report for this request (D5) when it sent one: the
    /// wall time of `serve_request` — the WSGI call, through the iterable's
    /// `close()`, and the frames written back — and the thread's CPU time over
    /// the same span. It is untrusted, so [`Scheduler::charge`] clamps it
    /// between the view's first frame and the time the scheduler observed.
    ///
    /// A response that streams is charged here, when its stream ends, which is
    /// exactly when the report arrives.
    fn view_end(
        &mut self,
        key: ViewKey,
        id: u32,
        cost: Option<RequestCost>,
    ) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let forward = match self.requests.get(&stream_id) {
            Some(request) => !request.complete && request.head_sent,
            None => false,
        };
        if !forward {
            return Ok(());
        }
        self.http_push(stream_id, Frame::RespEnd)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            if !request.cancelled {
                request.outcome = Outcome::Ok;
            }
        }
        self.mark_finished(stream_id);
        // D5: the response's last byte is where a request's cost is fixed —
        // with the view's report when it sent one, with the time the scheduler
        // observed itself when it did not — so the event carries what was
        // actually charged, and the flow is behind the others before its thread
        // even exits. [`Scheduler::charge`] is idempotent, and
        // [`Scheduler::release`] charges the requests that never end a response
        // at all (a `504`, a cancellation, a view that died).
        if let Some(cost) = cost
            && let Some(request) = self.requests.get_mut(&stream_id)
        {
            request.report = Some(cost);
        }
        self.charge(stream_id);
        self.emit_request(stream_id);
        Ok(())
    }

    /// `RESP_ABORT` from a view: the thread gave up, with or without a head.
    ///
    /// §8.1's outcomes have no "the app raised", so this is reported as
    /// `view_died`, the same as a process that went away mid-response.
    fn view_abort(&mut self, key: ViewKey, id: u32, reason: &str) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let head_sent = match self.requests.get(&stream_id) {
            Some(request) if !request.complete => request.head_sent,
            _ => return Ok(()),
        };
        if head_sent {
            self.http_push(
                stream_id,
                Frame::RespAbort {
                    reason: reason.to_string(),
                },
            )?;
        } else {
            self.respond(stream_id, 502, "Bad Gateway", "bad gateway\n", None)?;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = Outcome::ViewDied;
        }
        self.mark_finished(stream_id);
        self.emit_request(stream_id);
        Ok(())
    }

    /// `WINDOW` from a view: the app consumed that much of the request body,
    /// which extends the credit http may send under (§3.1).
    fn view_window(&mut self, key: ViewKey, id: u32, bytes: u32) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        match self.requests.get(&stream_id) {
            Some(request) if !request.complete => {}
            _ => return Ok(()),
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.upload_credit = request.upload_credit.saturating_add(bytes);
        }
        self.http_push(stream_id, Frame::Window { bytes })
    }
}

/// Milliseconds as a float, for `request` events.
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The `Retry-After` a `503` carries, in seconds: §2.4's queue timeout rounded
/// up, never less than one second (§8: "`503` with `Retry-After`").
///
/// A client that is turned away because every worker is busy should be told
/// when to come back, and the queue timeout is the number the server can
/// actually promise: it is how long a request that *did* fit was willing to
/// wait. Zero — a queue that never waits — still says one second, because
/// `Retry-After: 0` asks a client to spin.
fn retry_after_seconds(timeout: Duration) -> u64 {
    let seconds = timeout
        .as_secs()
        .saturating_add(u64::from(timeout.subsec_nanos() > 0));
    seconds.max(1)
}

/// The reason phrase and body of a status the scheduler writes itself.
fn canned(status: u16) -> (&'static str, &'static str) {
    match status {
        400 => ("Bad Request", "bad request\n"),
        404 => ("Not Found", "not found\n"),
        502 => ("Bad Gateway", "bad gateway\n"),
        503 => ("Service Unavailable", "unavailable\n"),
        504 => ("Gateway Timeout", "gateway timeout\n"),
        _ => ("Error", "error\n"),
    }
}

/// Sleeps until `deadline`, or forever when there is none.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Owns `ctl.chan` for the whole run: reads init's messages into the
/// scheduler, and writes the scheduler's messages out.
///
/// The two directions share one task because the channel is one object. Only
/// the *scheduler's* loop matters for the "never wait for a peer" rule: it is
/// this task that would wait for init, and by then every message is already in
/// the unbounded queue.
async fn ctl_task<C: CtlChannel>(
    mut ctl: C,
    inbound: mpsc::Sender<CtlIn>,
    mut outbound: mpsc::UnboundedReceiver<OutFrame>,
    pending: Arc<AtomicUsize>,
    limits: Limits,
) {
    loop {
        tokio::select! {
            message = ctl.recv() => match message {
                Ok((message, fd)) => {
                    if inbound.send(CtlIn::Message(message, fd)).await.is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = inbound.send(CtlIn::Failed(error)).await;
                    return;
                }
            },
            // Only `Ctl` frames are ever pushed here, and dropping the queue
            // means the run is over.
            outgoing = outbound.recv() => {
                let Some(OutFrame::Frame { frame: Frame::Ctl(message), .. }) = outgoing else {
                    return;
                };
                let size = ctl_frame_size(&message, limits);
                if ctl.send(message).await.is_err() {
                    let _ = inbound.send(CtlIn::Failed(SchedulerError::closed())).await;
                    return;
                }
                release(&pending, size);
            }
        }
    }
}

/// Serves `http.chan` from `ctl.chan` and one `view.chan` per route.
///
/// Returns when init sends `DRAIN` (once the in-flight requests are done, or
/// `BLASTIT_REQUEST_TIMEOUT` after the message) or when `http.chan` closes. It
/// fails on a broken `ctl.chan`, on a `ctl.chan` message the protocol does not
/// allow, and on a `request`/`response` frame from http that does not decode.
///
/// # Contract with http
///
/// http never reuses a stream id while that stream is still in flight: the
/// scheduler tracks a request by its stream id until the view reports `DONE`
/// for it, and a second `REQ_HEAD` for a live id is a protocol error that ends
/// the run rather than letting one request inherit another's worker slot.
///
/// # Errors
///
/// [`SchedulerError`] for a broken `ctl.chan`, a message on it that the
/// protocol does not allow, a frame from http that does not decode, or one of
/// the byte-capped peer queues overflowing.
pub async fn run<C: CtlChannel + Send + 'static>(
    cfg: SchedulerConfig,
    routes: Vec<RouteInfo>,
    table: RoutingTable,
    http: UnixStream,
    ctl: C,
    emit: Emit,
) -> Result<(), SchedulerError> {
    let limits = cfg.frame_limits;
    let (http_read, http_write) = http.into_split();
    let (http_in_tx, mut http_in_rx) = mpsc::channel(HTTP_IN_CAPACITY);
    let (http_out, http_out_rx, http_pending) = Outbound::new(limits, HTTP_OUTBOUND_CAP);
    let (view_in_tx, mut view_in_rx) = mpsc::channel(VIEW_IN_CAPACITY);
    let (ctl_out, ctl_out_rx, ctl_pending) = Outbound::new(limits, CTL_OUTBOUND_CAP);

    tokio::spawn(read_frames(
        http_read,
        limits,
        http_in_tx.clone(),
        |id, frame| HttpIn::Frame { id, frame },
        |failure| match failure {
            Some(error) => HttpIn::Failed(error),
            None => HttpIn::Closed,
        },
    ));
    tokio::spawn(write_peer(http_write, limits, http_out_rx, http_pending));

    let (ctl_in_tx, mut ctl_in_rx) = mpsc::channel(CTL_IN_CAPACITY);
    tokio::spawn(ctl_task(ctl, ctl_in_tx, ctl_out_rx, ctl_pending, limits));

    // Held for the whole run, so `http_in_rx` only ever ends with an explicit
    // `HttpIn::Closed`.
    let _keepalive = http_in_tx;
    let mut scheduler = Scheduler::new(cfg, routes, table, emit, http_out, view_in_tx, ctl_out);
    scheduler.start()?;

    loop {
        let deadline = scheduler.next_deadline();
        tokio::select! {
            message = ctl_in_rx.recv() => match message {
                Some(CtlIn::Message(message, fd)) => scheduler.on_ctl(message, fd)?,
                Some(CtlIn::Failed(error)) => return Err(error),
                None => return Err(SchedulerError::closed()),
            },
            Some(event) = http_in_rx.recv() => scheduler.on_http(event)?,
            Some(event) = view_in_rx.recv() => scheduler.on_view(event)?,
            () = sleep_until(deadline) => {}
        }
        scheduler.pump()?;
        if scheduler.done() {
            return Ok(());
        }
    }
}
