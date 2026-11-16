//! The uid table (§2).

/// uids of blastit's components, derived from `BLASTIT_UID_BASE` (§2).
///
/// Every process derives the same table from the same manifest, so a view's uid
/// identifies its **sandbox**, not one endpoint of it: several routes of one
/// `view` carry the same sandbox and therefore run under one uid (§2.3, §7.3).
/// uids stay below [`UidLayout::MAX_UID_EXCLUSIVE`] for portability, which is
/// what [`crate::Manifest::validate`] enforces.
///
/// ```
/// use blastit_manifest::UidLayout;
///
/// let layout = UidLayout::default();
/// assert_eq!(layout.http(), 20_001);
/// assert_eq!(layout.scheduler(), 20_002);
/// assert_eq!(layout.dbproxy(), 20_003);
/// assert_eq!(layout.sandbox(0), 21_000);
/// assert_eq!(layout.sandbox(3), 21_003);
/// // The retired reader numbers one sandbox per route, so its routes number
/// // exactly as they did before sandboxes existed.
/// assert_eq!(layout.view(2), layout.sandbox(2));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UidLayout {
    /// Base of the layout, `BLASTIT_UID_BASE`; 20000 by default.
    pub base: u32,
}

impl UidLayout {
    /// The default base (§2, §8.2).
    pub const DEFAULT_BASE: u32 = 20_000;

    /// uids must stay strictly below this: DO maps the whole 32-bit range, but
    /// blastit stays under 65536 for portability (§2).
    pub const MAX_UID_EXCLUSIVE: u32 = 65_536;

    /// Offset of the first view uid above the base (§2).
    pub const VIEW_OFFSET: u32 = 1_000;

    /// Builds a layout on an explicit base.
    pub const fn new(base: u32) -> Self {
        Self { base }
    }

    /// The uid of the http process.
    pub const fn http(&self) -> u32 {
        self.base.saturating_add(1)
    }

    /// The uid of the scheduler process.
    pub const fn scheduler(&self) -> u32 {
        self.base.saturating_add(2)
    }

    /// The uid of the dbproxy process, which holds the real credentials.
    pub const fn dbproxy(&self) -> u32 {
        self.base.saturating_add(3)
    }

    /// The uid of the sandbox numbered `sandbox` (§2.3, §7.3).
    ///
    /// Every route of one `view` carries that view's [`crate::Route::sandbox`],
    /// so the whole view — its socket directory and its worker included — runs
    /// under one uid. A static mount owns no uid: it spawns no process.
    pub const fn sandbox(&self, sandbox: u32) -> u32 {
        self.uid_at(sandbox as u64)
    }

    /// The uid of the sandbox at `index`.
    ///
    /// The retired TOML reader numbers one sandbox per route, so this is its
    /// numbering: `view(route_index)` is the uid that route runs under. A
    /// Blastfile numbers by [`crate::Route::sandbox`] instead, through
    /// [`UidLayout::sandbox`].
    pub const fn view(&self, index: usize) -> u32 {
        self.uid_at(index as u64)
    }

    /// The uid `offset` places above [`UidLayout::VIEW_OFFSET`], saturating.
    const fn uid_at(&self, offset: u64) -> u32 {
        let uid = (self.base as u64)
            .saturating_add(Self::VIEW_OFFSET as u64)
            .saturating_add(offset);
        if uid > u32::MAX as u64 {
            u32::MAX
        } else {
            uid as u32
        }
    }
}

impl Default for UidLayout {
    fn default() -> Self {
        Self::new(Self::DEFAULT_BASE)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::collections::BTreeMap;

    use super::UidLayout;
    use crate::error::ManifestError;
    use crate::model::{Manifest, Route, StaticMount, StaticPolicy};

    /// A route in one sandbox, with nothing else declared.
    fn route(name: &str, sandbox: u32) -> Route {
        Route::new(
            name.to_string(),
            vec![".*".to_string()],
            None,
            Vec::new(),
            false,
            false,
            BTreeMap::new(),
        )
        .with_sandbox(sandbox)
    }

    /// A manifest of routes, in order.
    fn manifest(routes: Vec<Route>) -> Manifest {
        Manifest::assemble(
            1,
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            routes,
            Vec::new(),
        )
    }

    /// Two endpoints of one view share a sandbox and therefore one uid; the
    /// next sandbox takes the next uid (§2.3, §7.3).
    #[test]
    fn routes_of_one_sandbox_share_one_uid() {
        let layout = UidLayout::default();
        let get = route("cart-get", 0);
        let post = route("cart-post", 0);
        let about = route("about", 1);

        assert_eq!(
            layout.sandbox(get.sandbox()),
            layout.sandbox(post.sandbox()),
            "two endpoints of one view are two uids"
        );
        assert_eq!(layout.sandbox(get.sandbox()), 21_000);
        assert_eq!(
            layout.sandbox(about.sandbox()),
            layout.sandbox(get.sandbox()) + 1,
            "the next sandbox does not take the next uid"
        );
    }

    /// A mount is not a route: it owns no uid, so a static-only file allocates
    /// no view uid at all (§9.5).
    #[test]
    fn a_mount_owns_no_uid() {
        let mount = StaticMount {
            prefix: "/".to_string(),
            policy: StaticPolicy {
                root: "public".to_string(),
                index: "index.html".to_string(),
                spa: false,
                immutable: Vec::new(),
            },
        };
        let manifest = manifest(Vec::new()).with_mounts(vec![mount]);
        assert_eq!(manifest.mounts().len(), 1);
        assert!(manifest.routes().is_empty(), "a mount is not a route");

        // At a base whose *first* route uid is already out of range, a
        // mount-only manifest still validates: nothing was numbered.
        assert_eq!(
            manifest.validate(64_536).expect("a mount owns no uid"),
            Vec::new()
        );
        assert_eq!(UidLayout::default().sandbox(0), 21_000);
    }

    /// The range check counts the highest sandbox, not the route count: two
    /// routes of one sandbox cost one uid, and the second sandbox is what
    /// overflows (§7.3).
    #[test]
    fn the_highest_sandbox_is_what_the_range_check_counts() {
        // base 64535: sandbox 0 is 65535 (in range), sandbox 1 is 65536 (out).
        let one_sandbox = manifest(vec![route("cart-get", 0), route("cart-post", 0)]);
        assert_eq!(
            one_sandbox.validate(64_535).expect("one sandbox fits"),
            vec![]
        );

        let two_sandboxes = manifest(vec![
            route("cart-get", 0),
            route("cart-post", 0),
            route("about", 1),
        ]);
        let errors = two_sandboxes
            .validate(64_535)
            .expect_err("the second sandbox overflows");
        assert!(
            matches!(
                errors.errors()[0],
                ManifestError::TooManyRoutes {
                    index: 2,
                    uid: 65_536,
                    base: 64_535,
                    max: 65_536,
                }
            ),
            "{errors:?}"
        );
    }
}
