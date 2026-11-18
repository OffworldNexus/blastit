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
//!   answered it and never returned, retiring that worker (`MB.3.41`).
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
//!   answered `502`/`view_died` (`MB.26.8`). The promise is only that wide: the
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
//! client waited. That fact is not on `RESP_HEAD`; it is `DONE`'s `bailed`
//! (`MB.3.41`), so a `5xx` a view answered is labelled and charged at its
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
/// builds (`MB.36.2`).
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

/// One live view process of one route.
///
/// A route holds a pool of these ([`ViewSet`]): the pid init reported is the
/// identity, so two workers of one route are told apart exactly the way a
/// replaced instance used to be.
#[derive(Debug)]
struct View {
    key: ViewKey,
    /// The view sent `READY`; only then may it be dispatched to (§4).
    ready: bool,
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
    /// When init reported the process gone and the ordered `view.chan` has not
    /// reached [`ViewIn::Closed`] yet (`MB.3.45`).
    ///
    /// The scheduler learns a process exited on `ctl.chan`, a different input
    /// from the frames the process itself wrote, so the two can arrive in
    /// either order. The frames are the answer: a view that writes
    /// `RESP_HEAD`/`RESP_END_COST`/`DONE` and then retires **by design** (a
    /// contained bailout, `php.rs`) must be read before its requests are
    /// failed. Nothing new is dispatched to an exited view, and
    /// [`Scheduler::expire_exited_views`] bounds the wait.
    exited: Option<Instant>,
    /// When the worker answered a request the scheduler still tracked with
    /// `DONE { Bailed }`, and the ordered `view.chan` has not closed yet
    /// (`MB.26.8`).
    ///
    /// `Bailed` is the handler never returning — a contained `exit()`/`die()`/
    /// fatal — and the runtime retires the process after it (the view's
    /// `WorkerRuntime::recycle`, `crates/blastit-view/src/php.rs`). So the
    /// worker has just said it cannot serve another request, one `ctl.chan`
    /// report before [`View::exited`] arrives: without this, the pump between
    /// `DONE` and the close dispatches the queued head into a process that has
    /// already answered and is going away, and the close then bills that request
    /// `view_died`. Nothing new is dispatched to a retiring view; the close (or
    /// [`Scheduler::expire_exited_views`]) removes it and the route grows a
    /// replacement for the queue.
    retiring: Option<Instant>,
    /// A reload arrived while this worker was busy: nothing new reaches it
    /// (`View::admits`), and [`Scheduler::retire_finished_views`] retires it
    /// with this reason as soon as its last request is `DONE` (§4.6).
    stale: Option<RetireReason>,
    /// The `reload_epoch` current when this worker's `SPAWN` was sent: one
    /// whose birth predates the last reload holds the old code and is retired
    /// before it ever serves (§4.6).
    spawned_epoch: u64,
}

impl View {
    /// Whether this worker may take a request now.
    ///
    /// Ready, below its route's per-process ceiling, not stopping, not stale
    /// (a reload invalidated it), not on its way out (`exited`, or `retiring`
    /// after a `DONE { Bailed }`), and with its channel open. It borrows
    /// nothing from the [`Scheduler`] but the cap, so the selection in
    /// [`Scheduler::reserve`] can filter over the pool and take one entry
    /// mutably afterwards.
    fn admits(&self, per_process_cap: u32) -> bool {
        self.ready
            && !self.stopping
            && self.stale.is_none()
            && self.exited.is_none()
            && self.retiring.is_none()
            && self.inflight < per_process_cap
            && self.out.as_ref().is_some_and(Outbound::is_open)
    }

    /// When the worker became unable to serve, whichever report came first.
    ///
    /// Two signals say it is going away — init's `Ctl::Exited` and the
    /// worker's own `DONE { Bailed }` — on two different channels, so the
    /// earlier one is what [`Scheduler::expire_exited_views`] bounds.
    fn leaving_at(&self) -> Option<Instant> {
        match (self.exited, self.retiring) {
            (Some(exited), Some(retiring)) => Some(exited.min(retiring)),
            (exited, retiring) => exited.or(retiring),
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

/// Every worker of one route: the processes the scheduler dispatches into.
///
/// One route owns one of these, and only while it has a worker or a `SPAWN` in
/// flight ([`Scheduler::forget_if_idle`]). The membership is the whole point of
/// `ADR-0015`: a route's concurrency is its processes' ceilings, and the pool is
/// what lets a request-per-process provider serve two requests at once.
#[derive(Debug, Default)]
struct ViewSet {
    /// One entry per worker init reported, by the pid it gave.
    views: BTreeMap<u32, View>,
    /// The route's one outstanding `SPAWN`: the [`Scheduler::reload_epoch`] it
    /// was sent in, and when it stops being believable (§4.3's
    /// [`Scheduler::expire_spawns`]).
    spawning: Option<(u64, Instant)>,
    /// Workers forked but not yet `READY`: they already hold a heap copy, which
    /// is why growth waits for them before the first memory measurement.
    pending_ready: u32,
}

impl ViewSet {
    /// Workers plus the fork already promised; a `SPAWNED` worker is already in
    /// `views`, so nothing is counted twice.
    fn len(&self) -> u32 {
        u32::try_from(self.views.len())
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(self.spawning.is_some()))
    }

    /// Whether this route holds no worker and owes no fork.
    fn is_empty(&self) -> bool {
        self.views.is_empty() && self.spawning.is_none()
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
    /// The head has not been decoded (see [`Scheduler::begin_raw`]), so the
    /// flow is resolved from the payload's borrowed headers; `fair.rs` shares
    /// one reader between this and the decoded form the tests use, so the key
    /// cannot differ between the two.
    ///
    /// `now` is the scheduler's clock, converted once: the queue stamps
    /// `last_active` from the same clock its eviction rules read, so an idle
    /// flow's debt ages against the clock the scheduler actually uses (a paused
    /// one in a test, the wall clock in production) instead of against
    /// [`std::time::Instant::now`] behind its back.
    fn resolve_view(
        &mut self,
        peer: &str,
        head: &ReqHeadView<'_>,
        now: Instant,
    ) -> Option<FlowRef> {
        self.fair_mut()
            .map(|queue| queue.resolve_view(peer, head, now.into_std()))
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

/// The parts of a request head the scheduler itself reads.
///
/// The head is not decoded into a [`blastit_frame::ReqHead`]: its payload is
/// forwarded to the view verbatim (see [`Scheduler::begin_raw`]), so only what
/// routing and the `request` event need is kept here. The header list is not
/// among them — the fair-queue flow is resolved once, at admission, from the
/// borrowed head.
struct RequestHead {
    /// Request method, verbatim and case-preserved.
    method: Vec<u8>,
    /// Request target (origin form, e.g. `/x?y=1`), verbatim.
    target: Vec<u8>,
    /// Client address as the front end resolved it (ARCHITECTURE.md §2.4).
    remote_addr: String,
}

/// One request, from its `REQ_HEAD` to the `DONE` that releases its slot.
struct Request {
    /// http's stream id, for the response frames.
    stream_id: u32,
    /// The id reported as `request_id` in events (§8.1).
    request_id: String,
    /// The few head fields the scheduler reads; the rest travel to the view in
    /// `raw` untouched.
    head: RequestHead,
    /// The request head exactly as http sent it, forwarded to the view with
    /// only its id rewritten. Taken here on dispatch, so the payload is moved,
    /// never copied.
    raw: Option<RawFrame>,
    /// The route this request was routed to, if any (`None` for the requests
    /// the scheduler answers itself: health, `400`, `404`).
    route: Option<usize>,
    /// Index of the matching path regex in the route (§8.1 `rule`).
    rule: Option<u32>,
    /// The routed `PATH_INFO`: decoded and `SCRIPT_NAME`-stripped (§8.1).
    /// `None` when the target could not be decoded at all.
    path_info: Option<String>,
    arrived: Instant,
    /// Set while the request waits in the admission queue, and while it waits
    /// for the body that finishes it — a `Buffer` body still being collected,
    /// or the `REQ_END` that proves an `AllowEmptyOnly` one empty. Either way
    /// `expire_queue` answers it with `503 queue timeout` once
    /// [`SchedulerConfig::queue_timeout`] elapses.
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
    /// How the body must be handled, decided from the head alone (`R12`, `R16`).
    body_plan: BodyPlan,
    /// The status `body_plan` refused with, stored rather than answered on a
    /// route with `[route.static]` (design §4.4): the responder's method/body
    /// gate outranks a codec error on a path it answers, and only a `Forward`
    /// ever answers this.
    #[cfg(feature = "static")]
    body_plan_error: Option<u16>,
    /// Whether the request may be dispatched: an `AllowEmptyOnly` or `Buffer`
    /// body sets it only when its `REQ_END` proved the body complete (a
    /// `Buffer` one also transcoded), and a `StreamPassthrough` at its head
    /// (`R12`, `R16`).
    body_ready: bool,
    /// The matched mount claims this path and the head passed §9.2 step 0:
    /// `dispatch_static` is what admits it, and no view is involved until the
    /// mount misses and a view matched (§9.2 steps 1–2).
    static_pending: bool,
    /// The mount whose root is resolved, kept beside the segments.
    #[cfg(feature = "static")]
    static_mount: Option<usize>,
    /// The path below the mount's prefix, split into segments and kept until
    /// `dispatch_static` borrows it for `StaticRoot::resolve`; a queued static
    /// request therefore opens no descriptor (§9.2 step 1).
    static_parts: Option<Vec<String>>,
    /// `rewrite::declares_body` for this head: `Request` keeps no header list,
    /// so the one bit the answer needs is taken from the borrowed head.
    #[cfg(feature = "static")]
    static_has_body: bool,
    /// The request's `If-None-Match`, for the responder's `304` (design §4.5).
    /// `None` unless a static route asked for it.
    #[cfg(feature = "static")]
    static_if_none_match: Option<Vec<u8>>,
    /// The bytes still owed for a static `GET`, from the descriptor the
    /// resolver opened. `Some` only after `StaticRoot::respond` handed it over.
    #[cfg(feature = "static")]
    static_body: Option<StaticBody>,
    /// One static read is in flight for this request: its result is what
    /// releases the slot, never a timeout or a cancellation (design §4.6).
    pumping: bool,
    /// This request is an unrouted `--learn` request using the single fallback
    /// slot (`R26`, design §4.2 D.2).
    learn_fallback: bool,
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
    /// between the view's first frame and the time the scheduler observed, and
    /// ignores it when the view **bailed out** of a `5xx` — a failing view's
    /// report is not evidence. An app's own `500` returned, so its report keeps
    /// the clamp ([`answered_a_bailout`]).
    report: Option<RequestCost>,
    /// What was charged to the flow for this request, in nanoseconds: the
    /// reported cost clamped between the view's first frame and the observed
    /// time, the observed time when there was no report, or the request's own
    /// wall time when a view bailed out of a `5xx` it had answered (`R19`). This
    /// is the `cost_ns` of the `request` event (§8.1).
    charged_ns: u64,
    /// The flow has been charged for this request. Exactly one of the path that
    /// concludes the request and the release does it: the charge is idempotent,
    /// so whichever runs second is a no-op (D5).
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
    /// The `RESP_HEAD` http was sent came **from the view**, not from the
    /// scheduler's own `respond` (`MB.3.45`).
    ///
    /// `head_sent` answers "is the response under way"; this answers "did the
    /// view answer this request". They are one and the same for an app's
    /// response and different for every `5xx` the scheduler wrote itself (the
    /// `502` of a dead view, a `504` timeout, a `503` refusal), which is what
    /// [`answered_a_bailout`] and [`completed_outcome`] have to know: a view
    /// that later bails out of a request the scheduler already answered must
    /// not turn that answer into the `R19` crash bill.
    view_answered: bool,
    /// The view's handler bailed out of this request rather than returning
    /// (`DONE`'s `bailed`): a contained `exit()`/`die()`/fatal (`MB.3.41`).
    ///
    /// This is a different fact from the status it may have answered. A `5xx`
    /// an app answers itself (a framework's `500`, a script's
    /// `http_response_code(500)`) is an answer; only this flag is the `R19`
    /// crash that is charged its wall time.
    bailed: bool,
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
    /// The target cannot be routed: a `400` (§2.4). The decode or a C0 byte
    /// failed before any mount was consulted.
    Bad,
    /// A mount claimed the path and the resolver refuses it (`..`, a backslash,
    /// a leading `//`, a path over the ceiling): `400` final (§9.2 step 0).
    #[cfg(feature = "static")]
    MountRefused,
    /// A mount matched and the resolved path is a dotfile or a PHP source:
    /// `404` final, before any view (§9.2 step 2's `Hidden`).
    #[cfg(feature = "static")]
    Hidden,
    /// A view's path pattern matched but no view declared this method: `405`
    /// with the union of their declared methods (§9.2 step 4).
    MethodNotAllowed {
        /// The `Allow` header value, in declaration order.
        allow: String,
    },
    /// The health path, answered by the scheduler itself.
    Health,
    /// Nothing matched: a `404` (§2.4), the `--learn` fallback, or a mount that
    /// answered nothing and whose `fallback` did not either.
    Unrouted {
        /// The decoded target path, used only by the `--learn` fallback to
        /// attribute the request (`R26`).
        path: String,
        /// The mount that claims this path, when one does (§9.2 step 1).
        #[cfg(feature = "static")]
        mount: Option<MountHead>,
    },
    /// A route, one of its rules, and the `PATH_INFO` that matched.
    Routed {
        /// Route index.
        route: usize,
        /// Index of the matching path regex.
        rule: u32,
        /// The decoded, stripped path.
        path_info: String,
        /// The mount that claims this path, when one does: its file answers
        /// before the view, and its `fallback` answers after it (§9.2).
        #[cfg(feature = "static")]
        mount: Option<MountHead>,
    },
}

/// One matched mount and the request path below its prefix (§9.2).
#[cfg(feature = "static")]
struct MountHead {
    /// Index of the mount in `SchedulerConfig::mounts`.
    mount: usize,
    /// The decoded request path with the mount's prefix removed, split into
    /// segments: `StaticRoot::resolve`'s input. The dotfile and PHP-source
    /// rules were applied to the **full** path before the strip.
    segments: Vec<String>,
}

#[cfg(feature = "static")]
impl Resolution {
    /// The mount that claims this request, if any.
    fn mount(&self) -> Option<&MountHead> {
        match self {
            Self::Unrouted { mount, .. } | Self::Routed { mount, .. } => mount.as_ref(),
            Self::Bad | Self::Health | Self::MethodNotAllowed { .. } => None,
            Self::MountRefused | Self::Hidden => None,
        }
    }
}

/// The head half of a request to a route with `[route.static]` (design §4.3
/// steps 3–4, §4.4).
///
/// Built in [`Scheduler::begin_raw`] while the head is still the borrowed
/// payload, so [`Request`] keeps no header list: the answer is built from the
/// two facts this carries and the segments the resolver takes.
#[cfg(feature = "static")]
enum StaticHead {
    /// Steps 3–4 passed: `dispatch_static` resolves these segments once the
    /// request holds a slot, so a queued request opens no descriptor.
    Resolve {
        /// The mount whose root answers this path (§9.2 step 1).
        mount: usize,
        /// The path below the mount's prefix, split into segments.
        segments: Vec<String>,
        /// `rewrite::declares_body` for this head.
        has_body: bool,
        /// The `If-None-Match` the responder honours for a `304` (§4.5).
        if_none_match: Option<Vec<u8>>,
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
    fn new(
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
    fn spawnable(&self, route: usize) -> bool {
        self.routes.get(route).is_some()
    }

    /// Whether the run is over.
    fn done(&self) -> bool {
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
        // A view on its way out — init reported it gone (`MB.3.45`) or it
        // answered `DONE { Bailed }` (`MB.26.8`) — whose `view.chan` has not
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
    fn pump(&mut self) -> Result<(), SchedulerError> {
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
    fn on_view(&mut self, event: ViewIn) -> Result<(), SchedulerError> {
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
    fn on_ctl(&mut self, message: Ctl, fd: Option<OwnedFd>) -> Result<(), SchedulerError> {
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

    /// Queues a raw frame towards one view, the same way [`Scheduler::view_push`]
    /// does but without re-encoding it.
    ///
    /// The byte cap is charged as [`crate::task::Outbound::push_raw`]'s own
    /// size, which is the size the frame will take on the wire, so recycling a
    /// view is decided against the same budget as a decoded frame.
    fn view_push_raw(&mut self, key: ViewKey, raw: RawFrame) -> Result<(), SchedulerError> {
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

    /// Routes a new request whose head was not decoded, then dispatches or
    /// queues it (§2.4).
    ///
    /// Only `http.chan` reaches here (see [`Scheduler::on_http`]). The head is
    /// parsed just far enough to route it and to resolve its fair-queue flow,
    /// and the raw payload is kept for the view: everything the scheduler does
    /// not read — scheme, server name and port, and every header the app sees —
    /// reaches the view byte for byte, and no header is copied into an owned
    /// `Vec` on the way.
    fn begin_raw(&mut self, stream_id: u32, raw: RawFrame) -> Result<(), SchedulerError> {
        if self.requests.contains_key(&stream_id) {
            // http's contract is that a stream id is not reused while it is in
            // flight; anything else would let one request inherit another's
            // worker slot, so it is fatal rather than guessed at.
            return Err(SchedulerError::protocol(format!(
                "http.chan reused stream id {stream_id} while it was still in flight"
            )));
        }
        // http encodes from a typed `Frame`, so a head that does not parse is a
        // bug in this build, not a peer's fault (§3.1).
        let head = ReqHeadView::parse(&raw.payload).map_err(SchedulerError::Frame)?;
        let request_id = self.next_request_id();
        // The event's one clock: `arrived` and the flow the queue stamps are
        // read from it, so a request cannot age against two clocks at once.
        let now = Instant::now();
        let resolution = self.resolve(&head);
        // D5: the flow is resolved once per request, from the raw peer and the
        // headers, and it is the *same* resolution the front end used for
        // `REMOTE_ADDR`. It is resolved for a routed request before the request
        // is queued or refused, exactly as before. The queue is given this
        // event's clock, so what it stamps and what it later compares are one
        // clock.
        let flow = match &resolution {
            Resolution::Routed { .. } => self.queue.resolve_view(head.peer_addr(), &head, now),
            _ => None,
        };
        // `R12`: the head alone decides whether this route may have a body at
        // all, and `R16`: `raw` streams with zero buffering. It is planned from
        // the view, before the payload is moved into the request, so the
        // headers are never copied out to be read.
        let plan = self.plan_request_body(&head, &resolution);
        // Design §4.3 steps 3–4, and §4.4's `has_body`: on a route with
        // `[route.static]` the head is also decoded to segments here — before a
        // slot, a queue entry or any descriptor — so a `..` or a dotfile is
        // answered without ever reaching `admit_ready`.
        #[cfg(feature = "static")]
        let static_head = self.static_head(&head, &resolution);
        // The few head fields routing and the `request` event need, copied out
        // before the raw payload is moved into the request.
        let facts = RequestHead {
            method: head.method().to_vec(),
            target: head.target().to_vec(),
            remote_addr: head.remote_addr().to_string(),
        };
        let mut request = Request {
            stream_id,
            request_id,
            head: facts,
            raw: Some(raw),
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
            body_plan: BodyPlan::AllowEmptyOnly,
            #[cfg(feature = "static")]
            body_plan_error: None,
            body_ready: true,
            static_pending: false,
            #[cfg(feature = "static")]
            static_mount: None,
            static_parts: None,
            #[cfg(feature = "static")]
            static_has_body: false,
            #[cfg(feature = "static")]
            static_if_none_match: None,
            #[cfg(feature = "static")]
            static_body: None,
            pumping: false,
            learn_fallback: false,
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
            view_answered: false,
            bailed: false,
            complete: false,
            emitted: false,
            timed_out: false,
            slots: false,
            upload_credit: INITIAL_WINDOW,
            download_credit: INITIAL_WINDOW,
        };
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
            #[cfg(feature = "static")]
            Resolution::MountRefused => {
                // §9.2 step 0: a shape the resolver refuses is a `400` before
                // any mount or view is consulted.
                self.requests.insert(stream_id, request);
                self.refuse_static(stream_id, 400, None)
            }
            #[cfg(feature = "static")]
            Resolution::Hidden => {
                // §9.2 step 0: the full path is a dotfile or a PHP source
                // before any mount or view is consulted, and no view may answer
                // it.
                self.requests.insert(stream_id, request);
                self.refuse_static(stream_id, 404, None)
            }
            Resolution::MethodNotAllowed { allow } => {
                // §9.2 step 4: a view's pattern matched and no view declared
                // this method.
                self.requests.insert(stream_id, request);
                self.refuse_static(stream_id, 405, Some(allow.as_bytes()))
            }
            Resolution::Health => {
                self.requests.insert(stream_id, request);
                self.respond(stream_id, 200, "OK", "ok", None)?;
                self.finish_immediately(stream_id, Outcome::Ok)
            }
            Resolution::Unrouted { path, .. } => {
                request.path_info = Some(path);
                // §9.2 step 1/3: a mount that claims the path answers its file
                // or its `fallback` even when no view matched. It takes a slot
                // exactly like a routed static request, and a miss that reaches
                // step 4 is a `404` (or the `--learn` fallback).
                #[cfg(feature = "static")]
                if let Some(StaticHead::Resolve {
                    mount,
                    segments,
                    has_body,
                    if_none_match,
                }) = static_head
                {
                    request.static_pending = true;
                    request.static_mount = Some(mount);
                    request.static_parts = Some(segments);
                    request.static_has_body = has_body;
                    request.static_if_none_match = if_none_match;
                    request.body_plan = BodyPlan::AllowEmptyOnly;
                    request.body_ready = true;
                    self.requests.insert(stream_id, request);
                    return self.admit_ready(stream_id);
                }
                self.requests.insert(stream_id, request);
                if self.cfg.learn_mode {
                    // `--learn` never short-circuits an unrouted request: it is
                    // served by the app one at a time so its SQL can be
                    // attributed to `ObsTarget::Unrouted` (`R26`).
                    let retry_after = retry_after_seconds(self.cfg.queue_timeout);
                    if self.learn_fallback_queue.len() >= self.cfg.queue_size {
                        return self.reject(
                            stream_id,
                            503,
                            "Service Unavailable",
                            "queue full\n",
                            Outcome::QueueFull,
                            Some(retry_after),
                        );
                    }
                    if let Some(request) = self.requests.get_mut(&stream_id) {
                        request.queued_since = Some(now);
                    }
                    self.learn_fallback_queue.push_back(stream_id);
                    self.pump_learn_fallback()
                } else {
                    self.reject(
                        stream_id,
                        404,
                        "Not Found",
                        "not found\n",
                        Outcome::Unmatched,
                        None,
                    )
                }
            }
            Resolution::Routed {
                route,
                rule,
                path_info,
                ..
            } => {
                request.route = Some(route);
                request.rule = Some(rule);
                request.path_info = Some(path_info);
                // D5: the flow was resolved once, from the raw peer and the
                // headers of this request's head, before it was moved here —
                // and it is the *same* resolution the front end used for
                // `REMOTE_ADDR`, stamped by the queue with this event's clock.
                request.flow = flow;
                // §9.2 steps 1–2: a mount's file answers first and a miss falls
                // through to this view. The request is admitted at the head; the
                // plan is still computed (a miss that forwards needs it) but its
                // error is *stored*, not answered: the mount's method/body gate
                // outranks a codec error on a path a file answers.
                #[cfg(feature = "static")]
                if let Some(StaticHead::Resolve {
                    mount,
                    segments,
                    has_body,
                    if_none_match,
                }) = static_head
                {
                    request.static_pending = true;
                    request.static_mount = Some(mount);
                    request.static_parts = Some(segments);
                    request.static_has_body = has_body;
                    request.static_if_none_match = if_none_match;
                    request.body_plan = match plan {
                        Ok(plan) => plan,
                        // Nothing is buffered or transcoded for a static
                        // answer; the plan's error is what the view answers
                        // when the mount misses.
                        Err(_) => BodyPlan::AllowEmptyOnly,
                    };
                    request.body_plan_error = plan.err();
                    request.body_ready = true;
                    self.requests.insert(stream_id, request);
                    return self.admit_ready(stream_id);
                }
                // `R12`/`R16`: `plan` was decided from the same head, and `Err`
                // carries the status a body this route will not take is
                // answered with.
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(status) => {
                        self.requests.insert(stream_id, request);
                        return self.refuse_body(stream_id, status);
                    }
                };
                request.body_plan = plan;
                // `R12` from the other end: only a `raw` route may see its head
                // before `REQ_END`, because it forwards every byte as it
                // arrives (`R16`). An `AllowEmptyOnly` route that reaches a
                // view holds no body at all, and nothing in the head can prove
                // a chunked — or not yet arrived — body empty: `REQ_END` is the
                // one proof. Until it arrives the request is not body-ready, so
                // it cannot be dispatched and no view can answer the head this
                // request must instead `413` on the first body byte. A
                // `Buffer` body is collected until it transcodes or overflows.
                // Both wait under `queued_since`, which bounds a body that
                // never ends (`expire_queue`, `503 queue timeout`) instead of
                // parking the request — and both are counted against
                // `queue_size` here, because a request waiting for a body is a
                // request a peer is holding open, exactly like one in the
                // admission queue. Without that count a peer could park an
                // unbounded number of heads, each holding its raw payload.
                let waiting = !matches!(plan, BodyPlan::StreamPassthrough);
                request.body_ready = !waiting;
                if waiting {
                    request.queued_since = Some(now);
                }
                self.requests.insert(stream_id, request);
                let over_capacity = if !waiting {
                    false
                } else if matches!(plan, BodyPlan::Buffer { .. }) {
                    // A structured body is buffered up to
                    // `BLASTIT_MAX_BODY_BYTES` and transcoded before the view
                    // sees a byte.
                    self.buffering_count() >= self.cfg.queue_size
                } else {
                    // An empty-only request holds no bytes, but it holds its
                    // stream: the same bound, for the same reason.
                    self.awaiting_end_count() >= self.cfg.queue_size
                };
                if over_capacity {
                    return self.reject(
                        stream_id,
                        503,
                        "Service Unavailable",
                        "queue full\n",
                        Outcome::QueueFull,
                        Some(retry_after_seconds(self.cfg.queue_timeout)),
                    );
                }
                if waiting {
                    return Ok(());
                }
                self.admit_ready(stream_id)
            }
        }
    }

    /// The static head half of §9.2 and §4.4's `has_body`.
    ///
    /// Runs while the head is still the borrowed payload, so nothing it returns
    /// borrows the request: [`Request`] keeps no header list, and this is where
    /// the one `declares_body` bit and the `If-None-Match` the responder honours
    /// come from. `None` for a request no mount claims, which takes the view
    /// path unchanged.
    ///
    /// The segments are already decoded and split, and the dotfile and
    /// PHP-source rules were applied to the full path in [`Scheduler::resolve`],
    /// so a refused (`400`) or hidden (`404`) path is answered there, before
    /// `admit_ready`: no slot is taken, nothing queues, and no `..` probe can
    /// turn into a `503` (§9.2).
    #[cfg(feature = "static")]
    fn static_head(&self, head: &ReqHeadView<'_>, resolution: &Resolution) -> Option<StaticHead> {
        let mount = resolution.mount()?;
        let headers: Vec<(Vec<u8>, Vec<u8>)> = head
            .headers()
            .map(|(name, value)| (name.to_vec(), value.to_vec()))
            .collect();
        let has_body = declares_body(&headers);
        let if_none_match = head
            .headers()
            .find(|(name, _)| name.eq_ignore_ascii_case(b"if-none-match"))
            .map(|(_, value)| value.to_vec());
        Some(StaticHead::Resolve {
            mount: mount.mount,
            segments: mount.segments.clone(),
            has_body,
            if_none_match,
        })
    }

    /// The `[route.ingress]` plan for a routed request, from its head alone.
    ///
    /// The head is still the raw payload's borrowed view here (see
    /// [`Scheduler::begin_raw`]), so the method and the three headers the plan
    /// reads are taken from it without decoding or copying the rest — the same
    /// head the view will receive byte for byte.
    ///
    /// A request the scheduler answers itself never reaches a view, so it has
    /// no ingress to honour and may only ever hold an empty body.
    ///
    /// A routed request whose head **declares** a body is refused here, before
    /// it is dispatched: `Content-Length: 0` and a chunked head are the only
    /// shapes that may still be empty, and only `REQ_END` proves it (see
    /// [`Scheduler::begin_raw`]).
    ///
    /// `Err` carries the status the scheduler answers with (`400` for a codec
    /// mismatch, `413` for a body on a route that takes none).
    fn plan_request_body(
        &self,
        head: &ReqHeadView<'_>,
        resolution: &Resolution,
    ) -> Result<BodyPlan, u16> {
        let Resolution::Routed { route, .. } = resolution else {
            return Ok(BodyPlan::AllowEmptyOnly);
        };
        // A route with no `[route.ingress]` may only ever hold an empty body.
        // `plan_body` already answers a declared non-zero length with
        // `BodyNotAllowed` — the `413` of `R12` — and it is asked here, from
        // the head, so that answer is synchronous: no view has the head yet,
        // no body byte has arrived and no `REQ_END` is needed. Only the length
        // is read, so none of the three header `String`s below is built for
        // the routes that declare no ingress at all.
        let Some(policy) = self.routes.get(*route).and_then(|route| route.ingress) else {
            let method = std::str::from_utf8(head.method()).unwrap_or_default();
            return plan_body(method, None, None, None, declared_content_length(head))
                .map_err(|reject| reject.status());
        };
        let method = String::from_utf8_lossy(head.method()).into_owned();
        let content_type = head_header_text(head, b"content-type");
        let content_encoding = head_header_text(head, b"content-encoding");
        let content_length = declared_content_length(head);
        plan_body(
            &method,
            Some(policy),
            content_type.as_deref(),
            content_encoding.as_deref(),
            content_length,
        )
        .map_err(|reject| reject.status())
    }

    /// Hands a request whose head/body is ready to the admission machinery.
    ///
    /// A request to a route with `[route.static]` is admitted by the slot alone
    /// (design §4.6): its static branch comes before `can_dispatch`, because no
    /// view exists to be asked.
    fn admit_ready(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        if self.static_pending(stream_id) {
            return self.admit_static(stream_id);
        }
        let Some(route) = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.route)
        else {
            return Ok(());
        };
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

    /// Admits a request a mount may answer itself (§9.2): the same bounded
    /// queue, `queue_timeout` and `503` + `Retry-After` as the view path, with
    /// the slot as the only eligibility.
    fn admit_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
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
        } else if self.can_dispatch_static() {
            self.dispatch_static(stream_id)
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

    /// Whether a static answer may take a slot: `slots < max_workers`, with no
    /// view involved (design §4.6).
    ///
    /// Because a static response keeps its slot until its last byte — or its
    /// read result — this bound is also the reader and descriptor bound: at most
    /// `max_workers` reads and `max_workers` static descriptors.
    fn can_dispatch_static(&self) -> bool {
        self.slots < self.cfg.max_workers
    }

    /// Whether `stream_id` has matched a mount and not resolved yet.
    fn static_pending(&self, stream_id: u32) -> bool {
        self.requests
            .get(&stream_id)
            .is_some_and(|request| request.static_pending)
    }

    /// Whether a static read still owns `stream_id`.
    ///
    /// A `pread` cannot be cancelled, so a timed-out or cancelled request keeps
    /// its slot and its descriptor until the read returns: the result releases
    /// both, and never the timeout (design §4.6).
    fn static_read_parked(&self, stream_id: u32) -> bool {
        self.requests
            .get(&stream_id)
            .is_some_and(|request| request.pumping)
    }

    /// The first queued request that still has to resolve against a mount's
    /// root.
    ///
    /// Only used by the fair walk, whose ordering is a view-budget decision and
    /// has nothing to say about a request that never touches a view.
    fn first_static_queued(&self) -> Option<u32> {
        self.queue.ids().into_iter().find(|stream_id| {
            self.requests
                .get(stream_id)
                .is_some_and(|request| request.static_pending && request.static_parts.is_some())
        })
    }

    /// How many requests are still collecting a structured body.
    fn buffering_count(&self) -> usize {
        self.requests
            .values()
            .filter(|request| {
                // Only a half-collected structured body holds bytes: an
                // `AllowEmptyOnly` request waiting for its `REQ_END` holds
                // none, so it must not take a buffer slot from a route that
                // does buffer.
                matches!(request.body_plan, BodyPlan::Buffer { .. })
                    && !request.body_ready
                    && !request.complete
                    && request.view.is_none()
            })
            .count()
    }

    /// How many requests are held at their head, waiting for the `REQ_END` that
    /// proves their body empty (`R12`).
    ///
    /// A request in this state has not been dispatched and holds no body byte,
    /// but a peer is keeping its stream open, so it takes the same
    /// `queue_size` allowance a half-collected body does: [`Scheduler::begin_raw`]
    /// answers one over that with `503 queue full` instead of parking it.
    fn awaiting_end_count(&self) -> usize {
        self.requests
            .values()
            .filter(|request| {
                !matches!(request.body_plan, BodyPlan::Buffer { .. })
                    && !request.body_ready
                    && !request.complete
                    && request.view.is_none()
            })
            .count()
    }

    /// The view an unrouted `--learn` request is served by: the first route's,
    /// when the manifest declared one (`R26`).
    fn learn_route(&self) -> Option<usize> {
        (!self.routes.is_empty()).then_some(0)
    }

    /// Starts the next unrouted `--learn` request, if the single slot is free
    /// and a view can take it (`R26`, design §4.2 D.2).
    ///
    /// The slot is what keeps `ObsTarget::Unrouted` attribution unambiguous:
    /// two unrouted requests never run at once, so every statement a session
    /// reports belongs to exactly one request.
    fn pump_learn_fallback(&mut self) -> Result<(), SchedulerError> {
        if !self.cfg.learn_mode || self.learn_fallback_busy {
            return Ok(());
        }
        let Some(&stream_id) = self.learn_fallback_queue.front() else {
            return Ok(());
        };
        if !self.requests.contains_key(&stream_id) {
            self.learn_fallback_queue.pop_front();
            return Ok(());
        }
        let Some(route) = self.learn_route() else {
            // Nothing to run the request through: learn mode has no route to
            // observe, so the request is still an honest `404`.
            self.learn_fallback_queue.pop_front();
            return self.reject(
                stream_id,
                404,
                "Not Found",
                "not found\n",
                Outcome::Unmatched,
                None,
            );
        };
        if !self.can_dispatch(route) {
            if !self.has_dispatchable(route) && !self.draining {
                // Its request waits in `learn_fallback_queue`, not in `queue`,
                // so `needed_processes` would be 0 and the one fallback slot
                // would never get a process (§4.3).
                self.grow(route, 1)?;
            }
            return Ok(());
        }
        self.learn_fallback_queue.pop_front();
        self.learn_fallback_busy = true;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.learn_fallback = true;
            request.route = Some(route);
            // An unrouted request has no declared ingress: never short-circuit
            // it, and never buffer it (`R26`).
            request.body_plan = BodyPlan::StreamPassthrough;
            request.body_ready = true;
            request.queued_since = None;
        }
        self.dispatch(stream_id)
    }

    /// Turns the request target into a routing decision (§2.4, §6).
    fn resolve(&self, head: &ReqHeadView<'_>) -> Resolution {
        let path = path_of_target(head.target());
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
        // An unrouted request reports the path exactly as decoded, before any
        // mount prefix is stripped: that is what the client asked for.
        let request_path = path.to_string();
        let Ok(method) = std::str::from_utf8(head.method()) else {
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
                None => {
                    return Resolution::Unrouted {
                        path: request_path,
                        #[cfg(feature = "static")]
                        mount: None,
                    };
                }
            },
            None => path,
        };
        // §9.2 step 0: the decode and the path-shape refusal, plus the dotfile
        // and PHP-source rules, all run on the **full** path before the prefix
        // is stripped, so a mount can neither hide a refusal nor manufacture
        // one. They only apply when a mount claims the path: an app is free to
        // serve a dotfile of its own.
        #[cfg(feature = "static")]
        let mount = match self.mounts.lookup(path) {
            None => None,
            Some(found) => {
                let Ok(parts) = segments(path) else {
                    return Resolution::MountRefused;
                };
                if is_dotfile(&parts) {
                    return Resolution::Hidden;
                }
                if parts.last().is_some_and(|segment| is_php_source(segment)) {
                    return Resolution::Hidden;
                }
                let below = segments(&self.cfg.mounts[found.mount].prefix)
                    .map(|prefix| prefix.len())
                    .unwrap_or(0);
                let relative = parts.get(below..).unwrap_or_default();
                Some(MountHead {
                    mount: found.mount,
                    segments: relative.iter().map(|part| (*part).to_string()).collect(),
                })
            }
        };
        match self.table.route(method, path) {
            Some(matched) => Resolution::Routed {
                route: matched.route,
                rule: u32::try_from(matched.rule).unwrap_or(u32::MAX),
                path_info: path.to_string(),
                #[cfg(feature = "static")]
                mount,
            },
            None => {
                // §9.2 step 4: a path a view's pattern matched but whose method
                // no view declared is a `405`, not an unrouted `404`.
                if let Some(allow) = self.table.methods_for(path) {
                    return Resolution::MethodNotAllowed {
                        allow: allow.join(", "),
                    };
                }
                Resolution::Unrouted {
                    path: request_path,
                    #[cfg(feature = "static")]
                    mount,
                }
            }
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
        match request.body_plan {
            // A route that accepts no body may see `REQ_END`, never a byte
            // (`R12`, `ADR-0006`).
            BodyPlan::AllowEmptyOnly => {
                if body.is_empty() {
                    return Ok(());
                }
                self.refuse_body(stream_id, 413)
            }
            BodyPlan::StreamPassthrough => {
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
            // `R14`/`R15`: the whole body is validated and re-encoded before a
            // view sees it, so it is buffered here — bounded, and with the
            // consumed credit handed back so an upload may exceed one window.
            BodyPlan::Buffer { .. } => {
                let allowed = request.upload_credit as u64;
                if body.len() as u64 > allowed {
                    return self.http_credit_violation(stream_id);
                }
                let total = request.bytes_in.saturating_add(body.len() as u64);
                if total > self.cfg.max_body_bytes {
                    return self.refuse_body(stream_id, 413);
                }
                let credit = u32::try_from(body.len()).unwrap_or(u32::MAX);
                let count = body.len() as u64;
                if let Some(request) = self.requests.get_mut(&stream_id) {
                    request.bytes_in += count;
                    request.upload_credit = request
                        .upload_credit
                        .saturating_sub(credit)
                        .saturating_add(credit);
                    request.body.push(body);
                }
                // The scheduler consumed those bytes, so the client may send
                // that much more while the body is still being collected.
                if credit > 0 {
                    self.http_push(stream_id, Frame::Window { bytes: credit })?;
                }
                Ok(())
            }
        }
    }

    /// Forwards `REQ_END`, or remembers it for a queued request.
    fn request_end(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete || request.end_seen {
            return Ok(());
        }
        if let BodyPlan::Buffer { codec } = request.body_plan {
            // The body is complete: canonicalise it before dispatch so the view
            // only ever sees valid, re-encoded bytes and no stale framing
            // headers (`R14`, `R15`).
            //
            // The head reached here as the raw payload (see
            // [`Scheduler::begin_raw`]). Canonicalising replaces the framing
            // headers the client sent, so this is the one place that has to
            // decode it — the re-encoded head is what the view then reads.
            // Routes with no ingress configured take the streaming path below
            // and never touch their head's bytes.
            let Some(raw) = request.raw.as_ref() else {
                // A buffered request keeps its raw head until dispatch; losing
                // it would be a bug in this build, not a peer's fault.
                return Err(SchedulerError::protocol(
                    "a buffered request reached REQ_END without its raw head",
                ));
            };
            let mut head = match raw.decode().map_err(SchedulerError::Frame)? {
                Frame::ReqHead(head) => head,
                _ => {
                    return Err(SchedulerError::protocol(
                        "a request's raw head did not decode as REQ_HEAD",
                    ));
                }
            };
            let id = raw.id;
            let method = String::from_utf8_lossy(&head.method).into_owned();
            let content_type = header_text(&head, b"content-type");
            let raw_body: Vec<u8> = request.body.iter().flatten().copied().collect();
            if raw_body.len() as u64 > self.cfg.max_body_bytes {
                return self.refuse_body(stream_id, 413);
            }
            let transcoded =
                match transcode_buffered(codec, &method, content_type.as_deref(), &raw_body) {
                    Ok(body) => body,
                    Err(reject) => return self.refuse_body(stream_id, reject.status()),
                };
            apply_transcoded_headers(
                &mut head.headers,
                transcoded.content_type,
                transcoded.bytes.len(),
            );
            let head = Frame::ReqHead(head)
                .to_raw(id)
                .map_err(SchedulerError::Frame)?;
            // A request already waiting for a slot keeps its one queue entry,
            // its `queued_since` and its timeout: `enqueue` would list a FIFO
            // id a second time (the `VecDeque` does not dedupe), so the
            // transcoded body is what the walk will hand over, not a second
            // admission (design §4.6: "no re-drive").
            let queued = self.queue.contains(stream_id);
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.raw = Some(head);
                request.body = if transcoded.bytes.is_empty() {
                    Vec::new()
                } else {
                    vec![transcoded.bytes]
                };
                request.end_seen = true;
                request.body_ready = true;
                if !queued {
                    request.queued_since = None;
                }
            }
            if queued {
                return Ok(());
            }
            return self.admit_ready(stream_id);
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.end_seen = true;
        }
        let Some((view, held)) = self
            .requests
            .get(&stream_id)
            .map(|request| (request.view, !request.body_ready))
        else {
            self.dropped += 1;
            return Ok(());
        };
        // An `AllowEmptyOnly` request was held back at its head precisely so a
        // view could not answer it before the `413`: this empty `REQ_END` is the
        // proof it is empty, so it is here — and nowhere earlier — that it
        // becomes ready and joins admission. Its head-arrival stamp stops
        // counting now: `enqueue` starts the queue clock for a request that has
        // to wait for a slot.
        match view {
            Some((key, id)) => self.view_push(key, id, Frame::ReqEnd),
            None => {
                if !held {
                    return Ok(());
                }
                if let Some(request) = self.requests.get_mut(&stream_id) {
                    request.body_ready = true;
                    request.queued_since = None;
                }
                self.admit_ready(stream_id)
            }
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
        // The response ends here, so its span is final: charge before the event
        // that reports it (`MB.3.45`). The fair-queue debit lands here, while
        // the slot is still held to the view's `DONE`, not at the later
        // release: the span billed is the request's own and the debit belongs
        // to the charge (`MB.3.49`, [`Scheduler::charge`]). `charge` is
        // idempotent, so that `DONE` charges nothing more.
        self.charge(stream_id);
        self.emit_request(stream_id);
        if view.is_none() {
            // Never dispatched: it leaves the queue, and no slot was ever held.
            // A static read that is still parked keeps both the slot and the
            // descriptor until it returns (design §4.6).
            if self.static_read_parked(stream_id) {
                return Ok(());
            }
            self.release_static_slot(stream_id);
            self.release(stream_id);
        }
        Ok(())
    }

    /// http granted the stream more response-body credit (§3.1): the view gets
    /// to send that much more.
    ///
    /// A static answer has no view to tell: the credit is what the pump was
    /// waiting for, so it is woken directly.
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
            None => self.pump_static(stream_id),
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

    // ------------------------------------------------------------------ pool

    /// The per-process request ceiling in force for `route`: boot data, never a
    /// runtime question (§4.1).
    ///
    /// `None` on the route means `BLASTIT_VIEW_MAX_WORKERS`, and the result is
    /// clamped into `1..=that` so no route can be configured to admit nothing
    /// or more than one process may run.
    fn per_process_cap(&self, route: usize) -> u32 {
        let ceiling = self.cfg.view_worker_cap().max(1);
        self.routes
            .get(route)
            .and_then(|info| info.per_process_cap)
            .map_or(ceiling, |cap| cap.clamp(1, ceiling))
    }

    /// Whether `route` may own more than one process: boot data (`ADR-0015`
    /// decision 1).
    fn process_pool(&self, route: usize) -> bool {
        self.routes.get(route).is_some_and(|info| info.process_pool)
    }

    /// How many processes `route` may own.
    ///
    /// A pooled (request-per-process) route may own as many as the budget, which
    /// is what the accepted DX promises a route; a threaded route owns exactly
    /// one, because a second copy of the preloaded heap would buy no concurrency
    /// its threads do not already give. Never inferred from a value: with
    /// `BLASTIT_VIEW_MAX_WORKERS=1` a threaded route still owns one process.
    fn process_limit(&self, route: usize) -> u32 {
        if self.process_pool(route) {
            self.cfg.max_workers.max(1)
        } else {
            1
        }
    }

    /// The processes `route`'s requests need at its per-process ceiling (§4.3).
    fn needed_processes(&self, route: usize) -> u32 {
        self.route_requests(route)
            .div_ceil(self.per_process_cap(route))
    }

    /// Requests `route` has now: in flight plus waiting in the admission queue.
    fn route_requests(&self, route: usize) -> u32 {
        let inflight = self.pools.get(&route).map_or(0u32, |pool| {
            pool.views
                .values()
                .map(|view| view.inflight)
                .fold(0u32, u32::saturating_add)
        });
        inflight.saturating_add(u32::try_from(self.queued_for(route)).unwrap_or(u32::MAX))
    }

    /// How many requests are waiting in the admission queue for `route`.
    fn queued_for(&self, route: usize) -> usize {
        self.queued_for_streams(route).count()
    }

    /// The admission-queue stream ids routed to `route`.
    ///
    /// [`Admission::routes`] is empty for the FIFO queue, so this reads the
    /// routes off the requests — the same walk [`Scheduler::route_waiting`]
    /// uses, which is why it works for both queue modes.
    fn queued_for_streams(&self, route: usize) -> impl Iterator<Item = u32> + '_ {
        self.queue.ids().into_iter().filter(move |stream_id| {
            self.requests
                .get(stream_id)
                .and_then(|request| request.route)
                == Some(route)
        })
    }

    /// Every route with at least one request in the admission queue, in index
    /// order and without repeats.
    fn queued_routes(&self) -> Vec<usize> {
        let mut routes: Vec<usize> = self
            .queue
            .ids()
            .into_iter()
            .filter_map(|stream_id| {
                self.requests
                    .get(&stream_id)
                    .and_then(|request| request.route)
            })
            .collect();
        routes.sort_unstable();
        routes.dedup();
        routes
    }

    /// Whether some other route has requests and no process at all (§4.4).
    ///
    /// Deliberately narrow: a route that already owns a worker — busy or not —
    /// can serve from it, so it never holds up another route's growth, or two
    /// busy routes would freeze each other. A route already being forked or one
    /// in backoff is excluded too: nothing this route refuses to do would help
    /// it. `other < route` is the tie-break that keeps two cold routes from
    /// deferring to each other for ever: within one pump the lowest-indexed cold
    /// route forks first, which ends every other route's deferral next time.
    fn other_route_starved(&self, route: usize) -> bool {
        self.queued_routes().into_iter().any(|other| {
            other < route
                && self.pools.get(&other).is_none_or(ViewSet::is_empty)
                && !self.backoff.contains_key(&other)
        })
    }

    /// The live view processes: every worker init reported, plus the fork each
    /// route has already been promised (§4.4).
    ///
    /// The derived cap counts **processes**, not routes: the number exists to
    /// bound heap copies, and a copy is per process.
    fn live_processes(&self) -> u32 {
        self.pools
            .values()
            .map(ViewSet::len)
            .fold(0u32, u32::saturating_add)
    }

    /// The live worker named by `key`, if the pool still holds it.
    fn view_mut(&mut self, key: ViewKey) -> Option<&mut View> {
        self.pools
            .get_mut(&key.route)?
            .views
            .get_mut(&key.pid)
            .filter(|view| view.key == key)
    }

    /// Drops a route's pool once it holds neither a worker nor a promise.
    ///
    /// That is the invariant [`ViewSet`] states, and the reason a route that
    /// served nothing keeps no state.
    fn forget_if_idle(&mut self, route: usize) {
        if self.pools.get(&route).is_some_and(ViewSet::is_empty) {
            self.pools.remove(&route);
        }
    }

    /// Emits one `manifest.warning` for the scheduler's own bookkeeping.
    ///
    /// The sentence names a route index or a fixed reason and never a request,
    /// a header or a body value.
    fn emit_warning(&mut self, message: String) {
        (self.emit)(Event::now(
            Component::Scheduler,
            EventKind::ManifestWarning(ManifestWarning::new(message)),
        ));
    }

    // -------------------------------------------------------------- dispatch

    /// Whether a route can take a request right now.
    fn can_dispatch(&self, route: usize) -> bool {
        self.slots < self.cfg.max_workers && self.has_dispatchable(route)
    }

    /// Whether some worker of `route` admits a request now (§4.2).
    fn has_dispatchable(&self, route: usize) -> bool {
        let cap = self.per_process_cap(route);
        self.pools
            .get(&route)
            .is_some_and(|pool| pool.views.values().any(|view| view.admits(cap)))
    }

    /// Dispatches every queued request it can, spawning views as needed.
    ///
    /// With fair queuing off (the default) this is §2.4's FIFO walk: the
    /// requests are considered in arrival order, and one that cannot be served
    /// — its route's view is not ready — is skipped rather than allowed to hold
    /// up the ones behind it.
    ///
    /// A queued request a mount matched is taken through
    /// [`Scheduler::dispatch_static`] once a slot is free, never through the
    /// view walk: that is what makes "more than `max_workers` static streams
    /// complete in waves" true (§9.2).
    fn dispatch_queued(&mut self) -> Result<(), SchedulerError> {
        if self.queue.is_fair() {
            return self.dispatch_queued_fair();
        }
        let queued: Vec<u32> = self.queue.ids();
        for stream_id in queued {
            if !self.queue.contains(stream_id) {
                continue;
            }
            if self.slots >= self.cfg.max_workers {
                return Ok(());
            }
            if self.static_pending(stream_id) {
                if !self.can_dispatch_static() {
                    continue;
                }
                self.dispatch_static(stream_id)?;
                // A miss that a view will answer hands the slot back and puts
                // the request in the queue again as an ordinary one: it leaves
                // for its view in this same walk, or nothing would fork one for
                // it.
                if !self.queue.contains(stream_id) {
                    continue;
                }
            }
            let Some(route) = self
                .requests
                .get(&stream_id)
                .and_then(|request| request.route)
            else {
                continue;
            };
            if self.can_dispatch(route) {
                self.dispatch(stream_id)?;
                continue;
            }
            if !self.has_dispatchable(route) && !self.draining {
                // The route has requests no worker of it can take: grow it by
                // what they need (§4.3). A route that is merely busy keeps its
                // workers and this does nothing.
                self.grow(route, self.needed_processes(route))?;
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
            // A static request never competes for a view's budget, which is the
            // only thing the fair order decides: its eligibility is the slot
            // (`can_dispatch_static`), so it is admitted here and only the view
            // path below is ordered (design §4.6). A `Forward` clears its
            // `static_pending` first, so a handback is ordered like any other
            // request for that route.
            if let Some(stream_id) = self.first_static_queued() {
                self.queue.remove(stream_id);
                self.dispatch_static(stream_id)?;
                continue;
            }
            let mut best: Option<(NextFlow, u32)> = None;
            for route in self.queue.routes() {
                if !self.can_dispatch(route as usize) {
                    if !self.has_dispatchable(route as usize) && !self.draining {
                        self.grow(route as usize, self.needed_processes(route as usize))?;
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
        let (raw, body, end, extra_credit) = {
            let Some(request) = self.requests.get_mut(&stream_id) else {
                return Ok(());
            };
            request.queued_since = None;
            request.dispatched_at = Some(now);
            request.deadline = Some(now + self.cfg.request_timeout);
            request.view = Some((key, id));
            request.slots = true;
            (
                request.raw.take(),
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

        // A routed request always carries its raw head until here, so this is
        // the whole frame the view gets, id rewritten and payload untouched.
        debug_assert!(raw.is_some(), "a dispatched request carries its raw head");
        if let Some(mut raw) = raw {
            raw.set_id(id);
            self.view_push_raw(key, raw)?;
        }
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

    /// Takes a worker slot and a request id for `stream_id`, from the LRU worker
    /// of its route that admits one (§4.2).
    ///
    /// The least-recently-used admitting worker wins, ties by ascending pid: a
    /// request never queues behind a busy sibling while an idle one exists, and
    /// `last_used` stays the scheduler's one notion of recency, shared with
    /// eviction.
    fn reserve(&mut self, stream_id: u32) -> Option<(ViewKey, u32, Outbound)> {
        let route = self.requests.get(&stream_id)?.route?;
        let cap = self.per_process_cap(route);
        let last_used = self.lru;
        let pool = self.pools.get_mut(&route)?;
        let pid = pool
            .views
            .values()
            .filter(|view| view.admits(cap))
            .min_by_key(|view| (view.last_used, view.key.pid))
            .map(|view| view.key.pid)?;
        let view = pool.views.get_mut(&pid)?;
        let out = view.out.clone()?;
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

    /// The queue towards one worker, if that worker is still the live one of its
    /// route.
    ///
    /// The `key` filter is what keeps a frame from reaching whichever sibling
    /// shares the route: `(route, pid)` is the only identity a `view.chan` frame
    /// has.
    fn view_out(&self, key: ViewKey) -> Option<Outbound> {
        self.pools
            .get(&key.route)
            .and_then(|pool| pool.views.get(&key.pid))
            .filter(|view| view.key == key)
            .and_then(|view| view.out.clone())
    }

    // ---------------------------------------------------------------- static

    /// Admits one request to a mount (§9.2 steps 1–4).
    ///
    /// This is the one function that takes a static admission slot: it sets
    /// `slots`, `view = None`, `dispatched_at` and `deadline` exactly as
    /// [`Scheduler::dispatch`] does — or `next_deadline` and `expire_requests`
    /// would never fire for it — and then resolves the mount's root to produce
    /// the one verdict §9.2 settles:
    ///
    /// 1. a file answers, through the method/body gate (`405`/`413`);
    /// 2. `Missing` falls through to the view a route matched, if any;
    /// 3. with no view left to try, the mount's declared `fallback` answers a
    ///    `GET`/`HEAD` that declares no body;
    /// 4. `Hidden`/`Refused`/a missing `fallback` is final, and a miss with no
    ///    view and no fallback is a `404` (or the `--learn` fallback).
    #[cfg(feature = "static")]
    fn dispatch_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get_mut(&stream_id) else {
            return Ok(());
        };
        let Some(mount) = request.static_mount else {
            return Ok(());
        };
        let Some(parts) = request.static_parts.take() else {
            return Ok(());
        };
        let method = request.head.method.clone();
        let has_body = request.static_has_body;
        let if_none_match = request.static_if_none_match.take();
        debug_assert!(
            !request.slots,
            "a static request takes exactly one admission slot"
        );
        let now = Instant::now();
        request.queued_since = None;
        request.dispatched_at = Some(now);
        request.deadline = Some(now + self.cfg.request_timeout);
        request.view = None;
        request.static_pending = false;
        request.slots = true;
        self.queue.remove(stream_id);
        self.slots += 1;

        let Some(root) = self.mount_roots.get(mount).cloned() else {
            // A mount with no opened root is a boot-order bug; fail closed
            // rather than answer a file this role never proved it can read.
            return self.refuse_static(stream_id, 404, None);
        };
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        // §9.2 step 1 only: `resolve_file` never answers the declared
        // `fallback`, because that is step 3's and would shadow every view.
        let outcome = root.resolve_file(&borrowed);
        match outcome {
            // Step 2: nothing at the path, so the view table is consulted
            // first. A routed request forwards; one no view matched has to
            // answer from the mount itself.
            StaticOutcome::Missing => self.mount_missed(stream_id, mount, &method, has_body),
            // Step 1 and the final refusals: the file/refusal gate.
            other => match decide(other, &method, has_body, false) {
                Rewrite::Serve(file) => self.serve_static(
                    stream_id,
                    root.as_ref(),
                    file,
                    &method,
                    if_none_match.as_deref(),
                ),
                Rewrite::Reject { status, allow } => self.refuse_static(stream_id, status, allow),
                // `Missing` is handled above, so a `Forward` here would mean
                // `decide` served a shape it was not given.
                Rewrite::Forward => {
                    debug_assert!(false, "a non-miss static outcome must not forward");
                    self.refuse_static(stream_id, 404, None)
                }
            },
        }
    }

    /// §9.2 steps 2–4 for a mount that answered no file.
    ///
    /// A request whose path a view also matched forwards to it — the view queue
    /// owns the answer from here, exactly as before. A request no view matched
    /// asks the mount's declared `fallback`, on `GET`/`HEAD` only, and is a
    /// `404` otherwise (or the `--learn` fallback, which is the app).
    #[cfg(feature = "static")]
    fn mount_missed(
        &mut self,
        stream_id: u32,
        mount: usize,
        method: &[u8],
        has_body: bool,
    ) -> Result<(), SchedulerError> {
        if self
            .requests
            .get(&stream_id)
            .is_some_and(|request| request.route.is_some())
        {
            return self.forward_static(stream_id);
        }
        // Step 4's `405` is a view rule (§9.2); a method other than `GET`/`HEAD`
        // never reaches the fallback and is a `404` here.
        if method != b"GET" && method != b"HEAD" {
            return self.mount_learn_or_404(stream_id);
        }
        // A `GET`/`HEAD` that declares a body is a `413`, the same rule a file
        // answer uses.
        if has_body {
            return self.refuse_static(stream_id, 413, None);
        }
        let has_fallback = self
            .cfg
            .mounts
            .get(mount)
            .is_some_and(|mount| mount.policy.fallback.is_some());
        let Some(root) = self.mount_roots.get(mount).cloned() else {
            return self.refuse_static(stream_id, 404, None);
        };
        if !has_fallback {
            return self.mount_learn_or_404(stream_id);
        }
        match root.resolve_fallback() {
            StaticOutcome::File(file) => {
                self.serve_static(stream_id, root.as_ref(), file, method, None)
            }
            // A fallback that is missing, a dotfile, a symlink or a PHP source
            // is `404` final.
            _ => self.refuse_static(stream_id, 404, None),
        }
    }

    /// §9.2 step 4's final miss: the `--learn` fallback in learn mode, else a
    /// `404`.
    ///
    /// The static slot is released first: a learn request is served by the
    /// app's ordinary view path, which takes its own slot.
    #[cfg(feature = "static")]
    fn mount_learn_or_404(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        if !self.cfg.learn_mode {
            return self.refuse_static(stream_id, 404, None);
        }
        self.release_static_slot(stream_id);
        self.queue.remove(stream_id);
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.static_pending = false;
            request.dispatched_at = None;
            request.deadline = None;
            request.queued_since = Some(Instant::now());
        }
        self.learn_fallback_queue.push_back(stream_id);
        self.pump_learn_fallback()
    }

    /// Pushes a static head and starts its body pump (design §4.6).
    ///
    /// Only the status and the header **names** are ever recorded anywhere:
    /// `StaticResponse` has no `Debug` on purpose (`MB.2.4.2`), and a header
    /// value may be content-derived, so the response is never formatted.
    #[cfg(feature = "static")]
    fn serve_static(
        &mut self,
        stream_id: u32,
        root: &StaticRoot,
        file: StaticFile,
        method: &[u8],
        if_none_match: Option<&[u8]>,
    ) -> Result<(), SchedulerError> {
        let StaticResponse {
            status,
            reason,
            headers,
            body,
        } = root.respond(file, method, if_none_match);
        let has_body = body.is_some();
        let head = RespHead {
            status,
            reason: reason.as_bytes().to_vec(),
            headers,
        };
        self.http_push(stream_id, Frame::RespHead(head))?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.head_sent = true;
            request.status = status;
            request.static_body = body;
            // Nothing of the app's plan survives an answer: a served path never
            // buffers, transcodes or forwards, whatever the head declared.
            request.body_plan = BodyPlan::AllowEmptyOnly;
            request.body_plan_error = None;
        }
        if has_body {
            self.pump_static(stream_id)
        } else {
            // `HEAD` and `304`: the head is the whole answer, and no read is
            // ever spawned for it.
            self.finish_static(stream_id)
        }
    }

    /// Hands the slot back and re-enters the ordinary path (design §4.6).
    ///
    /// A `Forward` is the only verdict that reaches the app, and it must release
    /// its static slot first: the view path takes its own, from its own queue
    /// dispatch. That is the only way a front controller's *first* request can
    /// spawn its view — a slot-holder that never entered the queue could not.
    ///
    /// A miss does **not** hand the head to a view just because the mount could
    /// not answer it: `R12` is the route's rule, and it is decided here exactly
    /// as in [`Scheduler::begin_raw`]. Only a `raw` route (`StreamPassthrough`)
    /// is admitted at its head; a `Buffer` body is collected and transcoded, and
    /// an `AllowEmptyOnly` one is proved empty by its `REQ_END` and nothing
    /// else. Without that, a front controller's view would get `REQ_HEAD` for a
    /// request whose first body byte is a `413` — the very race this file's
    /// head-time refusal exists to close, on the one path a mount fronts.
    #[cfg(feature = "static")]
    fn forward_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        self.release_static_slot(stream_id);
        let (error, waiting, buffered) = match self.requests.get_mut(&stream_id) {
            Some(request) => {
                request.static_pending = false;
                // The view queue measures its own timeout from its own
                // dispatch, so this request carries neither of these any more.
                request.dispatched_at = None;
                request.deadline = None;
                (
                    request.body_plan_error.take(),
                    // Only a body that has not ended is waited for: a `REQ_END`
                    // that landed while the request was queued has already
                    // proved the body complete (and, for `Buffer`, transcoded
                    // it), so there is nothing left to wait for and the request
                    // must fall through to `admit_ready` — otherwise nothing
                    // would ever re-drive it and the queue timeout would answer
                    // a request that was ready.
                    !request.end_seen && !matches!(request.body_plan, BodyPlan::StreamPassthrough),
                    matches!(request.body_plan, BodyPlan::Buffer { .. }),
                )
            }
            None => return Ok(()),
        };
        self.queue.remove(stream_id);
        if let Some(status) = error {
            // The stored codec error is the app path's own answer, exactly as
            // before this route gained a policy (design §4.4).
            return self.refuse_body(stream_id, status);
        }
        if waiting {
            // `expire_queue` bounds a client that never ends the body, and the
            // static slot was handed back above. The count of waiting bodies a
            // route may hold is the app path's own bound, so a static front
            // controller cannot admit more of them than any other route: that
            // path ([`Scheduler::begin_raw`]) marks the request
            // `body_ready = false` *before* it asks the count, so the request
            // being admitted counts itself and `queue_size = N` means `N - 1`
            // others. Asking first and clearing after would admit one more here
            // than on any other route.
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.body_ready = false;
            }
            let at_capacity = if buffered {
                // A structured body is collected and transcoded before the view
                // sees a byte.
                self.buffering_count() >= self.cfg.queue_size
            } else {
                // An empty-only one holds no bytes, but it holds its stream.
                self.awaiting_end_count() >= self.cfg.queue_size
            };
            if at_capacity {
                return self.reject(
                    stream_id,
                    503,
                    "Service Unavailable",
                    "queue full\n",
                    Outcome::QueueFull,
                    Some(retry_after_seconds(self.cfg.queue_timeout)),
                );
            }
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.queued_since = Some(Instant::now());
            }
            return Ok(());
        }
        self.admit_ready(stream_id)
    }

    /// Pumps one static body on the existing credit/window path (design §4.6).
    ///
    /// Steps 1–3 of the design, in order: a body with nothing left ends the
    /// response **before** any read; a read in flight is waited for; otherwise,
    /// with credit available, the [`StaticBody`] moves into one blocking
    /// `pread`, whose result comes back over `static_in`. Nothing here loops or
    /// spawns a thread per request: one read is ever outstanding per slot, and
    /// `slots <= max_workers` is therefore the reader bound as well.
    #[cfg(feature = "static")]
    fn pump_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        let Some(body) = request.static_body.as_ref() else {
            return Ok(());
        };
        let remaining = body.len.saturating_sub(body.offset);
        if remaining == 0 {
            // Step 1: a zero-length file is an ordinary `200` with
            // `Content-Length: 0` and no body frame; the slot goes back here,
            // before any read.
            return self.finish_static(stream_id);
        }
        if request.pumping || request.download_credit == 0 {
            // Step 3: wait for the read result or for `WINDOW`.
            return Ok(());
        }
        // Step 2: one `pread` of at most the remaining bytes, the credit http
        // granted, one frame and one chunk — never more than any of them.
        let want = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(usize::try_from(request.download_credit).unwrap_or(usize::MAX))
            .min(self.cfg.frame_limits.max_payload())
            .min(STATIC_CHUNK_BYTES);
        if want == 0 {
            // A read of nothing can never advance the offset: it would push an
            // empty frame and ask again, leaving the response to be answered by
            // `request_timeout` instead of the read that had already failed.
            // Fail closed, on the same rule as a short read (design §4.6).
            return self.abort_static(stream_id);
        }
        let body = match self.requests.get_mut(&stream_id) {
            Some(request) if request.static_body.is_some() => {
                request.pumping = true;
                request.static_body.take()
            }
            _ => None,
        };
        let Some(body) = body else {
            return Ok(());
        };
        let sender = self.static_in.clone();
        tokio::task::spawn_blocking(move || {
            // A `pread` on a stalled mount cannot be cancelled, which is why the
            // body travels with the read: the scheduler's slot stays charged
            // until this result arrives.
            let file = std::fs::File::from(body.file);
            let mut buf = vec![0u8; want];
            let result = file.read_at(&mut buf, body.offset);
            let file: OwnedFd = file.into();
            let body = StaticBody {
                file,
                offset: body.offset,
                len: body.len,
            };
            // The scheduler may be gone (a shutdown): the frame is dropped with
            // the body then, which closes the descriptor.
            let _ = sender.blocking_send(StaticRead {
                stream_id,
                body,
                want,
                result: result.map(|read| {
                    buf.truncate(read);
                    buf
                }),
            });
        });
        Ok(())
    }

    /// One finished static read: push its bytes, or abort the response.
    ///
    /// The short-read rule is design §4.6's: a body is never truncated under a
    /// `Content-Length` the head already promised, so an error or fewer bytes
    /// than were asked for is `RespAbort` and never a silently short `200`.
    #[cfg(feature = "static")]
    fn on_static_read(&mut self, read: StaticRead) -> Result<(), SchedulerError> {
        let stream_id = read.stream_id;
        let forward = match self.requests.get_mut(&stream_id) {
            Some(request) => {
                // The read result is what clears `pumping`: only now may the
                // slot be handed back (design §4.6).
                request.pumping = false;
                !request.complete
            }
            None => {
                // A request that is already gone: nothing may answer it, and
                // its body is dropped with the descriptor.
                self.dropped += 1;
                return Ok(());
            }
        };
        let mut body = read.body;
        let bytes = match read.result {
            Ok(bytes) if forward && bytes.len() == read.want => bytes,
            _ => {
                drop(body);
                if forward {
                    return self.abort_static(stream_id);
                }
                return self.release_finished_static(stream_id);
            }
        };
        let count = bytes.len() as u64;
        body.offset = body.offset.saturating_add(count);
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.static_body = Some(body);
            request.download_credit = request
                .download_credit
                .saturating_sub(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
            request.bytes_out = request.bytes_out.saturating_add(count);
        }
        self.http_push(stream_id, Frame::RespBody(bytes))?;
        self.pump_static(stream_id)
    }

    /// Ends a static response whose bytes are all out: `RESP_END`, the slot and
    /// the request.
    #[cfg(feature = "static")]
    fn finish_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        self.http_push(stream_id, Frame::RespEnd)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.static_body = None;
            if !request.cancelled {
                request.outcome = Outcome::Ok;
            }
        }
        self.mark_finished(stream_id);
        self.release_finished_static(stream_id)
    }

    /// Aborts a static response whose bytes cannot be delivered in full.
    ///
    /// The head is always out before a read is spawned, so this is always a
    /// `RESP_ABORT` and never a second status. §8.1's outcome list is closed and
    /// has no "the responder failed" member, so this reports `view_died`: the
    /// response died mid-stream, exactly as a view's thread would.
    #[cfg(feature = "static")]
    fn abort_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        self.http_push(
            stream_id,
            Frame::RespAbort {
                reason: STATIC_READ_FAILED.to_string(),
            },
        )?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.static_body = None;
            request.outcome = Outcome::ViewDied;
        }
        self.mark_finished(stream_id);
        self.release_finished_static(stream_id)
    }

    /// Charges, logs and releases one static request that is over.
    ///
    /// The slot goes back exactly once — [`Scheduler::release_static_slot`] is
    /// the only place that decrements it, and it is a no-op on a request that
    /// already let go — and the request is only dropped when it did.
    #[cfg(feature = "static")]
    fn release_finished_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        // The last byte — or the abort — is where the cost is fixed, exactly as
        // a streamed view response does at its `RESP_END`.
        self.charge(stream_id);
        self.emit_request(stream_id);
        if self.release_static_slot(stream_id) {
            self.release(stream_id);
        }
        Ok(())
    }

    /// Hands a static admission slot back, once.
    ///
    /// The only place `slots` is decremented for a static answer. `false` when
    /// the request holds no slot, or when a read is still parked on it: the
    /// read result owns the transition then, so a timeout or a cancellation can
    /// never free a descriptor that is still in use (design §4.6).
    fn release_static_slot(&mut self, stream_id: u32) -> bool {
        let Some(request) = self.requests.get_mut(&stream_id) else {
            return false;
        };
        if !slot_releasable(request.slots, request.pumping) {
            return false;
        }
        request.slots = false;
        self.slots = self.slots.saturating_sub(1);
        true
    }

    /// The three calls the view path makes into the responder, in a build
    /// without `static` (`MB.36.2`).
    ///
    /// [`run`] refuses a manifest that declares a mount in that build
    /// ([`SchedulerError::StaticNotCompiled`]), so no request ever matches a
    /// mount, sets `static_pending`, or holds a static body: each of these is
    /// unreachable, and "nothing to do" is what the compiler is told.
    #[cfg(not(feature = "static"))]
    fn dispatch_static(&mut self, _stream_id: u32) -> Result<(), SchedulerError> {
        Ok(())
    }

    /// See the `static` build's `pump_static`.
    #[cfg(not(feature = "static"))]
    fn pump_static(&mut self, _stream_id: u32) -> Result<(), SchedulerError> {
        Ok(())
    }

    /// See the `static` build's `on_static_read`: nothing ever sends on that
    /// channel, so this is never called.
    #[cfg(not(feature = "static"))]
    fn on_static_read(&mut self, _read: StaticReadMsg) -> Result<(), SchedulerError> {
        Ok(())
    }

    /// Answers a request the static responder refused (design §4.4).
    ///
    /// The one helper for all four refusals: `canned` supplies the reason and
    /// body, `Allow` is added only for the `405` the caller chose (with its one
    /// value, `GET, HEAD`), and `Cache-Control: no-store` is added to every one
    /// of them.
    fn refuse_static(
        &mut self,
        stream_id: u32,
        status: u16,
        allow: Option<&[u8]>,
    ) -> Result<(), SchedulerError> {
        // The slot goes back before the status is written, so the answer never
        // holds admission capacity while it is being framed.
        self.release_static_slot(stream_id);
        let (reason, body) = canned(status);
        let mut extra: Vec<(&[u8], &[u8])> = Vec::with_capacity(2);
        if let Some(allow) = allow {
            extra.push((&b"Allow"[..], allow));
        }
        extra.push((&b"Cache-Control"[..], &b"no-store"[..]));
        self.respond_with(stream_id, status, reason, body, None, &extra)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = Outcome::Unmatched;
        }
        self.mark_finished(stream_id);
        self.charge(stream_id);
        self.emit_request(stream_id);
        self.release(stream_id);
        Ok(())
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
        // A structured body that never completes, and an unrouted `--learn`
        // request that never gets the single fallback slot, are bounded by the
        // same queue timeout.
        let mut waiting: Vec<u32> = self
            .requests
            .values()
            .filter(|request| {
                !request.body_ready
                    && request
                        .queued_since
                        .is_some_and(|since| now.duration_since(since) >= timeout)
                    && request.view.is_none()
            })
            .map(|request| request.stream_id)
            .collect();
        waiting.extend(
            self.learn_fallback_queue
                .iter()
                .copied()
                .filter(|stream_id| {
                    self.requests
                        .get(stream_id)
                        .and_then(|request| request.queued_since)
                        .is_some_and(|since| now.duration_since(since) >= timeout)
                }),
        );
        for stream_id in waiting {
            self.learn_fallback_queue
                .retain(|queued| *queued != stream_id);
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
            // The timeout answer is final now, so its charge must precede its
            // event (`MB.3.45`): `emit_request` reports `charged_ns`, and the
            // request never pays the queue wait it spent before dispatch.
            self.charge(stream_id);
            self.emit_request(stream_id);
            if let Some((key, id)) = view {
                self.view_push(key, id, Frame::Cancel)?;
            } else {
                // A view-less request has no `DONE` coming. Its static slot is
                // handed back here, unless a read is parked on it: that read
                // cannot be cancelled, so its result releases the slot and the
                // descriptor (design §4.6).
                if self.static_read_parked(stream_id) {
                    continue;
                }
                self.release_static_slot(stream_id);
                self.release(stream_id);
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

    /// Retires what is finished: a worker over `BLASTIT_VIEW_MAX_REQUESTS` with
    /// nothing in flight, one a reload marked stale, or one idle for
    /// `BLASTIT_VIEW_IDLE_TIMEOUT` (§2.4).
    ///
    /// Every rule is per process now, so a burst's extras are the ones that go
    /// and a route keeps whatever its load needs.
    fn retire_finished_views(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let mut victims: Vec<(ViewKey, RetireReason)> = Vec::new();
        for view in self.pools.values().flat_map(|pool| pool.views.values()) {
            if !view.ready || view.inflight > 0 {
                continue;
            }
            if view.stopping {
                victims.push((view.key, RetireReason::MaxRequests));
                continue;
            }
            if let Some(reason) = view.stale.clone() {
                // A reload arrived while this worker was busy: its last request
                // is `DONE`, so it can go now (§4.6).
                victims.push((view.key, reason));
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
    /// A view with a request in flight is never a candidate; every worker of
    /// every route is, so a burst gives one process back at a time.
    fn oldest_idle_view(&self) -> Option<ViewKey> {
        self.pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| view.ready && view.inflight == 0 && view.leaving_at().is_none())
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
    ///
    /// Kept as the denominator of the measurement on purpose: a worker forked
    /// but not yet `READY` is not counted, so the rise it caused is attributed
    /// to the next sample and the per-view cost is *over*-estimated — the cap
    /// lands lower than it could, never higher.
    fn ready_view_count(&self) -> u32 {
        u32::try_from(
            self.pools
                .values()
                .flat_map(|pool| pool.views.values())
                .filter(|view| view.ready)
                .count(),
        )
        .unwrap_or(u32::MAX)
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

    /// Forks one more worker for `route` until it has `wanted` processes (§4.3).
    ///
    /// Callers: a queued request's route ([`Scheduler::dispatch_queued`],
    /// `wanted = needed_processes`), or [`Scheduler::start`] for a
    /// preforked/eager route (`wanted = 1`). One call sends at most one `SPAWN`,
    /// and every bound below is independent of `slots`: the budget bounds
    /// requests in flight, and a worker with nothing to do is what `wanted`
    /// prevents.
    ///
    /// * one `SPAWN` in flight per route, because init forks sequentially and
    ///   buys nothing from a queue of them;
    /// * until the first real memory measurement, growth waits for
    ///   `pending_ready == 0`, so the derived cap is re-derived between two
    ///   forks instead of being priced from the estimate it is known to
    ///   understate (unless the operator pinned the cap);
    /// * never more than the requests need, nor more than the route may own,
    ///   nor more than the backoff, the starving neighbour, the eviction rule
    ///   and the live-process cap allow.
    fn grow(&mut self, route: usize, wanted: u32) -> Result<(), SchedulerError> {
        if self.draining || !self.spawnable(route) {
            // A route the responder answers on its own never gets a view
            // (design §4.6): asking init for one would fork a process no
            // request could ever be dispatched to.
            return Ok(());
        }
        let pool = self.pools.get(&route);
        if pool.is_some_and(|pool| pool.spawning.is_some()) {
            return Ok(());
        }
        if self.measured_view_mem.is_none()
            && self.cfg.max_view_processes.is_none()
            && pool.is_some_and(|pool| pool.pending_ready > 0)
        {
            // Wait for the sample. With `BLASTIT_MAX_VIEW_PROCESSES` set there
            // is nothing to measure, so the gate would only serialize forks
            // for ever.
            return Ok(());
        }
        let owned = pool.map_or(0, ViewSet::len);
        if owned >= wanted {
            return Ok(());
        }
        if owned >= self.process_limit(route) {
            return Ok(());
        }
        if let Some(until) = self.backoff.get(&route).copied()
            && Instant::now() < until
        {
            // R9: a failed fork cools this route's *growth*, never its siblings.
            return Ok(());
        }
        if self.other_route_starved(route) {
            return Ok(());
        }
        if !self.make_room(route)? {
            return Ok(());
        }
        // `make_room` retires at most one process, and the cap it retired for
        // may still be exceeded — a measured cap can land below the number of
        // processes that are already live. Then the spawn waits for the next
        // round of shedding rather than adding to the overshoot.
        if let Some(cap) = self.view_cap()
            && self.live_processes() >= cap
        {
            return Ok(());
        }
        let epoch = self.reload_epoch;
        let route_id = u32::try_from(route).unwrap_or(u32::MAX);
        // The slot is the deadline `expire_spawns` bounds: a `SPAWN` init never
        // answers must not hold the route's growth for ever.
        self.pools.entry(route).or_default().spawning =
            Some((epoch, Instant::now() + self.cfg.request_timeout));
        if let Err(error) = self.ctl_push(Ctl::Spawn { route: route_id }) {
            if let Some(pool) = self.pools.get_mut(&route) {
                pool.spawning = None;
            }
            return Err(error);
        }
        Ok(())
    }

    /// A `SPAWN` that will not be answered: free the slot, cool the route's
    /// growth, leave every sibling and every queued request alone (§4.7).
    ///
    /// Reached from `EXITED { pid: 0 }` (init could not fork) and from
    /// [`Scheduler::expire_spawns`] (no answer at all). A message for a route
    /// with nothing outstanding is stale and does nothing.
    fn spawn_failed(&mut self, route: usize) {
        let Some(pool) = self.pools.get_mut(&route) else {
            return;
        };
        if pool.spawning.take().is_none() {
            return;
        }
        self.backoff
            .insert(route, Instant::now() + self.cfg.spawn_backoff);
        // One `manifest.warning`, naming the route and nothing from any request.
        self.emit_warning(format!(
            "route {route}: the view process could not be started"
        ));
        self.forget_if_idle(route);
    }

    /// Frees every spawn slot whose `SPAWN` was never answered (§4.7).
    ///
    /// The slot is booked with `request_timeout` when the `SPAWN` is sent, so a
    /// message that never comes costs the route one backoff instead of holding
    /// its growth until the process exits.
    fn expire_spawns(&mut self) {
        let now = Instant::now();
        let expired: Vec<usize> = self
            .pools
            .iter()
            .filter(|(_, pool)| pool.spawning.is_some_and(|(_, deadline)| now >= deadline))
            .map(|(route, _)| *route)
            .collect();
        for route in expired {
            self.spawn_failed(route);
        }
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
            .is_some_and(|cap| self.live_processes() >= cap);
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
    /// least recently used of the candidates wins, across every route but the
    /// asking one: a burst's coldest process goes before a route's only worker
    /// is ever considered.
    fn evictable_view(&self, route: usize) -> Option<ViewKey> {
        self.pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| {
                view.key.route != route
                    && view.ready
                    && view.inflight == 0
                    // A worker on its way out frees itself; killing it would
                    // only report an eviction for a process that had already
                    // answered its last request (`MB.26.8`).
                    && view.leaving_at().is_none()
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
    ///
    /// A key the pool no longer holds is a no-op: a retired worker's channel
    /// closes too, and that is not a second retirement. `pending_ready` is
    /// released here for a worker that never became `READY`, so a pool that
    /// retires a fork before its `READY` does not keep the route's growth
    /// waiting for a heap copy that no longer exists (§4.6).
    fn retire(&mut self, key: ViewKey, reason: RetireReason) -> Result<(), SchedulerError> {
        let held = self
            .pools
            .get(&key.route)
            .and_then(|pool| pool.views.get(&key.pid))
            .is_some_and(|view| view.key == key);
        if !held {
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
        let removed = self
            .pools
            .get_mut(&key.route)
            .and_then(|pool| pool.views.remove(&key.pid));
        let Some(view) = removed else {
            return Ok(());
        };
        if !view.ready
            && let Some(pool) = self.pools.get_mut(&key.route)
        {
            pool.pending_ready = pool.pending_ready.saturating_sub(1);
        }
        // Dropping the queue ends the writer task, which closes the
        // scheduler's end of this view's channel.
        drop(view.out);
        // §2.4/§8 Sizing: a route must not be forked again immediately after a
        // retirement that was not about its own work. A worker retired for
        // memory pressure would otherwise be respawned by the very next request
        // and copy the same heap back, which is the retire-then-respawn thrash
        // §8 Sizing warns about. Retiring a worker that has served, or one a
        // reload invalidated, is not rate-limited: the first is ordinary LRU
        // work, and the second is what grows a fresh worker for the requests
        // already waiting (§4.7).
        if matches!(reason, RetireReason::MemoryPressure) {
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
        self.forget_if_idle(key.route);
        Ok(())
    }

    /// Kills a pid the pool does not hold.
    ///
    /// [`Scheduler::retire`] cannot: it is a no-op for a key with no live
    /// `View`, and this process must be killed or it keeps its route's uid and
    /// socket directory. No `process.retired` event is emitted: the decision was
    /// not about a worker the scheduler owns, and the `Exited` that follows is
    /// the event.
    fn retire_unheld(&mut self, key: ViewKey, reason: RetireReason) -> Result<(), SchedulerError> {
        let live = self
            .pools
            .get(&key.route)
            .and_then(|pool| pool.views.get(&key.pid))
            .is_some_and(|view| view.key == key);
        if live {
            return self.retire(key, reason);
        }
        self.ctl_push(Ctl::Retire {
            route: u32::try_from(key.route).unwrap_or(u32::MAX),
            pid: key.pid,
            reason: reason.as_str().to_string(),
        })
    }

    /// `RELOAD`: retire every idle (or not-yet-ready) worker of every route,
    /// mark the busy ones stale, and make each route's in-flight `SPAWN` stale
    /// by epoch (§4.6).
    ///
    /// One transaction over every pool: a save must never leave one worker of a
    /// route serving the old code beside a fresh one. `admits` already refuses a
    /// stale worker, so a busy one takes no new request; it retires at `DONE`
    /// through [`Scheduler::retire_finished_views`]. Nothing here cools a route
    /// down, so the queued requests stay queued and the next pump's `grow` forks
    /// a fresh worker for them.
    fn reload(&mut self) -> Result<(), SchedulerError> {
        self.reload_epoch = self.reload_epoch.wrapping_add(1);
        let keys: Vec<(ViewKey, bool)> = self
            .pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| view.stale.is_none())
            .map(|view| (view.key, view.ready && view.inflight > 0))
            .collect();
        for (key, busy) in keys {
            if !busy {
                self.retire(key, RetireReason::Reload)?;
            } else if let Some(view) = self.view_mut(key) {
                view.stale = Some(RetireReason::Reload);
            }
        }
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
        let slot = self
            .pools
            .get_mut(&route)
            .and_then(|pool| pool.spawning.take());
        let Some((sent_epoch, _deadline)) = slot else {
            // Nothing waits for this process: close our end and have init reap
            // it, so an unknown view cannot linger holding a uid.
            drop(fd);
            self.dropped += 1;
            return self.retire_unheld(
                ViewKey { route, pid },
                RetireReason::Other("unexpected_spawn".to_string()),
            );
        };
        let key = ViewKey { route, pid };
        // A `bool` so the pool borrow ends before the retirement below.
        let duplicate = self
            .pools
            .get(&route)
            .is_some_and(|pool| pool.views.contains_key(&pid));
        if duplicate {
            // The kernel does not hand a live child's pid to a second child, so
            // the entry the pool holds was already gone and its `Exited` was
            // lost. Two processes must never share one `ViewKey`: retire the
            // pid — which kills the *newcomer* that now owns it, and fails the
            // dead entry's requests, drops its queue and emits its event
            // through the one path that does all four — then close our end of
            // the newcomer's channel.
            self.dropped += 1;
            drop(fd);
            return self.retire(key, RetireReason::Other("duplicate_pid".to_string()));
        }
        let std_stream = std::os::unix::net::UnixStream::from(fd);
        std_stream
            .set_nonblocking(true)
            .map_err(SchedulerError::io)?;
        let stream = UnixStream::from_std(std_stream).map_err(SchedulerError::io)?;
        let (read, write) = stream.into_split();
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
        let pool = self.pools.entry(route).or_default();
        pool.views.insert(
            pid,
            View {
                key,
                ready: false,
                stopping: false,
                inflight: 0,
                sent: 0,
                next_request_id: 0,
                idle_since: None,
                last_used: 0,
                out: Some(out),
                exited: None,
                retiring: None,
                stale: None,
                spawned_epoch: sent_epoch,
            },
        );
        pool.pending_ready = pool.pending_ready.saturating_add(1);
        if sent_epoch < self.reload_epoch {
            // A worker forked for a `SPAWN` sent before the last reload is stale
            // from birth: retire it now rather than let it serve old code
            // (§4.6).
            self.retire(key, RetireReason::Reload)?;
        }
        Ok(())
    }

    /// A view is ready to serve (§4).
    ///
    /// A worker whose `SPAWN` predates the last reload holds the old code, so
    /// it is retired here instead of being made usable (§4.6). Either way this
    /// is where the worker stops being a `pending_ready` heap copy — the one
    /// decrement a worker gets, because the removal paths release it only for a
    /// worker that never became ready.
    fn view_ready(&mut self, key: ViewKey) -> Result<(), SchedulerError> {
        let epoch = self.reload_epoch;
        let birth_stale = self
            .view_mut(key)
            .is_some_and(|view| view.spawned_epoch < epoch);
        if birth_stale {
            return self.retire(key, RetireReason::Reload);
        }
        let mut marked = false;
        if let Some(view) = self.view_mut(key) {
            let pending = !view.ready;
            view.ready = true;
            // A view that has never served anything is idle from now on, so
            // even a preforked process that nobody asked for is retired by
            // `BLASTIT_VIEW_IDLE_TIMEOUT` (§2.4).
            if view.inflight == 0 {
                view.idle_since = Some(Instant::now());
            }
            if pending && let Some(pool) = self.pools.get_mut(&key.route) {
                pool.pending_ready = pool.pending_ready.saturating_sub(1);
            }
            marked = true;
        }
        if marked {
            // §8 Sizing: this is the first moment the view's share of the
            // preloaded heap exists in the aggregate reading.
            self.measure_view_memory();
        }
        Ok(())
    }

    /// init reported a view process gone (§4).
    ///
    /// The process's own `view.chan` decides what it said before it went away:
    /// the reader task sends every frame it decoded and only then
    /// [`ViewIn::Closed`] ([`crate::task::read_frames`]), so failing the
    /// in-flight requests here would bill a request the view **answered** as a
    /// dead view with no cost. That is `MB.3.48`'s 1-in-20 `php-sapi` flake: a
    /// contained bailout writes `RESP_HEAD`, `RESP_END_COST` and `DONE{Bailed}`
    /// and then retires by design (`crates/blastit-view/src/php.rs`), and
    /// `Ctl::Exited` can reach the scheduler first on the other channel.
    ///
    /// So the failure is left to [`Scheduler::view_gone`], which
    /// [`ViewIn::Closed`] runs *after* those frames, and
    /// [`Scheduler::expire_exited_views`] bounds that wait by
    /// `request_timeout` so a channel that never closes cannot hang a request.
    /// A process with no channel to wait for — one retired already, or one
    /// whose `SPAWNED` never arrived — is handled immediately.
    fn view_exited(&mut self, key: ViewKey) {
        if key.pid == 0 {
            // No process exists: the `SPAWN` was refused or its fork failed
            // (§4.7).
            return self.spawn_failed(key.route);
        }
        let live = self.view_mut(key).is_some();
        let channel = live
            && self
                .pools
                .get(&key.route)
                .and_then(|pool| pool.views.get(&key.pid))
                .is_some_and(|view| view.out.is_some());
        if !channel {
            self.view_gone(key);
            return;
        }
        if let Some(view) = self.view_mut(key)
            && view.exited.is_none()
        {
            view.exited = Some(Instant::now());
        }
    }

    /// Marks a worker that answered `DONE { Bailed }` as retiring (`MB.26.8`).
    ///
    /// The handler never returned, so the runtime retires the process after
    /// this request — the view says so one `ctl.chan` report before init's
    /// `Ctl::Exited` can. Until the ordered `view.chan` closes, the process is
    /// still alive and the frames it wrote are still being read, so this is the
    /// same window [`Scheduler::view_exited`] handles from the other channel:
    /// nothing is failed here. What changes is the next dispatch — a queued
    /// request must grow a replacement, never be handed to a worker that has
    /// already answered and is going away.
    fn view_retiring(&mut self, key: ViewKey) {
        if let Some(view) = self.view_mut(key)
            && view.retiring.is_none()
        {
            view.retiring = Some(Instant::now());
        }
    }

    /// Fails the requests of a view on its way out whose `view.chan` never
    /// closed, `request_timeout` after the report (`MB.3.45`, `MB.26.8`).
    ///
    /// The ordered close is what normally decides; this is the bound that keeps
    /// a lost descriptor from turning a dead (or a retiring) view into a request
    /// that waits forever. It is only ever reached when no `ViewIn::Closed`
    /// came.
    fn expire_exited_views(&mut self) {
        let now = Instant::now();
        let overdue: Vec<ViewKey> = self
            .pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| {
                view.leaving_at()
                    .is_some_and(|leaving| now >= leaving + self.cfg.request_timeout)
            })
            .map(|view| view.key)
            .collect();
        for key in overdue {
            self.view_gone(key);
        }
    }

    /// A view process is gone: fail its requests, free their slots, and let
    /// the next request for its route grow a new one (§2.4).
    ///
    /// The requests are failed and released whether or not the pool still holds
    /// the worker — a retired worker's channel closes too, and that close is
    /// what frees the slots its requests were holding. The route's queue is
    /// failed only when the fork that died never served **and** the route has no
    /// worker and no `SPAWN` left: a sibling that can answer must keep its
    /// requests (§4.7).
    fn view_gone(&mut self, key: ViewKey) {
        let removed = self
            .pools
            .get_mut(&key.route)
            .filter(|pool| pool.views.get(&key.pid).is_some_and(|view| view.key == key))
            .and_then(|pool| pool.views.remove(&key.pid));
        let ready = removed.as_ref().is_some_and(|view| view.ready);
        if removed.is_some()
            && !ready
            && let Some(pool) = self.pools.get_mut(&key.route)
        {
            pool.pending_ready = pool.pending_ready.saturating_sub(1);
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
        if removed.is_some() && !ready {
            // A worker that never served anything: cool the route's *growth*
            // down. Its siblings keep serving, and their requests are untouched.
            self.backoff
                .insert(key.route, Instant::now() + self.cfg.spawn_backoff);
            let empty = self.pools.get(&key.route).is_none_or(ViewSet::is_empty);
            if empty {
                // No process left at all and no fork owed: the route's queue is
                // answered now, as it always was. The ids are collected first:
                // the iterator holds `&self` and the body takes `&mut self`.
                let waiting: Vec<u32> = self.queued_for_streams(key.route).collect();
                for stream_id in waiting {
                    self.queue.remove(stream_id);
                    let _ = self.fail_request(stream_id, Outcome::ViewDied, 502);
                    self.release(stream_id);
                }
            }
        }
        self.forget_if_idle(key.route);
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
        self.respond_with(stream_id, status, reason, body, retry_after, &[])
    }

    /// [`Scheduler::respond`], with headers the caller adds after the two the
    /// canned answers always carry.
    ///
    /// The static refusals are the one caller: `Allow` on the `405`, and
    /// `Cache-Control: no-store` on all four (design §4.4). The app path passes
    /// no extra header, so its bytes are unchanged.
    fn respond_with(
        &mut self,
        stream_id: u32,
        status: u16,
        reason: &str,
        body: &str,
        retry_after: Option<u64>,
        extra: &[(&[u8], &[u8])],
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
        for (name, value) in extra {
            headers.push((name.to_vec(), value.to_vec()));
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
        // Charge before the event that reports the charge (`MB.3.45`): a
        // scheduler-written answer is complete here, so this span is final and
        // the event must not say `cost_ns: 0`.
        self.charge(stream_id);
        self.emit_request(stream_id);
        self.release(stream_id);
        Ok(())
    }

    /// Answers a request whose body the route will not take, cancelling a view
    /// that may already have seen the head (`R12`, `R14`, `R15`).
    ///
    /// The view keeps its slot until its `DONE` arrives, exactly as a client
    /// cancellation does: the response is over, but the thread is still alive.
    fn refuse_body(&mut self, stream_id: u32, status: u16) -> Result<(), SchedulerError> {
        let view = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.view);
        if let Some((key, id)) = view {
            self.view_push(key, id, Frame::Cancel)?;
        }
        let (reason, body) = canned(status);
        self.respond(stream_id, status, reason, body, None)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = Outcome::Unmatched;
        }
        self.mark_finished(stream_id);
        // The response is over, so its span is final here even though the slot
        // is not: charge and emit now (`MB.3.45`). The fair-queue debit lands
        // at this refusal, not at the `DONE` that frees the slot (`MB.3.49`,
        // [`Scheduler::charge`]).
        self.charge(stream_id);
        self.emit_request(stream_id);
        if view.is_none() {
            self.release(stream_id);
        }
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
        // The span is final here, so the event carries it (`MB.3.45`).
        self.charge(stream_id);
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
        // The scheduler wrote this answer; charge the span it held a worker for
        // before the event reports it (`MB.3.45`). A request that died with its
        // view still keeps the dispatch→completion rule: no view answered it.
        self.charge(stream_id);
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
            if let Some(view) = self.view_mut(key) {
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
        if request.learn_fallback {
            // The single `--learn` slot is free again; `pump` starts the next
            // unrouted request right after this event.
            self.learn_fallback_busy = false;
        }
        if let Some(flow) = request.flow {
            self.queue.release_flow(flow, Instant::now());
        }
    }

    /// What this request cost, and what the flow is charged for it (D5, `R19`).
    ///
    /// For a request a view answered, the cost is the view's own report when
    /// there is one, **clamped** into `[floor, observed]`, where `observed` is
    /// the time the scheduler saw between dispatch and completion and `floor`
    /// the time up to the view's first response frame. Both bounds are the
    /// scheduler's own measurements, and both are needed because a view process
    /// is sandboxed and may be compromised: a report that claims more time than
    /// the request could possibly have taken would let one client buy the whole
    /// queue, and a report of nothing would let it keep its place at the front
    /// for free — while the floor is exactly the time the view cannot hide,
    /// because shortening it means delaying its own client's response.
    ///
    /// With no report — the view crashed, timed out, was cancelled, the client
    /// went away, or the view was built before D5 — the observed time is charged
    /// instead.
    ///
    /// A request a view **bailed out of** (`view_answered`, `bailed` and
    /// `status >= 500`: a `5xx` a contained `exit()`/`die()`/fatal answered —
    /// `zend_bailout()` swallowed by the SAPI shim, reported by `DONE`) is
    /// charged `R19`'s crash/`504` ceiling instead, and its report is not
    /// believed at all. A failing view is exactly the one whose report must not
    /// be believed: a script that dies 18 ms into a request but says it worked
    /// for 3 ms would let a client crash-loop for a fifth of its cost.
    /// `ceiling = observed_wall_ns * max_threads`, and the live thread count of
    /// the single-threaded PHP worker `ADR-0010` forks is one; no kernel floor
    /// can exceed one process's own wall time, so `ceiling.max(kernel_floor_ns)`
    /// is that wall. The span charged is the one the `request` event reports as
    /// `duration_ms` — arrival to the last byte — so a bailed request is always
    /// charged at least the wall time its client waited.
    ///
    /// The requirement is both a view's own `RESP_HEAD` **and** a `DONE` that
    /// says `bailed` ([`answered_a_bailout`]): a `5xx` an app answered itself
    /// returned from its handler, so it keeps the clamp like any other answer,
    /// and a `5xx` the scheduler wrote itself (the `502` of a view that never
    /// answered, a `504` timeout, a `503` refusal) keeps the
    /// dispatch→completion rule — the view never reported anything for it, so
    /// there is nothing of its to distrust, and the crash bill's
    /// arrival→last-byte span would charge a queue wait the request never spent
    /// on a worker.
    ///
    /// It is idempotent, and every path that concludes a request charges before
    /// it emits that request's `request` event (`MB.3.45`): the response end,
    /// the scheduler's own answer, a cancellation, a refusal, a view failure.
    /// [`Scheduler::release`] charges as the fallback for the requests no
    /// earlier path could charge. Two of them are the deferred `5xx` a view
    /// answered: the one whose `DONE` labels it (`MB.3.41`), and the one whose
    /// view's channel closes before that `DONE` ever arrives
    /// ([`Scheduler::view_gone`] cannot charge it either, because it is already
    /// complete). The third is a `DONE` that answers a response whose head is
    /// out but whose `RESP_END` never came ([`Scheduler::view_done`]'s
    /// `head_sent && !complete` arm). Every other request is charged by the
    /// path that concluded it, and the second call is a no-op.
    ///
    /// The fair-queue debit therefore lands at the first of those charges, not
    /// at the release that frees the slot: when the request already has a view,
    /// a client cancellation and a refused body both leave the slot held to
    /// that view's `DONE`, so the flow is debited when the response ends and
    /// its release comes later (`MB.3.49`); a view-less request releases in the
    /// same turn, so its debit and its release coincide. That endpoint is
    /// deliberate: the span billed is the request's own, and splitting the
    /// debit from the charge would need a residual carried to release while the
    /// event must already report the charge.
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
        // The event's own `duration_ms` span (`arrival` to the last byte), which
        // is what `R19`'s ceiling is taken from when a view failed the request;
        // `finished_at` is set before every charge, and `now` is the same
        // instant the event would fall back to.
        let finished = request.finished_at.unwrap_or(now);
        let charged = charge_ns(Charge {
            reported: request.report.map(|report| report.wall_ns),
            floor,
            observed,
            wall: nanos(finished.saturating_duration_since(request.arrived)),
            failed: answered_a_bailout(request.view_answered, request.status, request.bailed),
        });
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
    ///
    /// [`Scheduler::charge`] runs first, always: the event reports
    /// `charged_ns`, so an event emitted before its charge says `cost_ns: 0`
    /// for a request that did hold a worker (`MB.3.45`). The debug assertion is
    /// what keeps that ordering true for every path.
    fn emit_request(&mut self, stream_id: u32) {
        let Some(request) = self.requests.get(&stream_id) else {
            return;
        };
        if request.emitted {
            return;
        }
        debug_assert!(
            request.charged,
            "a request event is emitted only after the request was charged"
        );
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
        //
        // Both are handed to `from_target` as borrowed text, which is what
        // strips the query and allocates the `path` once: cloning `path_info`
        // first would allocate the same string twice per request.
        let method = String::from_utf8_lossy(&request.head.method);
        let routed_path = match &request.path_info {
            Some(path_info) => Cow::Borrowed(path_info.as_str()),
            None => String::from_utf8_lossy(path_of_target(&request.head.target)),
        };
        let mut info = RequestInfo::from_target(&*method, &routed_path)
            .with_status(request.status)
            .with_outcome(request.outcome)
            .with_timings(queued_ms, duration_ms)
            .with_bytes(request.bytes_in, request.bytes_out)
            .with_remote_addr(request.head.remote_addr.clone())
            .with_cost(
                request.charged_ns,
                // The view's own CPU measurement, when it could take one:
                // absent is not the same as zero (§8.1).
                request.report.and_then(|report| report.cpu_ns),
            );
        if let Some(rule) = request.rule {
            info = info.with_rule(rule);
        }
        let mut event = Event::now(Component::Scheduler, EventKind::Request(info))
            .with_request_id(request.request_id.clone());
        if let Some(route) = request.route
            && !request.learn_fallback
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
    /// A `DONE` that arrives twice releases the slot the first time and is
    /// counted as dropped the second (`release` removed the dispatch entry). One
    /// that arrives for a request the scheduler already completed — its `504`,
    /// its `502`, a cancellation, or the `5xx` it held back for exactly this
    /// frame — still frees the slot; the only thing it can change is the label
    /// of a `5xx` the view itself answered
    /// ([`a_done_may_relabel_a_completed_5xx`]).
    ///
    /// `MB.3.41`: this frame is also the only one that says whether the handler
    /// **returned** or **bailed**, so a `5xx` a view answered is labelled and
    /// charged here rather than at its `RESP_END` (see [`Scheduler::view_end`]).
    /// `view_died` stays what the events crate says it is — the view process
    /// died mid-request — and a `5xx` a view **completed** is never that,
    /// whether its handler returned (`ok`) or bailed (`bailed`).
    fn view_done(
        &mut self,
        key: ViewKey,
        id: u32,
        outcome: DoneOutcome,
    ) -> Result<(), SchedulerError> {
        // A `DONE { Bailed }` the worker sends to conclude its **own** request
        // retires it: the handler never returned, so the runtime retires the
        // process after that request, and the pump between this frame and the
        // close must grow a replacement for the queue instead of dispatching
        // into it (`MB.26.8`).
        //
        // A `DONE { Bailed }` for a request the scheduler already answered
        // itself is `LateDone`'s shape (`MB.3.45`): its slot was freed by that
        // answer, and the worker's fate is the ordered close's business, so the
        // route still dispatches to it. Two things narrow the mark to the first
        // shape, and neither is a policy this frame could widen:
        //
        // * the early return just below — no dispatch entry, no mark — which is
        //   what a `DONE` arriving after `release` hits (a repeated `DONE`, or a
        //   request the scheduler answered and let go); and
        // * the `!timed_out && !cancelled` guard below, which is what a cancel
        //   or a timeout of a *dispatched* request hits: `release` has not run
        //   yet (it runs at the bottom of this function, because the view keeps
        //   its slot until its `DONE`), so the entry is still there and that
        //   guard is the only thing that speaks.
        //
        // So a client disconnect during a handler that bails is **not**
        // covered: the worker still retires (`crates/blastit-view/src/php.rs`
        // sets `worker.bailed |= retire` and `recycle` acts on it), but its
        // `DONE` is forced to `Aborted` for the cancelled stream (`if
        // io.cancelled`), `cancelled` is set, and no mark is made. Such a
        // worker is bounded only by `expire_exited_views`, one `request_timeout`
        // after its channel closes, instead of at the next dispatch. The fix is
        // for the transport to say it is retiring — an explicit `RETIRING`
        // frame rather than an outcome the scheduler has to infer — which is a
        // protocol change and belongs to its own node.
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let (complete, head_sent, view_answered, cancelled, timed_out, status) =
            match self.requests.get(&stream_id) {
                Some(request) => (
                    request.complete,
                    request.head_sent,
                    request.view_answered,
                    request.cancelled,
                    request.timed_out,
                    request.status,
                ),
                None => return Ok(()),
            };
        if outcome == DoneOutcome::Bailed && !timed_out && !cancelled {
            self.view_retiring(key);
        }
        let bailed = outcome == DoneOutcome::Bailed;
        if !complete {
            match outcome {
                DoneOutcome::Completed | DoneOutcome::Bailed if head_sent => {
                    // The thread finished; only its `RESP_END` is missing.
                    self.http_push(stream_id, Frame::RespEnd)?;
                    if let Some(request) = self.requests.get_mut(&stream_id) {
                        request.complete = true;
                        request.bailed = bailed;
                        if !request.cancelled {
                            request.outcome =
                                completed_outcome(view_answered, request.status, bailed);
                        }
                    }
                    self.mark_finished(stream_id);
                }
                DoneOutcome::Completed | DoneOutcome::Bailed | DoneOutcome::Errored => {
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
        } else if let Some(request) = self.requests.get_mut(&stream_id) {
            // A completed `5xx` was held back for this frame: whether the
            // handler returned (the app's own failure, an answer) or bailed
            // (the `R19` crash) is a fact only `DONE` carries.
            if a_done_may_relabel_a_completed_5xx(
                request.view_answered,
                request.cancelled,
                request.status,
            ) {
                request.bailed = bailed;
                request.outcome = completed_outcome(request.view_answered, request.status, bailed);
            }
        }
        // `release` is what charges the request and emits its event, so a `5xx`
        // is billed exactly once, with the label this frame decided.
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
            // The status a `DONE` may label this response with came from the
            // view itself, which is a different fact from "a head went out"
            // (`MB.3.45`): the scheduler's own `5xx` must not become `bailed`.
            request.view_answered = true;
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
    /// exactly when the report arrives — **unless** the view answered a `5xx`
    /// (`MB.3.41`). That one response is finalized at its `DONE`, one frame
    /// later on the same channel, because the status cannot say whether the
    /// handler returned (the app's own failure, an answer) or bailed (`R19`'s
    /// crash). [`Scheduler::release`] is what charges and emits it, and the
    /// paths that reach it are exact (`MB.3.45`): the request's `DONE`, or the
    /// ordered close of the view's channel — which is also how a
    /// `stuck_threads` retire ends, because a retire drops that channel
    /// ([`Scheduler::retire`]). A live view that simply loses that one frame
    /// holds the request — and its event — until one of those happens:
    /// `client_cancel` returns early on a completed request and
    /// `expire_requests` only marks it overdue (the `stuck_threads` retire that
    /// may follow ends it through the close).
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
        let mut deferred = false;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            if !request.cancelled {
                // Anything below a `5xx` is an answer: the view produced a
                // complete response, and the status says the rest.
                deferred = request.status >= 500;
                if !deferred {
                    request.outcome = Outcome::Ok;
                }
            }
        }
        self.mark_finished(stream_id);
        // D5: the response's last byte is where a request's cost is fixed —
        // with the view's report when it sent one, with the time the scheduler
        // observed itself when it did not — so the event carries what was
        // actually charged, and the flow is behind the others before its thread
        // even exits. Every path that concludes a request charges before it
        // emits (`MB.3.45`), so [`Scheduler::release`] only charges a request
        // that reaches it uncharged — this deferred `5xx` once its `DONE`
        // labels it or its view's channel closes first, and a `DONE` for a
        // response whose head is out but whose `RESP_END` never came
        // ([`Scheduler::view_done`]).
        if let Some(cost) = cost
            && let Some(request) = self.requests.get_mut(&stream_id)
        {
            request.report = Some(cost);
        }
        if !deferred {
            self.charge(stream_id);
            self.emit_request(stream_id);
        }
        Ok(())
    }

    /// `RESP_ABORT` from a view: the thread gave up, with or without a head.
    ///
    /// This is the runtime's own failure — a transport error, a WSGI callable
    /// that raised before answering — not a handler that bailed after answering
    /// (that one is `DONE`'s `bailed`, `MB.3.41`). §8.1's outcomes have no "the
    /// app raised", so it is reported as `view_died`, the same as a process that
    /// went away mid-response.
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
        // A `502` the scheduler wrote for a view that aborted: the span is
        // final, so its event carries the charge (`MB.3.45`).
        self.charge(stream_id);
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

/// Whether a static admission slot may be handed back now (design §4.6).
///
/// `holds_slot && !pumping`: a request that never took a slot has nothing to
/// give back, and one whose `pread` is still parked keeps it until the read
/// result arrives — a `pread` on a stalled mount cannot be cancelled, so
/// releasing it earlier would free the slot (and the descriptor) while the
/// blocking task still owns it.
///
/// It is the one predicate [`Scheduler::release_static_slot`] consults, and the
/// timeout and cancellation paths reach the slot through that function, so a
/// figure that disagrees with this one cannot free a parked request.
fn slot_releasable(holds_slot: bool, pumping: bool) -> bool {
    holds_slot && !pumping
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

/// The outcome of a response a view completed: `ok`, or — when the handler
/// bailed out of a `5xx` it had already answered — [`Outcome::Bailed`].
///
/// The status alone decides nothing (`MB.3.41`). An app that answers
/// `http_response_code(500)`, Django's own `500`, a Node or JVM error response:
/// the view **returned** from the handler and produced a complete response, so
/// it is [`Outcome::Ok`] — the crate's gloss for exactly that, "the view
/// answered" — and `status` carries the failure. Only `bailed` (the `DONE` the
/// SAPI shim sends for a contained `exit()`/`die()`/fatal) is a request whose
/// handler never returned, and it is not `view_died` either: it answered before
/// it went away, so it is no death mid-request ([`Outcome::Bailed`]'s own
/// gloss). The worker retires after that request — a bailout leaves the
/// interpreter unusable (`crates/blastit-view/src/php.rs`) — and its ordered
/// `view.chan` frames are read before the close that would fail what it owes
/// ([`Scheduler::view_exited`]).
///
/// `view_answered` is required because a bailout with no `RESP_HEAD` from the
/// view answered nothing at all, so there is no completed response to label. It
/// has to be the view's own head (`MB.3.45`): a `5xx` the scheduler wrote itself
/// is an answer the view never gave.
fn completed_outcome(view_answered: bool, status: u16, bailed: bool) -> Outcome {
    if view_answered && status >= 500 && bailed {
        Outcome::Bailed
    } else {
        Outcome::Ok
    }
}

/// Whether a view **bailed out of a `5xx` it had answered**, which is what
/// `R19` charges the crash ceiling for.
///
/// All three halves are needed, and each keeps a different request out of the
/// crash bill (`MB.3.41`):
///
/// * `bailed` is the `DONE`'s own word — the handler did not return. An app's
///   own `500` returned, so its report is believed within the clamp exactly
///   like a `200`'s.
/// * `view_answered` is set only when a `RESP_HEAD` came from the view, so a
///   `5xx` the scheduler wrote itself — the `502` of a view that never
///   answered, the `504` of `BLASTIT_REQUEST_TIMEOUT`, a `503` refusal — is not
///   a view failure and keeps the dispatch→completion charge: the view never
///   reported for it, and the crash bill's arrival→last-byte span would charge
///   a queue wait the request never spent on a worker.
///   `head_sent` cannot say this: `respond` sets that one too, for a head the
///   scheduler wrote, and a view that bails out *after* the scheduler already
///   answered that same request must not turn it into the crash bill
///   (`MB.3.45`).
/// * `status >= 500` keeps a bailout that still completed its response
///   (`echo "…"; exit;` answers `200`) out of the crash rule: the client got
///   everything the app produced.
const fn answered_a_bailout(view_answered: bool, status: u16, bailed: bool) -> bool {
    view_answered && bailed && status >= 500
}

/// Whether a `DONE` may label a response the scheduler already completed
/// (`MB.3.45`).
///
/// Only a response the **view** answered can carry the handler's own verdict: a
/// `5xx` the scheduler wrote itself — a `504` timeout, the `502` of a view that
/// died — is that answer, and a view that finishes afterwards, even with
/// `DONE { Bailed }`, must not turn it into `bailed` nor into `R19`'s crash
/// bill. A cancelled request keeps the label it already has for the same reason.
const fn a_done_may_relabel_a_completed_5xx(
    view_answered: bool,
    cancelled: bool,
    status: u16,
) -> bool {
    !cancelled && view_answered && status >= 500
}

/// The facts one request's charge is decided from (D5, `R19`).
struct Charge {
    /// What the view reported (`RESP_END_COST`'s wall time), when it sent one.
    reported: Option<u64>,
    /// Dispatch to the view's first response frame.
    floor: u64,
    /// Dispatch to completion: the scheduler's own upper bound.
    observed: u64,
    /// Arrival to the last byte: the event's `duration_ms`, in nanoseconds.
    wall: u64,
    /// A view answered this request by **bailing out of a `5xx`**
    /// ([`answered_a_bailout`]).
    failed: bool,
}

/// What one request is charged, in nanoseconds.
///
/// A request a view **bailed out of** is charged its **wall** time — `R19`'s
/// `ceiling.max(kernel_floor_ns)`, which is that wall at the one live thread
/// such a worker has, and which no kernel floor can exceed for one process —
/// and its report is not believed at all. A failing view is exactly the one
/// whose report must not be believed: a script that dies 18 ms into a request
/// but says it worked for 3 ms would let a client crash-loop for a fifth of its
/// cost. Everything else keeps the documented clamp: the report between the
/// view's first frame and the scheduler's own observation, or the observation
/// when there was no report — including an app's own `500`, whose handler
/// returned and whose report is therefore as good as a `200`'s, and a `502` or
/// `504` the scheduler wrote itself: the view never reported for it, and the
/// crash bill's arrival→last-byte span would charge a queue wait the request
/// never spent on a worker. The two are decided by [`answered_a_bailout`], never
/// by the status alone.
fn charge_ns(charge: Charge) -> u64 {
    if charge.failed {
        return charge.wall;
    }
    match charge.reported {
        Some(reported) => reported.clamp(charge.floor, charge.observed),
        None => charge.observed,
    }
}

/// The reason phrase and body of a status the scheduler writes itself.
///
/// `405` is the static responder's method gate (`rewrite::decide`, design §4.4)
/// and the only status here that is not also an app-path refusal: a `POST` to a
/// path the responder serves is answered `Method Not Allowed`, with the one
/// `Allow: GET, HEAD` the rejection helper adds.
fn canned(status: u16) -> (&'static str, &'static str) {
    match status {
        400 => ("Bad Request", "bad request\n"),
        404 => ("Not Found", "not found\n"),
        405 => ("Method Not Allowed", "method not allowed\n"),
        413 => ("Content Too Large", "content too large\n"),
        502 => ("Bad Gateway", "bad gateway\n"),
        503 => ("Service Unavailable", "unavailable\n"),
        504 => ("Gateway Timeout", "gateway timeout\n"),
        _ => ("Error", "error\n"),
    }
}

/// The first value of `name` in `head`, as text, when it is valid UTF-8.
///
/// Only `Content-Type`/`Content-Encoding` are read this way, and only to hand
/// them to `blastit-ingress`, which never echoes them back unvalidated.
fn header_text(head: &ReqHead, name: &[u8]) -> Option<String> {
    head.headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| std::str::from_utf8(value).ok())
        .map(str::to_owned)
}

/// The first value of `name` in a head still in its raw payload, as text.
///
/// The same read as [`header_text`], over the borrowed [`ReqHeadView`] the
/// scheduler plans a request's body from: the header list is walked in place,
/// so neither the head nor the value is copied until it is actually needed.
fn head_header_text(head: &ReqHeadView<'_>, name: &[u8]) -> Option<String> {
    head.headers()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| std::str::from_utf8(value).ok())
        .map(str::to_owned)
}

/// The `Content-Length` a head still in its raw payload declares, if any.
///
/// The header list is walked in place, so no `String` is built for a route that
/// declares no ingress and reads only this. A length this build cannot parse
/// (a non-ASCII value, a sign, a bad digit) is `u64::MAX`: a declared body this
/// build cannot size is treated as unbounded, so it fails closed instead of
/// looking like zero.
fn declared_content_length(head: &ReqHeadView<'_>) -> Option<u64> {
    head.headers()
        .find(|(name, _)| name.eq_ignore_ascii_case(b"content-length"))
        .map(|(_, value)| {
            std::str::from_utf8(value)
                .ok()
                .and_then(|text| text.trim().parse::<u64>().ok())
                .unwrap_or(u64::MAX)
        })
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
    // A manifest that declares a static mount needs the responder this binary
    // was built without: refuse at boot, before a single channel is served
    // (`MB.36.2`, ADR-0016 decision 6).
    #[cfg(not(feature = "static"))]
    if !cfg.mounts.is_empty() {
        return Err(SchedulerError::StaticNotCompiled);
    }
    let limits = cfg.frame_limits;
    let (http_read, http_write) = http.into_split();
    let (http_in_tx, mut http_in_rx) = mpsc::channel(HTTP_IN_CAPACITY);
    let (http_out, http_out_rx, http_pending) = Outbound::new(limits, HTTP_OUTBOUND_CAP);
    let (view_in_tx, mut view_in_rx) = mpsc::channel(VIEW_IN_CAPACITY);
    let (ctl_out, ctl_out_rx, ctl_pending) = Outbound::new(limits, CTL_OUTBOUND_CAP);

    // http's frames are read without decoding: it is blastit's own Rust code,
    // so its bytes are well-formed by construction, and the request head is
    // forwarded to the view verbatim instead of being copied header by header.
    // The view channel is read the strict way (see `view_spawned`); do not
    // switch it to `read_raw_frames`.
    tokio::spawn(read_raw_frames(
        http_read,
        limits,
        http_in_tx.clone(),
        |id, raw| HttpIn::Frame { id, raw },
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
    // The static responder is compiled in: one root per mount is opened here as
    // the scheduler's uid and before a single request is served. A root that
    // cannot be opened is a broken declaration, not a degraded answer, so it
    // exits the role (§9.2).
    #[cfg(feature = "static")]
    let (mounts, mount_roots, static_in, mut static_in_rx) = {
        let mounts = MountTable::from_mounts(&cfg.mounts);
        let mut mount_roots: Vec<Arc<StaticRoot>> = Vec::with_capacity(cfg.mounts.len());
        for mount in &cfg.mounts {
            let root = StaticRoot::open(&cfg.manifest_dir, &mount.policy)
                .map_err(|error| SchedulerError::protocol(error.to_string()))?
                .with_prefix(&mount.prefix)
                // The one source of the per-file ceiling: no environment
                // variable and no manifest key (design §4.6).
                .with_max_bytes(cfg.static_max_bytes);
            mount_roots.push(Arc::new(root));
        }
        let (static_in, static_in_rx) = mpsc::channel::<StaticReadMsg>(STATIC_IN_CAPACITY);
        (mounts, mount_roots, static_in, static_in_rx)
    };
    // Nothing ever sends on it (see `StaticReadMsg`), and the sender is held
    // for the whole run, so its `select!` arm never completes.
    #[cfg(not(feature = "static"))]
    let (_static_in, mut static_in_rx) = mpsc::channel::<StaticReadMsg>(1);
    #[cfg(feature = "static")]
    let mut scheduler = Scheduler::new(
        cfg,
        routes,
        table,
        emit,
        http_out,
        view_in_tx,
        ctl_out,
        mounts,
        mount_roots,
        static_in,
    );
    #[cfg(not(feature = "static"))]
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
            Some(read) = static_in_rx.recv() => scheduler.on_static_read(read)?,
            () = sleep_until(deadline) => {}
        }
        scheduler.pump()?;
        if scheduler.done() {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{
        Charge, a_done_may_relabel_a_completed_5xx, answered_a_bailout, charge_ns,
        completed_outcome, slot_releasable,
    };
    use blastit_events::Outcome;

    /// Design §4.6's table for the one predicate that frees a static slot.
    ///
    /// The protected `critical_rewrite.rs` cannot reach a private item, and
    /// `lib.rs` (which would have to export it) is outside this leaf's paths,
    /// so the three rows live here: a request that holds a slot and has no read
    /// parked gives it back; one with a read parked gives it back only when
    /// that read returns; one that never took a slot gives back nothing.
    #[test]
    fn a_static_slot_is_releasable_only_when_its_read_is_not_parked() {
        assert!(slot_releasable(true, false), "(true, false) → true");
        assert!(!slot_releasable(true, true), "(true, true) → false");
        assert!(!slot_releasable(false, false), "(false, false) → false");
        assert!(!slot_releasable(false, true), "(false, true) → false");
    }

    /// `MB.3.38`, narrowed by `MB.3.41`: a response a view completed is `ok`
    /// whatever its status, and a contained bailout is the one thing that is
    /// not.
    ///
    /// The protected `critical_php_embed.rs` leg can only see this on a host
    /// with a NTS `libphp`, and `make check` runs on hosts without one, so this
    /// is the label's only witness in CI. Both directions are asserted: an app
    /// that answers `2xx`, `4xx` **or its own `5xx`** answered the request, so
    /// it is `ok` (the status carries the failure, as `Outcome::Ok`'s own doc
    /// says); a contained `exit()`/`die()`/fatal answered a `5xx` **without
    /// returning**, which is `bailed` and nothing else. `view_died` is the event
    /// table's "the view process died mid-request" — the closed list's label
    /// for a response that died under the view — so a completed response is
    /// never it.
    #[test]
    fn a_completed_response_is_a_bailed_5xx_or_an_answer() {
        for status in [100, 200, 204, 301, 304, 400, 404, 413] {
            assert_eq!(
                completed_outcome(true, status, false),
                Outcome::Ok,
                "{status} is not a failure"
            );
        }
        // An app's own `500`: Django's handler, a Node/JVM error response, a
        // script's `http_response_code(500)`. The view answered.
        for status in [500, 501, 502, 503, 504, 599] {
            assert_eq!(
                completed_outcome(true, status, false),
                Outcome::Ok,
                "a {status} the app answered itself is an answer, not a death"
            );
        }
        // A contained bailout that answered the same `5xx`.
        for status in [500, 501, 502, 503, 504, 599] {
            assert_eq!(
                completed_outcome(true, status, true),
                Outcome::Bailed,
                "a contained bailout answering {status} is `bailed`"
            );
        }
        // A `< 500` a bailed handler still answered in full: the client got its
        // response, so it is an answer (`exit()` at the end of a script).
        assert_eq!(
            completed_outcome(true, 200, true),
            Outcome::Ok,
            "a bailout after a completed 200 is not a failed request"
        );
        // Without a `RESP_HEAD` from the view nothing was answered, so no
        // status can make the request `bailed`.
        assert_eq!(completed_outcome(false, 500, true), Outcome::Ok);
        assert_eq!(completed_outcome(false, 500, false), Outcome::Ok);
    }

    /// `MB.3.41`: only a `5xx` a view **answered by bailing** is the `R19`
    /// crash case.
    ///
    /// `view_answered` is the flag that says the `RESP_HEAD` came from the view
    /// (`view_head`), and `bailed` is the `DONE` that says the handler never
    /// returned. `head_sent` is not that flag: `respond`, the scheduler's own
    /// answer, sets it too (`MB.3.45`). `view_answered` and `bailed` are both
    /// required, and that keeps the scheduler's own `502`/`504`/`503` out of
    /// it — a timed-out request must not be charged the queue wait it spent
    /// before it ever reached a worker — while a script's own `500` is charged
    /// like any other answer.
    #[test]
    fn only_a_bailed_5xx_a_view_answered_is_a_crash() {
        // A contained bailout: the shim's `500` is a real `RESP_HEAD`, and
        // `DONE` says `bailed`.
        assert!(answered_a_bailout(true, 500, true));
        assert!(answered_a_bailout(true, 502, true));
        // An app's own `500` returned: the report is the view's own claim about
        // a request it completed, so it is clamped, not billed as a crash.
        assert!(!answered_a_bailout(true, 500, false));
        // The scheduler's own failures: no view answered them.
        assert!(!answered_a_bailout(false, 502, false), "a dead view's 502");
        assert!(!answered_a_bailout(false, 503, false), "a queue refusal");
        assert!(!answered_a_bailout(false, 504, false), "a request timeout");
        // A bailout on a successful response is not a failed request either.
        assert!(!answered_a_bailout(true, 200, true));
        // A view that answered successfully is not a failure.
        assert!(!answered_a_bailout(true, 200, false));
        assert!(!answered_a_bailout(true, 404, false));
    }

    /// `MB.3.41`: a bailed `500` pays its **wall**, while a `5xx` the scheduler
    /// wrote itself keeps its **dispatch→completion** charge — no queue-wait
    /// bill — and an app's own `500` keeps the documented clamp.
    ///
    /// The numbers are one real shape from `critical_php_embed.rs`: `queued_ms`
    /// ~14.7 of an ~18 ms request, so arrival→last byte (`wall`) is 18 ms while
    /// dispatch→completion (`observed`) is 3.3 ms.
    #[test]
    fn a_bailed_500_pays_its_wall_and_a_scheduler_502_or_504_does_not() {
        const WALL: u64 = 18_000_000;
        const OBSERVED: u64 = 3_300_000;
        const FLOOR: u64 = 3_200_000;
        // The handler bailed after answering `500` and reported 3.2 ms: it pays
        // its wall, so a crash loop cannot cost a fifth of the time it spends.
        assert_eq!(
            charge_ns(Charge {
                reported: Some(FLOOR),
                floor: FLOOR,
                observed: OBSERVED,
                wall: WALL,
                failed: answered_a_bailout(true, 500, true),
            }),
            WALL,
            "a contained bailout is charged the 18 ms its client waited, not its 3.2 ms report"
        );
        // The same report for a `500` the app answered and returned from: the
        // clamp, exactly as a `200` (an answered request is not a crash).
        assert_eq!(
            charge_ns(Charge {
                reported: Some(FLOOR),
                floor: FLOOR,
                observed: OBSERVED,
                wall: WALL,
                failed: answered_a_bailout(true, 500, false),
            }),
            FLOOR,
            "an app's own 500 keeps the documented clamp, queue wait excluded"
        );
        // A `504`: no view answered, so the charge stays the
        // dispatch→completion span the request actually held a worker for.
        assert_eq!(
            charge_ns(Charge {
                reported: None,
                floor: FLOOR,
                observed: OBSERVED,
                wall: WALL,
                failed: answered_a_bailout(false, 504, false),
            }),
            OBSERVED,
            "a timed-out request keeps the documented clamp, queue wait excluded"
        );
        // A `502` for a view that died mid-request: its own outcome, and the
        // same dispatch→completion charge.
        assert_eq!(
            charge_ns(Charge {
                reported: None,
                floor: FLOOR,
                observed: OBSERVED,
                wall: WALL,
                failed: answered_a_bailout(false, 502, false),
            }),
            OBSERVED,
            "a dead view's 502 is charged dispatch to completion, never the queue wait"
        );
        // And the successful path is untouched: a report is clamped, not billed
        // as the wall.
        assert_eq!(
            charge_ns(Charge {
                reported: Some(1_000_000),
                floor: FLOOR,
                observed: OBSERVED,
                wall: WALL,
                failed: false,
            }),
            FLOOR,
            "an under-reporting 200 is still clamped up to the view's first frame"
        );
    }

    /// `MB.3.45`: a `DONE` may label a completed `5xx` only when the view
    /// answered it.
    ///
    /// The response the scheduler wrote itself — the `504` of a request
    /// timeout, the `502` of a view that died — is already finished when a live
    /// view's `DONE { Bailed }` arrives afterwards; relabelling it `bailed`
    /// would both lie about who answered and let `answered_a_bailout` bill the
    /// whole arrival→finished wall, queue wait included, instead of the span
    /// the request held a worker for.
    #[test]
    fn a_done_cannot_label_a_5xx_the_scheduler_answered() {
        // The view answered a `5xx`: its `DONE` decides whether the handler
        // returned or bailed.
        assert!(a_done_may_relabel_a_completed_5xx(true, false, 500));
        assert!(a_done_may_relabel_a_completed_5xx(true, false, 599));
        // The scheduler wrote the `504`: the view's later `DONE` labels
        // nothing.
        assert!(!a_done_may_relabel_a_completed_5xx(false, false, 504));
        assert!(!a_done_may_relabel_a_completed_5xx(false, false, 502));
        // The label the client saw is final for a cancelled request, and a
        // `2xx`/`4xx` is an answer whatever the view's thread did next.
        assert!(!a_done_may_relabel_a_completed_5xx(true, true, 500));
        assert!(!a_done_may_relabel_a_completed_5xx(true, false, 200));
        assert!(!a_done_may_relabel_a_completed_5xx(true, false, 404));
    }
}
