//! Admission: the queue, the worker budget and the per-view bookkeeping.

use super::response::retry_after_seconds;
use super::*;

/// One live view process of one route.
///
/// A route holds a pool of these ([`ViewSet`]): the pid init reported is the
/// identity, which is also how a replaced instance is told from its successor.
#[derive(Debug)]
pub(super) struct View {
    pub(super) key: ViewKey,
    /// The view sent `READY`; only then may it be dispatched to (§4).
    pub(super) ready: bool,
    /// `BLASTIT_VIEW_MAX_REQUESTS` is reached: dispatch nothing more to it.
    pub(super) stopping: bool,
    /// Request threads currently alive in this process (§2.4: the budget counts
    /// threads, and a thread lives until `DONE`).
    pub(super) inflight: u32,
    /// Requests ever dispatched to this process.
    pub(super) sent: u64,
    /// Next request id to hand out; ids are unique among in-flight requests.
    pub(super) next_request_id: u32,
    /// When the last in-flight request finished, for idle retirement.
    pub(super) idle_since: Option<Instant>,
    /// Last time this view was dispatched to, for the LRU cap.
    pub(super) last_used: u64,
    /// Frames towards this view, with its own writer task behind them.
    pub(super) out: Option<Outbound>,
    /// When init reported the process gone and the ordered `view.chan` has not
    /// reached [`ViewIn::Closed`] yet.
    ///
    /// The scheduler learns a process exited on `ctl.chan`, a different input
    /// from the frames the process itself wrote, so the two can arrive in
    /// either order. The frames are the answer: a view that writes
    /// `RESP_HEAD`/`RESP_END_COST`/`DONE` and then retires **by design** (a
    /// contained bailout, `blastit-view-php`) must be read before its requests are
    /// failed. Nothing new is dispatched to an exited view, and
    /// [`Scheduler::expire_exited_views`] bounds the wait.
    pub(super) exited: Option<Instant>,
    /// When the worker answered a request the scheduler still tracked with
    /// `DONE { Bailed }`, and the ordered `view.chan` has not closed yet.
    ///
    /// `Bailed` is the handler never returning — a contained `exit()`/`die()`/
    /// fatal — and the runtime retires the process after it (the PHP worker's
    /// `recycle`, `crates/blastit-view-php/src/worker.rs`). So the
    /// worker has just said it cannot serve another request, one `ctl.chan`
    /// report before [`View::exited`] arrives: without this, the pump between
    /// `DONE` and the close dispatches the queued head into a process that has
    /// already answered and is going away, and the close then bills that request
    /// `view_died`. Nothing new is dispatched to a retiring view; the close (or
    /// [`Scheduler::expire_exited_views`]) removes it and the route grows a
    /// replacement for the queue.
    pub(super) retiring: Option<Instant>,
    /// A reload arrived while this worker was busy: nothing new reaches it
    /// (`View::admits`), and [`Scheduler::retire_finished_views`] retires it
    /// with this reason as soon as its last request is `DONE` (§4.6).
    pub(super) stale: Option<RetireReason>,
    /// The `reload_epoch` current when this worker's `SPAWN` was sent: one
    /// whose birth predates the last reload holds the old code and is retired
    /// before it ever serves (§4.6).
    pub(super) spawned_epoch: u64,
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
    pub(super) fn admits(&self, per_process_cap: u32) -> bool {
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
    pub(super) fn leaving_at(&self) -> Option<Instant> {
        match (self.exited, self.retiring) {
            (Some(exited), Some(retiring)) => Some(exited.min(retiring)),
            (exited, retiring) => exited.or(retiring),
        }
    }

    /// The next free request id for this view.
    pub(super) fn allocate_request_id(&mut self) -> u32 {
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
pub(super) struct ViewSet {
    /// One entry per worker init reported, by the pid it gave.
    pub(super) views: BTreeMap<u32, View>,
    /// The route's one outstanding `SPAWN`: the [`Scheduler::reload_epoch`] it
    /// was sent in, and when it stops being believable (§4.3's
    /// [`Scheduler::expire_spawns`]).
    pub(super) spawning: Option<(u64, Instant)>,
    /// Workers forked but not yet `READY`: they already hold a heap copy, which
    /// is why growth waits for them before the first memory measurement.
    pub(super) pending_ready: u32,
}

impl ViewSet {
    /// Workers plus the fork already promised; a `SPAWNED` worker is already in
    /// `views`, so nothing is counted twice.
    pub(super) fn len(&self) -> u32 {
        u32::try_from(self.views.len())
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(self.spawning.is_some()))
    }

    /// Whether this route holds no worker and owes no fork.
    pub(super) fn is_empty(&self) -> bool {
        self.views.is_empty() && self.spawning.is_none()
    }
}

/// The admission queue: plain FIFO (§2.4), or the fair queue (D5).
///
/// Both hold the stream ids of the requests that are waiting for a worker
/// slot; only the order they hand them back differs.
pub(super) enum Admission {
    /// §2.4's bounded FIFO, in arrival order.
    Fifo(VecDeque<u32>),
    /// The two-level fair queue, in budget order.
    Fair(Box<FairQueue>),
}

impl Admission {
    /// The queue the configuration asks for.
    pub(super) fn new(fair_queuing: bool, cfg: crate::config::FairConfig) -> Self {
        if fair_queuing {
            Self::Fair(Box::new(FairQueue::new(cfg)))
        } else {
            Self::Fifo(VecDeque::new())
        }
    }

    /// The fair queue, when the configuration asked for one.
    pub(super) fn fair(&self) -> Option<&FairQueue> {
        match self {
            Self::Fifo(_) => None,
            Self::Fair(queue) => Some(queue),
        }
    }

    /// The fair queue, when the configuration asked for one.
    pub(super) fn fair_mut(&mut self) -> Option<&mut FairQueue> {
        match self {
            Self::Fifo(_) => None,
            Self::Fair(queue) => Some(queue),
        }
    }

    /// Whether fair queuing is on.
    pub(super) fn is_fair(&self) -> bool {
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
    pub(super) fn resolve_view(
        &mut self,
        peer: &str,
        head: &ReqHeadView<'_>,
        now: Instant,
    ) -> Option<FlowRef> {
        self.fair_mut()
            .map(|queue| queue.resolve_view(peer, head, now.into_std()))
    }

    /// Queue a request that is waiting for `route`.
    pub(super) fn push(&mut self, stream_id: u32, route: u32, flow: Option<FlowRef>, now: Instant) {
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
    pub(super) fn remove(&mut self, stream_id: u32) {
        match self {
            Self::Fifo(queue) => queue.retain(|queued| *queued != stream_id),
            Self::Fair(queue) => queue.remove(stream_id),
        }
    }

    /// Whether a request is still waiting.
    pub(super) fn contains(&self, stream_id: u32) -> bool {
        match self {
            Self::Fifo(queue) => queue.contains(&stream_id),
            Self::Fair(queue) => queue.contains(stream_id),
        }
    }

    /// How many requests are waiting.
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Fifo(queue) => queue.len(),
            Self::Fair(queue) => queue.len(),
        }
    }

    /// Whether nothing is waiting.
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every waiting request, in no particular order.
    ///
    /// The queue timeouts and the shutdown check walk all of them, so the
    /// order they come back in does not matter.
    pub(super) fn ids(&self) -> Vec<u32> {
        match self {
            Self::Fifo(queue) => queue.iter().copied().collect(),
            Self::Fair(queue) => queue.ids(),
        }
    }

    /// Every route that has something waiting (D5: empty for the FIFO, whose
    /// walk reads the routes off the requests themselves).
    pub(super) fn routes(&self) -> Vec<u32> {
        match self {
            Self::Fifo(_) => Vec::new(),
            Self::Fair(queue) => queue.routes(),
        }
    }

    /// The request that goes next for `route`, and the flow it belongs to
    /// (D5).
    pub(super) fn peek(&self, route: u32) -> Option<NextFlow> {
        self.fair()?.peek(route)
    }

    /// Take the next request for `route` (D5).
    pub(super) fn pop(&mut self, route: u32) -> Option<u32> {
        self.fair_mut()?.pop(route)
    }

    /// Charge a flow for a request (D5).
    pub(super) fn debit(&mut self, flow: &FlowRef, charged_ns: u64, now: Instant) {
        if let Some(queue) = self.fair_mut() {
            queue.debit(flow, Cost(charged_ns), now.into_std());
        }
    }

    /// Let a flow go once the request that held it is over (D5).
    pub(super) fn release_flow(&mut self, flow: FlowRef, now: Instant) {
        if let Some(queue) = self.fair_mut() {
            queue.release(flow, now.into_std());
        }
    }

    /// Collect the flows that may be collected now (D5).
    pub(super) fn gc(&mut self, now: Instant) {
        if let Some(queue) = self.fair_mut() {
            queue.gc(now.into_std());
        }
    }
}
impl Scheduler {
    /// Hands a request whose head/body is ready to the admission machinery.
    ///
    /// A request to a route with `[route.static]` is admitted by the slot alone
    /// (design §4.6): its static branch comes before `can_dispatch`, because no
    /// view exists to be asked.
    pub(super) fn admit_ready(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
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
    pub(super) fn admit_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
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
    pub(super) fn can_dispatch_static(&self) -> bool {
        self.slots < self.cfg.max_workers
    }

    /// Whether `stream_id` has matched a mount and not resolved yet.
    pub(super) fn static_pending(&self, stream_id: u32) -> bool {
        self.requests
            .get(&stream_id)
            .is_some_and(|request| request.static_pending)
    }

    /// Whether a static read still owns `stream_id`.
    ///
    /// A `pread` cannot be cancelled, so a timed-out or cancelled request keeps
    /// its slot and its descriptor until the read returns: the result releases
    /// both, and never the timeout (design §4.6).
    pub(super) fn static_read_parked(&self, stream_id: u32) -> bool {
        self.requests
            .get(&stream_id)
            .is_some_and(|request| request.pumping)
    }

    /// The first queued request that still has to resolve against a mount's
    /// root.
    ///
    /// Only used by the fair walk, whose ordering is a view-budget decision and
    /// has nothing to say about a request that never touches a view.
    pub(super) fn first_static_queued(&self) -> Option<u32> {
        self.queue.ids().into_iter().find(|stream_id| {
            self.requests
                .get(stream_id)
                .is_some_and(|request| request.static_pending && request.static_parts.is_some())
        })
    }

    /// How many requests are still collecting a structured body.
    pub(super) fn buffering_count(&self) -> usize {
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
    pub(super) fn awaiting_end_count(&self) -> usize {
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
}
impl Scheduler {
    /// The per-process request ceiling in force for `route`: boot data, never a
    /// runtime question (§4.1).
    ///
    /// `None` on the route means `BLASTIT_VIEW_MAX_WORKERS`, and the result is
    /// clamped into `1..=that` so no route can be configured to admit nothing
    /// or more than one process may run.
    pub(super) fn per_process_cap(&self, route: usize) -> u32 {
        let ceiling = self.cfg.view_worker_cap().max(1);
        self.routes
            .get(route)
            .and_then(|info| info.per_process_cap)
            .map_or(ceiling, |cap| cap.clamp(1, ceiling))
    }

    /// Whether `route` may own more than one process: boot data (`ADR-0015`
    /// decision 1).
    pub(super) fn process_pool(&self, route: usize) -> bool {
        self.routes.get(route).is_some_and(|info| info.process_pool)
    }

    /// How many processes `route` may own.
    ///
    /// A pooled (request-per-process) route may own as many as the budget, which
    /// is what the accepted DX promises a route; a threaded route owns exactly
    /// one, because a second copy of the preloaded heap would buy no concurrency
    /// its threads do not already give. Never inferred from a value: with
    /// `BLASTIT_VIEW_MAX_WORKERS=1` a threaded route still owns one process.
    pub(super) fn process_limit(&self, route: usize) -> u32 {
        if self.process_pool(route) {
            self.cfg.max_workers.max(1)
        } else {
            1
        }
    }

    /// The processes `route`'s requests need at its per-process ceiling (§4.3).
    pub(super) fn needed_processes(&self, route: usize) -> u32 {
        self.route_requests(route)
            .div_ceil(self.per_process_cap(route))
    }

    /// Requests `route` has now: in flight plus waiting in the admission queue.
    pub(super) fn route_requests(&self, route: usize) -> u32 {
        let inflight = self.pools.get(&route).map_or(0u32, |pool| {
            pool.views
                .values()
                .map(|view| view.inflight)
                .fold(0u32, u32::saturating_add)
        });
        inflight.saturating_add(u32::try_from(self.queued_for(route)).unwrap_or(u32::MAX))
    }

    /// How many requests are waiting in the admission queue for `route`.
    pub(super) fn queued_for(&self, route: usize) -> usize {
        self.queued_for_streams(route).count()
    }

    /// The admission-queue stream ids routed to `route`.
    ///
    /// [`Admission::routes`] is empty for the FIFO queue, so this reads the
    /// routes off the requests — the same walk [`Scheduler::route_waiting`]
    /// uses, which is why it works for both queue modes.
    pub(super) fn queued_for_streams(&self, route: usize) -> impl Iterator<Item = u32> + '_ {
        self.queue.ids().into_iter().filter(move |stream_id| {
            self.requests
                .get(stream_id)
                .and_then(|request| request.route)
                == Some(route)
        })
    }

    /// Every route with at least one request in the admission queue, in index
    /// order and without repeats.
    pub(super) fn queued_routes(&self) -> Vec<usize> {
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
    pub(super) fn other_route_starved(&self, route: usize) -> bool {
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
    pub(super) fn live_processes(&self) -> u32 {
        self.pools
            .values()
            .map(ViewSet::len)
            .fold(0u32, u32::saturating_add)
    }

    /// The live worker named by `key`, if the pool still holds it.
    pub(super) fn view_mut(&mut self, key: ViewKey) -> Option<&mut View> {
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
    pub(super) fn forget_if_idle(&mut self, route: usize) {
        if self.pools.get(&route).is_some_and(ViewSet::is_empty) {
            self.pools.remove(&route);
        }
    }

    /// Emits one `manifest.warning` for the scheduler's own bookkeeping.
    ///
    /// The sentence names a route index or a fixed reason and never a request,
    /// a header or a body value.
    pub(super) fn emit_warning(&mut self, message: String) {
        (self.emit)(Event::now(
            Component::Scheduler,
            EventKind::ManifestWarning(ManifestWarning::new(message)),
        ));
    }

    // -------------------------------------------------------------- dispatch

    /// Whether a route can take a request right now.
    pub(super) fn can_dispatch(&self, route: usize) -> bool {
        self.slots < self.cfg.max_workers && self.has_dispatchable(route)
    }

    /// Whether some worker of `route` admits a request now (§4.2).
    pub(super) fn has_dispatchable(&self, route: usize) -> bool {
        let cap = self.per_process_cap(route);
        self.pools
            .get(&route)
            .is_some_and(|pool| pool.views.values().any(|view| view.admits(cap)))
    }
}
impl Scheduler {
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
    pub(super) fn dispatch_queued(&mut self) -> Result<(), SchedulerError> {
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
    pub(super) fn dispatch_queued_fair(&mut self) -> Result<(), SchedulerError> {
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
    pub(super) fn requeue(&mut self, stream_id: u32) {
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
    pub(super) fn dispatch(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
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
    pub(super) fn reserve(&mut self, stream_id: u32) -> Option<(ViewKey, u32, Outbound)> {
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
    pub(super) fn view_out(&self, key: ViewKey) -> Option<Outbound> {
        self.pools
            .get(&key.route)
            .and_then(|pool| pool.views.get(&key.pid))
            .filter(|view| view.key == key)
            .and_then(|view| view.out.clone())
    }

    // ---------------------------------------------------------------- static
}
