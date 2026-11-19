//! Static mounts and the router resolution of a request target.

use super::response::canned;
#[cfg(feature = "static")]
use super::response::retry_after_seconds;
use super::*;

/// What routing a request target produced.
pub(super) enum Resolution {
    /// The target cannot be routed: a `400` (§2.4). The decode or a C0 byte
    /// failed before any mount was consulted.
    Bad,
    /// A mount claimed the path and the resolver refuses it (`..`, a backslash,
    /// a leading `//`, a path over the ceiling): `400` final (§9.2 step 0).
    #[cfg(feature = "static")]
    MountRefused,
    /// A mount matched and the resolved path is a dotfile or a PHP source:
    /// `404` final, before any view (§9.2 step 2's `Hidden`).
    #[cfg(feature = "static")]
    Hidden,
    /// A view's path pattern matched but no view declared this method: `405`
    /// with the union of their declared methods (§9.2 step 4).
    MethodNotAllowed {
        /// The `Allow` header value, in declaration order.
        allow: String,
    },
    /// The health path, answered by the scheduler itself.
    Health,
    /// Nothing matched: a `404` (§2.4), the `--learn` fallback, or a mount that
    /// answered nothing and whose `fallback` did not either.
    Unrouted {
        /// The decoded target path, used only by the `--learn` fallback to
        /// attribute the request (`R26`).
        path: String,
        /// The mount that claims this path, when one does (§9.2 step 1).
        #[cfg(feature = "static")]
        mount: Option<MountHead>,
    },
    /// A route, one of its rules, and the `PATH_INFO` that matched.
    Routed {
        /// Route index.
        route: usize,
        /// Index of the matching path regex.
        rule: u32,
        /// The decoded, stripped path.
        path_info: String,
        /// The mount that claims this path, when one does: its file answers
        /// before the view, and its `fallback` answers after it (§9.2).
        #[cfg(feature = "static")]
        mount: Option<MountHead>,
    },
}

/// One matched mount and the request path below its prefix (§9.2).
#[cfg(feature = "static")]
pub(super) struct MountHead {
    /// Index of the mount in `SchedulerConfig::mounts`.
    pub(super) mount: usize,
    /// The decoded request path with the mount's prefix removed, split into
    /// segments: `StaticRoot::resolve`'s input. The dotfile and PHP-source
    /// rules were applied to the **full** path before the strip.
    pub(super) segments: Vec<String>,
}

#[cfg(feature = "static")]
impl Resolution {
    /// The mount that claims this request, if any.
    pub(super) fn mount(&self) -> Option<&MountHead> {
        match self {
            Self::Unrouted { mount, .. } | Self::Routed { mount, .. } => mount.as_ref(),
            Self::Bad | Self::Health | Self::MethodNotAllowed { .. } => None,
            Self::MountRefused | Self::Hidden => None,
        }
    }
}

/// The head half of a request to a route with `[route.static]` (design §4.3
/// steps 3–4, §4.4).
///
/// Built in [`Scheduler::begin_raw`] while the head is still the borrowed
/// payload, so [`Request`] keeps no header list: the answer is built from the
/// two facts this carries and the segments the resolver takes.
#[cfg(feature = "static")]
pub(super) enum StaticHead {
    /// Steps 3–4 passed: `dispatch_static` resolves these segments once the
    /// request holds a slot, so a queued request opens no descriptor.
    Resolve {
        /// The mount whose root answers this path (§9.2 step 1).
        mount: usize,
        /// The path below the mount's prefix, split into segments.
        segments: Vec<String>,
        /// `rewrite::declares_body` for this head.
        has_body: bool,
        /// The `If-None-Match` the responder honours for a `304` (§4.5).
        if_none_match: Option<Vec<u8>>,
    },
}

impl Scheduler {
    /// The static head half of §9.2 and §4.4's `has_body`.
    ///
    /// Runs while the head is still the borrowed payload, so nothing it returns
    /// borrows the request: [`Request`] keeps no header list, and this is where
    /// the one `declares_body` bit and the `If-None-Match` the responder honours
    /// come from. `None` for a request no mount claims, which takes the view
    /// path unchanged.
    ///
    /// The segments are already decoded and split, and the dotfile and
    /// PHP-source rules were applied to the full path in [`Scheduler::resolve`],
    /// so a refused (`400`) or hidden (`404`) path is answered there, before
    /// `admit_ready`: no slot is taken, nothing queues, and no `..` probe can
    /// turn into a `503` (§9.2).
    #[cfg(feature = "static")]
    pub(super) fn static_head(
        &self,
        head: &ReqHeadView<'_>,
        resolution: &Resolution,
    ) -> Option<StaticHead> {
        let mount = resolution.mount()?;
        let headers: Vec<(Vec<u8>, Vec<u8>)> = head
            .headers()
            .map(|(name, value)| (name.to_vec(), value.to_vec()))
            .collect();
        let has_body = declares_body(&headers);
        let if_none_match = head
            .headers()
            .find(|(name, _)| name.eq_ignore_ascii_case(b"if-none-match"))
            .map(|(_, value)| value.to_vec());
        Some(StaticHead::Resolve {
            mount: mount.mount,
            segments: mount.segments.clone(),
            has_body,
            if_none_match,
        })
    }

    /// The `[route.ingress]` plan for a routed request, from its head alone.
    ///
    /// The head is still the raw payload's borrowed view here (see
    /// [`Scheduler::begin_raw`]), so the method and the three headers the plan
    /// reads are taken from it without decoding or copying the rest — the same
    /// head the view will receive byte for byte.
    ///
    /// A request the scheduler answers itself never reaches a view, so it has
    /// no ingress to honour and may only ever hold an empty body.
    ///
    /// A routed request whose head **declares** a body is refused here, before
    /// it is dispatched: `Content-Length: 0` and a chunked head are the only
    /// shapes that may still be empty, and only `REQ_END` proves it (see
    /// [`Scheduler::begin_raw`]).
    ///
    /// `Err` carries the status the scheduler answers with (`400` for a codec
    /// mismatch, `413` for a body on a route that takes none).
    pub(super) fn plan_request_body(
        &self,
        head: &ReqHeadView<'_>,
        resolution: &Resolution,
    ) -> Result<BodyPlan, u16> {
        let Resolution::Routed { route, .. } = resolution else {
            return Ok(BodyPlan::AllowEmptyOnly);
        };
        // A route with no `[route.ingress]` may only ever hold an empty body.
        // `plan_body` already answers a declared non-zero length with
        // `BodyNotAllowed` — the `413` of `R12` — and it is asked here, from
        // the head, so that answer is synchronous: no view has the head yet,
        // no body byte has arrived and no `REQ_END` is needed. Only the length
        // is read, so none of the three header `String`s below is built for
        // the routes that declare no ingress at all.
        let Some(policy) = self.routes.get(*route).and_then(|route| route.ingress) else {
            let method = std::str::from_utf8(head.method()).unwrap_or_default();
            return plan_body(method, None, None, None, declared_content_length(head))
                .map_err(|reject| reject.status());
        };
        let method = String::from_utf8_lossy(head.method()).into_owned();
        let content_type = head_header_text(head, b"content-type");
        let content_encoding = head_header_text(head, b"content-encoding");
        let content_length = declared_content_length(head);
        plan_body(
            &method,
            Some(policy),
            content_type.as_deref(),
            content_encoding.as_deref(),
            content_length,
        )
        .map_err(|reject| reject.status())
    }
}
impl Scheduler {
    /// Admits one request to a mount (§9.2 steps 1–4).
    ///
    /// This is the one function that takes a static admission slot: it sets
    /// `slots`, `view = None`, `dispatched_at` and `deadline` exactly as
    /// [`Scheduler::dispatch`] does — or `next_deadline` and `expire_requests`
    /// would never fire for it — and then resolves the mount's root to produce
    /// the one verdict §9.2 settles:
    ///
    /// 1. a file answers, through the method/body gate (`405`/`413`);
    /// 2. `Missing` falls through to the view a route matched, if any;
    /// 3. with no view left to try, the mount's declared `fallback` answers a
    ///    `GET`/`HEAD` that declares no body;
    /// 4. `Hidden`/`Refused`/a missing `fallback` is final, and a miss with no
    ///    view and no fallback is a `404` (or the `--learn` fallback).
    #[cfg(feature = "static")]
    pub(super) fn dispatch_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get_mut(&stream_id) else {
            return Ok(());
        };
        let Some(mount) = request.static_mount else {
            return Ok(());
        };
        let Some(parts) = request.static_parts.take() else {
            return Ok(());
        };
        let method = request.head.method.clone();
        let has_body = request.static_has_body;
        let if_none_match = request.static_if_none_match.take();
        debug_assert!(
            !request.slots,
            "a static request takes exactly one admission slot"
        );
        let now = Instant::now();
        request.queued_since = None;
        request.dispatched_at = Some(now);
        request.deadline = Some(now + self.cfg.request_timeout);
        request.view = None;
        request.static_pending = false;
        request.slots = true;
        self.queue.remove(stream_id);
        self.slots += 1;

        let Some(root) = self.mount_roots.get(mount).cloned() else {
            // A mount with no opened root is a boot-order bug; fail closed
            // rather than answer a file this role never proved it can read.
            return self.refuse_static(stream_id, 404, None);
        };
        let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
        // §9.2 step 1 only: `resolve_file` never answers the declared
        // `fallback`, because that is step 3's and would shadow every view.
        let outcome = root.resolve_file(&borrowed);
        match outcome {
            // Step 2: nothing at the path, so the view table is consulted
            // first. A routed request forwards; one no view matched has to
            // answer from the mount itself.
            StaticOutcome::Missing => self.mount_missed(stream_id, mount, &method, has_body),
            // Step 1 and the final refusals: the file/refusal gate.
            other => match decide(other, &method, has_body, false) {
                Rewrite::Serve(file) => self.serve_static(
                    stream_id,
                    root.as_ref(),
                    file,
                    &method,
                    if_none_match.as_deref(),
                ),
                Rewrite::Reject { status, allow } => self.refuse_static(stream_id, status, allow),
                // `Missing` is handled above, so a `Forward` here would mean
                // `decide` served a shape it was not given.
                Rewrite::Forward => {
                    debug_assert!(false, "a non-miss static outcome must not forward");
                    self.refuse_static(stream_id, 404, None)
                }
            },
        }
    }

    /// §9.2 steps 2–4 for a mount that answered no file.
    ///
    /// A request whose path a view also matched forwards to it — the view queue
    /// owns the answer from here, exactly as before. A request no view matched
    /// asks the mount's declared `fallback`, on `GET`/`HEAD` only, and is a
    /// `404` otherwise (or the `--learn` fallback, which is the app).
    #[cfg(feature = "static")]
    pub(super) fn mount_missed(
        &mut self,
        stream_id: u32,
        mount: usize,
        method: &[u8],
        has_body: bool,
    ) -> Result<(), SchedulerError> {
        if self
            .requests
            .get(&stream_id)
            .is_some_and(|request| request.route.is_some())
        {
            return self.forward_static(stream_id);
        }
        // Step 4's `405` is a view rule (§9.2); a method other than `GET`/`HEAD`
        // never reaches the fallback and is a `404` here.
        if method != b"GET" && method != b"HEAD" {
            return self.mount_learn_or_404(stream_id);
        }
        // A `GET`/`HEAD` that declares a body is a `413`, the same rule a file
        // answer uses.
        if has_body {
            return self.refuse_static(stream_id, 413, None);
        }
        let has_fallback = self
            .cfg
            .mounts
            .get(mount)
            .is_some_and(|mount| mount.policy.fallback.is_some());
        let Some(root) = self.mount_roots.get(mount).cloned() else {
            return self.refuse_static(stream_id, 404, None);
        };
        if !has_fallback {
            return self.mount_learn_or_404(stream_id);
        }
        match root.resolve_fallback() {
            StaticOutcome::File(file) => {
                self.serve_static(stream_id, root.as_ref(), file, method, None)
            }
            // A fallback that is missing, a dotfile, a symlink or a PHP source
            // is `404` final.
            _ => self.refuse_static(stream_id, 404, None),
        }
    }

    /// §9.2 step 4's final miss: the `--learn` fallback in learn mode, else a
    /// `404`.
    ///
    /// The static slot is released first: a learn request is served by the
    /// app's ordinary view path, which takes its own slot.
    #[cfg(feature = "static")]
    pub(super) fn mount_learn_or_404(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        if !self.cfg.learn_mode {
            return self.refuse_static(stream_id, 404, None);
        }
        self.release_static_slot(stream_id);
        self.queue.remove(stream_id);
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.static_pending = false;
            request.dispatched_at = None;
            request.deadline = None;
            request.queued_since = Some(Instant::now());
        }
        self.learn_fallback_queue.push_back(stream_id);
        self.pump_learn_fallback()
    }

    /// Pushes a static head and starts its body pump (design §4.6).
    ///
    /// Only the status and the header **names** are ever recorded anywhere:
    /// `StaticResponse` has no `Debug` on purpose (`MB.2.4.2`), and a header
    /// value may be content-derived, so the response is never formatted.
    #[cfg(feature = "static")]
    pub(super) fn serve_static(
        &mut self,
        stream_id: u32,
        root: &StaticRoot,
        file: StaticFile,
        method: &[u8],
        if_none_match: Option<&[u8]>,
    ) -> Result<(), SchedulerError> {
        let StaticResponse {
            status,
            reason,
            headers,
            body,
        } = root.respond(file, method, if_none_match);
        let has_body = body.is_some();
        let head = RespHead {
            status,
            reason: reason.as_bytes().to_vec(),
            headers,
        };
        self.http_push(stream_id, Frame::RespHead(head))?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.head_sent = true;
            request.status = status;
            request.static_body = body;
            // Nothing of the app's plan survives an answer: a served path never
            // buffers, transcodes or forwards, whatever the head declared.
            request.body_plan = BodyPlan::AllowEmptyOnly;
            request.body_plan_error = None;
        }
        if has_body {
            self.pump_static(stream_id)
        } else {
            // `HEAD` and `304`: the head is the whole answer, and no read is
            // ever spawned for it.
            self.finish_static(stream_id)
        }
    }

    /// Hands the slot back and re-enters the ordinary path (design §4.6).
    ///
    /// A `Forward` is the only verdict that reaches the app, and it must release
    /// its static slot first: the view path takes its own, from its own queue
    /// dispatch. That is the only way a front controller's *first* request can
    /// spawn its view — a slot-holder that never entered the queue could not.
    ///
    /// A miss does **not** hand the head to a view just because the mount could
    /// not answer it: `R12` is the route's rule, and it is decided here exactly
    /// as in [`Scheduler::begin_raw`]. Only a `raw` route (`StreamPassthrough`)
    /// is admitted at its head; a `Buffer` body is collected and transcoded, and
    /// an `AllowEmptyOnly` one is proved empty by its `REQ_END` and nothing
    /// else. Without that, a front controller's view would get `REQ_HEAD` for a
    /// request whose first body byte is a `413` — the very race this file's
    /// head-time refusal exists to close, on the one path a mount fronts.
    #[cfg(feature = "static")]
    pub(super) fn forward_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        self.release_static_slot(stream_id);
        let (error, waiting, buffered) = match self.requests.get_mut(&stream_id) {
            Some(request) => {
                request.static_pending = false;
                // The view queue measures its own timeout from its own
                // dispatch, so this request carries neither of these any more.
                request.dispatched_at = None;
                request.deadline = None;
                (
                    request.body_plan_error.take(),
                    // Only a body that has not ended is waited for: a `REQ_END`
                    // that landed while the request was queued has already
                    // proved the body complete (and, for `Buffer`, transcoded
                    // it), so there is nothing left to wait for and the request
                    // must fall through to `admit_ready` — otherwise nothing
                    // would ever re-drive it and the queue timeout would answer
                    // a request that was ready.
                    !request.end_seen && !matches!(request.body_plan, BodyPlan::StreamPassthrough),
                    matches!(request.body_plan, BodyPlan::Buffer { .. }),
                )
            }
            None => return Ok(()),
        };
        self.queue.remove(stream_id);
        if let Some(status) = error {
            // The stored codec error is the app path's own answer, exactly as
            // before this route gained a policy (design §4.4).
            return self.refuse_body(stream_id, status);
        }
        if waiting {
            // `expire_queue` bounds a client that never ends the body, and the
            // static slot was handed back above. The count of waiting bodies a
            // route may hold is the app path's own bound, so a static front
            // controller cannot admit more of them than any other route: that
            // path ([`Scheduler::begin_raw`]) marks the request
            // `body_ready = false` *before* it asks the count, so the request
            // being admitted counts itself and `queue_size = N` means `N - 1`
            // others. Asking first and clearing after would admit one more here
            // than on any other route.
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.body_ready = false;
            }
            let at_capacity = if buffered {
                // A structured body is collected and transcoded before the view
                // sees a byte.
                self.buffering_count() >= self.cfg.queue_size
            } else {
                // An empty-only one holds no bytes, but it holds its stream.
                self.awaiting_end_count() >= self.cfg.queue_size
            };
            if at_capacity {
                return self.reject(
                    stream_id,
                    503,
                    "Service Unavailable",
                    "queue full\n",
                    Outcome::QueueFull,
                    Some(retry_after_seconds(self.cfg.queue_timeout)),
                );
            }
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.queued_since = Some(Instant::now());
            }
            return Ok(());
        }
        self.admit_ready(stream_id)
    }

    /// Pumps one static body on the existing credit/window path (design §4.6).
    ///
    /// Steps 1–3 of the design, in order: a body with nothing left ends the
    /// response **before** any read; a read in flight is waited for; otherwise,
    /// with credit available, the [`StaticBody`] moves into one blocking
    /// `pread`, whose result comes back over `static_in`. Nothing here loops or
    /// spawns a thread per request: one read is ever outstanding per slot, and
    /// `slots <= max_workers` is therefore the reader bound as well.
    #[cfg(feature = "static")]
    pub(super) fn pump_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        let Some(body) = request.static_body.as_ref() else {
            return Ok(());
        };
        let remaining = body.len.saturating_sub(body.offset);
        if remaining == 0 {
            // Step 1: a zero-length file is an ordinary `200` with
            // `Content-Length: 0` and no body frame; the slot goes back here,
            // before any read.
            return self.finish_static(stream_id);
        }
        if request.pumping || request.download_credit == 0 {
            // Step 3: wait for the read result or for `WINDOW`.
            return Ok(());
        }
        // Step 2: one `pread` of at most the remaining bytes, the credit http
        // granted, one frame and one chunk — never more than any of them.
        let want = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(usize::try_from(request.download_credit).unwrap_or(usize::MAX))
            .min(self.cfg.frame_limits.max_payload())
            .min(STATIC_CHUNK_BYTES);
        if want == 0 {
            // A read of nothing can never advance the offset: it would push an
            // empty frame and ask again, leaving the response to be answered by
            // `request_timeout` instead of the read that had already failed.
            // Fail closed, on the same rule as a short read (design §4.6).
            return self.abort_static(stream_id);
        }
        let body = match self.requests.get_mut(&stream_id) {
            Some(request) if request.static_body.is_some() => {
                request.pumping = true;
                request.static_body.take()
            }
            _ => None,
        };
        let Some(body) = body else {
            return Ok(());
        };
        let sender = self.static_in.clone();
        tokio::task::spawn_blocking(move || {
            // A `pread` on a stalled mount cannot be cancelled, which is why the
            // body travels with the read: the scheduler's slot stays charged
            // until this result arrives.
            let file = std::fs::File::from(body.file);
            let mut buf = vec![0u8; want];
            let result = file.read_at(&mut buf, body.offset);
            let file: OwnedFd = file.into();
            let body = StaticBody {
                file,
                offset: body.offset,
                len: body.len,
            };
            // The scheduler may be gone (a shutdown): the frame is dropped with
            // the body then, which closes the descriptor.
            let _ = sender.blocking_send(StaticRead {
                stream_id,
                body,
                want,
                result: result.map(|read| {
                    buf.truncate(read);
                    buf
                }),
            });
        });
        Ok(())
    }

    /// One finished static read: push its bytes, or abort the response.
    ///
    /// The short-read rule is design §4.6's: a body is never truncated under a
    /// `Content-Length` the head already promised, so an error or fewer bytes
    /// than were asked for is `RespAbort` and never a silently short `200`.
    #[cfg(feature = "static")]
    pub(super) fn on_static_read(&mut self, read: StaticRead) -> Result<(), SchedulerError> {
        let stream_id = read.stream_id;
        let forward = match self.requests.get_mut(&stream_id) {
            Some(request) => {
                // The read result is what clears `pumping`: only now may the
                // slot be handed back (design §4.6).
                request.pumping = false;
                !request.complete
            }
            None => {
                // A request that is already gone: nothing may answer it, and
                // its body is dropped with the descriptor.
                self.dropped += 1;
                return Ok(());
            }
        };
        let mut body = read.body;
        let bytes = match read.result {
            Ok(bytes) if forward && bytes.len() == read.want => bytes,
            _ => {
                drop(body);
                if forward {
                    return self.abort_static(stream_id);
                }
                return self.release_finished_static(stream_id);
            }
        };
        let count = bytes.len() as u64;
        body.offset = body.offset.saturating_add(count);
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.static_body = Some(body);
            request.download_credit = request
                .download_credit
                .saturating_sub(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
            request.bytes_out = request.bytes_out.saturating_add(count);
        }
        self.http_push(stream_id, Frame::RespBody(bytes))?;
        self.pump_static(stream_id)
    }

    /// Ends a static response whose bytes are all out: `RESP_END`, the slot and
    /// the request.
    #[cfg(feature = "static")]
    pub(super) fn finish_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        self.http_push(stream_id, Frame::RespEnd)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.static_body = None;
            if !request.cancelled {
                request.outcome = Outcome::Ok;
            }
        }
        self.mark_finished(stream_id);
        self.release_finished_static(stream_id)
    }

    /// Aborts a static response whose bytes cannot be delivered in full.
    ///
    /// The head is always out before a read is spawned, so this is always a
    /// `RESP_ABORT` and never a second status. §8.1's outcome list is closed and
    /// has no "the responder failed" member, so this reports `view_died`: the
    /// response died mid-stream, exactly as a view's thread would.
    #[cfg(feature = "static")]
    pub(super) fn abort_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        self.http_push(
            stream_id,
            Frame::RespAbort {
                reason: STATIC_READ_FAILED.to_string(),
            },
        )?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.static_body = None;
            request.outcome = Outcome::ViewDied;
        }
        self.mark_finished(stream_id);
        self.release_finished_static(stream_id)
    }

    /// Charges, logs and releases one static request that is over.
    ///
    /// The slot goes back exactly once — [`Scheduler::release_static_slot`] is
    /// the only place that decrements it, and it is a no-op on a request that
    /// already let go — and the request is only dropped when it did.
    #[cfg(feature = "static")]
    pub(super) fn release_finished_static(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        // The last byte — or the abort — is where the cost is fixed, exactly as
        // a streamed view response does at its `RESP_END`.
        self.charge(stream_id);
        self.emit_request(stream_id);
        if self.release_static_slot(stream_id) {
            self.release(stream_id);
        }
        Ok(())
    }

    /// Hands a static admission slot back, once.
    ///
    /// The only place `slots` is decremented for a static answer. `false` when
    /// the request holds no slot, or when a read is still parked on it: the
    /// read result owns the transition then, so a timeout or a cancellation can
    /// never free a descriptor that is still in use (design §4.6).
    pub(super) fn release_static_slot(&mut self, stream_id: u32) -> bool {
        let Some(request) = self.requests.get_mut(&stream_id) else {
            return false;
        };
        if !slot_releasable(request.slots, request.pumping) {
            return false;
        }
        request.slots = false;
        self.slots = self.slots.saturating_sub(1);
        true
    }

    /// The three calls the view path makes into the responder, in a build
    /// without `static` (`MB.36.2`).
    ///
    /// [`run`] refuses a manifest that declares a mount in that build
    /// ([`SchedulerError::StaticNotCompiled`]), so no request ever matches a
    /// mount, sets `static_pending`, or holds a static body: each of these is
    /// unreachable, and "nothing to do" is what the compiler is told.
    #[cfg(not(feature = "static"))]
    pub(super) fn dispatch_static(&mut self, _stream_id: u32) -> Result<(), SchedulerError> {
        Ok(())
    }

    /// See the `static` build's `pump_static`.
    #[cfg(not(feature = "static"))]
    pub(super) fn pump_static(&mut self, _stream_id: u32) -> Result<(), SchedulerError> {
        Ok(())
    }

    /// See the `static` build's `on_static_read`: nothing ever sends on that
    /// channel, so this is never called.
    #[cfg(not(feature = "static"))]
    pub(super) fn on_static_read(&mut self, _read: StaticReadMsg) -> Result<(), SchedulerError> {
        Ok(())
    }

    /// Answers a request the static responder refused (design §4.4).
    ///
    /// The one helper for all four refusals: `canned` supplies the reason and
    /// body, `Allow` is added only for the `405` the caller chose (with its one
    /// value, `GET, HEAD`), and `Cache-Control: no-store` is added to every one
    /// of them.
    pub(super) fn refuse_static(
        &mut self,
        stream_id: u32,
        status: u16,
        allow: Option<&[u8]>,
    ) -> Result<(), SchedulerError> {
        // The slot goes back before the status is written, so the answer never
        // holds admission capacity while it is being framed.
        self.release_static_slot(stream_id);
        let (reason, body) = canned(status);
        let mut extra: Vec<(&[u8], &[u8])> = Vec::with_capacity(2);
        if let Some(allow) = allow {
            extra.push((&b"Allow"[..], allow));
        }
        extra.push((&b"Cache-Control"[..], &b"no-store"[..]));
        self.respond_with(stream_id, status, reason, body, None, &extra)?;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.complete = true;
            request.outcome = Outcome::Unmatched;
        }
        self.mark_finished(stream_id);
        self.charge(stream_id);
        self.emit_request(stream_id);
        self.release(stream_id);
        Ok(())
    }

    // ---------------------------------------------------------------- timers
}
pub(super) fn header_text(head: &ReqHead, name: &[u8]) -> Option<String> {
    head.headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| std::str::from_utf8(value).ok())
        .map(str::to_owned)
}

/// The first value of `name` in a head still in its raw payload, as text.
///
/// The same read as [`header_text`], over the borrowed [`ReqHeadView`] the
/// scheduler plans a request's body from: the header list is walked in place,
/// so neither the head nor the value is copied until it is actually needed.
pub(super) fn head_header_text(head: &ReqHeadView<'_>, name: &[u8]) -> Option<String> {
    head.headers()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| std::str::from_utf8(value).ok())
        .map(str::to_owned)
}

/// The `Content-Length` a head still in its raw payload declares, if any.
///
/// The header list is walked in place, so no `String` is built for a route that
/// declares no ingress and reads only this. A length this build cannot parse
/// (a non-ASCII value, a sign, a bad digit) is `u64::MAX`: a declared body this
/// build cannot size is treated as unbounded, so it fails closed instead of
/// looking like zero.
pub(super) fn declared_content_length(head: &ReqHeadView<'_>) -> Option<u64> {
    head.headers()
        .find(|(name, _)| name.eq_ignore_ascii_case(b"content-length"))
        .map(|(_, value)| {
            std::str::from_utf8(value)
                .ok()
                .and_then(|text| text.trim().parse::<u64>().ok())
                .unwrap_or(u64::MAX)
        })
}

/// Whether a static admission slot may be handed back now (design §4.6).
///
/// `holds_slot && !pumping`: a request that never took a slot has nothing to
/// give back, and one whose `pread` is still parked keeps it until the read
/// result arrives — a `pread` on a stalled mount cannot be cancelled, so
/// releasing it earlier would free the slot (and the descriptor) while the
/// blocking task still owns it.
///
/// It is the one predicate [`Scheduler::release_static_slot`] consults, and the
/// timeout and cancellation paths reach the slot through that function, so a
/// figure that disagrees with this one cannot free a parked request.
fn slot_releasable(holds_slot: bool, pumping: bool) -> bool {
    holds_slot && !pumping
}

#[cfg(test)]
mod tests {
    use super::slot_releasable;

    /// Design §4.6's table for the one predicate that frees a static slot.
    ///
    /// The protected `critical_rewrite.rs` cannot reach a private item, and
    /// `lib.rs` (which would have to export it) is outside this leaf's paths,
    /// so the three rows live here: a request that holds a slot and has no read
    /// parked gives it back; one with a read parked gives it back only when
    /// that read returns; one that never took a slot gives back nothing.
    #[test]
    fn a_static_slot_is_releasable_only_when_its_read_is_not_parked() {
        assert!(slot_releasable(true, false), "(true, false) → true");
        assert!(!slot_releasable(true, true), "(true, true) → false");
        assert!(!slot_releasable(false, false), "(false, false) → false");
        assert!(!slot_releasable(false, true), "(false, true) → false");
    }
}
