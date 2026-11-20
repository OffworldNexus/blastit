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
//!   stuck_threads, memory_pressure, lru_cap, reload).
//! * A `RESP_ABORT` from a view is reported with the `view_died` outcome, and
//!   so is a `DONE { Errored }`: §8.1's outcome list is closed and has no "the
//!   app failed", so a response that died under the view is labelled like a
//!   process that did. A `5xx` a view **completed** is neither: it is `ok` when
//!   the handler returned, and `bailed` when a contained `exit()`/`die()`/fatal
//!   answered it and never returned, retiring that worker.
//! * A `DRAIN` rejects new requests with `503`/`queue_full` (the closest of the
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
//! * A worker that answers `DONE { Bailed }` retires by design — its handler
//!   never returned, so the runtime retires the process after that request
//!   ([`View::retiring`]). Nothing new is dispatched to it: the pump between
//!   that frame and the ordered close grows a replacement for the queue, so a
//!   request queued behind a `DONE { Bailed }` retirement is served, never
//!   answered `502`/`view_died`. The promise is only that wide: the
//!   runtime also retires after answering a `DONE { Errored }` and after an
//!   `Aborted` a client disconnect forced, and neither is marked — the
//!   scheduler cannot tell them from a request it had already answered itself
//!   ([`Scheduler::view_done`]). Those two shapes are covered only by the
//!   bounded [`Scheduler::expire_exited_views`] path, one `request_timeout`
//!   after the retirement, not at the next dispatch.
//! * Spawning is also refused while aggregate usage is above
//!   `BLASTIT_MEM_HIGH_WATERMARK` unless an evictable view can be retired
//!   first, so memory pressure never becomes retire-then-respawn thrash. A view
//!   retired for memory pressure cools its route's growth down for
//!   [`SchedulerConfig::spawn_backoff`]; no other retirement reason does, so a
//!   reload or a `max_requests` recycle never freezes a route that has requests
//!   waiting.
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
//! cancellation, a view that died) charges the observed time instead. A request
//! a view **bailed out of** — the handler answered a `5xx` and then `exit()`ed,
//! `die()`d or hit a fatal the SAPI shim contained, so it never returned — is
//! the exception: its report is not believed at all and it is charged the
//! request's own wall time (arrival to last byte), the `R19` ceiling at the one
//! live thread such a worker has, so a crash-loop can never cost less than the
//! client waited. That fact is not on `RESP_HEAD`; it is `DONE`'s `bailed`,
//! so a `5xx` a view answered is labelled and charged at its
//! `DONE`, one frame later. A `5xx` an app **answered itself** (a framework's
//! `500`, a script's `http_response_code(500)`) returned from its handler, so it
//! is `ok` with that status and keeps the clamp; a `5xx` the scheduler wrote
//! itself (`502`/`504`/`503`) keeps the dispatch→completion charge, because no
//! view answered it. `view_died` is the event table's "the view process died
//! mid-request", and the list is closed, so it is also the label for a response
//! that died under a live view (a `RESP_ABORT`, a `DONE { Errored }`) and for a
//! stream failed on both sides; a `5xx` a view completed is never it. The event
//! keeps both numbers as `cost_ns` (what was charged) and `cpu_ns` (what the
//! worker reported, `null` when it had no thread clock to read), and it is
//! still emitted when the scheduler learns the request ended — at `RESP_END`
//! for a response, otherwise at the statement that concludes it (the
//! scheduler's own answer, a cancellation, or release).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, VecDeque};
#[cfg(feature = "static")]
use std::io;
use std::os::fd::OwnedFd;
#[cfg(feature = "static")]
use std::os::unix::fs::FileExt;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use blastit_events::{
    Component, Event, EventKind, ManifestWarning, Outcome, ProcessCap, ProcessRetired, RequestInfo,
    RetireReason,
};
use blastit_frame::{
    Ctl, DoneOutcome, Frame, FrameError, INITIAL_WINDOW, Limits, RawFrame, ReqHead, ReqHeadView,
    RequestCost, RespHead, TypeCode,
};
use blastit_ingress::{BodyPlan, apply_transcoded_headers, plan_body, transcode_buffered};
#[cfg(feature = "static")]
use blastit_manifest::MountTable;
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
#[cfg(feature = "static")]
use crate::rewrite::{Rewrite, decide, declares_body};
use crate::route::RouteInfo;
#[cfg(feature = "static")]
use crate::static_asset::{
    STATIC_CHUNK_BYTES, StaticFile, StaticOutcome, StaticRoot, is_dotfile, is_php_source, segments,
};
#[cfg(feature = "static")]
use crate::static_response::{StaticBody, StaticResponse};
use crate::task::{
    OutFrame, Outbound, PushError, ctl_frame_size, read_frames, read_raw_frames, release,
    write_peer,
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
/// How many finished static reads may wait for the scheduler before a blocking
/// task waits. At most one read per admission slot can ever be outstanding, so
/// this is a handful of windows rather than a queue.
#[cfg(feature = "static")]
const STATIC_IN_CAPACITY: usize = 64;
/// The `reason` a static response is aborted with when its file cannot be read
/// in full (design §4.6): a short read is never answered with a body shorter
/// than the `Content-Length` the head promised.
#[cfg(feature = "static")]
const STATIC_READ_FAILED: &str = "static: read failed";

/// One finished static read, on its way back to the scheduler.
///
/// The [`StaticBody`] travels with the bytes so the descriptor the resolver
/// opened is never duplicated: a blocked read keeps it, and the scheduler's next
/// read takes it back from here.
#[cfg(feature = "static")]
struct StaticRead {
    /// The `http.chan` stream this read belongs to.
    stream_id: u32,
    /// The body source, with its `offset` still at the byte this read started
    /// from; the scheduler advances it when it accepts the bytes.
    body: StaticBody,
    /// How many bytes were asked for. Anything else — an error, or a read that
    /// returned fewer bytes than this — aborts the response.
    want: usize,
    /// The bytes read, or why the read failed.
    result: io::Result<Vec<u8>>,
}

/// The message a finished static read travels back on.
#[cfg(feature = "static")]
type StaticReadMsg = StaticRead;
/// A build without the `static` responder opens no read task and no root, so
/// its channel carries the unit message and its arm never completes: the
/// sender is held for the whole run, and [`run`]'s loop is the same in both
/// builds.
#[cfg(not(feature = "static"))]
type StaticReadMsg = ();

/// Where a `request` or `process.retired` event goes.
///
/// init owns stdout (§8.1): the scheduler hands every event to this closure,
/// which writes it to the events pipe. **It must not block**: it is called
/// inline from the scheduler's loop, and a blocking callback would stop every
/// stream on every channel.
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

/// A frame from `http.chan`, or the end of that channel.
enum HttpIn {
    /// One frame, muxed by the stream id http allocated, with its payload left
    /// as it was received (see [`Scheduler::on_http`]).
    Frame { id: u32, raw: RawFrame },
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

mod admission;
mod learn;
mod mounts;
mod reload;
mod request;
mod response;
mod run;
mod views;

use admission::{Admission, ViewSet};
use reload::MemSample;
use request::Request;
pub use run::run;

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
    /// The file's static mounts, compiled for the longest-prefix match (§9.2).
    #[cfg(feature = "static")]
    mounts: MountTable,
    /// One root per mount, opened by [`run`] before the first request: a mount
    /// whose root cannot be opened exits the role.
    #[cfg(feature = "static")]
    mount_roots: Vec<Arc<StaticRoot>>,
    /// The sending half of the static read results, cloned into one blocking
    /// task per read.
    #[cfg(feature = "static")]
    static_in: mpsc::Sender<StaticRead>,
    started: Instant,
    counter: u64,
    /// Every request that has not been released yet, by http stream id.
    requests: HashMap<u32, Request>,
    /// Requests waiting for a worker slot, oldest first (§2.4's bounded FIFO)
    /// or in fair order (D5).
    queue: Admission,
    /// The one unrouted `--learn` request currently served (`R26`).
    learn_fallback_busy: bool,
    /// Unrouted `--learn` requests waiting for that one slot, oldest first.
    learn_fallback_queue: VecDeque<u32>,
    /// The live workers of each route: a pool, at most one `SPAWN` in flight
    /// (`ADR-0015`).
    pools: HashMap<usize, ViewSet>,
    /// The reload generation: bumped by every [`Ctl::Reload`], stamped on every
    /// `SPAWN` and on every worker a `SPAWNED` creates, so a worker born before
    /// the last reload is retired before it serves (§4.6).
    reload_epoch: u64,
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
    pub(super) fn new(
        cfg: SchedulerConfig,
        routes: Vec<RouteInfo>,
        table: RoutingTable,
        emit: Emit,
        http_out: Outbound,
        view_in: mpsc::Sender<ViewIn>,
        ctl_out: Outbound,
        #[cfg(feature = "static")] mounts: MountTable,
        #[cfg(feature = "static")] mount_roots: Vec<Arc<StaticRoot>>,
        #[cfg(feature = "static")] static_in: mpsc::Sender<StaticRead>,
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
            #[cfg(feature = "static")]
            mounts,
            #[cfg(feature = "static")]
            mount_roots,
            #[cfg(feature = "static")]
            static_in,
            script_name,
            started: Instant::now(),
            counter: 0,
            requests: HashMap::new(),
            queue,
            learn_fallback_busy: false,
            learn_fallback_queue: VecDeque::new(),
            pools: HashMap::new(),
            reload_epoch: 0,
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
    pub(super) fn next_request_id(&mut self) -> String {
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
    pub(super) fn start(&mut self) -> Result<(), SchedulerError> {
        self.derive_view_cap(false);
        let wanted: Vec<usize> = (0..self.routes.len())
            .filter(|route| self.cfg.eager_spawn || self.routes[*route].prefork)
            // Every route is a view now; a mount owns no uid and starts no
            // process (§9.2).
            .filter(|route| self.spawnable(*route))
            .collect();
        for route in wanted {
            self.grow(route, 1)?;
        }
        Ok(())
    }

    /// Whether a view for `route` can ever serve a request.
    ///
    /// Every route is a view now: a `static "<prefix>"` mount is the file's and
    /// owns no uid, and a mount miss always falls through to the view table
    /// (§9.2), so a route is spawnable exactly when it exists.
    pub(super) fn spawnable(&self, route: usize) -> bool {
        self.routes.get(route).is_some()
    }

    /// Whether the run is over.
    pub(super) fn done(&self) -> bool {
        if self.http_closed {
            return true;
        }
        if !self.draining {
            return false;
        }
        let idle = self.slots == 0
            && self.queue.is_empty()
            && self.learn_fallback_queue.is_empty()
            && !self.learn_fallback_busy;
        let expired = match self.drain_deadline {
            Some(deadline) => Instant::now() >= deadline,
            None => true,
        };
        idle || expired
    }

    /// When the scheduler must wake up next, if ever.
    pub(super) fn next_deadline(&self) -> Option<Instant> {
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
        // A structured body still being collected, and an unrouted `--learn`
        // request waiting for the single fallback slot, both age the same way.
        for request in self.requests.values() {
            if !request.body_ready
                && let Some(since) = request.queued_since
            {
                take(since + self.cfg.queue_timeout);
            }
        }
        for stream_id in &self.learn_fallback_queue {
            if let Some(since) = self
                .requests
                .get(stream_id)
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
        // A view on its way out — init reported it gone, or it
        // answered `DONE { Bailed }` — whose `view.chan` has not
        // closed yet is failed when this elapses.
        for pool in self.pools.values() {
            if let Some((_, deadline)) = pool.spawning {
                // A `SPAWN` nobody answers is freed when this elapses (§4.7).
                take(deadline);
            }
            for view in pool.views.values() {
                if let Some(leaving) = view.leaving_at() {
                    take(leaving + self.cfg.request_timeout);
                }
            }
        }
        if !self.cfg.idle_timeout.is_zero() {
            for view in self.pools.values().flat_map(|pool| pool.views.values()) {
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
    pub(super) fn pump(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        self.backoff.retain(|_, until| *until > now);
        self.expire_spawns();
        self.expire_queue()?;
        self.pump_learn_fallback()?;
        self.dispatch_queued()?;
        self.expire_requests()?;
        self.expire_exited_views();
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
    pub(super) fn collect_flows(&mut self, now: Instant) {
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
    pub(super) fn on_http(&mut self, event: HttpIn) -> Result<(), SchedulerError> {
        let (id, raw) = match event {
            HttpIn::Closed => {
                self.http_closed = true;
                return Ok(());
            }
            // http is blastit's own code: bytes that are not frames mean the
            // build is broken, not that a peer misbehaved (§3.1).
            HttpIn::Failed(error) => return Err(SchedulerError::Frame(error)),
            HttpIn::Frame { id, raw } => (id, raw),
        };
        // A `REQ_HEAD` is read where it lies and forwarded verbatim, so the
        // scheduler never copies and re-encodes a header it will not look at.
        //
        // This is safe *only* for `http.chan`: http is blastit's own Rust code
        // — no Python, no attacker input — and it encodes from a typed `Frame`,
        // so its bytes are well-formed by construction. `view.chan` must NOT
        // take this shortcut: it carries frames from a sandboxed, potentially
        // compromised process, and `on_view` still receives them fully decoded
        // from the strict `read_frames`, which validates every field. Do not
        // apply this trick to the view channel.
        if raw.ty == TypeCode::ReqHead.code() {
            return self.begin_raw(id, raw);
        }
        let frame = match raw.into_frame() {
            Ok(frame) => frame,
            // A non-head payload that does not decode is a bug in this build,
            // exactly as a decode error used to be.
            Err(error) => return Err(SchedulerError::Frame(error)),
        };
        match frame {
            Frame::ReqBody(body) => self.request_body(id, body),
            Frame::ReqEnd => self.request_end(id),
            Frame::Cancel => self.client_cancel(id),
            Frame::Window { bytes } => self.client_window(id, bytes),
            // http never sends a response frame, and its head was handled
            // above; count anything else and move on.
            _ => {
                self.dropped += 1;
                Ok(())
            }
        }
    }

    /// Handles one `view.chan` frame, or the loss of a view process.
    pub(super) fn on_view(&mut self, event: ViewIn) -> Result<(), SchedulerError> {
        let (key, id, frame) = match event {
            ViewIn::Closed { key } => {
                self.view_gone(key);
                return Ok(());
            }
            ViewIn::Frame { key, id, frame } => (key, id, frame),
        };
        match frame {
            Frame::Ready => self.view_ready(key),
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
    pub(super) fn on_ctl(
        &mut self,
        message: Ctl,
        fd: Option<OwnedFd>,
    ) -> Result<(), SchedulerError> {
        match message {
            Ctl::Spawned { route, pid } => self.view_spawned(route as usize, pid, fd),
            Ctl::Exited { route, pid, .. } => {
                self.view_exited(ViewKey {
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
            // One save, one transaction over every pool (§4.6).
            Ctl::Reload => self.reload(),
            // The scheduler is the only sender of these.
            other => Err(SchedulerError::protocol(format!(
                "unexpected {} on ctl.chan",
                other.type_code().name()
            ))),
        }
    }

    // --------------------------------------------------------------- pushes

    /// Queues a frame towards http.
    pub(super) fn http_push(&mut self, id: u32, frame: Frame) -> Result<(), SchedulerError> {
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
    pub(super) fn ctl_push(&mut self, message: Ctl) -> Result<(), SchedulerError> {
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
    pub(super) fn view_push(
        &mut self,
        key: ViewKey,
        id: u32,
        frame: Frame,
    ) -> Result<(), SchedulerError> {
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

    /// Queues a raw frame towards one view, the same way [`Scheduler::view_push`]
    /// does but without re-encoding it.
    ///
    /// The byte cap is charged as [`crate::task::Outbound::push_raw`]'s own
    /// size, which is the size the frame will take on the wire, so recycling a
    /// view is decided against the same budget as a decoded frame.
    pub(super) fn view_push_raw(
        &mut self,
        key: ViewKey,
        raw: RawFrame,
    ) -> Result<(), SchedulerError> {
        let Some(out) = self.view_out(key) else {
            return Ok(());
        };
        match out.push_raw(raw) {
            Ok(()) => Ok(()),
            Err(PushError::Closed) => {
                self.view_gone(key);
                Ok(())
            }
            Err(PushError::Overflow { .. }) => {
                self.retire(key, RetireReason::Other(CREDIT_OVERFLOW.to_string()))
            }
            // `push_raw` does not encode, so it cannot report a frame error;
            // treat one as the build bug it would be.
            Err(PushError::Frame(error)) => Err(SchedulerError::Frame(error)),
        }
    }

    // --------------------------------------------------------------- request
}
