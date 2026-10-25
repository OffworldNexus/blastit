//! What the scheduler knows about a route (ARCHITECTURE.md §2.4, §6).
//!
//! The routing table itself is [`blastit_manifest::RoutingTable`], which
//! matches `(method, PATH_INFO)` and returns a route *index*. The scheduler
//! keeps one [`RouteInfo`] per index, with the little that is not in the
//! routing table but is needed to serve the request: the route's name (for
//! events) and whether its view is preforked.
//!
//! The uid table stays in init (§4, §6): the scheduler names a route by its
//! index everywhere on `ctl.chan`, so it never needs a uid.

/// One manifest route, in routing-table order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// The route's `name` in the manifest, used as the `route` field of every
    /// event that belongs to it (§8.1).
    pub name: String,
    /// Whether the manifest asks for this view to exist at boot
    /// (`prefork = true`, §6). It also overrides `BLASTIT_VIEW_SPAWN`.
    pub prefork: bool,
}

impl RouteInfo {
    /// A route with this name and prefork flag.
    pub fn new(name: impl Into<String>, prefork: bool) -> Self {
        Self {
            name: name.into(),
            prefork,
        }
    }
}

/// Builds the scheduler's route list from a parsed manifest.
///
/// The order is the manifest's, which is the order [`RoutingTable::route`]
/// returns indices in.
///
/// [`RoutingTable::route`]: blastit_manifest::RoutingTable::route
pub fn routes_from_manifest(manifest: &blastit_manifest::Manifest) -> Vec<RouteInfo> {
    manifest
        .routes()
        .iter()
        .map(|route| RouteInfo::new(route.name(), route.prefork()))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn routes_follow_manifest_order() {
        let manifest = blastit_manifest::Manifest::from_toml_str(
            "version = 1\n\
             [[route]]\nname = \"first\"\npath = ['/a']\n\
             [[route]]\nname = \"hot\"\npath = ['/b']\nprefork = true\n",
        )
        .unwrap();
        assert_eq!(
            routes_from_manifest(&manifest),
            vec![RouteInfo::new("first", false), RouteInfo::new("hot", true),]
        );
        assert_eq!(RouteInfo::new("x", true).name, "x");
        assert!(RouteInfo::new("x", true).prefork);
    }
}
