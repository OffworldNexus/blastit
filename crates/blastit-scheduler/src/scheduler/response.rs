//! Responses the scheduler writes, what a request is charged, and the
//! `request` / `process.retired` events.

use super::*;

impl Scheduler {
    /// Sends a small `text/plain` response the scheduler wrote itself.
    ///
    /// `retry_after` is §8's `Retry-After` in seconds, which every `503` the
    /// scheduler writes carries: a client that is turned away because the
    /// server is busy should be told when to come back, and §2.4's queue
    /// timeout is the number the server can actually promise.
    pub(super) fn respond(
        &mut self,
        stream_id: u32,
        status: u16,
        reason: &str,
        body: &str,
        retry_after: Option<u64>,
    ) -> Result<(), SchedulerError> {
        self.respond_with(stream_id, status, reason, body, retry_after, &[])
    }

    /// [`Scheduler::respond`], with headers the caller adds after the two the
    /// canned answers always carry.
    ///
    /// The static refusals are the one caller: `Allow` on the `405`, and
    /// `Cache-Control: no-store` on all four (design §4.4). The app path passes
    /// no extra header, so its bytes are unchanged.
    pub(super) fn respond_with(
        &mut self,
        stream_id: u32,
        status: u16,
        reason: &str,
        body: &str,
        retry_after: Option<u64>,
        extra: &[(&[u8], &[u8])],
    ) -> Result<(), SchedulerError> {
        let mut headers = vec![
            (
                b"Content-Type".to_vec(),
                b"text/plain; charset=utf-8".to_vec(),
            ),
            (
                b"Content-Length".to_vec(),
                body.len().to_string().into_bytes(),
            ),
        ];
        if let Some(seconds) = retry_after {
            headers.push((b"Retry-After".to_vec(), seconds.to_string().into_bytes()));
        }
        for (name, value) in extra {
            headers.push((name.to_vec(), value.to_vec()));
        }
        let head = RespHead {
            status,
            reason: reason.as_bytes().to_vec(),
            headers,
        };
        self.http_push(stream_id, Frame::RespHead(head))?;
        if !body.is_empty() {
            self.http_push(stream_id, Frame::RespBody(body.as_bytes().to_vec()))?;
        }
        self.http_push(stream_id, Frame::RespEnd)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.head_sent = true;
            request.status = status;
            request.bytes_out = body.len() as u64;
        }
        self.mark_finished(stream_id);
        Ok(())
    }

    /// Records where the response ended, which is where `duration_ms` stops
    /// (§8.1) even though the `request` event may wait for the view's `DONE`.
    pub(super) fn mark_finished(&mut self, stream_id: u32) {
        if let Some(request) = self.requests.get_mut(&stream_id)
            && request.finished_at.is_none()
        {
            request.finished_at = Some(Instant::now());
        }
    }

    /// Answers a request the scheduler refused, and logs it.
    pub(super) fn reject(
        &mut self,
        stream_id: u32,
        status: u16,
        reason: &str,
        body: &str,
        outcome: Outcome,
        retry_after: Option<u64>,
    ) -> Result<(), SchedulerError> {
        self.respond(stream_id, status, reason, body, retry_after)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.outcome = outcome;
            request.complete = true;
        }
        self.mark_finished(stream_id);
        // Charge before the event that reports the charge: a
        // scheduler-written answer is complete here, so this span is final and
        // the event must not say `cost_ns: 0`.
        self.charge(stream_id);
        self.emit_request(stream_id);
        self.release(stream_id);
        Ok(())
    }

    /// Answers a request whose body the route will not take, cancelling a view
    /// that may already have seen the head (`R12`, `R14`, `R15`).
    ///
    /// The view keeps its slot until its `DONE` arrives, exactly as a client
    /// cancellation does: the response is over, but the thread is still alive.
    pub(super) fn refuse_body(
        &mut self,
        stream_id: u32,
        status: u16,
    ) -> Result<(), SchedulerError> {
        let view = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.view);
        if let Some((key, id)) = view {
            self.view_push(key, id, Frame::Cancel)?;
        }
        let (reason, body) = canned(status);
        self.respond(stream_id, status, reason, body, None)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = Outcome::Unmatched;
        }
        self.mark_finished(stream_id);
        // The response is over, so its span is final here even though the slot
        // is not: charge and emit now . The fair-queue debit lands
        // at this refusal, not at the `DONE` that frees the slot
        // ([`Scheduler::charge`]).
        self.charge(stream_id);
        self.emit_request(stream_id);
        if view.is_none() {
            self.release(stream_id);
        }
        Ok(())
    }

    /// Marks an answer the scheduler just wrote as complete, and logs it.
    pub(super) fn finish_immediately(
        &mut self,
        stream_id: u32,
        outcome: Outcome,
    ) -> Result<(), SchedulerError> {
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.outcome = outcome;
            request.complete = true;
        }
        self.mark_finished(stream_id);
        // The span is final here, so the event carries it.
        self.charge(stream_id);
        self.emit_request(stream_id);
        self.release(stream_id);
        Ok(())
    }

    /// Fails a request to http: a `502` when nothing was sent yet, a
    /// `RESP_ABORT` once the head is out.
    pub(super) fn fail_request(
        &mut self,
        stream_id: u32,
        outcome: Outcome,
        status: u16,
    ) -> Result<(), SchedulerError> {
        let (head_sent, complete) = match self.requests.get(&stream_id) {
            Some(request) => (request.head_sent, request.complete),
            None => return Ok(()),
        };
        if complete {
            return Ok(());
        }
        if head_sent {
            self.http_push(
                stream_id,
                Frame::RespAbort {
                    reason: outcome.as_str().to_string(),
                },
            )?;
        } else {
            let (reason, body) = canned(status);
            // A `502` is not a `503`: the server is not busy, the view died.
            self.respond(stream_id, status, reason, body, None)?;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = outcome;
        }
        self.mark_finished(stream_id);
        // The scheduler wrote this answer; charge the span it held a worker for
        // before the event reports it . A request that died with its
        // view still keeps the dispatch→completion rule: no view answered it.
        self.charge(stream_id);
        self.emit_request(stream_id);
        Ok(())
    }

    /// Releases a request: its worker slot, its view id and its memory.
    pub(super) fn release(&mut self, stream_id: u32) {
        self.charge(stream_id);
        self.emit_request(stream_id);
        let Some(request) = self.requests.remove(&stream_id) else {
            return;
        };
        if let Some((key, id)) = request.view {
            self.dispatch.remove(&(key, id));
            if let Some(view) = self.view_mut(key) {
                view.inflight = view.inflight.saturating_sub(1);
                if view.inflight == 0 {
                    view.idle_since = Some(Instant::now());
                }
            }
        }
        if request.slots {
            self.slots = self.slots.saturating_sub(1);
        }
        self.queue.remove(stream_id);
        if request.learn_fallback {
            // The single `--learn` slot is free again; `pump` starts the next
            // unrouted request right after this event.
            self.learn_fallback_busy = false;
        }
        if let Some(flow) = request.flow {
            self.queue.release_flow(flow, Instant::now());
        }
    }
}
impl Scheduler {
    /// What this request cost, and what the flow is charged for it (D5, `R19`).
    ///
    /// For a request a view answered, the cost is the view's own report when
    /// there is one, **clamped** into `[floor, observed]`, where `observed` is
    /// the time the scheduler saw between dispatch and completion and `floor`
    /// the time up to the view's first response frame. Both bounds are the
    /// scheduler's own measurements, and both are needed because a view process
    /// is sandboxed and may be compromised: a report that claims more time than
    /// the request could possibly have taken would let one client buy the whole
    /// queue, and a report of nothing would let it keep its place at the front
    /// for free — while the floor is exactly the time the view cannot hide,
    /// because shortening it means delaying its own client's response.
    ///
    /// With no report — the view crashed, timed out, was cancelled, the client
    /// went away, or the view was built before D5 — the observed time is charged
    /// instead.
    ///
    /// A request a view **bailed out of** (`view_answered`, `bailed` and
    /// `status >= 500`: a `5xx` a contained `exit()`/`die()`/fatal answered —
    /// `zend_bailout()` swallowed by the SAPI shim, reported by `DONE`) is
    /// charged `R19`'s crash/`504` ceiling instead, and its report is not
    /// believed at all. A failing view is exactly the one whose report must not
    /// be believed: a script that dies 18 ms into a request but says it worked
    /// for 3 ms would let a client crash-loop for a fifth of its cost.
    /// `ceiling = observed_wall_ns * max_threads`, and the live thread count of
    /// the single-threaded PHP worker `ADR-0010` forks is one; no kernel floor
    /// can exceed one process's own wall time, so `ceiling.max(kernel_floor_ns)`
    /// is that wall. The span charged is the one the `request` event reports as
    /// `duration_ms` — arrival to the last byte — so a bailed request is always
    /// charged at least the wall time its client waited.
    ///
    /// The requirement is both a view's own `RESP_HEAD` **and** a `DONE` that
    /// says `bailed` ([`answered_a_bailout`]): a `5xx` an app answered itself
    /// returned from its handler, so it keeps the clamp like any other answer,
    /// and a `5xx` the scheduler wrote itself (the `502` of a view that never
    /// answered, a `504` timeout, a `503` refusal) keeps the
    /// dispatch→completion rule — the view never reported anything for it, so
    /// there is nothing of its to distrust, and the crash bill's
    /// arrival→last-byte span would charge a queue wait the request never spent
    /// on a worker.
    ///
    /// It is idempotent, and every path that concludes a request charges before
    /// it emits that request's `request` event: the response end,
    /// the scheduler's own answer, a cancellation, a refusal, a view failure.
    /// [`Scheduler::release`] charges as the fallback for the requests no
    /// earlier path could charge. Two of them are the deferred `5xx` a view
    /// answered: the one whose `DONE` labels it, and the one whose
    /// view's channel closes before that `DONE` ever arrives
    /// ([`Scheduler::view_gone`] cannot charge it either, because it is already
    /// complete). The third is a `DONE` that answers a response whose head is
    /// out but whose `RESP_END` never came ([`Scheduler::view_done`]'s
    /// `head_sent && !complete` arm). Every other request is charged by the
    /// path that concluded it, and the second call is a no-op.
    ///
    /// The fair-queue debit therefore lands at the first of those charges, not
    /// at the release that frees the slot: when the request already has a view,
    /// a client cancellation and a refused body both leave the slot held to
    /// that view's `DONE`, so the flow is debited when the response ends and
    /// its release comes later; a view-less request releases in the
    /// same turn, so its debit and its release coincide. That endpoint is
    /// deliberate: the span billed is the request's own, and splitting the
    /// debit from the charge would need a residual carried to release while the
    /// event must already report the charge.
    pub(super) fn charge(&mut self, stream_id: u32) {
        let Some(request) = self.requests.get(&stream_id) else {
            return;
        };
        if request.charged {
            return;
        }
        let now = Instant::now();
        let observed = request
            .dispatched_at
            .map(|since| now.duration_since(since))
            .map(nanos)
            .unwrap_or(0);
        let floor = match (request.dispatched_at, request.head_at) {
            // Dispatch to the view's first frame: the view's own work on the
            // request, which it cannot hide without delaying its client.
            (Some(dispatched), Some(head)) => head.duration_since(dispatched),
            _ => Duration::ZERO,
        };
        let floor = nanos(floor)
            // A clock that cannot go backwards still cannot make the floor the
            // ceiling: `clamp` would panic on `floor > observed`.
            .min(observed);
        // The event's own `duration_ms` span (`arrival` to the last byte), which
        // is what `R19`'s ceiling is taken from when a view failed the request;
        // `finished_at` is set before every charge, and `now` is the same
        // instant the event would fall back to.
        let finished = request.finished_at.unwrap_or(now);
        let charged = charge_ns(Charge {
            reported: request.report.map(|report| report.wall_ns),
            floor,
            observed,
            wall: nanos(finished.saturating_duration_since(request.arrived)),
            failed: answered_a_bailout(request.view_answered, request.status, request.bailed),
        });
        let flow = request.flow;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.charged_ns = charged;
            request.charged = true;
        }
        if let Some(flow) = flow {
            self.queue.debit(&flow, charged, Instant::now());
        }
    }

    /// Emits the one `request` event a request gets, once (§8.1).
    ///
    /// [`Scheduler::charge`] runs first, always: the event reports
    /// `charged_ns`, so an event emitted before its charge says `cost_ns: 0`
    /// for a request that did hold a worker . The debug assertion is
    /// what keeps that ordering true for every path.
    pub(super) fn emit_request(&mut self, stream_id: u32) {
        let Some(request) = self.requests.get(&stream_id) else {
            return;
        };
        if request.emitted {
            return;
        }
        debug_assert!(
            request.charged,
            "a request event is emitted only after the request was charged"
        );
        let now = Instant::now();
        let queued_until = request.dispatched_at.unwrap_or(now);
        let queued_ms = millis(queued_until.duration_since(request.arrived));
        // §8.1: `duration_ms` is the time to the last response byte, which is
        // where `finished_at` was set. The event itself may wait for the view's
        // `DONE` (D5), which is where the cost report travels.
        let duration_ms = millis(
            request
                .finished_at
                .unwrap_or(now)
                .duration_since(request.arrived),
        );
        // §8.1: the `path` is the routed `PATH_INFO`, and never a query
        // string. A target the scheduler could not decode has no `PATH_INFO`
        // at all, so it reports the target with its query string removed.
        //
        // Both are handed to `from_target` as borrowed text, which is what
        // strips the query and allocates the `path` once: cloning `path_info`
        // first would allocate the same string twice per request.
        let method = String::from_utf8_lossy(&request.head.method);
        let routed_path = match &request.path_info {
            Some(path_info) => Cow::Borrowed(path_info.as_str()),
            None => String::from_utf8_lossy(path_of_target(&request.head.target)),
        };
        let mut info = RequestInfo::from_target(&*method, &routed_path)
            .with_status(request.status)
            .with_outcome(request.outcome)
            .with_timings(queued_ms, duration_ms)
            .with_bytes(request.bytes_in, request.bytes_out)
            .with_remote_addr(request.head.remote_addr.clone())
            .with_cost(
                request.charged_ns,
                // The view's own CPU measurement, when it could take one:
                // absent is not the same as zero (§8.1).
                request.report.and_then(|report| report.cpu_ns),
            );
        if let Some(rule) = request.rule {
            info = info.with_rule(rule);
        }
        let mut event = Event::now(Component::Scheduler, EventKind::Request(info))
            .with_request_id(request.request_id.clone());
        if let Some(route) = request.route
            && !request.learn_fallback
            && let Some(route) = self.routes.get(route)
        {
            event = event.with_route(route.name.clone());
        } else if !request.learn_fallback && served_by_static(request) {
            // §9.2: the `static` mount answered, and saying so is the whole
            // point of the column: a served file used to read
            // `GET /css/app.css` with an unmatched route, as if nothing had
            // matched it. A request neither a mount nor a route claimed keeps
            // `None` and still renders `(unmatched)`.
            event = event.with_route("static".to_string());
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.emitted = true;
        }
        (self.emit)(event);
    }

    /// Emits `process.retired` for a view the scheduler retired (§8.1).
    pub(super) fn emit_retired(&mut self, key: ViewKey, reason: &RetireReason) {
        let uid = self
            .routes
            .get(key.route)
            .map(|route| route.uid)
            .unwrap_or(0);
        let mut event = Event::now(
            Component::Scheduler,
            EventKind::ProcessRetired(ProcessRetired::new(key.pid, uid, reason.clone())),
        );
        if let Some(route) = self.routes.get(key.route) {
            event = event.with_route(route.name.clone());
        }
        (self.emit)(event);
    }

    // ----------------------------------------------------------- view frames
}
impl Scheduler {
    /// `DONE` from a view: the thread exited, so the slot is free (§2.4).
    ///
    /// A `DONE` that arrives twice releases the slot the first time and is
    /// counted as dropped the second (`release` removed the dispatch entry). One
    /// that arrives for a request the scheduler already completed — its `504`,
    /// its `502`, a cancellation, or the `5xx` it held back for exactly this
    /// frame — still frees the slot; the only thing it can change is the label
    /// of a `5xx` the view itself answered
    /// ([`a_done_may_relabel_a_completed_5xx`]).
    ///
    /// This frame is also the only one that says whether the handler
    /// **returned** or **bailed**, so a `5xx` a view answered is labelled and
    /// charged here rather than at its `RESP_END` (see [`Scheduler::view_end`]).
    /// `view_died` stays what the events crate says it is — the view process
    /// died mid-request — and a `5xx` a view **completed** is never that,
    /// whether its handler returned (`ok`) or bailed (`bailed`).
    pub(super) fn view_done(
        &mut self,
        key: ViewKey,
        id: u32,
        outcome: DoneOutcome,
    ) -> Result<(), SchedulerError> {
        // A `DONE { Bailed }` the worker sends to conclude its **own** request
        // retires it: the handler never returned, so the runtime retires the
        // process after that request, and the pump between this frame and the
        // close must grow a replacement for the queue instead of dispatching
        // into it.
        //
        // A `DONE { Bailed }` for a request the scheduler already answered
        // itself is `LateDone`'s shape: its slot was freed by that
        // answer, and the worker's fate is the ordered close's business, so the
        // route still dispatches to it. Two things narrow the mark to the first
        // shape, and neither is a policy this frame could widen:
        //
        // * the early return just below — no dispatch entry, no mark — which is
        //   what a `DONE` arriving after `release` hits (a repeated `DONE`, or a
        //   request the scheduler answered and let go); and
        // * the `!timed_out && !cancelled` guard below, which is what a cancel
        //   or a timeout of a *dispatched* request hits: `release` has not run
        //   yet (it runs at the bottom of this function, because the view keeps
        //   its slot until its `DONE`), so the entry is still there and that
        //   guard is the only thing that speaks.
        //
        // So a client disconnect during a handler that bails is **not**
        // covered: the worker still retires (`crates/blastit-view/src/php.rs`
        // sets `worker.bailed |= retire` and `recycle` acts on it), but its
        // `DONE` is forced to `Aborted` for the cancelled stream (`if
        // io.cancelled`), `cancelled` is set, and no mark is made. Such a
        // worker is bounded only by `expire_exited_views`, one `request_timeout`
        // after its channel closes, instead of at the next dispatch. The fix is
        // for the transport to say it is retiring — an explicit `RETIRING`
        // frame rather than an outcome the scheduler has to infer — which is a
        // protocol change and belongs to its own node.
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let (complete, head_sent, view_answered, cancelled, timed_out, status) =
            match self.requests.get(&stream_id) {
                Some(request) => (
                    request.complete,
                    request.head_sent,
                    request.view_answered,
                    request.cancelled,
                    request.timed_out,
                    request.status,
                ),
                None => return Ok(()),
            };
        if outcome == DoneOutcome::Bailed && !timed_out && !cancelled {
            self.view_retiring(key);
        }
        let bailed = outcome == DoneOutcome::Bailed;
        if !complete {
            match outcome {
                DoneOutcome::Completed | DoneOutcome::Bailed if head_sent => {
                    // The thread finished; only its `RESP_END` is missing.
                    self.http_push(stream_id, Frame::RespEnd)?;
                    if let Some(request) = self.requests.get_mut(&stream_id) {
                        request.complete = true;
                        request.bailed = bailed;
                        if !request.cancelled {
                            request.outcome =
                                completed_outcome(view_answered, request.status, bailed);
                        }
                    }
                    self.mark_finished(stream_id);
                }
                DoneOutcome::Completed | DoneOutcome::Bailed | DoneOutcome::Errored => {
                    self.fail_request(stream_id, Outcome::ViewDied, 502)?;
                }
                DoneOutcome::Aborted => {
                    let outcome = if cancelled {
                        Outcome::ClientGone
                    } else {
                        Outcome::ViewDied
                    };
                    let status = if head_sent { status } else { 502 };
                    self.fail_request(stream_id, outcome, status)?;
                }
            }
        } else if let Some(request) = self.requests.get_mut(&stream_id) {
            // A completed `5xx` was held back for this frame: whether the
            // handler returned (the app's own failure, an answer) or bailed
            // (the `R19` crash) is a fact only `DONE` carries.
            if a_done_may_relabel_a_completed_5xx(
                request.view_answered,
                request.cancelled,
                request.status,
            ) {
                request.bailed = bailed;
                request.outcome = completed_outcome(request.view_answered, request.status, bailed);
            }
        }
        // `release` is what charges the request and emits its event, so a `5xx`
        // is billed exactly once, with the label this frame decided.
        self.release(stream_id);
        Ok(())
    }

    /// `RESP_HEAD` from a view.
    pub(super) fn view_head(
        &mut self,
        key: ViewKey,
        id: u32,
        head: RespHead,
    ) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let forward = match self.requests.get(&stream_id) {
            Some(request) => !request.complete && !request.head_sent,
            None => false,
        };
        if !forward {
            return Ok(());
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.status = head.status;
            request.head_sent = true;
            // The status a `DONE` may label this response with came from the
            // view itself, which is a different fact from "a head went out":
            // the scheduler's own `5xx` must not become `bailed`.
            request.view_answered = true;
            // D5: the first frame the view produced is the floor of what its
            // request may be charged — the view cannot shorten that without
            // delaying its own response.
            request.head_at = Some(Instant::now());
        }
        self.http_push(stream_id, Frame::RespHead(head))
    }

    /// `RESP_BODY` from a view, which may not exceed the credit http granted.
    pub(super) fn view_body(
        &mut self,
        key: ViewKey,
        id: u32,
        body: Vec<u8>,
    ) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let (forward, credit) = match self.requests.get(&stream_id) {
            Some(request) => (
                !request.complete && request.head_sent,
                request.download_credit,
            ),
            None => (false, 0),
        };
        if !forward {
            return Ok(());
        }
        if body.len() as u64 > credit as u64 {
            // The view sent more than http allowed (§3.1): recycle it.
            return self.retire(key, RetireReason::Other(CREDIT_OVERFLOW.to_string()));
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.download_credit -= body.len() as u32;
            request.bytes_out += body.len() as u64;
        }
        self.http_push(stream_id, Frame::RespBody(body))
    }

    /// `RESP_END` from a view: the response is complete, the slot is not.
    ///
    /// `cost` is the view's report for this request (D5) when it sent one: the
    /// wall time of `serve_request` — the WSGI call, through the iterable's
    /// `close()`, and the frames written back — and the thread's CPU time over
    /// the same span. It is untrusted, so [`Scheduler::charge`] clamps it
    /// between the view's first frame and the time the scheduler observed.
    ///
    /// A response that streams is charged here, when its stream ends, which is
    /// exactly when the report arrives — **unless** the view answered a `5xx`
    /// . That one response is finalized at its `DONE`, one frame
    /// later on the same channel, because the status cannot say whether the
    /// handler returned (the app's own failure, an answer) or bailed (`R19`'s
    /// crash). [`Scheduler::release`] is what charges and emits it, and the
    /// paths that reach it are exact: the request's `DONE`, or the
    /// ordered close of the view's channel — which is also how a
    /// `stuck_threads` retire ends, because a retire drops that channel
    /// ([`Scheduler::retire`]). A live view that simply loses that one frame
    /// holds the request — and its event — until one of those happens:
    /// `client_cancel` returns early on a completed request and
    /// `expire_requests` only marks it overdue (the `stuck_threads` retire that
    /// may follow ends it through the close).
    pub(super) fn view_end(
        &mut self,
        key: ViewKey,
        id: u32,
        cost: Option<RequestCost>,
    ) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let forward = match self.requests.get(&stream_id) {
            Some(request) => !request.complete && request.head_sent,
            None => false,
        };
        if !forward {
            return Ok(());
        }
        self.http_push(stream_id, Frame::RespEnd)?;
        let mut deferred = false;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            if !request.cancelled {
                // Anything below a `5xx` is an answer: the view produced a
                // complete response, and the status says the rest.
                deferred = request.status >= 500;
                if !deferred {
                    request.outcome = Outcome::Ok;
                }
            }
        }
        self.mark_finished(stream_id);
        // D5: the response's last byte is where a request's cost is fixed —
        // with the view's report when it sent one, with the time the scheduler
        // observed itself when it did not — so the event carries what was
        // actually charged, and the flow is behind the others before its thread
        // even exits. Every path that concludes a request charges before it
        // emits, so [`Scheduler::release`] only charges a request
        // that reaches it uncharged — this deferred `5xx` once its `DONE`
        // labels it or its view's channel closes first, and a `DONE` for a
        // response whose head is out but whose `RESP_END` never came
        // ([`Scheduler::view_done`]).
        if let Some(cost) = cost
            && let Some(request) = self.requests.get_mut(&stream_id)
        {
            request.report = Some(cost);
        }
        if !deferred {
            self.charge(stream_id);
            self.emit_request(stream_id);
        }
        Ok(())
    }

    /// `RESP_ABORT` from a view: the thread gave up, with or without a head.
    ///
    /// This is the runtime's own failure — a transport error, a WSGI callable
    /// that raised before answering — not a handler that bailed after answering
    /// (that one is `DONE`'s `bailed`). §8.1's outcomes have no "the
    /// app raised", so it is reported as `view_died`, the same as a process that
    /// went away mid-response.
    pub(super) fn view_abort(
        &mut self,
        key: ViewKey,
        id: u32,
        reason: &str,
    ) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        let head_sent = match self.requests.get(&stream_id) {
            Some(request) if !request.complete => request.head_sent,
            _ => return Ok(()),
        };
        if head_sent {
            self.http_push(
                stream_id,
                Frame::RespAbort {
                    reason: reason.to_string(),
                },
            )?;
        } else {
            self.respond(stream_id, 502, "Bad Gateway", "bad gateway\n", None)?;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = Outcome::ViewDied;
        }
        self.mark_finished(stream_id);
        // A `502` the scheduler wrote for a view that aborted: the span is
        // final, so its event carries the charge.
        self.charge(stream_id);
        self.emit_request(stream_id);
        Ok(())
    }

    /// `WINDOW` from a view: the app consumed that much of the request body,
    /// which extends the credit http may send under (§3.1).
    pub(super) fn view_window(
        &mut self,
        key: ViewKey,
        id: u32,
        bytes: u32,
    ) -> Result<(), SchedulerError> {
        let Some(&stream_id) = self.dispatch.get(&(key, id)) else {
            self.dropped += 1;
            return Ok(());
        };
        match self.requests.get(&stream_id) {
            Some(request) if !request.complete => {}
            _ => return Ok(()),
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.upload_credit = request.upload_credit.saturating_add(bytes);
        }
        self.http_push(stream_id, Frame::Window { bytes })
    }
}
/// Milliseconds as a float, for `request` events.
pub(super) fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The `Retry-After` a `503` carries, in seconds: §2.4's queue timeout rounded
/// up, never less than one second (§8: "`503` with `Retry-After`").
///
/// A client that is turned away because every worker is busy should be told
/// when to come back, and the queue timeout is the number the server can
/// actually promise: it is how long a request that *did* fit was willing to
/// wait. Zero — a queue that never waits — still says one second, because
/// `Retry-After: 0` asks a client to spin.
pub(super) fn retry_after_seconds(timeout: Duration) -> u64 {
    let seconds = timeout
        .as_secs()
        .saturating_add(u64::from(timeout.subsec_nanos() > 0));
    seconds.max(1)
}

/// The outcome of a response a view completed: `ok`, or — when the handler
/// bailed out of a `5xx` it had already answered — [`Outcome::Bailed`].
///
/// The status alone decides nothing. An app that answers
/// `http_response_code(500)`, Django's own `500`, a Node or JVM error response:
/// the view **returned** from the handler and produced a complete response, so
/// it is [`Outcome::Ok`] — the crate's gloss for exactly that, "the view
/// answered" — and `status` carries the failure. Only `bailed` (the `DONE` the
/// SAPI shim sends for a contained `exit()`/`die()`/fatal) is a request whose
/// handler never returned, and it is not `view_died` either: it answered before
/// it went away, so it is no death mid-request ([`Outcome::Bailed`]'s own
/// gloss). The worker retires after that request — a bailout leaves the
/// interpreter unusable (`crates/blastit-view/src/php.rs`) — and its ordered
/// `view.chan` frames are read before the close that would fail what it owes
/// ([`Scheduler::view_exited`]).
///
/// `view_answered` is required because a bailout with no `RESP_HEAD` from the
/// view answered nothing at all, so there is no completed response to label. It
/// has to be the view's own head: a `5xx` the scheduler wrote itself
/// is an answer the view never gave.
pub(super) fn completed_outcome(view_answered: bool, status: u16, bailed: bool) -> Outcome {
    if view_answered && status >= 500 && bailed {
        Outcome::Bailed
    } else {
        Outcome::Ok
    }
}

/// Whether a view **bailed out of a `5xx` it had answered**, which is what
/// `R19` charges the crash ceiling for.
///
/// All three halves are needed, and each keeps a different request out of the
/// crash bill:
///
/// * `bailed` is the `DONE`'s own word — the handler did not return. An app's
///   own `500` returned, so its report is believed within the clamp exactly
///   like a `200`'s.
/// * `view_answered` is set only when a `RESP_HEAD` came from the view, so a
///   `5xx` the scheduler wrote itself — the `502` of a view that never
///   answered, the `504` of `BLASTIT_REQUEST_TIMEOUT`, a `503` refusal — is not
///   a view failure and keeps the dispatch→completion charge: the view never
///   reported for it, and the crash bill's arrival→last-byte span would charge
///   a queue wait the request never spent on a worker.
///   `head_sent` cannot say this: `respond` sets that one too, for a head the
///   scheduler wrote, and a view that bails out *after* the scheduler already
///   answered that same request must not turn it into the crash bill.
/// * `status >= 500` keeps a bailout that still completed its response
///   (`echo "…"; exit;` answers `200`) out of the crash rule: the client got
///   everything the app produced.
pub(super) const fn answered_a_bailout(view_answered: bool, status: u16, bailed: bool) -> bool {
    view_answered && bailed && status >= 500
}

/// Whether a `DONE` may label a response the scheduler already completed.
///
/// Only a response the **view** answered can carry the handler's own verdict: a
/// `5xx` the scheduler wrote itself — a `504` timeout, the `502` of a view that
/// died — is that answer, and a view that finishes afterwards, even with
/// `DONE { Bailed }`, must not turn it into `bailed` nor into `R19`'s crash
/// bill. A cancelled request keeps the label it already has for the same reason.
pub(super) const fn a_done_may_relabel_a_completed_5xx(
    view_answered: bool,
    cancelled: bool,
    status: u16,
) -> bool {
    !cancelled && view_answered && status >= 500
}

/// The facts one request's charge is decided from (D5, `R19`).
pub(super) struct Charge {
    /// What the view reported (`RESP_END_COST`'s wall time), when it sent one.
    pub(super) reported: Option<u64>,
    /// Dispatch to the view's first response frame.
    pub(super) floor: u64,
    /// Dispatch to completion: the scheduler's own upper bound.
    pub(super) observed: u64,
    /// Arrival to the last byte: the event's `duration_ms`, in nanoseconds.
    pub(super) wall: u64,
    /// A view answered this request by **bailing out of a `5xx`**
    /// ([`answered_a_bailout`]).
    pub(super) failed: bool,
}

/// What one request is charged, in nanoseconds.
///
/// A request a view **bailed out of** is charged its **wall** time — `R19`'s
/// `ceiling.max(kernel_floor_ns)`, which is that wall at the one live thread
/// such a worker has, and which no kernel floor can exceed for one process —
/// and its report is not believed at all. A failing view is exactly the one
/// whose report must not be believed: a script that dies 18 ms into a request
/// but says it worked for 3 ms would let a client crash-loop for a fifth of its
/// cost. Everything else keeps the documented clamp: the report between the
/// view's first frame and the scheduler's own observation, or the observation
/// when there was no report — including an app's own `500`, whose handler
/// returned and whose report is therefore as good as a `200`'s, and a `502` or
/// `504` the scheduler wrote itself: the view never reported for it, and the
/// crash bill's arrival→last-byte span would charge a queue wait the request
/// never spent on a worker. The two are decided by [`answered_a_bailout`], never
/// by the status alone.
pub(super) fn charge_ns(charge: Charge) -> u64 {
    if charge.failed {
        return charge.wall;
    }
    match charge.reported {
        Some(reported) => reported.clamp(charge.floor, charge.observed),
        None => charge.observed,
    }
}

/// The reason phrase and body of a status the scheduler writes itself.
///
/// `405` is the static responder's method gate (`rewrite::decide`, design §4.4)
/// and the only status here that is not also an app-path refusal: a `POST` to a
/// path the responder serves is answered `Method Not Allowed`, with the one
/// `Allow: GET, HEAD` the rejection helper adds.
pub(super) fn canned(status: u16) -> (&'static str, &'static str) {
    match status {
        400 => ("Bad Request", "bad request\n"),
        404 => ("Not Found", "not found\n"),
        405 => ("Method Not Allowed", "method not allowed\n"),
        413 => ("Content Too Large", "content too large\n"),
        502 => ("Bad Gateway", "bad gateway\n"),
        503 => ("Service Unavailable", "unavailable\n"),
        504 => ("Gateway Timeout", "gateway timeout\n"),
        _ => ("Error", "error\n"),
    }
}

/// Whether a `static` mount answered this request (§9.2).
///
/// The mount is recorded on every request the responder admitted, so this is
/// exactly "a mount served this" — a genuinely unmatched path has no mount and
/// keeps its `(unmatched)` rendering. A request a mount matched but a **view**
/// then answered keeps its route name, because the route branch is asked first.
fn served_by_static(request: &Request) -> bool {
    #[cfg(feature = "static")]
    {
        request.static_mount.is_some()
    }
    #[cfg(not(feature = "static"))]
    {
        // A build without the responder has no mount a request could hold.
        let _ = request;
        false
    }
}
