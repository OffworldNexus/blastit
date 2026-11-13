//! What the scheduler knows about a route (ARCHITECTURE.md §2.4, §6).
//!
//! The routing table itself is [`blastit_manifest::RoutingTable`], which
//! matches `(method, PATH_INFO)` and returns a route *index*. The scheduler
//! keeps one [`RouteInfo`] per index, with the little that is not in the
//! routing table but is needed to serve the request: the route's name (for
//! events), whether its view is preforked, the uid its process runs under, and
//! the `[route.ingress]` policy its request bodies are decoded with.

use blastit_ingress::{BodyCodec, IngressPolicy};
use blastit_manifest::{Manifest, StaticPolicy, UidLayout};

/// One manifest route, in routing-table order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// The route's `name` in the manifest, used as the `route` field of every
    /// event that belongs to it (§8.1).
    pub name: String,
    /// Whether the manifest asks for this view to exist at boot
    /// (`prefork = true`, §6). It also overrides `BLASTIT_VIEW_SPAWN`.
    pub prefork: bool,
    /// The uid the view process runs under, `UID_BASE + 1000 + index` (§6).
    /// The scheduler never *uses* it — only init does — but events carry it.
    pub uid: u32,
    /// The route's `[route.ingress]` policy (§4.2 A), or `None` when the route
    /// declared none — which is fail-closed exactly like `body = "none"`.
    pub ingress: Option<IngressPolicy>,
    /// The route's `[route.static]` policy (static design §4.2), or `None` when
    /// it declared none: a route with no policy is served by its view exactly
    /// as before.
    pub static_policy: Option<StaticPolicy>,
    /// Whether a request the static responder does not answer may reach the
    /// app (static design §4.6): the policy is present, its `spa` is unset and
    /// the manifest declares an `app`.
    pub static_forward: bool,
}

impl RouteInfo {
    /// A route with this name, prefork flag and uid, no declared ingress and no
    /// `[route.static]` policy.
    pub fn new(name: impl Into<String>, prefork: bool, uid: u32) -> Self {
        Self {
            name: name.into(),
            prefork,
            uid,
            ingress: None,
            static_policy: None,
            static_forward: false,
        }
    }

    /// This route with the `[route.ingress]` policy the manifest declared
    /// (§4.2 A).
    pub fn with_ingress(mut self, ingress: Option<IngressPolicy>) -> Self {
        self.ingress = ingress;
        self
    }

    /// This route with the `[route.static]` policy the manifest declared, and
    /// whether a miss may still reach the app (static design §4.2/§4.6).
    ///
    /// `static_forward` is `policy.is_some() && !policy.spa && has_app`, where
    /// `has_app` is `manifest.app().is_some()`. It is what stops a pure static
    /// route from ever spawning a view: with `spa = true` the fallback answers
    /// first, and without an `app` there is nothing to forward to.
    pub fn with_static(mut self, policy: Option<StaticPolicy>, has_app: bool) -> Self {
        self.static_forward = policy.as_ref().is_some_and(|policy| !policy.spa && has_app);
        self.static_policy = policy;
        self
    }

    /// The policy the scheduler enforces for this route's request bodies.
    ///
    /// A route that declared no `[route.ingress]` takes no body at all, exactly
    /// like `body = "none"` (§4.2 A: `None` at the use site is fail-closed).
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
/// Each route carries its `[route.ingress]` policy, so the scheduler can plan a
/// request body from the head alone (`R12`, `R16`), and its `[route.static]`
/// policy with the `static_forward` rule derived from the manifest's `app`
/// (static design §4.6).
///
/// [`RoutingTable::route`]: blastit_manifest::RoutingTable::route
pub fn routes_from_manifest(manifest: &Manifest, layout: &UidLayout) -> Vec<RouteInfo> {
    let has_app = manifest.app().is_some();
    manifest
        .routes()
        .iter()
        .enumerate()
        .map(|(index, route)| {
            RouteInfo::new(route.name(), route.prefork(), layout.view(index))
                .with_ingress(route.ingress())
                .with_static(route.static_policy().cloned(), has_app)
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn routes_follow_manifest_order_and_the_uid_layout() {
        let manifest = blastit_manifest::Manifest::from_toml_str(
            "version = 1\n\
             [[route]]\nname = \"first\"\npath = ['/a']\n\
             [[route]]\nname = \"hot\"\npath = ['/b']\nprefork = true\n",
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

        // Fail-closed: no `[route.ingress]` is `body = "none"`.
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

        let manifest = blastit_manifest::Manifest::from_toml_str(
            "version = 1\n[[route]]\nname = \"api\"\npath = ['/api']\n",
        )
        .unwrap();
        assert_eq!(
            routes_from_manifest(&manifest, &UidLayout::new(20_000))[0].ingress,
            None
        );
    }

    /// A `[route.static]` policy with `spa` set as given.
    fn static_policy(spa: bool) -> StaticPolicy {
        StaticPolicy {
            root: "public".to_string(),
            index: "index.html".to_string(),
            spa,
            immutable: Vec::new(),
        }
    }

    #[test]
    fn static_forward_is_a_policy_without_spa_and_with_an_app() {
        // The one shape that may forward, and therefore spawn a view.
        let route = RouteInfo::new("site", false, 7).with_static(Some(static_policy(false)), true);
        assert_eq!(route.static_policy, Some(static_policy(false)));
        assert!(route.static_forward);

        // `spa = true` answers every miss itself.
        let route = RouteInfo::new("site", false, 7).with_static(Some(static_policy(true)), true);
        assert!(!route.static_forward);
        // No `app` in the manifest: nothing to forward to.
        let route = RouteInfo::new("site", false, 7).with_static(Some(static_policy(false)), false);
        assert!(!route.static_forward);

        // A route with no policy is the unchanged view path.
        let plain = RouteInfo::new("app", false, 7);
        assert_eq!(plain.static_policy, None);
        assert!(!plain.static_forward);
        let route = plain.with_static(None, true);
        assert_eq!(route.static_policy, None);
        assert!(!route.static_forward);
    }

    #[test]
    fn routes_from_manifest_carries_the_policy_and_the_app_rule() {
        let front = blastit_manifest::Manifest::from_toml_str(
            "version = 1\napp = \"app:app\"\n\
             [[route]]\nname = \"site\"\npath = ['.*']\n\
             [route.static]\nroot = \"public\"\n",
        )
        .unwrap();
        let routes = routes_from_manifest(&front, &UidLayout::new(20_000));
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].static_policy, Some(static_policy(false)));
        assert!(routes[0].static_forward);

        let all_static = blastit_manifest::Manifest::from_toml_str(
            "version = 1\n\
             [[route]]\nname = \"site\"\npath = ['.*']\n\
             [route.static]\nroot = \"public\"\nspa = true\n",
        )
        .unwrap();
        let routes = routes_from_manifest(&all_static, &UidLayout::new(20_000));
        assert_eq!(routes[0].static_policy, Some(static_policy(true)));
        assert!(!routes[0].static_forward);
    }
}
