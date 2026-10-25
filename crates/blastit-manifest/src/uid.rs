//! The uid table (§2).

/// uids of blastit's components, derived from `BLASTIT_UID_BASE` (§2).
///
/// Every process derives the same table from the same manifest, so a view's uid
/// identifies its route. uids stay below [`UidLayout::MAX_UID_EXCLUSIVE`] for
/// portability, which is what [`crate::Manifest::validate`] enforces.
///
/// ```
/// use blastit_manifest::UidLayout;
///
/// let layout = UidLayout::default();
/// assert_eq!(layout.http(), 20_001);
/// assert_eq!(layout.scheduler(), 20_002);
/// assert_eq!(layout.dbproxy(), 20_003);
/// assert_eq!(layout.view(0), 21_000);
/// assert_eq!(layout.view(3), 21_003);
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

    /// The uid of the view process of the route at `index` (§6: routes are
    /// numbered in manifest order).
    pub const fn view(&self, index: usize) -> u32 {
        let uid = (self.base as u64)
            .saturating_add(Self::VIEW_OFFSET as u64)
            .saturating_add(index as u64);
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
