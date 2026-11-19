//! Retirement, reloads and the section-8 sizing supervision of view processes.

use super::admission::ViewSet;
use super::response::retry_after_seconds;
use super::*;

/// One §8 Sizing memory sample: the aggregate usage, and how many views had
/// sent `READY` when it was taken.
///
/// The difference between two samples is the memory one more view process
/// cost, which is the divisor §8's derived cap should have used all along.
#[derive(Debug, Clone, Copy)]
pub(super) struct MemSample {
    /// Aggregate usage in bytes.
    pub(super) used: u64,
    /// Views that had sent `READY`.
    pub(super) views: u32,
}
impl Scheduler {
    /// Rejects queued requests whose wait exceeded `BLASTIT_QUEUE_TIMEOUT`.
    pub(super) fn expire_queue(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let timeout = self.cfg.queue_timeout;
        let retry_after = retry_after_seconds(timeout);
        let expired: Vec<u32> = self
            .queue
            .ids()
            .into_iter()
            .filter(|stream_id| {
                self.requests
                    .get(stream_id)
                    .and_then(|request| request.queued_since)
                    .map(|since| now.duration_since(since) >= timeout)
                    .unwrap_or(false)
            })
            .collect();
        for stream_id in expired {
            self.queue.remove(stream_id);
            self.reject(
                stream_id,
                503,
                "Service Unavailable",
                "queue timeout\n",
                Outcome::QueueTimeout,
                Some(retry_after),
            )?;
        }
        // A structured body that never completes, and an unrouted `--learn`
        // request that never gets the single fallback slot, are bounded by the
        // same queue timeout.
        let mut waiting: Vec<u32> = self
            .requests
            .values()
            .filter(|request| {
                !request.body_ready
                    && request
                        .queued_since
                        .is_some_and(|since| now.duration_since(since) >= timeout)
                    && request.view.is_none()
            })
            .map(|request| request.stream_id)
            .collect();
        waiting.extend(
            self.learn_fallback_queue
                .iter()
                .copied()
                .filter(|stream_id| {
                    self.requests
                        .get(stream_id)
                        .and_then(|request| request.queued_since)
                        .is_some_and(|since| now.duration_since(since) >= timeout)
                }),
        );
        for stream_id in waiting {
            self.learn_fallback_queue
                .retain(|queued| *queued != stream_id);
            self.queue.remove(stream_id);
            self.reject(
                stream_id,
                503,
                "Service Unavailable",
                "queue timeout\n",
                Outcome::QueueTimeout,
                Some(retry_after),
            )?;
        }
        Ok(())
    }

    /// Gives a `504` to every dispatched request past its timeout, and recycles
    /// a view with too many overdue threads (§2.4).
    ///
    /// A request whose response is already over — answered or cancelled — is
    /// only *marked* overdue: its answer has been sent, its outcome is final,
    /// and writing another frame for it would corrupt the stream. It still
    /// counts towards the view's stuck threads, because its thread is still
    /// alive.
    pub(super) fn expire_requests(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let expired: Vec<(u32, Option<(ViewKey, u32)>)> = self
            .requests
            .values()
            .filter_map(|request| {
                if !request.slots || request.timed_out {
                    return None;
                }
                if now < request.deadline? {
                    return None;
                }
                Some((request.stream_id, request.view))
            })
            .collect();
        let mut affected: Vec<ViewKey> = Vec::new();
        for (stream_id, view) in expired {
            let (complete, head_sent) = match self.requests.get(&stream_id) {
                Some(request) => (request.complete, request.head_sent),
                None => continue,
            };
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.timed_out = true;
            }
            if let Some((key, _)) = view {
                affected.push(key);
            }
            if complete {
                continue;
            }
            if head_sent {
                self.http_push(
                    stream_id,
                    Frame::RespAbort {
                        reason: "request timeout".to_string(),
                    },
                )?;
            } else {
                self.respond(stream_id, 504, "Gateway Timeout", "gateway timeout\n", None)?;
            }
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.complete = true;
                request.outcome = Outcome::Timeout;
            }
            self.mark_finished(stream_id);
            // The timeout answer is final now, so its charge must precede its
            // event (`MB.3.45`): `emit_request` reports `charged_ns`, and the
            // request never pays the queue wait it spent before dispatch.
            self.charge(stream_id);
            self.emit_request(stream_id);
            if let Some((key, id)) = view {
                self.view_push(key, id, Frame::Cancel)?;
            } else {
                // A view-less request has no `DONE` coming. Its static slot is
                // handed back here, unless a read is parked on it: that read
                // cannot be cancelled, so its result releases the slot and the
                // descriptor (design §4.6).
                if self.static_read_parked(stream_id) {
                    continue;
                }
                self.release_static_slot(stream_id);
                self.release(stream_id);
            }
        }
        for key in affected {
            let overdue = self
                .requests
                .values()
                .filter(|request| {
                    request.view.map(|(view, _)| view) == Some(key) && request.timed_out
                })
                .count() as u32;
            if overdue > self.cfg.stuck_threads {
                self.retire(key, RetireReason::StuckThreads)?;
            }
        }
        Ok(())
    }
}
impl Scheduler {
    /// Retires what is finished: a worker over `BLASTIT_VIEW_MAX_REQUESTS` with
    /// nothing in flight, one a reload marked stale, or one idle for
    /// `BLASTIT_VIEW_IDLE_TIMEOUT` (§2.4).
    ///
    /// Every rule is per process now, so a burst's extras are the ones that go
    /// and a route keeps whatever its load needs.
    pub(super) fn retire_finished_views(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let mut victims: Vec<(ViewKey, RetireReason)> = Vec::new();
        for view in self.pools.values().flat_map(|pool| pool.views.values()) {
            if !view.ready || view.inflight > 0 {
                continue;
            }
            if view.stopping {
                victims.push((view.key, RetireReason::MaxRequests));
                continue;
            }
            if let Some(reason) = view.stale.clone() {
                // A reload arrived while this worker was busy: its last request
                // is `DONE`, so it can go now (§4.6).
                victims.push((view.key, reason));
                continue;
            }
            if !self.cfg.idle_timeout.is_zero()
                && let Some(since) = view.idle_since
                && now.duration_since(since) >= self.cfg.idle_timeout
            {
                victims.push((view.key, RetireReason::Idle));
            }
        }
        for (key, reason) in victims {
            self.retire(key, reason)?;
        }
        Ok(())
    }

    /// Retires one idle view when aggregate memory is above
    /// `BLASTIT_MEM_HIGH_WATERMARK` (§8 Sizing, §9.2 #8).
    ///
    /// One process per check and never one with a request in flight: the point
    /// is to give memory back, not to fail a request. With nothing idle the
    /// check does nothing, and the next one — `memory_check_interval` later —
    /// tries again.
    pub(super) fn check_memory(&mut self) -> Result<(), SchedulerError> {
        let now = Instant::now();
        let Some(at) = self.memory_check_at else {
            return Ok(());
        };
        if now < at {
            return Ok(());
        }
        // Count from now, so a scheduler that was busy for several intervals
        // cannot burst through a backlog and retire several views in a row.
        self.memory_check_at = Some(now + self.cfg.memory_check_interval);
        let Some(watermark) = self.cfg.mem_high_watermark_bytes else {
            return Ok(());
        };
        let Some(usage) = self.memory_now() else {
            return Ok(());
        };
        if usage <= watermark {
            return Ok(());
        }
        let Some(victim) = self.oldest_idle_view() else {
            return Ok(());
        };
        self.retire(victim, RetireReason::MemoryPressure)
    }

    /// The idle view that has gone unused the longest, which is the one memory
    /// pressure retires first (§8 Sizing: "oldest-first").
    ///
    /// A view with a request in flight is never a candidate; every worker of
    /// every route is, so a burst gives one process back at a time.
    pub(super) fn oldest_idle_view(&self) -> Option<ViewKey> {
        self.pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| view.ready && view.inflight == 0 && view.leaving_at().is_none())
            .min_by_key(|view| view.last_used)
            .map(|view| view.key)
    }

    /// Reads the machine's aggregate memory usage, if it can be read (§9.2 #8).
    pub(super) fn read_memory(&self) -> Option<u64> {
        self.cfg
            .memory_usage
            .as_ref()
            .and_then(|probe| probe.read())
    }

    /// Reads the probe now, and keeps the reading for the next
    /// [`Scheduler::memory`] call.
    pub(super) fn memory_now(&mut self) -> Option<u64> {
        let usage = self.read_memory();
        self.mem_reading = Some((Instant::now(), usage));
        usage
    }

    /// The aggregate usage in bytes, re-read only when the cached reading is
    /// older than `memory_check_interval`.
    ///
    /// §8 Sizing samples memory on that cadence anyway, and a spawn that is
    /// refused for memory is retried on every pump: without the cache a busy
    /// scheduler would read the probe per event rather than per check.
    pub(super) fn memory(&mut self) -> Option<u64> {
        if let Some((at, usage)) = self.mem_reading
            && at.elapsed() < self.cfg.memory_check_interval
        {
            return usage;
        }
        self.memory_now()
    }

    /// Whether aggregate usage is above `BLASTIT_MEM_HIGH_WATERMARK` (§8
    /// Sizing). Memory that cannot be read is not pressure.
    pub(super) fn above_watermark(&mut self) -> bool {
        let Some(watermark) = self.cfg.mem_high_watermark_bytes else {
            return false;
        };
        self.memory().is_some_and(|usage| usage > watermark)
    }

    /// Takes §8 Sizing's derived view cap (§8 Sizing, §9.2 #8).
    ///
    /// §8 asks for the cap to come from the usage *after preload*, and that
    /// moment is not when the scheduler starts: init forks every view from its
    /// supervision loop, which begins only once `BLASTIT_APP`,
    /// `BLASTIT_PRELOAD` and `BLASTIT_WARMUP` have been imported (§4), and the
    /// scheduler itself is forked before CPython exists. So the cap is taken
    /// twice:
    ///
    /// * `after_preload: false`, from [`Scheduler::start`], so the eager or
    ///   preforked burst obeys the cap it can already see rather than forking
    ///   everything the manifest names;
    /// * `after_preload: true`, at the first `SPAWNED` — the first proof the
    ///   preloaded heap is part of the reading — which replaces it, and which
    ///   also gives [`Scheduler::measure_view_memory`] the baseline it counts
    ///   the measured cost per view from.
    ///
    /// A reading that cannot be taken leaves the cap unset, which §8 asks for.
    pub(super) fn derive_view_cap(&mut self, after_preload: bool) {
        if self.cfg.max_view_processes.is_some()
            || self.measured_view_mem.is_some()
            || (self.derived_view_cap.is_some() && (self.derived_after_preload || !after_preload))
        {
            return;
        }
        let Some(total) = self.cfg.mem_total_bytes else {
            return;
        };
        let Some(used) = self.memory_now() else {
            return;
        };
        if after_preload {
            // §8 Sizing: this reading is the `used` every later derivation
            // starts from, and the baseline the measured cost is counted
            // against.
            self.preload_used = Some(used);
            self.mem_sample = Some(MemSample { used, views: 0 });
        }
        self.derived_view_cap = Some(derived_view_cap(total, used, self.cfg.view_mem_estimate));
        self.derived_after_preload = after_preload;
    }

    /// §8 Sizing: measures what one more view process costs, from the rise in
    /// aggregate usage per view that came up, and re-derives the cap from it.
    ///
    /// Measured cost is the number §8 Sizing's default was always missing: each
    /// view copies most of the preloaded heap (about 135 MiB under gVisor on
    /// CPython 3.14, against a `BLASTIT_VIEW_MEM_ESTIMATE` of 32 MiB), so a cap
    /// derived from the configured estimate bounds nothing at all. The estimate
    /// is a moving average clamped to at least the configured one: a machine
    /// that measures nothing keeps the operator's number, and one that measures
    /// more than that gets the cap that fits.
    ///
    /// Nothing happens when `BLASTIT_MAX_VIEW_PROCESSES` is set (§8 only
    /// derives the default), when the machine's memory cannot be read, or
    /// before the first `SPAWNED` gave a baseline to measure against.
    pub(super) fn measure_view_memory(&mut self) {
        if self.cfg.max_view_processes.is_some() || self.cfg.mem_total_bytes.is_none() {
            return;
        }
        let Some(usage) = self.memory_now() else {
            return;
        };
        let views = self.ready_view_count();
        let Some(previous) = self.mem_sample else {
            self.mem_sample = Some(MemSample { used: usage, views });
            return;
        };
        if views <= previous.views || usage <= previous.used {
            // Nothing to learn: no view came up since the last sample, or the
            // machine gave memory back rather than spending it. The sample
            // point still moves forward.
            self.mem_sample = Some(MemSample {
                used: usage.max(previous.used),
                views: views.max(previous.views),
            });
            return;
        }
        let added = u64::from(views - previous.views);
        let per_view = ((usage - previous.used) / added).max(self.cfg.view_mem_estimate);
        self.measured_view_mem = Some(match self.measured_view_mem {
            Some(previous) => (previous + per_view) / 2,
            None => per_view,
        });
        self.mem_sample = Some(MemSample { used: usage, views });
        self.derive_measured_cap();
    }

    /// The views that have sent `READY`, which is how many heap copies exist.
    ///
    /// Kept as the denominator of the measurement on purpose: a worker forked
    /// but not yet `READY` is not counted, so the rise it caused is attributed
    /// to the next sample and the per-view cost is *over*-estimated — the cap
    /// lands lower than it could, never higher.
    pub(super) fn ready_view_count(&self) -> u32 {
        u32::try_from(
            self.pools
                .values()
                .flat_map(|pool| pool.views.values())
                .filter(|view| view.ready)
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    /// Re-derives §8 Sizing's cap from the measured cost per view, and reports
    /// the pair when it changed.
    pub(super) fn derive_measured_cap(&mut self) {
        let (Some(total), Some(used), Some(estimate)) = (
            self.cfg.mem_total_bytes,
            self.preload_used,
            self.measured_view_mem,
        ) else {
            return;
        };
        let cap = derived_view_cap(total, used, estimate);
        self.derived_view_cap = Some(cap);
        self.derived_after_preload = true;
        if self.reported_cap != Some((estimate, cap)) {
            self.reported_cap = Some((estimate, cap));
            self.emit_cap(estimate, cap);
        }
    }

    /// Emits `process.cap` (§8.1): the measured cost per view and the cap it
    /// buys. Only ever emitted when one of the two changes.
    pub(super) fn emit_cap(&mut self, estimate: u64, cap: u32) {
        let event = Event::now(
            Component::Scheduler,
            EventKind::ProcessCap(ProcessCap::new(estimate, cap)),
        );
        (self.emit)(event);
    }

    /// The live view process cap in force: `BLASTIT_MAX_VIEW_PROCESSES`, or the
    /// §8 derived default once it has been measured.
    pub(super) fn view_cap(&self) -> Option<u32> {
        self.cfg.max_view_processes.or(self.derived_view_cap)
    }

    // ------------------------------------------------------------- lifecycle

    /// Forks one more worker for `route` until it has `wanted` processes (§4.3).
    ///
    /// Callers: a queued request's route ([`Scheduler::dispatch_queued`],
    /// `wanted = needed_processes`), or [`Scheduler::start`] for a
    /// preforked/eager route (`wanted = 1`). One call sends at most one `SPAWN`,
    /// and every bound below is independent of `slots`: the budget bounds
    /// requests in flight, and a worker with nothing to do is what `wanted`
    /// prevents.
    ///
    /// * one `SPAWN` in flight per route, because init forks sequentially and
    ///   buys nothing from a queue of them;
    /// * until the first real memory measurement, growth waits for
    ///   `pending_ready == 0`, so the derived cap is re-derived between two
    ///   forks instead of being priced from the estimate it is known to
    ///   understate (unless the operator pinned the cap);
    /// * never more than the requests need, nor more than the route may own,
    ///   nor more than the backoff, the starving neighbour, the eviction rule
    ///   and the live-process cap allow.
    pub(super) fn grow(&mut self, route: usize, wanted: u32) -> Result<(), SchedulerError> {
        if self.draining || !self.spawnable(route) {
            // A route the responder answers on its own never gets a view
            // (design §4.6): asking init for one would fork a process no
            // request could ever be dispatched to.
            return Ok(());
        }
        let pool = self.pools.get(&route);
        if pool.is_some_and(|pool| pool.spawning.is_some()) {
            return Ok(());
        }
        if self.measured_view_mem.is_none()
            && self.cfg.max_view_processes.is_none()
            && pool.is_some_and(|pool| pool.pending_ready > 0)
        {
            // Wait for the sample. With `BLASTIT_MAX_VIEW_PROCESSES` set there
            // is nothing to measure, so the gate would only serialize forks
            // for ever.
            return Ok(());
        }
        let owned = pool.map_or(0, ViewSet::len);
        if owned >= wanted {
            return Ok(());
        }
        if owned >= self.process_limit(route) {
            return Ok(());
        }
        if let Some(until) = self.backoff.get(&route).copied()
            && Instant::now() < until
        {
            // R9: a failed fork cools this route's *growth*, never its siblings.
            return Ok(());
        }
        if self.other_route_starved(route) {
            return Ok(());
        }
        if !self.make_room(route)? {
            return Ok(());
        }
        // `make_room` retires at most one process, and the cap it retired for
        // may still be exceeded — a measured cap can land below the number of
        // processes that are already live. Then the spawn waits for the next
        // round of shedding rather than adding to the overshoot.
        if let Some(cap) = self.view_cap()
            && self.live_processes() >= cap
        {
            return Ok(());
        }
        let epoch = self.reload_epoch;
        let route_id = u32::try_from(route).unwrap_or(u32::MAX);
        // The slot is the deadline `expire_spawns` bounds: a `SPAWN` init never
        // answers must not hold the route's growth for ever.
        self.pools.entry(route).or_default().spawning =
            Some((epoch, Instant::now() + self.cfg.request_timeout));
        if let Err(error) = self.ctl_push(Ctl::Spawn { route: route_id }) {
            if let Some(pool) = self.pools.get_mut(&route) {
                pool.spawning = None;
            }
            return Err(error);
        }
        Ok(())
    }

    /// A `SPAWN` that will not be answered: free the slot, cool the route's
    /// growth, leave every sibling and every queued request alone (§4.7).
    ///
    /// Reached from `EXITED { pid: 0 }` (init could not fork) and from
    /// [`Scheduler::expire_spawns`] (no answer at all). A message for a route
    /// with nothing outstanding is stale and does nothing.
    pub(super) fn spawn_failed(&mut self, route: usize) {
        let Some(pool) = self.pools.get_mut(&route) else {
            return;
        };
        if pool.spawning.take().is_none() {
            return;
        }
        self.backoff
            .insert(route, Instant::now() + self.cfg.spawn_backoff);
        // One `manifest.warning`, naming the route and nothing from any request.
        self.emit_warning(format!(
            "route {route}: the view process could not be started"
        ));
        self.forget_if_idle(route);
    }

    /// Frees every spawn slot whose `SPAWN` was never answered (§4.7).
    ///
    /// The slot is booked with `request_timeout` when the `SPAWN` is sent, so a
    /// message that never comes costs the route one backoff instead of holding
    /// its growth until the process exits.
    pub(super) fn expire_spawns(&mut self) {
        let now = Instant::now();
        let expired: Vec<usize> = self
            .pools
            .iter()
            .filter(|(_, pool)| pool.spawning.is_some_and(|(_, deadline)| now >= deadline))
            .map(|(route, _)| *route)
            .collect();
        for route in expired {
            self.spawn_failed(route);
        }
    }

    /// Makes room for a new view process for `route`, or says the spawn has to
    /// wait (§2.4, §8 Sizing).
    ///
    /// A full cap and a reading above `BLASTIT_MEM_HIGH_WATERMARK` both mean a
    /// process has to go first, and both use the same victim: the
    /// least-recently-used view that has nothing in flight and owes nothing to
    /// its own route's queue. Retiring one for memory pressure while the cap
    /// has room is deliberate — §8 Sizing would rather give a process back than
    /// add one to a machine that is already over the watermark.
    ///
    /// Returns `false` when the spawn must not happen: the caller leaves the
    /// request queued.
    pub(super) fn make_room(&mut self, route: usize) -> Result<bool, SchedulerError> {
        let cap_full = self
            .view_cap()
            .is_some_and(|cap| self.live_processes() >= cap);
        let over_watermark = self.above_watermark();
        if !cap_full && !over_watermark {
            return Ok(true);
        }
        let Some(victim) = self.evictable_view(route) else {
            return Ok(false);
        };
        let reason = if cap_full {
            RetireReason::LruCap
        } else {
            RetireReason::MemoryPressure
        };
        self.retire(victim, reason)?;
        Ok(true)
    }

    /// The view that may be retired to make room for `route`.
    ///
    /// It must be live, `READY`, have nothing in flight, and have no request
    /// waiting in the admission queue for its own route. A process that has not
    /// finished the request it was forked for — or one that is still waiting
    /// for its first `READY` — is never the victim: choosing one would kill the
    /// answer to a request that is about to be dispatched, and doing that once
    /// per pump is exactly the livelock this predicate exists to stop. The
    /// least recently used of the candidates wins, across every route but the
    /// asking one: a burst's coldest process goes before a route's only worker
    /// is ever considered.
    pub(super) fn evictable_view(&self, route: usize) -> Option<ViewKey> {
        self.pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| {
                view.key.route != route
                    && view.ready
                    && view.inflight == 0
                    // A worker on its way out frees itself; killing it would
                    // only report an eviction for a process that had already
                    // answered its last request (`MB.26.8`).
                    && view.leaving_at().is_none()
                    && !self.route_waiting(view.key.route)
            })
            .min_by_key(|view| view.last_used)
            .map(|view| view.key)
    }

    /// Whether any request still in the admission queue is routed to `route`.
    pub(super) fn route_waiting(&self, route: usize) -> bool {
        if let Some(queue) = self.queue.fair() {
            return queue.waiting_for_route(u32::try_from(route).unwrap_or(u32::MAX));
        }
        self.queue.ids().into_iter().any(|stream_id| {
            self.requests
                .get(&stream_id)
                .and_then(|request| request.route)
                == Some(route)
        })
    }

    /// Retires one view process: fail what it owes, ask init to kill it, and
    /// keep its slots until it is gone (§2.4).
    ///
    /// A key the pool no longer holds is a no-op: a retired worker's channel
    /// closes too, and that is not a second retirement. `pending_ready` is
    /// released here for a worker that never became `READY`, so a pool that
    /// retires a fork before its `READY` does not keep the route's growth
    /// waiting for a heap copy that no longer exists (§4.6).
    pub(super) fn retire(
        &mut self,
        key: ViewKey,
        reason: RetireReason,
    ) -> Result<(), SchedulerError> {
        let held = self
            .pools
            .get(&key.route)
            .and_then(|pool| pool.views.get(&key.pid))
            .is_some_and(|view| view.key == key);
        if !held {
            return Ok(());
        }
        let affected: Vec<u32> = self
            .dispatch
            .iter()
            .filter(|((view, _), _)| *view == key)
            .map(|(_, stream_id)| *stream_id)
            .collect();
        for stream_id in &affected {
            self.fail_request(*stream_id, Outcome::ViewDied, 502)?;
        }
        let removed = self
            .pools
            .get_mut(&key.route)
            .and_then(|pool| pool.views.remove(&key.pid));
        let Some(view) = removed else {
            return Ok(());
        };
        if !view.ready
            && let Some(pool) = self.pools.get_mut(&key.route)
        {
            pool.pending_ready = pool.pending_ready.saturating_sub(1);
        }
        // Dropping the queue ends the writer task, which closes the
        // scheduler's end of this view's channel.
        drop(view.out);
        // §2.4/§8 Sizing: a route must not be forked again immediately after a
        // retirement that was not about its own work. A worker retired for
        // memory pressure would otherwise be respawned by the very next request
        // and copy the same heap back, which is the retire-then-respawn thrash
        // §8 Sizing warns about. Retiring a worker that has served, or one a
        // reload invalidated, is not rate-limited: the first is ordinary LRU
        // work, and the second is what grows a fresh worker for the requests
        // already waiting (§4.7).
        if matches!(reason, RetireReason::MemoryPressure) {
            self.backoff
                .insert(key.route, Instant::now() + self.cfg.spawn_backoff);
        }
        let route_id = u32::try_from(key.route).unwrap_or(u32::MAX);
        self.ctl_push(Ctl::Retire {
            route: route_id,
            pid: key.pid,
            reason: reason.as_str().to_string(),
        })?;
        self.emit_retired(key, &reason);
        self.forget_if_idle(key.route);
        Ok(())
    }

    /// Kills a pid the pool does not hold.
    ///
    /// [`Scheduler::retire`] cannot: it is a no-op for a key with no live
    /// `View`, and this process must be killed or it keeps its route's uid and
    /// socket directory. No `process.retired` event is emitted: the decision was
    /// not about a worker the scheduler owns, and the `Exited` that follows is
    /// the event.
    pub(super) fn retire_unheld(
        &mut self,
        key: ViewKey,
        reason: RetireReason,
    ) -> Result<(), SchedulerError> {
        let live = self
            .pools
            .get(&key.route)
            .and_then(|pool| pool.views.get(&key.pid))
            .is_some_and(|view| view.key == key);
        if live {
            return self.retire(key, reason);
        }
        self.ctl_push(Ctl::Retire {
            route: u32::try_from(key.route).unwrap_or(u32::MAX),
            pid: key.pid,
            reason: reason.as_str().to_string(),
        })
    }

    /// `RELOAD`: retire every idle (or not-yet-ready) worker of every route,
    /// mark the busy ones stale, and make each route's in-flight `SPAWN` stale
    /// by epoch (§4.6).
    ///
    /// One transaction over every pool: a save must never leave one worker of a
    /// route serving the old code beside a fresh one. `admits` already refuses a
    /// stale worker, so a busy one takes no new request; it retires at `DONE`
    /// through [`Scheduler::retire_finished_views`]. Nothing here cools a route
    /// down, so the queued requests stay queued and the next pump's `grow` forks
    /// a fresh worker for them.
    pub(super) fn reload(&mut self) -> Result<(), SchedulerError> {
        self.reload_epoch = self.reload_epoch.wrapping_add(1);
        let keys: Vec<(ViewKey, bool)> = self
            .pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| view.stale.is_none())
            .map(|view| (view.key, view.ready && view.inflight > 0))
            .collect();
        for (key, busy) in keys {
            if !busy {
                self.retire(key, RetireReason::Reload)?;
            } else if let Some(view) = self.view_mut(key) {
                view.stale = Some(RetireReason::Reload);
            }
        }
        Ok(())
    }
}
