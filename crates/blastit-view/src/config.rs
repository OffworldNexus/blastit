//! What a view process is told about itself and its channel.

use blastit_frame::Limits;

/// How many bytes of a request body one request may have queued (and not yet
/// consumed by the application) before the view gives up on it.
///
/// `wsgi.input` is fed by `REQ_BODY` frames, and the application decides when
/// to read them. An application that reads slowly — or not at all — would
/// otherwise let the I/O thread buffer the whole upload (ARCHITECTURE.md §3.1
/// says uploads are not buffered in memory). A bounded *channel* would instead
/// block the I/O thread and so stall every other request in the process, so the
/// queue is unbounded but metered: once a request holds more than this many
/// unconsumed bytes, the view stops queueing its body, marks the request
/// aborted (`RESP_ABORT`), and makes further reads raise `OSError`.
///
/// One mebibyte is the default frame limit (§3.1) and a comfortable margin over
/// the largest body a normal request carries; a legitimate multipart upload
/// streams, and streaming never queues more than the application is behind by.
pub const BODY_BUFFER_LIMIT: usize = 1024 * 1024;

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
    fn the_body_buffer_limit_holds_a_whole_body_frame() {
        // A body frame that fits the channel must never overflow the queue on
        // its own, or a normal single-frame upload could not be buffered.
        assert!(BODY_BUFFER_LIMIT >= Limits::default().max_payload());
    }
}
