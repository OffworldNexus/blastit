//! What the scheduler knows about a route (ARCHITECTURE.md §2.4, §6).
//!
//! The routing table itself is [`blastit_manifest::RoutingTable`], which
//! matches `(method, PATH_INFO)` and returns a route *index*. The scheduler
//! keeps one [`RouteInfo`] per index, with the little that is not in the
//! routing table but is needed to serve the request: the route's name (for
//! events), whether its view is preforked, and the uid its process runs under.

use blastit_manifest::{Manifest, UidLayout};

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
}

impl RouteInfo {
    /// A route with this name, prefork flag and uid.
    pub fn new(name: impl Into<String>, prefork: bool, uid: u32) -> Self {
        Self {
            name: name.into(),
            prefork,
            uid,
        }
    }
}

/// Builds the scheduler's route list from a parsed manifest and the uid
/// layout init derived from it.
///
/// The order is the manifest's, which is the order
/// [`RoutingTable::route`] returns indices in, and which [`UidLayout::view`]
/// numbers uids by.
///
/// [`RoutingTable::route`]: blastit_manifest::RoutingTable::route
pub fn routes_from_manifest(manifest: &Manifest, layout: &UidLayout) -> Vec<RouteInfo> {
    manifest
        .routes()
        .iter()
        .enumerate()
        .map(|(index, route)| RouteInfo::new(route.name(), route.prefork(), layout.view(index)))
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
}
