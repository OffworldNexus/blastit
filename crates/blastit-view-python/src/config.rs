//! What a WSGI view process is told about itself and its channel.

use blastit_frame::Limits;

/// The configuration of one WSGI view process.
///
/// A view process serves exactly one manifest route under one uid
/// (ARCHITECTURE.md §2.5); everything here is fixed for the process's lifetime.
/// The transport-level half of the same configuration — the route's name and
/// the channel's frame limits — is `blastit_view::ViewConfig`; this one adds
/// what only a PEP 3333 interpreter has: `SCRIPT_NAME` and the worker cap.
#[derive(Debug, Clone, Default)]
pub struct WsgiConfig {
    /// The route's name, exposed to the application as `blastit.route`, for
    /// logging only.
    pub route_name: String,

    /// The route's mount point, used as `SCRIPT_NAME` and stripped from the
    /// front of `PATH_INFO`. Empty for a route mounted at the root.
    pub script_name: String,

    /// The frame size limit of `view.chan` (`BLASTIT_FRAME_MAX`, §3.1). Bodies
    /// larger than one frame are split into several `*_BODY` frames.
    pub frame_limits: Limits,

    /// The largest number of requests this process serves at once, or `None`
    /// for no limit. Every request gets its own OS thread, so the scheduler's
    /// global and per-route budgets are what normally keep this bounded; the
    /// limit is a safety net (see the `serve` loop's module documentation).
    pub max_threads: Option<u32>,
}

impl WsgiConfig {
    /// The default configuration for `route`: mounted at the root, default
    /// frame limits, no thread limit.
    pub fn for_route(route: impl Into<String>) -> Self {
        Self {
            route_name: route.into(),
            ..Self::default()
        }
    }

    /// Reject a `script_name` that cannot be a mount point.
    ///
    /// A mount point is empty (the route is mounted at the root) or a path with
    /// a leading slash and no trailing one, so that stripping it from a request
    /// path is unambiguous: `/app` is a mount point, `app` and `/app/` are not,
    /// because neither could be removed from `/app/list` without guessing where
    /// the boundary is.
    ///
    /// # Errors
    ///
    /// A message naming the offending value.
    pub fn validate(&self) -> Result<(), String> {
        let script = self.script_name.as_str();
        if script.is_empty() {
            return Ok(());
        }
        if !script.starts_with('/') {
            return Err(format!("script_name {script:?} must start with '/'"));
        }
        if script.ends_with('/') {
            return Err(format!(
                "script_name {script:?} must not end with '/'; use \"\" for the root"
            ));
        }
        Ok(())
    }
}
