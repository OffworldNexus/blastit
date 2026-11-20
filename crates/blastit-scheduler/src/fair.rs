//! Fair queuing between clients (ARCHITECTURE.md §2.4, D5).
//!
//! A port of the reference implementation's two-level fair scheduler, adapted
//! to blastit's admission queue. The problem it solves is the same one
//! §2.4's plain `BLASTIT_QUEUE_SIZE` FIFO has: with one queue, requests are
//! served in arrival order, so one client that sends a burst — or one client
//! whose requests are expensive — fills the worker budget and everyone else
//! waits behind it.
//!
//! # Two levels, one virtual clock
//!
//! Every waiting request belongs to a **flow**:
//!
//! * an **outer** flow, keyed by the client's IP prefix
//!   (`BLASTIT_FQ_OUTER_PREFIX_V4`, `BLASTIT_FQ_OUTER_PREFIX_V6`): one abusive
//!   network cannot starve another;
//! * an **inner** flow, keyed by a hash of the client address and of the
//!   headers and cookies the operator listed (`BLASTIT_FQ_INNER_HEADERS`,
//!   `BLASTIT_FQ_INNER_COOKIES`, `BLASTIT_FQ_INNER_INCLUDE_IP`): one browser
//!   behind a NAT cannot starve its neighbours.
//!
//! Both levels keep a virtual time `used`, and both are dispatched
//! lowest-first: of the flows with work, the one that has used the least goes
//! first, and within an outer flow the inner flow that has used the least goes
//! first. `used` is not a timestamp but a budget: a flow that becomes
//! backlogged after being quiet restarts at what its level has reached less
//! one `BLASTIT_FQ_BURST`, so a new client may spend one burst before it starts
//! queueing behind the others, and every request then charges what it cost, up
//! to one `BLASTIT_FQ_MAX_DEBT` ahead of the clock, so no flow can borrow
//! unboundedly against the future.
//!
//! `v` is the queue's own virtual time: it only moves forward, to the `used` of
//! the flow being dispatched. A flow whose `used` has fallen behind `v` — it
//! was idle while the queue moved on — is collected as soon as it has nothing
//! left to serve, as is one that has been idle for `BLASTIT_FQ_DEBT_TTL`:
//! without that, the tables would hold every client that ever connected.
//!
//! # One clock
//!
//! Every method that needs the time of day takes it as an argument, and the
//! scheduler converts its own clock once per event (`Instant::now()` on tokio's
//! clock, `.into_std()`). Nothing in here reads a clock of its own, so the
//! instant a flow is stamped with and the instant its idleness is measured
//! against are always the same clock — the paused one a test drives, or the
//! wall clock in production.
//!
//! # Bounded tables
//!
//! Both levels are bounded (`BLASTIT_FQ_OUTER_MAX`,
//! `BLASTIT_FQ_INNER_MAX_PER_OUTER`, `BLASTIT_FQ_INNER_MAX`). An outer flow
//! that does not fit shares one **overflow** flow with every other one that
//! does not fit, and so does an inner flow: the bound is a bound, and the
//! overflow flow is what it costs. A request whose client address cannot be
//! read at all (`peer_addr` was not an address) goes to the overflow flow too,
//! because "we cannot tell who this is" is the same statement as "we cannot
//! keep this apart from the others".
//!
//! # Privacy
//!
//! The inner key is a hash of header values and cookies: personal data and
//! secrets, which §8.1 forbids logging. So the hash is never logged either —
//! [`FlowRef`]'s `Debug` prints the IP prefix and hides the key — and the
//! hasher is seeded per process ([`std::collections::hash_map::RandomState`]),
//! so the keys cannot be precomputed from outside.
//!
//! # Deviations from the reference implementation
//!
//! * Its two fixed lanes (events, loop steps) become one lane per **route**.
//!   blastit's admission queue only ever serves the route a queued request
//!   belongs to, and a route whose view is not ready must not hold up the
//!   routes whose views are.
//! * It orders within one lane; here the best flow is also compared **across**
//!   routes, because every route competes for one worker budget.
//! * Its cost is the loop time it measures itself, charged while the request
//!   runs; blastit's comes from the view process (D5), is clamped by the
//!   scheduler, and is charged when the request ends.

use std::collections::hash_map::RandomState;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::net::IpAddr;
use std::time::Instant;

use blastit_frame::{ReqHeadView, values_of};

use crate::config::FairConfig;

/// Identifies the flow a request belongs to: one client fingerprint (inner)
/// inside one IP prefix (outer).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct FlowRef {
    /// The IP prefix every request of this flow shares.
    pub(crate) outer: OuterKey,
    /// The hash of the client address and the configured headers and cookies.
    pub(crate) inner: u64,
}

impl fmt::Debug for FlowRef {
    /// Prints the IP prefix and **not** the inner key: that key is a hash of
    /// header and cookie values, and a hash of a secret is still a secret's
    /// shadow (§8.1: header values are never logged).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlowRef")
            .field("outer", &self.outer)
            .field("inner", &"<hidden>")
            .finish()
    }
}

/// An IP prefix. An IPv4 address is stored in the low 32 bits of `masked`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct OuterKey {
    /// 4 or 6. `0` marks the overflow flow, which is not a prefix at all.
    pub(crate) family: u8,
    /// The address with everything outside the prefix cleared.
    pub(crate) masked: u128,
}

/// The one flow every outer flow that does not fit shares.
const OVERFLOW_OUTER: OuterKey = OuterKey {
    family: 0,
    masked: 0,
};
/// The one inner flow that every inner flow that does not fit shares.
const INNER_OVERFLOW: u64 = u64::MAX;

/// What one request cost the view process that served it, in nanoseconds.
///
/// The scheduler passes what it will charge, already clamped to the time it
/// observed itself: a view process is sandboxed and may be compromised, so its
/// number is untrusted (§2.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cost(pub(crate) u64);

/// The requests of one route, in arrival order.
type RouteQueue = VecDeque<u32>;
/// The inner flows with work for one route, ordered by their virtual time.
type InnerBacklog = BTreeSet<(u64, u64)>;
/// The outer flows with work for one route, ordered by their virtual time.
type OuterBacklog = BTreeSet<(u64, OuterKey)>;

/// One client fingerprint inside one outer flow.
struct InnerFlow {
    /// Virtual time: what this flow has been charged.
    used: u64,
    /// Requests of this flow that are being served right now.
    in_flight: u32,
    /// One queue per route this flow has work for.
    queues: HashMap<u32, RouteQueue>,
    /// When this flow last had a request admitted or charged.
    last_active: Instant,
}

impl InnerFlow {
    /// A flow that has used nothing yet, with one burst of credit.
    fn new(v: u64, burst: u64) -> Self {
        Self {
            used: v.saturating_sub(burst),
            in_flight: 0,
            queues: HashMap::new(),
            last_active: Instant::now(),
        }
    }

    /// Whether this flow has nothing queued.
    fn idle(&self) -> bool {
        self.queues.values().all(RouteQueue::is_empty)
    }

    /// Whether this flow has work for `route`.
    fn has_work(&self, route: u32) -> bool {
        self.queues
            .get(&route)
            .is_some_and(|queue| !queue.is_empty())
    }
}

/// One IP prefix.
struct OuterFlow {
    /// Virtual time: what this flow has been charged.
    used: u64,
    /// The virtual time its inner flows are measured against.
    v: u64,
    /// Requests of this flow that are being served right now.
    in_flight: u32,
    /// The inner flows that exist, bounded by the configuration.
    inners: HashMap<u64, InnerFlow>,
    /// Per route, the inner flows with work, ordered by `used`.
    backlog: HashMap<u32, InnerBacklog>,
    /// When this flow last had a request admitted or charged.
    last_active: Instant,
}

impl OuterFlow {
    /// A flow that has used nothing yet, with one burst of credit.
    fn new(v: u64, burst: u64) -> Self {
        Self {
            used: v.saturating_sub(burst),
            v,
            in_flight: 0,
            inners: HashMap::new(),
            backlog: HashMap::new(),
            last_active: Instant::now(),
        }
    }

    /// Whether any route of this flow has something queued.
    fn backlogged(&self) -> bool {
        !self.backlog.is_empty()
    }

    /// Make this outer flow's backlog for `route` match whether `inner` has
    /// work for it, and report whether the outer flow has any work there.
    ///
    /// Every mutation of a flow's queues ends in a call to this, which is what
    /// keeps the ordered sets exactly as large as the work they stand for.
    fn touch(&mut self, route: u32, inner: u64) -> bool {
        let has_work = self.inners.get(&inner).is_some_and(|f| f.has_work(route));
        let used = self.inners.get(&inner).map_or(0, |f| f.used);
        let mut emptied = false;
        match self.backlog.get_mut(&route) {
            Some(set) => {
                set.retain(|(_, key)| *key != inner);
                if has_work {
                    set.insert((used, inner));
                }
                emptied = set.is_empty();
            }
            None => {
                if has_work {
                    self.backlog.entry(route).or_default().insert((used, inner));
                }
            }
        }
        if emptied {
            self.backlog.remove(&route);
        }
        self.backlog.contains_key(&route)
    }
}

/// Make the root backlog for `route` match whether `outer` has work for it.
fn touch_root(
    root: &mut HashMap<u32, OuterBacklog>,
    route: u32,
    outer: OuterKey,
    has_work: bool,
    used: u64,
) {
    let mut emptied = false;
    match root.get_mut(&route) {
        Some(set) => {
            set.retain(|(_, key)| *key != outer);
            if has_work {
                set.insert((used, outer));
            }
            emptied = set.is_empty();
        }
        None => {
            if has_work {
                root.entry(route).or_default().insert((used, outer));
            }
        }
    }
    if emptied {
        root.remove(&route);
    }
}

/// What one queued request is waiting as.
struct Entry {
    /// The route it must be dispatched to.
    route: u32,
    /// Its flow.
    flow: FlowRef,
}

/// The request that goes next for one route, and what its flow has spent.
///
/// Two `used` numbers rather than one, because the order is two-level: a
/// caller deciding between routes compares `outer` first and `inner` second,
/// which is exactly what the route's own backlog does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NextFlow {
    /// The outer flow's virtual time.
    pub(crate) outer: u64,
    /// The inner flow's virtual time.
    pub(crate) inner: u64,
    /// The request itself.
    pub(crate) stream_id: u32,
}

/// The two-level fair queue (see the module documentation).
///
/// A `FairQueue` only decides *order*: which of the queued requests goes next.
/// Admission itself — the worker budget, the per-view cap, the queue size — is
/// the scheduler's, unchanged.
pub(crate) struct FairQueue {
    /// The queue's virtual time: the largest `used` it has dispatched.
    v: u64,
    /// The outer flows, bounded by `BLASTIT_FQ_OUTER_MAX`.
    outers: HashMap<OuterKey, OuterFlow>,
    /// Per route, the outer flows that have something to serve, by `used`.
    root_backlog: HashMap<u32, OuterBacklog>,
    /// Every waiting request, and what it is waiting for.
    entries: HashMap<u32, Entry>,
    /// The overflow outer flow, once one has been handed out.
    overflow_outer: Option<OuterKey>,
    /// How many inner flows exist, across every outer flow.
    inner_count: usize,
    cfg: FairConfig,
    /// Per-process random: the inner keys cannot be precomputed outside.
    hasher: RandomState,
}

impl FairQueue {
    /// An empty queue with this configuration.
    pub(crate) fn new(cfg: FairConfig) -> Self {
        Self {
            v: 0,
            outers: HashMap::new(),
            root_backlog: HashMap::new(),
            entries: HashMap::new(),
            overflow_outer: None,
            inner_count: 0,
            cfg,
            hasher: RandomState::new(),
        }
    }

    /// How many requests are waiting.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether `stream_id` is waiting.
    pub(crate) fn contains(&self, stream_id: u32) -> bool {
        self.entries.contains_key(&stream_id)
    }

    /// Whether any request of `route` is waiting.
    ///
    /// The LRU view cap asks this before retiring a process (§8 Sizing): a view
    /// that would answer a queued request is not a replacement's victim.
    pub(crate) fn waiting_for_route(&self, route: u32) -> bool {
        self.entries.values().any(|entry| entry.route == route)
    }

    /// Every route that has something waiting.
    pub(crate) fn routes(&self) -> Vec<u32> {
        self.entries
            .values()
            .map(|entry| entry.route)
            .collect::<BTreeSet<u32>>()
            .into_iter()
            .collect()
    }

    /// Every waiting request, in no particular order: queue timeouts and
    /// shutdown look at the whole queue.
    pub(crate) fn ids(&self) -> Vec<u32> {
        self.entries.keys().copied().collect()
    }

    /// The flow of the request that goes next for `route` — the two-level
    /// "lowest `used` first" choice.
    ///
    /// Both levels are reported, because both are what the order is made of:
    /// the outer flow's `used` is the client's budget, the inner flow's is the
    /// one behind NAT, and the route's own backlog compares them in that order
    /// (`OuterBacklog`, then `InnerBacklog`), so a caller comparing candidates
    /// across routes uses the same key.
    pub(crate) fn peek(&self, route: u32) -> Option<NextFlow> {
        let (outer_used, outer_key) = *self.root_backlog.get(&route)?.iter().next()?;
        let outer = self.outers.get(&outer_key)?;
        let (inner_used, inner_key) = *outer.backlog.get(&route)?.iter().next()?;
        let stream_id = *outer.inners.get(&inner_key)?.queues.get(&route)?.front()?;
        Some(NextFlow {
            outer: outer_used,
            inner: inner_used,
            stream_id,
        })
    }

    /// Take the next request for `route`, and move the queue's clock to it.
    pub(crate) fn pop(&mut self, route: u32) -> Option<u32> {
        let (outer_used, outer_key) = *self.root_backlog.get(&route)?.iter().next()?;
        self.v = self.v.max(outer_used);
        let outer = self.outers.get_mut(&outer_key)?;
        let (inner_used, inner_key) = *outer.backlog.get(&route)?.iter().next()?;
        outer.v = outer.v.max(inner_used);

        let stream_id = outer
            .inners
            .get_mut(&inner_key)?
            .queues
            .get_mut(&route)?
            .pop_front()?;
        let has_work = outer.touch(route, inner_key);
        let used = outer.used;
        touch_root(&mut self.root_backlog, route, outer_key, has_work, used);
        self.entries.remove(&stream_id);
        Some(stream_id)
    }

    /// Queue `stream_id`, which is waiting for `route` in `flow`.
    ///
    /// `now` is the caller's clock, the same one [`FairQueue::debit`],
    /// [`FairQueue::release`] and [`FairQueue::gc`] are given: what is stamped
    /// here is what those rules compare against.
    pub(crate) fn push(&mut self, stream_id: u32, route: u32, flow: FlowRef, now: Instant) {
        let FlowRef { outer, inner } = flow;
        let v = self.v;
        let burst = self.cfg.burst_ns();

        let outer_flow = self
            .outers
            .entry(outer)
            .or_insert_with(|| OuterFlow::new(v, burst));
        outer_flow.last_active = now;
        let outer_backlogged = outer_flow.backlog.contains_key(&route);
        let outer_v = outer_flow.v;

        // The inner flow's own clock catches up with its outer's when it has
        // no work for this route: a client that comes back after being quiet
        // starts from where the queue is, less one burst.
        if let Some(inner_flow) = outer_flow.inners.get_mut(&inner)
            && !inner_flow.has_work(route)
        {
            inner_flow.used = inner_flow.used.max(outer_v.saturating_sub(burst));
        }
        let inner_flow = outer_flow
            .inners
            .entry(inner)
            .or_insert_with(|| InnerFlow::new(outer_v, burst));
        inner_flow.last_active = now;
        inner_flow
            .queues
            .entry(route)
            .or_default()
            .push_back(stream_id);
        outer_flow.touch(route, inner);

        // Same at the outer level: it only falls back to the queue's clock
        // when it was not already serving this route.
        if !outer_backlogged {
            outer_flow.used = outer_flow.used.max(v.saturating_sub(burst));
            let has_work = outer_flow.backlog.contains_key(&route);
            let used = outer_flow.used;
            touch_root(&mut self.root_backlog, route, outer, has_work, used);
        }

        self.entries.insert(stream_id, Entry { route, flow });
    }

    /// Forget a queued request: it timed out, was cancelled, or was dispatched.
    ///
    /// Idempotent, and cheap when the request is not queued at all.
    pub(crate) fn remove(&mut self, stream_id: u32) {
        let Some(entry) = self.entries.remove(&stream_id) else {
            return;
        };
        let route = entry.route;
        let FlowRef { outer, inner } = entry.flow;
        let Some(outer_flow) = self.outers.get_mut(&outer) else {
            return;
        };
        if let Some(queue) = outer_flow
            .inners
            .get_mut(&inner)
            .and_then(|inner_flow| inner_flow.queues.get_mut(&route))
        {
            queue.retain(|queued| *queued != stream_id);
            if queue.is_empty()
                && let Some(inner_flow) = outer_flow.inners.get_mut(&inner)
            {
                inner_flow.queues.remove(&route);
            }
        }
        let has_work = outer_flow.touch(route, inner);
        let used = outer_flow.used;
        touch_root(&mut self.root_backlog, route, outer, has_work, used);
    }

    /// Charge a flow for one request (see [`Cost`]).
    ///
    /// The charge is capped one `BLASTIT_FQ_MAX_DEBT` ahead of each level, so
    /// no flow can buy an unbounded future just by being slow.
    pub(crate) fn debit(&mut self, flow: &FlowRef, cost: Cost, now: Instant) {
        let max_debt = self.cfg.max_debt_ns();
        let cap = self.v.saturating_add(max_debt);
        let Some(outer_flow) = self.outers.get_mut(&flow.outer) else {
            return;
        };
        let inner_cap = outer_flow.v.saturating_add(max_debt);
        if let Some(inner_flow) = outer_flow.inners.get_mut(&flow.inner) {
            inner_flow.used = inner_flow.used.saturating_add(cost.0).min(inner_cap);
            inner_flow.last_active = now;
        }
        outer_flow.used = outer_flow.used.saturating_add(cost.0).min(cap);
        outer_flow.last_active = now;

        // Both levels changed, so every ordered set this flow appears in is
        // re-keyed. Only routes it actually has work for are in them.
        let routes: Vec<u32> = outer_flow.backlog.keys().copied().collect();
        for route in &routes {
            outer_flow.touch(*route, flow.inner);
        }
        let updated: Vec<(u32, bool)> = routes
            .iter()
            .map(|route| (*route, outer_flow.backlog.contains_key(route)))
            .collect();
        let used = outer_flow.used;
        for (route, has_work) in updated {
            touch_root(&mut self.root_backlog, route, flow.outer, has_work, used);
        }
    }

    /// One request of this flow ended: it may leave the tables now.
    ///
    /// A flow is collected when it has nothing queued, nothing in flight, and
    /// has either caught up with its level's virtual time or been idle for
    /// `BLASTIT_FQ_DEBT_TTL`. Until then its debt keeps it behind the others,
    /// which is the point of charging it.
    pub(crate) fn release(&mut self, flow: FlowRef, now: Instant) {
        let ttl = self.cfg.debt_ttl;
        let v = self.v;
        let mut drop_inner = false;
        let mut drop_outer = false;
        if let Some(outer_flow) = self.outers.get_mut(&flow.outer) {
            outer_flow.in_flight = outer_flow.in_flight.saturating_sub(1);
            let outer_v = outer_flow.v;
            if let Some(inner_flow) = outer_flow.inners.get_mut(&flow.inner) {
                inner_flow.in_flight = inner_flow.in_flight.saturating_sub(1);
            }
            drop_inner = outer_flow
                .inners
                .get(&flow.inner)
                .is_some_and(|inner_flow| {
                    inner_flow.idle()
                        && inner_flow.in_flight == 0
                        && (budget_reached(outer_v, inner_flow.used)
                            || now.duration_since(inner_flow.last_active) > ttl)
                });
            if drop_inner && outer_flow.inners.remove(&flow.inner).is_some() {
                self.inner_count = self.inner_count.saturating_sub(1);
            }
            if drop_inner {
                let routes: Vec<u32> = outer_flow.backlog.keys().copied().collect();
                for route in &routes {
                    outer_flow.touch(*route, flow.inner);
                }
            }
            drop_outer = outer_flow.inners.is_empty()
                && outer_flow.in_flight == 0
                && !outer_flow.backlogged()
                && (budget_reached(v, outer_flow.used)
                    || now.duration_since(outer_flow.last_active) > ttl);
        }
        if drop_outer {
            self.outers.remove(&flow.outer);
            if self.overflow_outer == Some(flow.outer) {
                self.overflow_outer = None;
            }
        }
        if drop_inner || drop_outer {
            self.refresh_root();
        }
    }

    /// Prune every root backlog entry whose outer flow is gone or has nothing
    /// left for that route.
    fn refresh_root(&mut self) {
        let mut empty: Vec<u32> = Vec::new();
        for (route, set) in &mut self.root_backlog {
            set.retain(|(_, key)| {
                self.outers
                    .get(key)
                    .is_some_and(|outer_flow| outer_flow.backlog.contains_key(route))
            });
            if set.is_empty() {
                empty.push(*route);
            }
        }
        for route in empty {
            self.root_backlog.remove(&route);
        }
    }

    /// Collect every flow that may be collected now.
    ///
    /// [`FairQueue::release`] does this for the flow of the request that just
    /// ended; this is the sweep for a flow whose last request ended long ago
    /// while the queue stayed busy.
    pub(crate) fn gc(&mut self, now: Instant) {
        let v = self.v;
        let ttl = self.cfg.debt_ttl;
        let mut dead_outers = Vec::new();
        for (outer_key, outer_flow) in &mut self.outers {
            let dead_inners: Vec<u64> = outer_flow
                .inners
                .iter()
                .filter(|(_, inner_flow)| {
                    inner_flow.idle()
                        && inner_flow.in_flight == 0
                        && (budget_reached(outer_flow.v, inner_flow.used)
                            || now.duration_since(inner_flow.last_active) > ttl)
                })
                .map(|(inner_key, _)| *inner_key)
                .collect();
            for inner_key in dead_inners {
                if outer_flow.inners.remove(&inner_key).is_some() {
                    self.inner_count = self.inner_count.saturating_sub(1);
                    let routes: Vec<u32> = outer_flow.backlog.keys().copied().collect();
                    for route in &routes {
                        outer_flow.touch(*route, inner_key);
                    }
                }
            }
            if outer_flow.inners.is_empty()
                && outer_flow.in_flight == 0
                && !outer_flow.backlogged()
                && (budget_reached(v, outer_flow.used)
                    || now.duration_since(outer_flow.last_active) > ttl)
            {
                dead_outers.push(*outer_key);
            }
        }
        if dead_outers.is_empty() {
            return;
        }
        for outer_key in dead_outers {
            self.outers.remove(&outer_key);
            if self.overflow_outer == Some(outer_key) {
                self.overflow_outer = None;
            }
        }
        self.refresh_root();
    }

    /// The flow a new request belongs to, read from a head that has not been
    /// decoded.
    ///
    /// `http.chan` requests are not decoded into a [`blastit_frame::ReqHead`]
    /// (see [`ReqHeadView`]), so the headers arrive borrowed out of the frame's
    /// payload. This shares [`FairQueue::resolve_iter`] with every other
    /// head-reading entry point, so the flow — and hence the scheduling — is
    /// the same whichever path read the head (D5).
    pub(crate) fn resolve_view(
        &mut self,
        peer: &str,
        head: &ReqHeadView<'_>,
        now: Instant,
    ) -> FlowRef {
        self.resolve_iter(peer, head.headers(), now)
    }

    /// The flow a new request belongs to (see the module documentation), over
    /// headers as borrowed slices.
    ///
    /// Both entry points above meet here, and everything that reads header
    /// values — the client-IP resolver and the inner key — is reached only from
    /// here, so the two paths cannot drift.
    ///
    /// This also counts the request as **admitted**: the flow is held until
    /// [`FairQueue::release`] is called for it, which is what keeps a flow that
    /// is being served from being collected.
    ///
    /// `now` is the caller's clock, the one [`FairQueue::release`] and
    /// [`FairQueue::gc`] compare `last_active` against.
    fn resolve_iter<'h, I>(&mut self, peer: &str, headers: I, now: Instant) -> FlowRef
    where
        I: Iterator<Item = (&'h [u8], &'h [u8])> + Clone,
    {
        let burst = self.cfg.burst_ns();
        let Some(ip) = self
            .cfg
            .client_ip
            .resolve_iter(peer, headers.clone())
            .map(|c| c.ip())
        else {
            // Not an address at all: the overflow flow is exactly where
            // "everything we cannot tell apart" belongs.
            return self.admit(OVERFLOW_OUTER, INNER_OVERFLOW, burst, now);
        };

        let base_outer = self.outer_key(ip);
        let outer =
            if self.outers.contains_key(&base_outer) || self.outers.len() < self.cfg.outer_max {
                base_outer
            } else {
                match self.overflow_outer {
                    Some(key) if self.outers.contains_key(&key) => key,
                    _ => {
                        self.overflow_outer = Some(OVERFLOW_OUTER);
                        OVERFLOW_OUTER
                    }
                }
            };

        let candidate = self.inner_key(headers, ip);
        let inner = match self.outers.get(&outer) {
            Some(outer_flow)
                if outer_flow.inners.contains_key(&candidate)
                    || (outer_flow.inners.len() < self.cfg.inner_max_per_outer
                        && self.inner_count < self.cfg.inner_max) =>
            {
                candidate
            }
            None if self.inner_count < self.cfg.inner_max => candidate,
            _ => INNER_OVERFLOW,
        };
        self.admit(outer, inner, burst, now)
    }

    /// Count one request as admitted to `outer`/`inner`, creating what is
    /// missing.
    fn admit(&mut self, outer: OuterKey, inner: u64, burst: u64, now: Instant) -> FlowRef {
        let v = self.v;
        let outer_flow = self
            .outers
            .entry(outer)
            .or_insert_with(|| OuterFlow::new(v, burst));
        outer_flow.in_flight = outer_flow.in_flight.saturating_add(1);
        outer_flow.last_active = now;
        let outer_v = outer_flow.v;
        if !outer_flow.inners.contains_key(&inner) {
            self.inner_count = self.inner_count.saturating_add(1);
        }
        let inner_flow = outer_flow
            .inners
            .entry(inner)
            .or_insert_with(|| InnerFlow::new(outer_v, burst));
        inner_flow.in_flight = inner_flow.in_flight.saturating_add(1);
        inner_flow.last_active = now;
        FlowRef { outer, inner }
    }

    /// The outer flow key of one address.
    fn outer_key(&self, ip: IpAddr) -> OuterKey {
        match ip {
            IpAddr::V4(v4) => OuterKey {
                family: 4,
                masked: u128::from(u32::from(v4) & mask32(self.cfg.outer_prefix_v4)),
            },
            IpAddr::V6(v6) => OuterKey {
                family: 6,
                masked: u128::from(v6) & mask128(self.cfg.outer_prefix_v6),
            },
        }
    }

    /// The inner flow key: a hash of the client address (when configured) and
    /// of the configured headers and cookies.
    ///
    /// An absent value is hashed as an empty attribute rather than skipped, so
    /// that "no `Authorization` header" and "an empty `Authorization` header"
    /// are one flow and not two, and a client cannot multiply its flows by
    /// leaving a header out.
    fn inner_key<'h, I>(&self, headers: I, ip: IpAddr) -> u64
    where
        I: Iterator<Item = (&'h [u8], &'h [u8])> + Clone,
    {
        let mut hasher = self.hasher.build_hasher();
        let mut attribute = |name: &str, values: &[&str]| {
            hasher.write(name.as_bytes());
            hasher.write_u8(0);
            if values.is_empty() {
                hasher.write_u8(0);
            } else {
                for value in values {
                    hasher.write(value.as_bytes());
                    hasher.write_u8(0);
                }
            }
        };

        if self.cfg.inner_include_ip {
            let ip = ip.to_string();
            attribute("client-ip", &[ip.as_str()]);
        }
        for name in &self.cfg.inner_headers {
            let values: Vec<&str> = values_of(headers.clone(), name)
                .filter_map(|value| std::str::from_utf8(value).ok())
                .collect();
            attribute(name, &values);
        }
        let cookies = collect_cookies(headers);
        for name in &self.cfg.inner_cookies {
            let value = cookies
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str());
            let values: Vec<&str> = value.into_iter().collect();
            attribute(&format!("cookie:{name}"), &values);
        }
        hasher.finish()
    }

    /// The flow every request that cannot be told apart shares (D5).
    ///
    /// It is the key the overflow tables hand out and the key an unreadable
    /// peer gets, so a scheduler with no flow of its own can still queue a
    /// request somewhere it will be served fairly.
    pub(crate) fn overflow_flow() -> FlowRef {
        FlowRef {
            outer: OVERFLOW_OUTER,
            inner: INNER_OVERFLOW,
        }
    }
}

/// Every `name=value` pair of every `Cookie` header, in order.
///
/// Takes the headers as an iterator of borrowed slices so it reads the same
/// pairs whether they were decoded into owned `Vec`s or are still in the
/// `REQ_HEAD` payload; the `Cookie` values themselves are the only thing it
/// copies, because a cookie name is built from several pieces of its value.
fn collect_cookies<'h, I>(headers: I) -> Vec<(String, String)>
where
    I: Iterator<Item = (&'h [u8], &'h [u8])>,
{
    let mut cookies = Vec::new();
    for value in values_of(headers, "cookie") {
        let Ok(value) = std::str::from_utf8(value) else {
            continue;
        };
        for pair in value.split(';') {
            let pair = pair.trim();
            if let Some((name, value)) = pair.split_once('=') {
                cookies.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
    }
    cookies
}

/// Whether a flow has caught up with its level's virtual time.
fn budget_reached(level_v: u64, used: u64) -> bool {
    level_v >= used
}

/// The mask of an IPv4 prefix, clamped to the family's width.
fn mask32(prefix: u8) -> u32 {
    let prefix = prefix.min(32);
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

/// The mask of an IPv6 prefix, clamped to the family's width.
fn mask128(prefix: u8) -> u128 {
    let prefix = prefix.min(128);
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}
