//! The scheduler itself: one task, one state machine (ARCHITECTURE.md §2.4).
//!
//! [`run`] wires up `http.chan`, `ctl.chan` and one reader/writer pair per
//! `view.chan`, then loops over the four things that can happen: a frame from
//! http, a frame from a view, a message from init, or a timer. Everything that
//! decides what happens next — routing, admission, dispatch, timeouts,
//! retirement — lives in [`Scheduler`] and runs on that one task, so there are
//! no locks anywhere.
//!
//! # Where the decisions differ from §2.4, and why
//!
//! * The `request` event's `path` is the request target without its query
//!   string, as the brief asks, not the *decoded* `PATH_INFO` of §8.1.
//! * `request_timeout` is measured from dispatch, not from arrival: a request
//!   that waited in the queue then gets its full time on a worker thread, and
//!   the wait itself is reported separately in `queued_ms`.
//! * Percent-decoding and `SCRIPT_NAME` failures reuse the `unmatched`
//!   outcome: no view was involved, and §8.1's outcome list is closed.
//! * The health path is matched against the decoded path *before*
//!   `BLASTIT_SCRIPT_NAME` is stripped, so it is the absolute path the
//!   operator configured.
//! * A `RESP_ABORT` from a view is reported with the `view_died` outcome, and
//!   a `DRAIN` rejects new requests with `503`/`queue_full` (the closest of the
//!   seven outcomes, which §8.1 closes).
//! * `process.retired` carries `uid: 0`: the uid table lives in init (§4, §6)
//!   and [`RouteInfo`] has no uid.
//! * Frames from a view that name a request it was never given are counted and
//!   dropped. §8.1's vocabulary has no event for a protocol violation inside
//!   the scheduler, so the count is not reported anywhere yet.
//! * A scheduler-written body is sent even for a `HEAD` request: deciding not
//!   to is http's business, not the scheduler's (§2.3).
//! * Two settings exist that §8.2 does not name, because §2.4 asks for the
//!   behaviour without giving it a variable: [`SchedulerConfig::spawn_backoff`]
//!   ("don't spin") and [`SchedulerConfig::body_buffer_limit`] (the bound on
//!   the body of a queued request).
//!
//! # Backpressure
//!
//! Every channel gets a bounded queue, so a peer that stops reading is a peer
//! that stops being written to. The one case where a request body has to be
//! held is a request waiting in the admission queue: it is buffered up to
//! [`SchedulerConfig::body_buffer_limit`], and once the queued bodies together
//! reach that bound the scheduler stops reading `http.chan` altogether, which
//! pushes back on http and the client. Nothing here buffers without a bound.

use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Duration;

use blastit_events::{
    Component, Event, EventKind, Outcome, ProcessRetired, RequestInfo, RetireReason,
};
use blastit_frame::{Ctl, DoneOutcome, Frame, ReqHead, RespHead};
use blastit_manifest::RoutingTable;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::config::SchedulerConfig;
use crate::ctl::CtlChannel;
use crate::error::SchedulerError;
use crate::path::{path_of_target, percent_decode, strip_script_name};
use crate::route::RouteInfo;
use crate::task::{read_frames, write_frames};

/// How many `http.chan` frames may pile up before http is made to wait.
const HTTP_IN_CAPACITY: usize = 64;
/// How many frames may be queued towards one view before its reader waits.
const VIEW_IN_CAPACITY: usize = 256;
/// How many frames may be queued towards http or a view before we wait.
const OUT_CAPACITY: usize = 64;

/// Frames towards one view process.
type ViewSink = mpsc::Sender<(u32, Frame)>;

/// Where a `request` or `process.retired` event goes.
///
/// init owns stdout (§8.1): the scheduler hands every event to this closure,
/// which writes it to the events pipe.
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

/// A frame from `http.chan`, or its end.
enum HttpIn {
    /// One frame, muxed by the stream id http allocated.
    Frame { id: u32, frame: Frame },
    /// http closed its end of the channel.
    Closed,
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

/// A frame from one `view.chan`, or its end.
enum ViewIn {
    /// One frame, muxed by the request id the scheduler allocated for that view.
    Frame { key: ViewKey, id: u32, frame: Frame },
    /// The view's end of the channel closed (or its process died).
    Closed { key: ViewKey },
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
    /// Frames towards this view. Dropping it ends the writer task and closes
    /// the scheduler's end of `view.chan`.
    out: Option<mpsc::Sender<(u32, Frame)>>,
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
    /// Body chunks held while the request is queued.
    body: Vec<Vec<u8>>,
    /// How many bytes `body` holds, for the backpressure bound.
    body_bytes: usize,
    /// `REQ_END` arrived.
    end_seen: bool,
    /// A `CANCEL` arrived from http.
    cancelled: bool,
    /// The view and request id this request was dispatched under.
    view: Option<(ViewKey, u32)>,
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
}

/// What routing a request target produced.
enum Resolution {
    /// The target cannot be routed: a `400` (§2.4).
    Bad,
    /// The health path, answered by the scheduler itself.
    Health,
    /// Nothing matched: a `404` (§2.4).
    Unrouted,
    /// A route and one of its rules.
    Routed { route: usize, rule: u32 },
}

/// The scheduler's whole state.
struct Scheduler {
    cfg: SchedulerConfig,
    routes: Vec<RouteInfo>,
    table: RoutingTable,
    emit: Emit,
    /// Frames towards http.
    http_out: mpsc::Sender<(u32, Frame)>,
    /// The sender every view reader task clones.
    view_in: mpsc::Sender<ViewIn>,
    started: Instant,
    counter: u64,
    /// Every request that has not been released yet, by http stream id.
    requests: HashMap<u32, Request>,
    /// Requests waiting for a worker slot, oldest first (§2.4's bounded FIFO).
    queue: VecDeque<u32>,
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
    /// Total body bytes buffered for queued requests.
    buffered_bytes: usize,
    /// http is not being read, because those bodies reached the bound.
    http_paused: bool,
    /// A `DRAIN` arrived: no new admission, no new spawns.
    draining: bool,
    /// When draining gives up on the requests that are left.
    drain_deadline: Option<Instant>,
    /// http closed `http.chan`.
    http_closed: bool,
    /// Frames dropped because they named an unknown stream or request.
    dropped: u64,
}

impl Scheduler {
    /// A scheduler with nothing in flight.
    fn new(
        cfg: SchedulerConfig,
        routes: Vec<RouteInfo>,
        table: RoutingTable,
        emit: Emit,
        http_out: mpsc::Sender<(u32, Frame)>,
        view_in: mpsc::Sender<ViewIn>,
    ) -> Self {
        Self {
            cfg,
            routes,
            table,
            emit,
            http_out,
            view_in,
            started: Instant::now(),
            counter: 0,
            requests: HashMap::new(),
            queue: VecDeque::new(),
            views: HashMap::new(),
            dispatch: HashMap::new(),
            slots: 0,
            backoff: HashMap::new(),
            lru: 0,
            buffered_bytes: 0,
            http_paused: false,
            draining: false,
            drain_deadline: None,
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
    async fn start<C: CtlChannel>(&mut self, ctl: &mut C) -> Result<(), SchedulerError> {
        let wanted: Vec<usize> = (0..self.routes.len())
            .filter(|route| self.cfg.eager_spawn || self.routes[*route].prefork)
            .collect();
        for route in wanted {
            self.try_spawn(route, ctl).await?;
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
        for stream_id in &self.queue {
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
        next
    }

    // ---------------------------------------------------------------- events

    /// Everything that is time-based, after any event: queue timeouts, work to
    /// dispatch, request timeouts, and retirements.
    async fn pump<C: CtlChannel>(&mut self, ctl: &mut C) -> Result<(), SchedulerError> {
        let now = Instant::now();
        self.backoff.retain(|_, until| *until > now);
        self.expire_queue().await;
        self.dispatch_queued(ctl).await?;
        self.expire_requests(ctl).await?;
        self.retire_finished_views(ctl).await?;
        Ok(())
    }

    /// Handles one `http.chan` frame.
    async fn on_http(&mut self, event: HttpIn) {
        let (id, frame) = match event {
            HttpIn::Closed => {
                self.http_closed = true;
                return;
            }
            HttpIn::Frame { id, frame } => (id, frame),
        };
        match frame {
            Frame::ReqHead(head) => self.begin(id, head).await,
            Frame::ReqBody(body) => self.request_body(id, body).await,
            Frame::ReqEnd => self.request_end(id).await,
            Frame::Cancel => self.client_cancel(id).await,
            // http never sends a response frame; count it and move on.
            _ => self.dropped += 1,
        }
    }

    /// Handles one `view.chan` frame, or the loss of a view process.
    async fn on_view(&mut self, event: ViewIn) {
        let (key, id, frame) = match event {
            ViewIn::Closed { key } => {
                self.view_gone(key).await;
                return;
            }
            ViewIn::Frame { key, id, frame } => (key, id, frame),
        };
        match frame {
            Frame::Ready => self.view_ready(key),
            Frame::Done { outcome } => self.view_done(key, id, outcome).await,
            Frame::RespHead(head) => self.view_head(key, id, head).await,
            Frame::RespBody(body) => self.view_body(key, id, body).await,
            Frame::RespEnd => self.view_end(key, id).await,
            Frame::RespAbort { reason } => self.view_abort(key, id, &reason).await,
            // A view may only address its own requests: drop anything else.
            _ => self.dropped += 1,
        }
    }

    /// Handles one `ctl.chan` message from init.
    async fn on_ctl(&mut self, message: Ctl, fd: Option<OwnedFd>) -> Result<(), SchedulerError> {
        match message {
            Ctl::Spawned { route, pid } => self.view_spawned(route as usize, pid, fd),
            Ctl::Exited { route, pid, .. } => {
                self.view_gone(ViewKey {
                    route: route as usize,
                    pid,
                })
                .await;
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

    // --------------------------------------------------------------- request

    /// Routes a new request and either dispatches it or queues it (§2.4).
    async fn begin(&mut self, stream_id: u32, head: ReqHead) {
        if self.requests.contains_key(&stream_id) {
            // A stream id is only reused once its response is over; anything
            // else would let one request inherit another's worker slot.
            self.dropped += 1;
            return;
        }
        let request_id = self.next_request_id();
        let mut request = Request {
            stream_id,
            request_id,
            head,
            route: None,
            rule: None,
            arrived: Instant::now(),
            queued_since: None,
            dispatched_at: None,
            deadline: None,
            status: 0,
            outcome: Outcome::Ok,
            bytes_in: 0,
            bytes_out: 0,
            body: Vec::new(),
            body_bytes: 0,
            end_seen: false,
            cancelled: false,
            view: None,
            head_sent: false,
            complete: false,
            emitted: false,
            timed_out: false,
            slots: false,
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
                )
                .await;
            }
            Resolution::Health => {
                self.requests.insert(stream_id, request);
                self.respond(stream_id, 200, "OK", "ok").await;
                self.finish_immediately(stream_id, Outcome::Ok).await;
            }
            Resolution::Unrouted => {
                self.requests.insert(stream_id, request);
                self.reject(
                    stream_id,
                    404,
                    "Not Found",
                    "not found\n",
                    Outcome::Unmatched,
                )
                .await;
            }
            Resolution::Routed { route, rule } => {
                request.route = Some(route);
                request.rule = Some(rule);
                self.requests.insert(stream_id, request);
                if self.draining {
                    self.reject(
                        stream_id,
                        503,
                        "Service Unavailable",
                        "draining\n",
                        Outcome::QueueFull,
                    )
                    .await;
                } else if self.can_dispatch(route) {
                    self.dispatch(stream_id).await;
                } else if self.queue.len() >= self.cfg.queue_size {
                    self.reject(
                        stream_id,
                        503,
                        "Service Unavailable",
                        "queue full\n",
                        Outcome::QueueFull,
                    )
                    .await;
                } else {
                    self.enqueue(stream_id);
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
        let Ok(path) = std::str::from_utf8(&decoded) else {
            return Resolution::Bad;
        };
        if path == self.cfg.health_path {
            return Resolution::Health;
        }
        let path = match self.cfg.script_name.as_deref() {
            Some(prefix) => match strip_script_name(path, prefix) {
                Some(rest) => rest,
                None => return Resolution::Unrouted,
            },
            None => path,
        };
        let Ok(method) = std::str::from_utf8(&request.head.method) else {
            return Resolution::Bad;
        };
        match self.table.route(method, path) {
            Some(matched) => Resolution::Routed {
                route: matched.route,
                rule: u32::try_from(matched.rule).unwrap_or(u32::MAX),
            },
            None => Resolution::Unrouted,
        }
    }

    /// Puts a request at the back of the admission queue.
    fn enqueue(&mut self, stream_id: u32) {
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.queued_since = Some(Instant::now());
        }
        self.queue.push_back(stream_id);
    }

    /// Records request body bytes: forwards them when the request is
    /// dispatched, holds them (bounded) while it waits.
    async fn request_body(&mut self, stream_id: u32, body: Vec<u8>) {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return;
        };
        if request.complete {
            return;
        }
        let view = request.view;
        let count = body.len() as u64;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.bytes_in += count;
        }
        if let Some((key, id)) = view {
            if let Some(out) = self.view_out(key) {
                let _ = out.send((id, Frame::ReqBody(body))).await;
            }
            return;
        }
        let held = body.len();
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.body_bytes += held;
            request.body.push(body);
        }
        self.recount_buffered();
    }

    /// Forwards `REQ_END`, or remembers it for a queued request.
    async fn request_end(&mut self, stream_id: u32) {
        let Some(request) = self.requests.get_mut(&stream_id) else {
            self.dropped += 1;
            return;
        };
        if request.complete {
            return;
        }
        request.end_seen = true;
        let view = request.view;
        if let Some((key, id)) = view
            && let Some(out) = self.view_out(key)
        {
            let _ = out.send((id, Frame::ReqEnd)).await;
        }
    }

    /// The client went away (§3.2): forward `CANCEL` and keep the slot.
    ///
    /// The request reports the status it had reached — `0` when the response
    /// never started — with the `client_gone` outcome (§8.1).
    async fn client_cancel(&mut self, stream_id: u32) {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return;
        };
        if request.complete {
            return;
        }
        let view = request.view;
        if let Some((key, id)) = view
            && let Some(out) = self.view_out(key)
        {
            let _ = out.send((id, Frame::Cancel)).await;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.cancelled = true;
            request.outcome = Outcome::ClientGone;
            request.complete = true;
        }
        self.emit_request(stream_id);
        if view.is_none() {
            // Never dispatched: it leaves the queue, and no slot was ever held.
            self.release(stream_id);
        }
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
                    && view.out.is_some()
                    && view.inflight < self.cfg.view_worker_cap()
            }
            None => false,
        }
    }

    /// Dispatches every queued request it can, spawning views as needed.
    async fn dispatch_queued<C: CtlChannel>(&mut self, ctl: &mut C) -> Result<(), SchedulerError> {
        let queued: Vec<u32> = self.queue.iter().copied().collect();
        for stream_id in queued {
            if !self.queue.contains(&stream_id) {
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
                self.dispatch(stream_id).await;
                continue;
            }
            if !self.views.contains_key(&route) && !self.draining {
                self.try_spawn(route, ctl).await?;
            }
        }
        Ok(())
    }

    /// Hands one request to its view, rewriting its id (§3.1).
    async fn dispatch(&mut self, stream_id: u32) {
        let Some((key, id, out)) = self.reserve(stream_id) else {
            return;
        };
        let now = Instant::now();
        let (head, body, end) = {
            let Some(request) = self.requests.get_mut(&stream_id) else {
                return;
            };
            request.queued_since = None;
            request.dispatched_at = Some(now);
            request.deadline = Some(now + self.cfg.request_timeout);
            request.view = Some((key, id));
            request.slots = true;
            // The body moves to the view, so it no longer counts against the
            // queued-body bound: reading http.chan resumes.
            request.body_bytes = 0;
            (
                request.head.clone(),
                std::mem::take(&mut request.body),
                request.end_seen,
            )
        };
        self.recount_buffered();
        self.queue.retain(|queued_id| *queued_id != stream_id);
        self.dispatch.insert((key, id), stream_id);
        self.slots += 1;

        let mut alive = true;
        if out.send((id, Frame::ReqHead(head))).await.is_err() {
            alive = false;
        }
        if alive {
            for chunk in body {
                if out.send((id, Frame::ReqBody(chunk))).await.is_err() {
                    alive = false;
                    break;
                }
            }
        }
        if alive && end && out.send((id, Frame::ReqEnd)).await.is_err() {
            alive = false;
        }
        if !alive {
            self.view_gone(key).await;
        }
    }

    /// Takes a worker slot and a request id for `stream_id`, if its view can
    /// take one.
    fn reserve(&mut self, stream_id: u32) -> Option<(ViewKey, u32, ViewSink)> {
        let route = self.requests.get(&stream_id)?.route?;
        let cap = self.cfg.view_worker_cap();
        let last_used = self.lru;
        let view = self.views.get_mut(&route)?;
        if !view.ready
            || view.spawning
            || view.stopping
            || view.inflight >= cap
            || view.out.is_none()
        {
            return None;
        }
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

    /// The sender towards a view, if that view is still the live one.
    fn view_out(&self, key: ViewKey) -> Option<ViewSink> {
        self.views
            .get(&key.route)
            .filter(|view| view.key == key)
            .and_then(|view| view.out.clone())
    }

    // ---------------------------------------------------------------- timers

    /// Rejects queued requests whose wait exceeded `BLASTIT_QUEUE_TIMEOUT`.
    async fn expire_queue(&mut self) {
        let now = Instant::now();
        let timeout = self.cfg.queue_timeout;
        let expired: Vec<u32> = self
            .queue
            .iter()
            .copied()
            .filter(|stream_id| {
                self.requests
                    .get(stream_id)
                    .and_then(|request| request.queued_since)
                    .map(|since| now.duration_since(since) >= timeout)
                    .unwrap_or(false)
            })
            .collect();
        for stream_id in expired {
            self.queue.retain(|queued_id| *queued_id != stream_id);
            self.reject(
                stream_id,
                503,
                "Service Unavailable",
                "queue timeout\n",
                Outcome::QueueTimeout,
            )
            .await;
        }
    }

    /// Gives a `504` to every dispatched request past its timeout, tells its
    /// view to cancel the thread, and recycles a view with too many of them
    /// (§2.4).
    async fn expire_requests<C: CtlChannel>(&mut self, ctl: &mut C) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let expired: Vec<(u32, ViewKey, u32)> = self
            .requests
            .values()
            .filter_map(|request| {
                if !request.slots || request.timed_out {
                    return None;
                }
                if now < request.deadline? {
                    return None;
                }
                let (key, id) = request.view?;
                Some((request.stream_id, key, id))
            })
            .collect();
        let mut affected: Vec<ViewKey> = Vec::new();
        for (stream_id, key, id) in expired {
            let head_sent = match self.requests.get(&stream_id) {
                Some(request) => request.head_sent,
                None => continue,
            };
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.timed_out = true;
            }
            if head_sent {
                let _ = self
                    .http_out
                    .send((
                        stream_id,
                        Frame::RespAbort {
                            reason: "request timeout".to_string(),
                        },
                    ))
                    .await;
            } else {
                self.respond(stream_id, 504, "Gateway Timeout", "gateway timeout\n")
                    .await;
            }
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.complete = true;
                request.outcome = Outcome::Timeout;
            }
            self.emit_request(stream_id);
            if let Some(out) = self.view_out(key) {
                let _ = out.send((id, Frame::Cancel)).await;
            }
            affected.push(key);
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
                self.retire(key, RetireReason::StuckThreads, ctl).await?;
            }
        }
        Ok(())
    }

    /// Retires what is finished: a view over `BLASTIT_VIEW_MAX_REQUESTS` with
    /// nothing in flight, or one idle for `BLASTIT_VIEW_IDLE_TIMEOUT` (§2.4).
    async fn retire_finished_views<C: CtlChannel>(
        &mut self,
        ctl: &mut C,
    ) -> Result<(), SchedulerError> {
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
            self.retire(key, reason, ctl).await?;
        }
        Ok(())
    }

    // ------------------------------------------------------------- lifecycle

    /// Asks init for a view process, respecting the LRU cap (§2.4).
    async fn try_spawn<C: CtlChannel>(
        &mut self,
        route: usize,
        ctl: &mut C,
    ) -> Result<(), SchedulerError> {
        if self.views.contains_key(&route) {
            return Ok(());
        }
        if let Some(until) = self.backoff.get(&route).copied() {
            if Instant::now() < until {
                return Ok(());
            }
            self.backoff.remove(&route);
        }
        if let Some(cap) = self.cfg.max_view_processes
            && self.views.len() as u32 >= cap
        {
            // Retire the least recently used idle view to make room; with none
            // idle the request keeps waiting (§2.4).
            let Some(victim) = self.least_recently_used_idle(route) else {
                return Ok(());
            };
            self.retire(victim, RetireReason::LruCap, ctl).await?;
        }
        self.views.insert(route, View::spawning(route));
        let route_id = u32::try_from(route).unwrap_or(u32::MAX);
        if let Err(error) = ctl.send(Ctl::Spawn { route: route_id }).await {
            self.views.remove(&route);
            return Err(error);
        }
        Ok(())
    }

    /// The idle view that was dispatched to longest ago, excluding `route`.
    fn least_recently_used_idle(&self, route: usize) -> Option<ViewKey> {
        self.views
            .values()
            .filter(|view| {
                view.key.route != route && !view.spawning && view.key.pid != 0 && view.inflight == 0
            })
            .min_by_key(|view| view.last_used)
            .map(|view| view.key)
    }

    /// Retires one view process: fail what it owes, ask init to kill it, and
    /// keep its slots until it is gone (§2.4).
    async fn retire<C: CtlChannel>(
        &mut self,
        key: ViewKey,
        reason: RetireReason,
        ctl: &mut C,
    ) -> Result<(), SchedulerError> {
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
            self.fail_request(*stream_id, Outcome::ViewDied, 502).await;
        }
        let Some(view) = self.views.remove(&key.route) else {
            return Ok(());
        };
        // Dropping the sender ends the writer task, which closes the
        // scheduler's end of this view's channel.
        drop(view.out);
        let route_id = u32::try_from(key.route).unwrap_or(u32::MAX);
        ctl.send(Ctl::Retire {
            route: route_id,
            pid: key.pid,
            reason: reason.as_str().to_string(),
        })
        .await?;
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
            // Nothing asked for this process: close our end and ignore it.
            drop(fd);
            return Ok(());
        }
        let std_stream = std::os::unix::net::UnixStream::from(fd);
        std_stream
            .set_nonblocking(true)
            .map_err(SchedulerError::io)?;
        let stream = UnixStream::from_std(std_stream).map_err(SchedulerError::io)?;
        let (read, write) = stream.into_split();
        let key = ViewKey { route, pid };
        let (out, incoming) = mpsc::channel(OUT_CAPACITY);
        let limits = self.cfg.frame_limits;
        tokio::spawn(read_frames(
            read,
            limits,
            self.view_in.clone(),
            move |id, frame| ViewIn::Frame { key, id, frame },
            move || ViewIn::Closed { key },
        ));
        tokio::spawn(write_frames(write, limits, incoming));
        if let Some(view) = self.views.get_mut(&route) {
            view.key = key;
            view.spawning = false;
            view.out = Some(out);
        }
        Ok(())
    }

    /// A view is ready to serve (§4).
    fn view_ready(&mut self, key: ViewKey) {
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
        }
    }

    /// A view process is gone: fail its requests, free their slots, and let
    /// the next request for its route spawn a new one (§2.4).
    async fn view_gone(&mut self, key: ViewKey) {
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
            self.fail_request(stream_id, Outcome::ViewDied, 502).await;
            // The threads died with the process, so their slots are free even
            // though no `DONE` will ever arrive.
            self.release(stream_id);
        }
        if live && !ready {
            // It never served anything: whatever was waiting for it gets a
            // `502`, and the route backs off before it is spawned again.
            let waiting: Vec<u32> = self
                .queue
                .iter()
                .copied()
                .filter(|stream_id| {
                    self.requests
                        .get(stream_id)
                        .and_then(|request| request.route)
                        == Some(key.route)
                })
                .collect();
            for stream_id in waiting {
                self.queue.retain(|queued_id| *queued_id != stream_id);
                self.fail_request(stream_id, Outcome::ViewDied, 502).await;
                self.release(stream_id);
            }
            self.backoff
                .insert(key.route, Instant::now() + self.cfg.spawn_backoff);
        }
    }

    // --------------------------------------------------------------- replies

    /// Sends a small `text/plain` response the scheduler wrote itself.
    async fn respond(&mut self, stream_id: u32, status: u16, reason: &str, body: &str) {
        let head = RespHead {
            status,
            reason: reason.as_bytes().to_vec(),
            headers: vec![
                (
                    b"Content-Type".to_vec(),
                    b"text/plain; charset=utf-8".to_vec(),
                ),
                (
                    b"Content-Length".to_vec(),
                    body.len().to_string().into_bytes(),
                ),
            ],
        };
        let _ = self.http_out.send((stream_id, Frame::RespHead(head))).await;
        if !body.is_empty() {
            let _ = self
                .http_out
                .send((stream_id, Frame::RespBody(body.as_bytes().to_vec())))
                .await;
        }
        let _ = self.http_out.send((stream_id, Frame::RespEnd)).await;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.head_sent = true;
            request.status = status;
            request.bytes_out = body.len() as u64;
        }
    }

    /// Answers a request the scheduler refused, and logs it.
    async fn reject(
        &mut self,
        stream_id: u32,
        status: u16,
        reason: &str,
        body: &str,
        outcome: Outcome,
    ) {
        self.respond(stream_id, status, reason, body).await;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.outcome = outcome;
            request.complete = true;
        }
        self.emit_request(stream_id);
        self.release(stream_id);
    }

    /// Marks an answer the scheduler just wrote as complete, and logs it.
    async fn finish_immediately(&mut self, stream_id: u32, outcome: Outcome) {
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.outcome = outcome;
            request.complete = true;
        }
        self.emit_request(stream_id);
        self.release(stream_id);
    }

    /// Fails a request to http: a `502` when nothing was sent yet, a
    /// `RESP_ABORT` once the head is out.
    async fn fail_request(&mut self, stream_id: u32, outcome: Outcome, status: u16) {
        let (head_sent, complete) = match self.requests.get(&stream_id) {
            Some(request) => (request.head_sent, request.complete),
            None => return,
        };
        if complete {
            return;
        }
        if head_sent {
            let _ = self
                .http_out
                .send((
                    stream_id,
                    Frame::RespAbort {
                        reason: outcome.as_str().to_string(),
                    },
                ))
                .await;
        } else {
            let (reason, body) = canned(status);
            self.respond(stream_id, status, reason, body).await;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = outcome;
        }
        self.emit_request(stream_id);
    }

    /// Releases a request: its worker slot, its view id and its memory.
    fn release(&mut self, stream_id: u32) {
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
        self.queue.retain(|queued_id| *queued_id != stream_id);
        self.recount_buffered();
    }

    /// Recomputes the buffered-body total and the backpressure flag.
    fn recount_buffered(&mut self) {
        self.buffered_bytes = self
            .requests
            .values()
            .map(|request| request.body_bytes)
            .sum();
        self.http_paused =
            self.cfg.body_buffer_limit != 0 && self.buffered_bytes >= self.cfg.body_buffer_limit;
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
        let duration_ms = millis(now.duration_since(request.arrived));
        let mut info = RequestInfo::from_target(
            String::from_utf8_lossy(&request.head.method).into_owned(),
            &String::from_utf8_lossy(&request.head.target),
        )
        .with_status(request.status)
        .with_outcome(request.outcome)
        .with_timings(queued_ms, duration_ms)
        .with_bytes(request.bytes_in, request.bytes_out)
        .with_remote_addr(request.head.remote_addr.clone());
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
    ///
    /// The uid is `0`: the uid table is init's (§4, §6) and the scheduler only
    /// knows route indices. init's own `process.spawned` / `process.exited`
    /// events carry the real one.
    fn emit_retired(&mut self, key: ViewKey, reason: &RetireReason) {
        let mut event = Event::now(
            Component::Scheduler,
            EventKind::ProcessRetired(ProcessRetired::new(key.pid, 0, reason.clone())),
        );
        if let Some(route) = self.routes.get(key.route) {
            event = event.with_route(route.name.clone());
        }
        (self.emit)(event);
    }

    // ----------------------------------------------------------- view frames

    /// `DONE` from a view: the thread exited, so the slot is free (§2.4).
    async fn view_done(&mut self, key: ViewKey, id: u32, outcome: DoneOutcome) {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return;
        };
        let (complete, head_sent, cancelled, status) = match self.requests.get(&stream_id) {
            Some(request) => (
                request.complete,
                request.head_sent,
                request.cancelled,
                request.status,
            ),
            None => return,
        };
        if !complete {
            match outcome {
                DoneOutcome::Completed if head_sent => {
                    // The thread finished; only its `RESP_END` is missing.
                    let _ = self.http_out.send((stream_id, Frame::RespEnd)).await;
                    if let Some(request) = self.requests.get_mut(&stream_id) {
                        request.complete = true;
                        if !request.cancelled {
                            request.outcome = Outcome::Ok;
                        }
                    }
                    self.emit_request(stream_id);
                }
                DoneOutcome::Completed | DoneOutcome::Errored => {
                    self.fail_request(stream_id, Outcome::ViewDied, 502).await;
                }
                DoneOutcome::Aborted => {
                    let outcome = if cancelled {
                        Outcome::ClientGone
                    } else {
                        Outcome::ViewDied
                    };
                    let status = if head_sent { status } else { 502 };
                    self.fail_request(stream_id, outcome, status).await;
                }
            }
        }
        self.release(stream_id);
    }

    /// `RESP_HEAD` from a view.
    async fn view_head(&mut self, key: ViewKey, id: u32, head: RespHead) {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return;
        };
        let forward = match self.requests.get(&stream_id) {
            Some(request) => !request.complete && !request.head_sent,
            None => false,
        };
        if !forward {
            return;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.status = head.status;
            request.head_sent = true;
        }
        let _ = self.http_out.send((stream_id, Frame::RespHead(head))).await;
    }

    /// `RESP_BODY` from a view.
    async fn view_body(&mut self, key: ViewKey, id: u32, body: Vec<u8>) {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return;
        };
        let forward = match self.requests.get(&stream_id) {
            Some(request) => !request.complete && request.head_sent,
            None => false,
        };
        if !forward {
            return;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.bytes_out += body.len() as u64;
        }
        let _ = self.http_out.send((stream_id, Frame::RespBody(body))).await;
    }

    /// `RESP_END` from a view: the response is complete, the slot is not.
    async fn view_end(&mut self, key: ViewKey, id: u32) {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return;
        };
        let forward = match self.requests.get(&stream_id) {
            Some(request) => !request.complete && request.head_sent,
            None => false,
        };
        if !forward {
            return;
        }
        let _ = self.http_out.send((stream_id, Frame::RespEnd)).await;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            if !request.cancelled {
                request.outcome = Outcome::Ok;
            }
        }
        self.emit_request(stream_id);
    }

    /// `RESP_ABORT` from a view: the thread gave up, with or without a head.
    ///
    /// §8.1's outcomes have no "the app raised", so this is reported as
    /// `view_died`, the same as a process that went away mid-response.
    async fn view_abort(&mut self, key: ViewKey, id: u32, reason: &str) {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return;
        };
        let head_sent = match self.requests.get(&stream_id) {
            Some(request) if !request.complete => request.head_sent,
            _ => return,
        };
        if head_sent {
            let _ = self
                .http_out
                .send((
                    stream_id,
                    Frame::RespAbort {
                        reason: reason.to_string(),
                    },
                ))
                .await;
        } else {
            self.respond(stream_id, 502, "Bad Gateway", "bad gateway\n")
                .await;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = Outcome::ViewDied;
        }
        self.emit_request(stream_id);
    }
}

/// Milliseconds as a float, for `request` events.
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
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

/// Serves `http.chan` from `ctl.chan` and one `view.chan` per route.
///
/// Returns when init sends `DRAIN` (once the in-flight requests are done, or
/// `BLASTIT_REQUEST_TIMEOUT` after the message) or when `http.chan` closes. It
/// fails only when `ctl.chan` fails: a view that dies, a client that
/// disappears and a queue that overflows are all ordinary answers (§2.4).
///
/// # Errors
///
/// [`SchedulerError`] for a broken `ctl.chan`, or a message on it that the
/// protocol does not allow.
pub async fn run<C: CtlChannel>(
    cfg: SchedulerConfig,
    routes: Vec<RouteInfo>,
    table: RoutingTable,
    http: UnixStream,
    mut ctl: C,
    emit: Emit,
) -> Result<(), SchedulerError> {
    let limits = cfg.frame_limits;
    let (http_read, http_write) = http.into_split();
    let (http_in_tx, mut http_in_rx) = mpsc::channel(HTTP_IN_CAPACITY);
    let (http_out_tx, http_out_rx) = mpsc::channel(OUT_CAPACITY);
    let (view_in_tx, mut view_in_rx) = mpsc::channel(VIEW_IN_CAPACITY);

    tokio::spawn(read_frames(
        http_read,
        limits,
        http_in_tx.clone(),
        |id, frame| HttpIn::Frame { id, frame },
        || HttpIn::Closed,
    ));
    tokio::spawn(write_frames(http_write, limits, http_out_rx));

    // Held for the whole run, so `http_in_rx` only ever ends with an explicit
    // `HttpIn::Closed`.
    let _keepalive = http_in_tx;
    let mut scheduler = Scheduler::new(cfg, routes, table, emit, http_out_tx, view_in_tx);
    scheduler.start(&mut ctl).await?;

    loop {
        let deadline = scheduler.next_deadline();
        let paused = scheduler.http_paused;
        tokio::select! {
            biased;
            message = ctl.recv() => {
                let (message, fd) = message?;
                scheduler.on_ctl(message, fd).await?;
            }
            Some(event) = http_in_rx.recv(), if !paused => scheduler.on_http(event).await,
            Some(event) = view_in_rx.recv() => scheduler.on_view(event).await,
            () = sleep_until(deadline) => {}
        }
        scheduler.pump(&mut ctl).await?;
        if scheduler.done() {
            return Ok(());
        }
    }
}
