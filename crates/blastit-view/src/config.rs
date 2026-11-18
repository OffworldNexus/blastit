//! What the transport knows about a view process.

use blastit_frame::Limits;

/// The transport-level configuration of one view process.
///
/// A view process serves exactly one manifest route under one uid
/// (ARCHITECTURE.md §2.5); these are the two fields every view kind needs, and
/// they are fixed for the process's lifetime.
///
/// The rest of a view's configuration belongs to its kind, because only that
/// kind can act on it: `blastit-view-python`'s `WsgiConfig` adds `SCRIPT_NAME`
/// and the worker cap, `blastit-view-php`'s `PhpRuntimeConfig` adds the entry
/// pin and the request limits. A second, optional field here would be one that
/// half the workers ignore.
#[derive(Debug, Clone, Default)]
pub struct ViewConfig {
    /// The route's name, for logs and for the application's own
    /// `blastit.route`.
    pub route_name: String,

    /// The frame size limit of `view.chan` (`BLASTIT_FRAME_MAX`, §3.1). Bodies
    /// larger than one frame are split into several `*_BODY` frames.
    pub frame_limits: Limits,
}

impl ViewConfig {
    /// The default configuration for `route`: default frame limits.
    pub fn for_route(route: impl Into<String>) -> Self {
        Self {
            route_name: route.into(),
            ..Self::default()
        }
    }
}
