//! What a view process is told about itself and its channel.

use blastit_frame::Limits;

/// The configuration of one view process.
///
/// A view process serves exactly one manifest route under one uid
/// (ARCHITECTURE.md §2.5); everything here is fixed for the process's lifetime.
#[derive(Debug, Clone, Default)]
pub struct ViewConfig {
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
    /// limit is a safety net (see [`crate::serve`]).
    pub max_threads: Option<u32>,
}

impl ViewConfig {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_route_fills_in_defaults() {
        let cfg = ViewConfig::for_route("web");
        assert_eq!(cfg.route_name, "web");
        assert!(cfg.script_name.is_empty());
        assert_eq!(cfg.frame_limits, Limits::default());
        assert_eq!(cfg.max_threads, None);
    }

    #[test]
    fn fields_can_be_overridden() {
        let cfg = ViewConfig {
            script_name: "/app".to_string(),
            max_threads: Some(4),
            ..ViewConfig::for_route("web")
        };
        assert_eq!(cfg.route_name, "web");
        assert_eq!(cfg.script_name, "/app");
        assert_eq!(cfg.max_threads, Some(4));
    }

    #[test]
    fn a_mount_point_is_empty_or_a_slash_prefixed_path() {
        for good in ["", "/app", "/deep/mount", "/a.b", "/app_v2"] {
            let cfg = ViewConfig {
                script_name: good.to_string(),
                ..ViewConfig::for_route("web")
            };
            assert!(cfg.validate().is_ok(), "{good:?} should be accepted");
        }
    }

    #[test]
    fn a_bad_mount_point_is_refused_with_a_reason() {
        // No leading slash, a trailing slash (including the lone slash: a mount
        // point that ends in `/` could not be stripped unambiguously), or a
        // doubled one.
        for bad in ["app", "app/", "/app/", "/", "//", "/app//sub/"] {
            let cfg = ViewConfig {
                script_name: bad.to_string(),
                ..ViewConfig::for_route("web")
            };
            let err = cfg.validate().expect_err("must be refused");
            assert!(err.contains("script_name"), "{bad:?}: {err}");
        }
    }
}
