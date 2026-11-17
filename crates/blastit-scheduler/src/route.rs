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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn routes_follow_manifest_order_and_the_uid_layout() {
        let manifest = blastit_manifest::Manifest::from_blastfile_str(
            "view first \"/a\" {}\n\
             view hot \"/b\" {\n  prefork\n}\n",
        )
        .unwrap();
        let layout = UidLayout::new(20_000);
        assert_eq!(
            routes_from_manifest(&manifest, &layout),
            vec![
                RouteInfo::new("first", false, 21_000),
                RouteInfo::new("hot", true, 21_001),
            ]
        );
        assert_eq!(RouteInfo::new("x", true, 7).uid, 7);
        assert_eq!(
            routes_from_manifest(&manifest, &UidLayout::default())[0].uid,
            UidLayout::default().view(0)
        );
    }

    #[test]
    fn a_route_without_ingress_takes_no_body_and_with_ingress_keeps_its_codec() {
        let json = IngressPolicy {
            body: BodyCodec::Json,
            allow_raw: false,
        };

        // Fail-closed: no `body` is `body none`.
        let plain = RouteInfo::new("plain", false, 7);
        assert_eq!(plain.ingress, None);
        assert_eq!(
            plain.ingress_policy(),
            IngressPolicy {
                body: BodyCodec::None,
                allow_raw: false,
            }
        );

        // The declared policy comes back unchanged, and the manifest path keeps
        // the fail-closed default until the manifest hands one over.
        let declared = RouteInfo::new("api", false, 7).with_ingress(Some(json));
        assert_eq!(declared.ingress_policy(), json);
        let raw = IngressPolicy {
            body: BodyCodec::Raw,
            allow_raw: true,
        };
        assert_eq!(
            RouteInfo::new("upload", true, 8)
                .with_ingress(Some(raw))
                .ingress_policy(),
            raw
        );

        let manifest =
            blastit_manifest::Manifest::from_blastfile_str("view api \"/api\" {}\n").unwrap();
        assert_eq!(
            routes_from_manifest(&manifest, &UidLayout::new(20_000))[0].ingress,
            None
        );
    }

    /// A `static` mount is a **mount**, not a route flag (§9.2): the route
    /// table is the views only, so no `RouteInfo` carries a policy or a "may
    /// forward" flag, and a mount that misses always falls through to the view
    /// table.
    ///
    /// The mount declares the URL root itself, which is the shape the
    /// scheduler serves; adding it to the manifest changes no route.
    #[test]
    fn a_static_block_is_a_mount_and_not_a_route_flag() {
        let source = "view app \"/*\" {}\n\
             static \"/\" {\n  root \"public\"\n}\n";
        let manifest = blastit_manifest::Manifest::from_blastfile_str(source).unwrap();
        assert_eq!(manifest.app(), None);
        assert_eq!(manifest.mounts().len(), 1);
        assert_eq!(manifest.mounts()[0].prefix, "/");

        // One route, one slot, no per-route static declaration.
        let routes = routes_from_manifest(&manifest, &UidLayout::new(20_000));
        assert_eq!(routes.len(), manifest.routes().len());
        assert_eq!(routes[0], RouteInfo::new("app", false, 21_000));
    }
}
