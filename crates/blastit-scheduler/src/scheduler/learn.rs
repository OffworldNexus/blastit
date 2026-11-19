//! The `--learn` fallback: one unrouted request at a time.

use super::*;

impl Scheduler {
    /// The view an unrouted `--learn` request is served by: the first route's,
    /// when the manifest declared one (`R26`).
    pub(super) fn learn_route(&self) -> Option<usize> {
        (!self.routes.is_empty()).then_some(0)
    }

    /// Starts the next unrouted `--learn` request, if the single slot is free
    /// and a view can take it (`R26`, design §4.2 D.2).
    ///
    /// The slot is what keeps `ObsTarget::Unrouted` attribution unambiguous:
    /// two unrouted requests never run at once, so every statement a session
    /// reports belongs to exactly one request.
    pub(super) fn pump_learn_fallback(&mut self) -> Result<(), SchedulerError> {
        if !self.cfg.learn_mode || self.learn_fallback_busy {
            return Ok(());
        }
        let Some(&stream_id) = self.learn_fallback_queue.front() else {
            return Ok(());
        };
        if !self.requests.contains_key(&stream_id) {
            self.learn_fallback_queue.pop_front();
            return Ok(());
        }
        let Some(route) = self.learn_route() else {
            // Nothing to run the request through: learn mode has no route to
            // observe, so the request is still an honest `404`.
            self.learn_fallback_queue.pop_front();
            return self.reject(
                stream_id,
                404,
                "Not Found",
                "not found\n",
                Outcome::Unmatched,
                None,
            );
        };
        if !self.can_dispatch(route) {
            if !self.has_dispatchable(route) && !self.draining {
                // Its request waits in `learn_fallback_queue`, not in `queue`,
                // so `needed_processes` would be 0 and the one fallback slot
                // would never get a process (§4.3).
                self.grow(route, 1)?;
            }
            return Ok(());
        }
        self.learn_fallback_queue.pop_front();
        self.learn_fallback_busy = true;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.learn_fallback = true;
            request.route = Some(route);
            // An unrouted request has no declared ingress: never short-circuit
            // it, and never buffer it (`R26`).
            request.body_plan = BodyPlan::StreamPassthrough;
            request.body_ready = true;
            request.queued_since = None;
        }
        self.dispatch(stream_id)
    }
}
