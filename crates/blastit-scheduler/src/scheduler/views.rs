//! The view-process lifecycle: spawned, ready, exited, retiring, gone.

use super::admission::{View, ViewSet};
use super::*;

impl Scheduler {
    /// init answered a `SPAWN` with a pid and the `view.chan` descriptor (§4).
    pub(super) fn view_spawned(
        &mut self,
        route: usize,
        pid: u32,
        fd: Option<OwnedFd>,
    ) -> Result<(), SchedulerError> {
        // §8 Sizing: this is the first moment the preloaded heap exists (see
        // `derive_view_cap`), so it is where the cap is measured for good.
        self.derive_view_cap(true);
        let Some(fd) = fd else {
            return Err(SchedulerError::protocol(
                "SPAWNED arrived without a view.chan descriptor",
            ));
        };
        let slot = self
            .pools
            .get_mut(&route)
            .and_then(|pool| pool.spawning.take());
        let Some((sent_epoch, _deadline)) = slot else {
            // Nothing waits for this process: close our end and have init reap
            // it, so an unknown view cannot linger holding a uid.
            drop(fd);
            self.dropped += 1;
            return self.retire_unheld(
                ViewKey { route, pid },
                RetireReason::Other("unexpected_spawn".to_string()),
            );
        };
        let key = ViewKey { route, pid };
        // A `bool` so the pool borrow ends before the retirement below.
        let duplicate = self
            .pools
            .get(&route)
            .is_some_and(|pool| pool.views.contains_key(&pid));
        if duplicate {
            // The kernel does not hand a live child's pid to a second child, so
            // the entry the pool holds was already gone and its `Exited` was
            // lost. Two processes must never share one `ViewKey`: retire the
            // pid — which kills the *newcomer* that now owns it, and fails the
            // dead entry's requests, drops its queue and emits its event
            // through the one path that does all four — then close our end of
            // the newcomer's channel.
            self.dropped += 1;
            drop(fd);
            return self.retire(key, RetireReason::Other("duplicate_pid".to_string()));
        }
        let std_stream = std::os::unix::net::UnixStream::from(fd);
        std_stream
            .set_nonblocking(true)
            .map_err(SchedulerError::io)?;
        let stream = UnixStream::from_std(std_stream).map_err(SchedulerError::io)?;
        let (read, write) = stream.into_split();
        let (out, outgoing, pending) = Outbound::new(self.cfg.frame_limits, VIEW_OUTBOUND_CAP);
        let limits = self.cfg.frame_limits;
        tokio::spawn(read_frames(
            read,
            limits,
            self.view_in.clone(),
            move |id, frame| ViewIn::Frame { key, id, frame },
            move |_| ViewIn::Closed { key },
        ));
        tokio::spawn(write_peer(write, limits, outgoing, pending));
        let pool = self.pools.entry(route).or_default();
        pool.views.insert(
            pid,
            View {
                key,
                ready: false,
                stopping: false,
                inflight: 0,
                sent: 0,
                next_request_id: 0,
                idle_since: None,
                last_used: 0,
                out: Some(out),
                exited: None,
                retiring: None,
                stale: None,
                spawned_epoch: sent_epoch,
            },
        );
        pool.pending_ready = pool.pending_ready.saturating_add(1);
        if sent_epoch < self.reload_epoch {
            // A worker forked for a `SPAWN` sent before the last reload is stale
            // from birth: retire it now rather than let it serve old code
            // (§4.6).
            self.retire(key, RetireReason::Reload)?;
        }
        Ok(())
    }

    /// A view is ready to serve (§4).
    ///
    /// A worker whose `SPAWN` predates the last reload holds the old code, so
    /// it is retired here instead of being made usable (§4.6). Either way this
    /// is where the worker stops being a `pending_ready` heap copy — the one
    /// decrement a worker gets, because the removal paths release it only for a
    /// worker that never became ready.
    pub(super) fn view_ready(&mut self, key: ViewKey) -> Result<(), SchedulerError> {
        let epoch = self.reload_epoch;
        let birth_stale = self
            .view_mut(key)
            .is_some_and(|view| view.spawned_epoch < epoch);
        if birth_stale {
            return self.retire(key, RetireReason::Reload);
        }
        let mut marked = false;
        if let Some(view) = self.view_mut(key) {
            let pending = !view.ready;
            view.ready = true;
            // A view that has never served anything is idle from now on, so
            // even a preforked process that nobody asked for is retired by
            // `BLASTIT_VIEW_IDLE_TIMEOUT` (§2.4).
            if view.inflight == 0 {
                view.idle_since = Some(Instant::now());
            }
            if pending && let Some(pool) = self.pools.get_mut(&key.route) {
                pool.pending_ready = pool.pending_ready.saturating_sub(1);
            }
            marked = true;
        }
        if marked {
            // §8 Sizing: this is the first moment the view's share of the
            // preloaded heap exists in the aggregate reading.
            self.measure_view_memory();
        }
        Ok(())
    }

    /// init reported a view process gone (§4).
    ///
    /// The process's own `view.chan` decides what it said before it went away:
    /// the reader task sends every frame it decoded and only then
    /// [`ViewIn::Closed`] ([`crate::task::read_frames`]), so failing the
    /// in-flight requests here would bill a request the view **answered** as a
    /// dead view with no cost. A rare `php-sapi` flake showed the shape: a
    /// contained bailout writes `RESP_HEAD`, `RESP_END_COST` and `DONE{Bailed}`
    /// and then retires by design (`crates/blastit-view/src/php.rs`), and
    /// `Ctl::Exited` can reach the scheduler first on the other channel.
    ///
    /// So the failure is left to [`Scheduler::view_gone`], which
    /// [`ViewIn::Closed`] runs *after* those frames, and
    /// [`Scheduler::expire_exited_views`] bounds that wait by
    /// `request_timeout` so a channel that never closes cannot hang a request.
    /// A process with no channel to wait for — one retired already, or one
    /// whose `SPAWNED` never arrived — is handled immediately.
    pub(super) fn view_exited(&mut self, key: ViewKey) {
        if key.pid == 0 {
            // No process exists: the `SPAWN` was refused or its fork failed
            // (§4.7).
            return self.spawn_failed(key.route);
        }
        let live = self.view_mut(key).is_some();
        let channel = live
            && self
                .pools
                .get(&key.route)
                .and_then(|pool| pool.views.get(&key.pid))
                .is_some_and(|view| view.out.is_some());
        if !channel {
            self.view_gone(key);
            return;
        }
        if let Some(view) = self.view_mut(key)
            && view.exited.is_none()
        {
            view.exited = Some(Instant::now());
        }
    }

    /// Marks a worker that answered `DONE { Bailed }` as retiring.
    ///
    /// The handler never returned, so the runtime retires the process after
    /// this request — the view says so one `ctl.chan` report before init's
    /// `Ctl::Exited` can. Until the ordered `view.chan` closes, the process is
    /// still alive and the frames it wrote are still being read, so this is the
    /// same window [`Scheduler::view_exited`] handles from the other channel:
    /// nothing is failed here. What changes is the next dispatch — a queued
    /// request must grow a replacement, never be handed to a worker that has
    /// already answered and is going away.
    pub(super) fn view_retiring(&mut self, key: ViewKey) {
        if let Some(view) = self.view_mut(key)
            && view.retiring.is_none()
        {
            view.retiring = Some(Instant::now());
        }
    }

    /// Fails the requests of a view on its way out whose `view.chan` never
    /// closed, `request_timeout` after the report.
    ///
    /// The ordered close is what normally decides; this is the bound that keeps
    /// a lost descriptor from turning a dead (or a retiring) view into a request
    /// that waits forever. It is only ever reached when no `ViewIn::Closed`
    /// came.
    pub(super) fn expire_exited_views(&mut self) {
        let now = Instant::now();
        let overdue: Vec<ViewKey> = self
            .pools
            .values()
            .flat_map(|pool| pool.views.values())
            .filter(|view| {
                view.leaving_at()
                    .is_some_and(|leaving| now >= leaving + self.cfg.request_timeout)
            })
            .map(|view| view.key)
            .collect();
        for key in overdue {
            self.view_gone(key);
        }
    }

    /// A view process is gone: fail its requests, free their slots, and let
    /// the next request for its route grow a new one (§2.4).
    ///
    /// The requests are failed and released whether or not the pool still holds
    /// the worker — a retired worker's channel closes too, and that close is
    /// what frees the slots its requests were holding. The route's queue is
    /// failed only when the fork that died never served **and** the route has no
    /// worker and no `SPAWN` left: a sibling that can answer must keep its
    /// requests (§4.7).
    pub(super) fn view_gone(&mut self, key: ViewKey) {
        let removed = self
            .pools
            .get_mut(&key.route)
            .filter(|pool| pool.views.get(&key.pid).is_some_and(|view| view.key == key))
            .and_then(|pool| pool.views.remove(&key.pid));
        let ready = removed.as_ref().is_some_and(|view| view.ready);
        if removed.is_some()
            && !ready
            && let Some(pool) = self.pools.get_mut(&key.route)
        {
            pool.pending_ready = pool.pending_ready.saturating_sub(1);
        }
        let affected: Vec<u32> = self
            .dispatch
            .iter()
            .filter(|((view, _), _)| *view == key)
            .map(|(_, stream_id)| *stream_id)
            .collect();
        for stream_id in affected {
            let _ = self.fail_request(stream_id, Outcome::ViewDied, 502);
            // The threads died with the process, so their slots are free even
            // though no `DONE` will ever arrive.
            self.release(stream_id);
        }
        if removed.is_some() && !ready {
            // A worker that never served anything: cool the route's *growth*
            // down. Its siblings keep serving, and their requests are untouched.
            self.backoff
                .insert(key.route, Instant::now() + self.cfg.spawn_backoff);
            let empty = self.pools.get(&key.route).is_none_or(ViewSet::is_empty);
            if empty {
                // No process left at all and no fork owed: the route's queue is
                // answered now, as it always was. The ids are collected first:
                // the iterator holds `&self` and the body takes `&mut self`.
                let waiting: Vec<u32> = self.queued_for_streams(key.route).collect();
                for stream_id in waiting {
                    self.queue.remove(stream_id);
                    let _ = self.fail_request(stream_id, Outcome::ViewDied, 502);
                    self.release(stream_id);
                }
            }
        }
        self.forget_if_idle(key.route);
    }

    // --------------------------------------------------------------- replies
}
