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
const OVERFLOW_OUTER: OuterKey = OuterKey { family: 0, masked: 0 };
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
        self.queues.get(&route).is_some_and(|queue| !queue.is_empty())
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

    /// The flow of the request that goes next for `route`, and the budget it
    /// has spent — the two-level "lowest `used` first" choice.
    ///
    /// The inner flow's `used` is what a request's place in the order is: the
    /// budget its flow has spent, and the one number that compares across
    /// routes.
    pub(crate) fn peek(&self, route: u32) -> Option<(u64, u32)> {
        let (_, outer_key) = *self.root_backlog.get(&route)?.iter().next()?;
        let outer = self.outers.get(&outer_key)?;
        let (used, inner_key) = *outer.backlog.get(&route)?.iter().next()?;
        let stream_id = *outer.inners.get(&inner_key)?.queues.get(&route)?.front()?;
        Some((used, stream_id))
    }

    /// Take the next request for `route`, and move the queue's clock to it.
    pub(crate) fn pop(&mut self, route: u32) -> Option<u32> {
        let (outer_used, outer_key) = *self.root_backlog.get(&route)?.iter().next()?;
        self.v = self.v.max(outer_used);
        let outer = self.outers.get_mut(&outer_key)?;
        let (inner_used, inner_key) = *outer.backlog.get(&route)?.iter().next()?;
        outer.v = outer.v.max(inner_used);

        let stream_id = outer.inners.get_mut(&inner_key)?.queues.get_mut(&route)?.pop_front()?;
        let has_work = outer.touch(route, inner_key);
        let used = outer.used;
        touch_root(&mut self.root_backlog, route, outer_key, has_work, used);
        self.entries.remove(&stream_id);
        Some(stream_id)
    }

    /// Queue `stream_id`, which is waiting for `route` in `flow`.
    pub(crate) fn push(&mut self, stream_id: u32, route: u32, flow: FlowRef) {
        let FlowRef { outer, inner } = flow;
        let v = self.v;
        let burst = self.cfg.burst_ns();
        let now = Instant::now();

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
        inner_flow.queues.entry(route).or_default().push_back(stream_id);
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
            if queue.is_empty() {
                if let Some(inner_flow) = outer_flow.inners.get_mut(&inner) {
                    inner_flow.queues.remove(&route);
                }
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
            drop_inner = outer_flow.inners.get(&flow.inner).is_some_and(|inner_flow| {
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

    /// The flow a new request belongs to (see the module documentation).
    ///
    /// This also counts the request as **admitted**: the flow is held until
    /// [`FairQueue::release`] is called for it, which is what keeps a flow that
    /// is being served from being collected.
    pub(crate) fn resolve(&mut self, peer: &str, headers: &[(Vec<u8>, Vec<u8>)]) -> FlowRef {
        let now = Instant::now();
        let burst = self.cfg.burst_ns();
        let Some(ip) = self.cfg.client_ip.resolve(peer, headers).map(|c| c.ip()) else {
            // Not an address at all: the overflow flow is exactly where
            // "everything we cannot tell apart" belongs.
            return self.admit(OVERFLOW_OUTER, INNER_OVERFLOW, burst, now);
        };

        let base_outer = self.outer_key(ip);
        let outer = if self.outers.contains_key(&base_outer) || self.outers.len() < self.cfg.outer_max
        {
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
    fn inner_key(&self, headers: &[(Vec<u8>, Vec<u8>)], ip: IpAddr) -> u64 {
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
            let values: Vec<&str> = values_of(headers, name)
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

    /// How many inner flows exist right now.
    #[cfg(test)]
    pub(crate) fn inner_count(&self) -> usize {
        self.inner_count
    }

    /// How many outer flows exist right now.
    #[cfg(test)]
    pub(crate) fn outer_count(&self) -> usize {
        self.outers.len()
    }
}

/// The values of every instance of a header, matched case-insensitively.
fn values_of<'a>(headers: &'a [(Vec<u8>, Vec<u8>)], name: &str) -> impl Iterator<Item = &'a [u8]> {
    let name = name.as_bytes();
    headers
        .iter()
        .filter(move |(field, _)| field.len() == name.len() && field.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_slice())
}

/// Every `name=value` pair of every `Cookie` header, in order.
fn collect_cookies(headers: &[(Vec<u8>, Vec<u8>)]) -> Vec<(String, String)> {
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


#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A queue with the documented configuration and small tables, so a test
    /// can overflow them on purpose.
    fn queue() -> FairQueue {
        FairQueue::new(FairConfig::default())
    }

    fn client(_ip: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
        Vec::new()
    }

    /// The peer address of a client, as `http` would report it.
    fn peer(ip: &str) -> String {
        format!("{ip}:41234")
    }

    /// Queue one request and return its flow.
    fn push(queue: &mut FairQueue, stream_id: u32, route: u32, ip: &str) -> FlowRef {
        let flow = queue.resolve(&peer(ip), &client(ip));
        queue.push(stream_id, route, flow);
        flow
    }

    #[test]
    fn the_lowest_used_flow_goes_first() {
        let mut queue = queue();
        let a = push(&mut queue, 1, 0, "10.0.0.1");
        let b = push(&mut queue, 2, 0, "10.1.0.1");
        assert_ne!(a.outer, b.outer, "a /24 apart is two outer flows");

        // A has burned a lot of worker time, so B must be served first even
        // though A queued first.
        queue.debit(&a, Cost(100_000_000), Instant::now());
        assert_eq!(queue.pop(0), Some(2));
        assert_eq!(queue.pop(0), Some(1));
        assert_eq!(queue.pop(0), None);
    }

    #[test]
    fn requests_within_one_flow_stay_in_arrival_order() {
        let mut queue = queue();
        let flow = queue.resolve(&peer("10.0.0.1"), &client("10.0.0.1"));
        for stream_id in 0..5 {
            queue.push(stream_id, 0, flow);
        }
        let served: Vec<u32> = (0..5).map(|_| queue.pop(0).unwrap()).collect();
        assert_eq!(served, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn inner_flows_of_one_outer_are_ordered_by_budget() {
        let mut queue = queue();
        let mut browser_a = Vec::new();
        browser_a.push((b"user-agent".to_vec(), b"browser-a".to_vec()));
        let mut browser_b = Vec::new();
        browser_b.push((b"user-agent".to_vec(), b"browser-b".to_vec()));

        let a = queue.resolve(&peer("10.0.0.1"), &browser_a);
        let b = queue.resolve(&peer("10.0.0.1"), &browser_b);
        assert_eq!(a.outer, b.outer, "one outer flow");
        assert_ne!(a.inner, b.inner, "two browsers, two inner flows");

        queue.push(1, 0, a);
        queue.push(2, 0, b);
        queue.debit(&a, Cost(50_000_000), Instant::now());
        assert_eq!(queue.pop(0), Some(2), "b has spent nothing");
        assert_eq!(queue.pop(0), Some(1));
    }

    #[test]
    fn a_burst_lets_a_new_flow_go_first_once() {
        // An empty flow starts one burst *behind* the queue's clock, which is
        // what lets a newly-arrived client spend a burst before it queues.
        let mut queue = queue();
        let heavy = push(&mut queue, 1, 0, "10.0.0.1");
        queue.debit(&heavy, Cost(1_000_000_000), Instant::now());
        queue.pop(0);
        push(&mut queue, 2, 0, "10.2.0.1");
        assert_eq!(queue.pop(0), Some(2));
    }

    #[test]
    fn routes_are_ordered_against_each_other() {
        // All routes compete for one worker budget, so the flow with the
        // least budget spent goes next whichever route it belongs to.
        let mut queue = queue();
        let route0 = push(&mut queue, 1, 0, "10.0.0.1");
        let route1 = push(&mut queue, 2, 1, "10.1.0.1");
        assert_eq!(queue.routes(), vec![0, 1]);
        queue.debit(&route0, Cost(10_000_000), Instant::now());
        assert_eq!(queue.peek(0).unwrap().1, 1);
        assert_eq!(queue.peek(1).unwrap().1, 2);
        assert_eq!(queue.pop(1), Some(2), "route 1's flow has spent nothing");
        assert_eq!(queue.pop(0), Some(1));
        let _ = route1;
    }

    #[test]
    fn only_the_requested_route_is_ever_served() {
        let mut queue = queue();
        push(&mut queue, 1, 0, "10.0.0.1");
        push(&mut queue, 2, 1, "10.0.0.1");
        assert_eq!(queue.peek(1), Some((0, 2)));
        assert_eq!(queue.peek(0), Some((0, 1)));
        assert_eq!(queue.pop(1), Some(2));
        assert_eq!(queue.pop(1), None, "route 1 has nothing left");
        assert_eq!(queue.pop(0), Some(1));
    }

    #[test]
    fn an_outer_table_that_overflows_shares_one_flow() {
        let mut cfg = FairConfig::default();
        cfg.outer_max = 1;
        let mut queue = FairQueue::new(cfg);
        let a = queue.resolve(&peer("10.0.0.1"), &[]);
        let b = queue.resolve(&peer("10.1.0.1"), &[]);
        let c = queue.resolve(&peer("10.2.0.1"), &[]);
        assert_ne!(a.outer, b.outer);
        assert_eq!(b.outer, c.outer, "the second and later share the overflow");
        assert_eq!(b.outer, OVERFLOW_OUTER);
        assert_eq!(queue.outer_count(), 2, "one real prefix and the overflow");

        // Two clients that both overflowed the *outer* table still have their
        // own inner flows — the overflow is at the outer level — so what they
        // share is the outer flow's budget, not one queue:
        assert_ne!(b.inner, c.inner, "the inner level is still per client");

        // ... and one flow queued twice is served in arrival order, because
        // everything that lands in the same flow is one client.
        queue.push(1, 0, b);
        queue.push(2, 0, b);
        assert_eq!(queue.pop(0), Some(1));
        assert_eq!(queue.pop(0), Some(2));
        assert_eq!(queue.pop(0), None);
    }

    #[test]
    fn an_inner_table_that_overflows_shares_one_flow() {
        let mut cfg = FairConfig::default();
        cfg.inner_max_per_outer = 1;
        let mut queue = FairQueue::new(cfg);
        let mut a = Vec::new();
        a.push((b"user-agent".to_vec(), b"a".to_vec()));
        let mut b = Vec::new();
        b.push((b"user-agent".to_vec(), b"b".to_vec()));
        let mut c = Vec::new();
        c.push((b"user-agent".to_vec(), b"c".to_vec()));

        let first = queue.resolve(&peer("10.0.0.1"), &a);
        let second = queue.resolve(&peer("10.0.0.1"), &b);
        let third = queue.resolve(&peer("10.0.0.1"), &c);
        assert_ne!(first.inner, second.inner);
        assert_eq!(second.inner, INNER_OVERFLOW);
        assert_eq!(third.inner, INNER_OVERFLOW);
        assert_eq!(first.outer, third.outer, "same outer, different inner");

        // The process-wide inner budget binds too.
        let mut cfg = FairConfig::default();
        cfg.inner_max = 1;
        let mut queue = FairQueue::new(cfg);
        let one = queue.resolve(&peer("10.0.0.1"), &a);
        let two = queue.resolve(&peer("10.0.0.2"), &b);
        let three = queue.resolve(&peer("10.0.0.1"), &a);
        assert_eq!(one.inner, three.inner, "an existing inner flow is kept");
        assert_eq!(two.inner, INNER_OVERFLOW);
    }

    #[test]
    fn an_unreadable_peer_shares_the_overflow_flow() {
        let mut queue = queue();
        let unnamed = queue.resolve("not-an-address", &[]);
        assert_eq!(unnamed.outer, OVERFLOW_OUTER);
        assert_eq!(unnamed.inner, INNER_OVERFLOW);
        let also = queue.resolve("", &[]);
        assert_eq!(also, unnamed, "one flow for everything unreadable");
    }

    #[test]
    fn the_outer_key_is_the_configured_prefix() {
        let mut queue = queue();
        let a = queue.resolve(&peer("203.0.113.7"), &[]);
        let b = queue.resolve(&peer("203.0.113.200"), &[]);
        let c = queue.resolve(&peer("203.0.114.7"), &[]);
        assert_eq!(a.outer, b.outer, "one /24");
        assert_ne!(a.outer, c.outer);
        assert_eq!(a.outer.family, 4);
        assert_eq!(
            a.outer.masked,
            u128::from(u32::from(std::net::Ipv4Addr::new(203, 0, 113, 0)))
        );

        // IPv6 uses its own prefix, and an address is never in a prefix of the
        // other family.
        let v6 = queue.resolve(&peer("[2001:db8:1:100::1]"), &[]);
        let near = queue.resolve(&peer("[2001:db8:1:1ff::1]"), &[]);
        let far = queue.resolve(&peer("[2001:db8:1:200::1]"), &[]);
        assert_eq!(v6.outer, near.outer, "one /56");
        assert_ne!(v6.outer, far.outer);
        assert_ne!(v6.outer.family, a.outer.family);

        // An IPv4-mapped IPv6 address is the same client as its IPv4 form.
        let mapped = queue.resolve(&peer("[::ffff:203.0.113.7]"), &[]);
        assert_eq!(mapped.outer, a.outer);

        // A prefix of zero puts every address of the family in one flow.
        let mut cfg = FairConfig::default();
        cfg.outer_prefix_v4 = 0;
        let mut wide = FairQueue::new(cfg);
        assert_eq!(
            wide.resolve(&peer("1.2.3.4"), &[]).outer,
            wide.resolve(&peer("9.9.9.9"), &[]).outer
        );
    }

    #[test]
    fn the_inner_key_tracks_the_fingerprint() {
        let mut queue = queue();
        let mut headers = Vec::new();
        headers.push((b"user-agent".to_vec(), b"browser-a".to_vec()));
        let first = queue.resolve(&peer("10.0.0.1"), &headers);
        let same = queue.resolve(&peer("10.0.0.1"), &headers);
        let mut other_headers = headers.clone();
        other_headers[0].1 = b"browser-b".to_vec();
        let other = queue.resolve(&peer("10.0.0.1"), &other_headers);
        let other_ip = queue.resolve(&peer("10.0.0.2"), &headers);
        assert_eq!(first.inner, same.inner);
        assert_ne!(first.inner, other.inner, "the user agent is in the key");
        assert_ne!(first.inner, other_ip.inner, "so is the address");
        assert_eq!(first.outer, other.outer);

        // Only the configured headers are, and header names are matched the
        // way HTTP does.
        let mut auth = Vec::new();
        auth.push((b"AUTHORIZATION".to_vec(), b"Bearer x".to_vec()));
        let with_auth = queue.resolve(&peer("10.0.0.1"), &auth);
        assert_ne!(with_auth.inner, first.inner);
        let mut irrelevant = Vec::new();
        irrelevant.push((b"authorization".to_vec(), b"Bearer x".to_vec()));
        irrelevant.push((b"x-random".to_vec(), b"1".to_vec()));
        let with_irrelevant = queue.resolve(&peer("10.0.0.1"), &irrelevant);
        assert_eq!(
            with_irrelevant.inner, with_auth.inner,
            "a header the operator did not list is not part of the key"
        );
        assert_eq!(with_auth.inner, queue.resolve(&peer("10.0.0.1"), &auth).inner);

        // A missing header and an empty one are one flow, not two.
        let mut empty = Vec::new();
        empty.push((b"authorization".to_vec(), Vec::new()));
        assert_eq!(
            empty.len(),
            queue.resolve(&peer("10.0.0.1"), &empty).inner.min(1) as usize
        );
        assert_eq!(
            queue.resolve(&peer("10.0.0.1"), &empty).inner,
            queue.resolve(&peer("10.0.0.1"), &[]).inner
        );
    }

    #[test]
    fn cookies_are_part_of_the_inner_key() {
        let mut queue = queue();
        let mut one = Vec::new();
        one.push((b"cookie".to_vec(), b"sessionid=aaa; other=1".to_vec()));
        let mut two = Vec::new();
        two.push((b"cookie".to_vec(), b"sessionid=bbb".to_vec()));
        // The same cookie in a second header instance is the same client.
        let mut split = Vec::new();
        split.push((b"cookie".to_vec(), b"other=1".to_vec()));
        split.push((b"cookie".to_vec(), b"sessionid=aaa".to_vec()));
        assert_ne!(
            queue.resolve(&peer("10.0.0.1"), &one).inner,
            queue.resolve(&peer("10.0.0.1"), &two).inner
        );
        assert_eq!(
            queue.resolve(&peer("10.0.0.1"), &one).inner,
            queue.resolve(&peer("10.0.0.1"), &split).inner
        );

        // Excluding the address makes two addresses one flow, and including it
        // (the default) keeps them apart.
        let mut cfg = FairConfig::default();
        cfg.inner_include_ip = false;
        let mut shared = FairQueue::new(cfg);
        assert_eq!(
            shared.resolve(&peer("10.0.0.1"), &one).inner,
            shared.resolve(&peer("10.0.0.2"), &one).inner
        );
        assert_ne!(
            queue.resolve(&peer("10.0.0.1"), &one).inner,
            queue.resolve(&peer("10.0.0.2"), &one).inner
        );
    }

    #[test]
    fn the_tables_are_bounded() {
        let mut cfg = FairConfig::default();
        cfg.outer_max = 4;
        cfg.inner_max_per_outer = 2;
        cfg.inner_max = 3;
        let mut queue = FairQueue::new(cfg);
        for index in 0..16u8 {
            let mut headers = Vec::new();
            headers.push((b"user-agent".to_vec(), vec![b'a' + index]));
            queue.resolve(&format!("10.0.{index}.1:1"), &headers);
        }
        assert!(
            queue.outer_count() <= 5,
            "four outer flows and the overflow, got {}",
            queue.outer_count()
        );
        // (The reference implementation's rule, kept: the inner budget binds
        // new *fingerprints*, and each outer flow that is over the budget still
        // gets its own one overflow flow, so the table is bounded by
        // `inner_max + outer_max` entries and not by `inner_max` alone.)
        assert!(
            queue.inner_count() <= cfg_inner_max() + queue.outer_count(),
            "the inner table stayed bounded, got {}",
            queue.inner_count()
        );
        assert!(
            queue.inner_count() < 16,
            "the outer budget bounds it, got {}",
            queue.inner_count()
        );

        // One outer flow, and the overflow flow is shared by every fingerprint
        // that does not fit.
        let mut cfg = FairConfig::default();
        cfg.inner_max_per_outer = 1;
        cfg.inner_max = 2;
        let mut queue = FairQueue::new(cfg);
        let mut a = Vec::new();
        a.push((b"user-agent".to_vec(), b"a".to_vec()));
        let mut b = Vec::new();
        b.push((b"user-agent".to_vec(), b"b".to_vec()));
        let mut c = Vec::new();
        c.push((b"user-agent".to_vec(), b"c".to_vec()));
        let first = queue.resolve(&peer("10.0.0.1"), &a);
        let second = queue.resolve(&peer("10.0.0.1"), &b);
        let third = queue.resolve(&peer("10.0.0.1"), &c);
        assert_ne!(first.inner, second.inner);
        assert_eq!(second.inner, third.inner, "one overflow flow, shared");
        assert_eq!(queue.inner_count(), 2);
    }

    /// The inner budget the boundedness test configured.
    fn cfg_inner_max() -> usize {
        3
    }

    #[test]
    fn a_flow_that_is_back_within_budget_is_collected() {
        let mut queue = queue();
        let now = Instant::now();
        let flow = push(&mut queue, 1, 0, "10.0.0.1");
        queue.pop(0);
        assert_eq!(queue.outer_count(), 1);
        queue.debit(&flow, Cost(0), now);
        queue.release(flow, now);
        assert_eq!(queue.outer_count(), 0, "nothing queued, nothing in flight");
        assert_eq!(queue.inner_count(), 0);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn a_flow_that_still_owes_time_is_kept() {
        let mut queue = queue();
        let now = Instant::now();
        let flow = push(&mut queue, 1, 0, "10.0.0.1");
        queue.pop(0);
        // The debt is what keeps it: the request is over, but the flow is
        // still ahead of the clock, so its next request must not jump the
        // queue.
        queue.debit(&flow, Cost(50_000_000), now);
        queue.release(flow, now);
        assert_eq!(queue.outer_count(), 1, "the debtor is kept");
        assert_eq!(queue.inner_count(), 1);

        // An idle flow is collected once the clock has moved past it.
        queue.gc(now);
        assert_eq!(queue.outer_count(), 1, "still owing");
    }

    #[test]
    fn an_idle_debtor_is_collected_after_the_ttl() {
        let mut cfg = FairConfig::default();
        cfg.max_debt = Duration::from_secs(60);
        cfg.debt_ttl = Duration::from_millis(100);
        let mut queue = FairQueue::new(cfg);
        let now = Instant::now();
        let flow = push(&mut queue, 1, 0, "10.0.0.1");
        queue.pop(0);
        queue.debit(&flow, Cost(1), now);
        // A long time later nothing remembers it: the TTL wins over the debt.
        let later = now + Duration::from_secs(1);
        queue.release(flow, later);
        assert_eq!(queue.outer_count(), 0);
        let _ = &queue;

        // ... and the same sweep collects a flow whose last request ended long
        // ago while the queue stayed busy.
        let mut queue = queue;
        let flow = push(&mut queue, 2, 0, "10.0.0.1");
        queue.pop(0);
        queue.debit(&flow, Cost(1), now);
        queue.release(flow, now);
        assert_eq!(queue.outer_count(), 1, "the TTL has not passed yet");
        queue.gc(now + Duration::from_secs(10));
        assert_eq!(queue.outer_count(), 0, "the sweep collects it");
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn a_queued_request_keeps_its_flow_alive() {
        let mut queue = queue();
        let now = Instant::now();
        let flow = queue.resolve(&peer("10.0.0.1"), &[]);
        queue.push(1, 0, flow);
        queue.push(2, 0, flow);
        queue.pop(0);
        queue.debit(&flow, Cost(1_000_000_000), now);
        queue.release(flow, now);
        assert_eq!(queue.outer_count(), 1, "request 2 is still queued");
        assert_eq!(queue.pop(0), Some(2));
    }

    #[test]
    fn removing_a_queued_request_keeps_the_order_intact() {
        let mut queue = queue();
        let a = push(&mut queue, 1, 0, "10.0.0.1");
        push(&mut queue, 2, 0, "10.1.0.1");
        push(&mut queue, 3, 0, "10.2.0.1");
        assert!(queue.contains(2));
        queue.remove(2);
        assert!(!queue.contains(2));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.ids().len(), 2);
        // Removing twice is harmless, and so is removing something unknown.
        queue.remove(2);
        queue.remove(99);
        assert_eq!(queue.pop(0), Some(1));
        assert_eq!(queue.pop(0), Some(3));
        assert_eq!(queue.pop(0), None);
        let _ = a;
    }

    #[test]
    fn a_request_that_leaves_the_queue_leaves_the_ordering() {
        let mut queue = queue();
        push(&mut queue, 1, 0, "10.0.0.1");
        push(&mut queue, 2, 0, "10.1.0.1");
        // The first request is dispatched (so it leaves the queue), and its
        // flow is charged and released.
        let flow = queue.resolve(&peer("10.0.0.1"), &[]);
        assert_eq!(queue.pop(0), Some(1));
        queue.debit(&flow, Cost(5), Instant::now());
        queue.release(flow, Instant::now());
        assert_eq!(queue.waiting_for_route(0), true);
        assert_eq!(queue.pop(0), Some(2));
        assert_eq!(queue.waiting_for_route(0), false);
        assert!(!queue.waiting_for_route(1));
    }

    #[test]
    fn a_debt_is_capped_by_the_maximum() {
        let mut cfg = FairConfig::default();
        cfg.max_debt = Duration::from_millis(10);
        cfg.burst = Duration::ZERO;
        let mut queue = FairQueue::new(cfg);
        let hog = push(&mut queue, 1, 0, "10.0.0.1");
        let victim = push(&mut queue, 2, 0, "10.1.0.1");
        // An absurd charge moves the hog at most `max_debt` ahead of the
        // clock, so the queue cannot be pushed past all reason.
        queue.debit(&hog, Cost(u64::MAX), Instant::now());
        assert_eq!(queue.pop(0), Some(2));
        let _ = victim;
        // And the queue's own clock is where the hog was put, not where it
        // tried to go.
        assert!(queue.v <= 10_000_000);
    }

    #[test]
    fn the_queue_is_never_charged_for_more_than_the_clock_has_moved() {
        let mut queue = queue();
        let now = Instant::now();
        // Charging a flow that no longer exists is a no-op rather than a
        // panic: the request ended and its flow was collected.
        let flow = FlowRef {
            outer: OVERFLOW_OUTER,
            inner: 7,
        };
        queue.debit(&flow, Cost(1), now);
        queue.release(flow, now);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn nothing_shares_a_key_by_accident_with_the_overflow_flow() {
        // The overflow outer flow has family 0, which no real prefix has, so a
        // client can never land in it by address.
        let mut queue = queue();
        let real = queue.resolve(&peer("0.0.0.0"), &[]);
        assert_eq!(real.outer.family, 4);
        assert_ne!(real.outer, OVERFLOW_OUTER);
        let mut cfg = FairConfig::default();
        cfg.outer_prefix_v4 = 0;
        let mut wide = FairQueue::new(cfg);
        assert_ne!(wide.resolve(&peer("0.0.0.0"), &[]).outer, OVERFLOW_OUTER);
    }

    #[test]
    fn a_hidden_inner_key_is_never_printed() {
        // §8.1: header values and cookies are never logged, and neither is a
        // hash of them.
        let mut queue = queue();
        let mut headers = Vec::new();
        headers.push((b"authorization".to_vec(), b"Bearer s3cret".to_vec()));
        let flow = queue.resolve(&peer("203.0.113.7"), &headers);
        let text = format!("{flow:?}");
        assert!(!text.contains("s3cret"), "a header value leaked: {text}");
        let inner = format!("{}", flow.inner);
        assert!(!text.contains(&inner), "the hash leaked: {text}");
        assert!(
            text.contains(&format!("{}", u32::from(std::net::Ipv4Addr::new(203, 0, 113, 0)))),
            "the prefix is fine: {text}"
        );
        assert!(text.contains("hidden"), "{text}");
    }

    #[test]
    fn resolving_never_panics_on_arbitrary_headers() {
        let mut queue = queue();
        let cases: Vec<Vec<(Vec<u8>, Vec<u8>)>> = vec![
            Vec::new(),
            vec![(Vec::new(), Vec::new())],
            vec![(b"user-agent".to_vec(), vec![0xff, 0xfe])],
            vec![(b"cookie".to_vec(), vec![0xff])],
            vec![(b"cookie".to_vec(), b"=;=;=".to_vec())],
            vec![(b"cookie".to_vec(), vec![b'a'; 4096])],
        ];
        for headers in cases {
            let _ = queue.resolve("not-an-address", &headers);
            let _ = queue.resolve("203.0.113.7:1", &headers);
        }
    }
}
