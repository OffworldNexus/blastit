//! What the scheduler knows about a route (ARCHITECTURE.md §2.4, §6).
//!
//! The routing table itself is [`blastit_manifest::RoutingTable`], which
//! matches `(method, PATH_INFO)` and returns a route *index*. The scheduler
//! keeps one [`RouteInfo`] per index, with the little that is not in the
//! routing table but is needed to serve the request: the route's name (for
//! events), whether its view is preforked, the uid its process runs under, and
//! the `body` policy its request bodies are decoded with.

use blastit_ingress::{BodyCodec, IngressPolicy};
use blastit_manifest::{Manifest, UidLayout};

/// One manifest route, in routing-table order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// The route's `name` in the manifest, used as the `route` field of every
    /// event that belongs to it (§8.1).
    pub name: String,
    /// Whether the manifest asks for this view to exist at boot
    /// (`prefork`, §6). It also overrides `BLASTIT_VIEW_SPAWN`.
    pub prefork: bool,
    /// The uid the view process runs under, `UID_BASE + 1000 + index` (§6).
    /// The scheduler never *uses* it — only init does — but events carry it.
    pub uid: u32,
    /// The route's `body` policy (§4.2 A), or `None` when the view declared
    /// none — which is fail-closed exactly like `body none`.
    pub ingress: Option<IngressPolicy>,
    /// Requests one process of this route runs at once; `None` means
    /// `BLASTIT_VIEW_MAX_WORKERS` (or the global budget when unset).
    ///
    /// Boot data written by the composition root from the provider it admitted
    /// (`ADR-0009`, `ADR-0015` decision 1): an NTS `libphp` worker runs one
    /// request, so its route says `Some(1)`. The scheduler never asks what
    /// runtime a route is.
    pub per_process_cap: Option<u32>,
    /// Whether this route may own more than one view process — a
    /// request-per-process provider. `false` is the threaded shape: one process,
    /// however many requests it runs at once (`ADR-0015` decision 1).
    ///
    /// Boot data, never inferred from a value: with
    /// `BLASTIT_VIEW_MAX_WORKERS=1` a threaded route still owns one process.
    pub process_pool: bool,
}

impl RouteInfo {
    /// A route with this name, prefork flag and uid, and no declared ingress.
    ///
    /// The concurrency facts default to the threaded shape: one process per
    /// route, admitting `BLASTIT_VIEW_MAX_WORKERS` requests at once.
    pub fn new(name: impl Into<String>, prefork: bool, uid: u32) -> Self {
        Self {
            name: name.into(),
            prefork,
            uid,
            ingress: None,
            per_process_cap: None,
            process_pool: false,
        }
    }

    /// This route with the `body` policy the manifest declared (§4.2 A).
    pub fn with_ingress(mut self, ingress: Option<IngressPolicy>) -> Self {
        self.ingress = ingress;
        self
    }

    /// This route with the request ceiling one of its processes runs at once.
    ///
    /// `None` — the threaded default — means `BLASTIT_VIEW_MAX_WORKERS`.
    #[must_use]
    pub fn with_per_process_cap(mut self, cap: u32) -> Self {
        self.per_process_cap = Some(cap);
        self
    }

    /// This route as a request-per-process provider, which may own a pool.
    #[must_use]
    pub fn with_process_pool(mut self, pooled: bool) -> Self {
        self.process_pool = pooled;
        self
    }

    /// The policy the scheduler enforces for this route's request bodies.
    ///
    /// A view that declared no `body` takes no body at all, exactly like
    /// `body none` (§4.2 A: `None` at the use site is fail-closed).
    pub fn ingress_policy(&self) -> IngressPolicy {
        self.ingress.unwrap_or(IngressPolicy {
            body: BodyCodec::None,
            allow_raw: false,
        })
    }
}

/// Builds the scheduler's route list from a parsed manifest and the uid
/// layout init derived from it.
///
/// The order is the manifest's, which is the order
/// [`RoutingTable::route`] returns indices in, and which [`UidLayout::view`]
/// numbers uids by.
///
/// Each route carries its `body` policy, so the scheduler can plan a request
/// body from the head alone (`R12`, `R16`). A route has no static
/// declaration any more: a `static "<prefix>"` mount is the file's, and a mount
/// that misses always falls through to the view table (§9.2), so no route
/// carries a "may forward" flag.
///
/// [`RoutingTable::route`]: blastit_manifest::RoutingTable::route
pub fn routes_from_manifest(manifest: &Manifest, layout: &UidLayout) -> Vec<RouteInfo> {
    manifest
        .routes()
        .iter()
        .enumerate()
        .map(|(index, route)| {
            RouteInfo::new(route.name(), route.prefork(), layout.view(index))
                .with_ingress(route.ingress())
        })
        .collect()
}
