//! Requests: body collection, routing decisions and the request lifecycle.

use super::mounts::{header_text, Resolution};
#[cfg(feature = "static")]
use super::mounts::{MountHead, StaticHead};
use super::response::retry_after_seconds;
use super::*;

/// The parts of a request head the scheduler itself reads.
///
/// The head is not decoded into a [`blastit_frame::ReqHead`]: its payload is
/// forwarded to the view verbatim (see [`Scheduler::begin_raw`]), so only what
/// routing and the `request` event need is kept here. The header list is not
/// among them — the fair-queue flow is resolved once, at admission, from the
/// borrowed head.
pub(super) struct RequestHead {
    /// Request method, verbatim and case-preserved.
    pub(super) method: Vec<u8>,
    /// Request target (origin form, e.g. `/x?y=1`), verbatim.
    pub(super) target: Vec<u8>,
    /// Client address as the front end resolved it (ARCHITECTURE.md §2.4).
    pub(super) remote_addr: String,
}

/// One request, from its `REQ_HEAD` to the `DONE` that releases its slot.
pub(super) struct Request {
    /// http's stream id, for the response frames.
    pub(super) stream_id: u32,
    /// The id reported as `request_id` in events (§8.1).
    pub(super) request_id: String,
    /// The few head fields the scheduler reads; the rest travel to the view in
    /// `raw` untouched.
    pub(super) head: RequestHead,
    /// The request head exactly as http sent it, forwarded to the view with
    /// only its id rewritten. Taken here on dispatch, so the payload is moved,
    /// never copied.
    pub(super) raw: Option<RawFrame>,
    /// The route this request was routed to, if any (`None` for the requests
    /// the scheduler answers itself: health, `400`, `404`).
    pub(super) route: Option<usize>,
    /// Index of the matching path regex in the route (§8.1 `rule`).
    pub(super) rule: Option<u32>,
    /// The routed `PATH_INFO`: decoded and `SCRIPT_NAME`-stripped (§8.1).
    /// `None` when the target could not be decoded at all.
    pub(super) path_info: Option<String>,
    pub(super) arrived: Instant,
    /// Set while the request waits in the admission queue, and while it waits
    /// for the body that finishes it — a `Buffer` body still being collected,
    /// or the `REQ_END` that proves an `AllowEmptyOnly` one empty. Either way
    /// `expire_queue` answers it with `503 queue timeout` once
    /// [`SchedulerConfig::queue_timeout`] elapses.
    pub(super) queued_since: Option<Instant>,
    /// Set when the request was handed to a view; the timeout runs from here.
    pub(super) dispatched_at: Option<Instant>,
    /// `dispatched_at + BLASTIT_REQUEST_TIMEOUT`.
    pub(super) deadline: Option<Instant>,
    pub(super) status: u16,
    pub(super) outcome: Outcome,
    pub(super) bytes_in: u64,
    pub(super) bytes_out: u64,
    /// Body chunks held while the request is queued. Credit bounds them: http
    /// may not send more than [`Request::upload_credit`] allows (§3.1).
    pub(super) body: Vec<Vec<u8>>,
    /// How the body must be handled, decided from the head alone (`R12`, `R16`).
    pub(super) body_plan: BodyPlan,
    /// The status `body_plan` refused with, stored rather than answered on a
    /// route with `[route.static]` (design §4.4): the responder's method/body
    /// gate outranks a codec error on a path it answers, and only a `Forward`
    /// ever answers this.
    #[cfg(feature = "static")]
    pub(super) body_plan_error: Option<u16>,
    /// Whether the request may be dispatched: an `AllowEmptyOnly` or `Buffer`
    /// body sets it only when its `REQ_END` proved the body complete (a
    /// `Buffer` one also transcoded), and a `StreamPassthrough` at its head
    /// (`R12`, `R16`).
    pub(super) body_ready: bool,
    /// The matched mount claims this path and the head passed §9.2 step 0:
    /// `dispatch_static` is what admits it, and no view is involved until the
    /// mount misses and a view matched (§9.2 steps 1–2).
    pub(super) static_pending: bool,
    /// The mount whose root is resolved, kept beside the segments.
    #[cfg(feature = "static")]
    pub(super) static_mount: Option<usize>,
    /// The path below the mount's prefix, split into segments and kept until
    /// `dispatch_static` borrows it for `StaticRoot::resolve`; a queued static
    /// request therefore opens no descriptor (§9.2 step 1).
    pub(super) static_parts: Option<Vec<String>>,
    /// `rewrite::declares_body` for this head: `Request` keeps no header list,
    /// so the one bit the answer needs is taken from the borrowed head.
    #[cfg(feature = "static")]
    pub(super) static_has_body: bool,
    /// The request's `If-None-Match`, for the responder's `304` (design §4.5).
    /// `None` unless a static route asked for it.
    #[cfg(feature = "static")]
    pub(super) static_if_none_match: Option<Vec<u8>>,
    /// The bytes still owed for a static `GET`, from the descriptor the
    /// resolver opened. `Some` only after `StaticRoot::respond` handed it over.
    #[cfg(feature = "static")]
    pub(super) static_body: Option<StaticBody>,
    /// One static read is in flight for this request: its result is what
    /// releases the slot, never a timeout or a cancellation (design §4.6).
    pub(super) pumping: bool,
    /// This request is an unrouted `--learn` request using the single fallback
    /// slot (`R26`, design §4.2 D.2).
    pub(super) learn_fallback: bool,
    /// `REQ_END` arrived.
    pub(super) end_seen: bool,
    /// A `CANCEL` arrived from http.
    pub(super) cancelled: bool,
    /// The view and request id this request was dispatched under.
    pub(super) view: Option<(ViewKey, u32)>,
    /// The fair-queuing flow this request belongs to (D5), when fair queuing
    /// is on. It is resolved once, when the request is admitted, and returned
    /// once, when the request is released.
    pub(super) flow: Option<FlowRef>,
    /// The view's own measurement of what this request cost it (D5), from the
    /// `RESP_END_COST` frame. Untrusted: [`Scheduler::charge`] clamps it
    /// between the view's first frame and the time the scheduler observed, and
    /// ignores it when the view **bailed out** of a `5xx` — a failing view's
    /// report is not evidence. An app's own `500` returned, so its report keeps
    /// the clamp ([`answered_a_bailout`]).
    pub(super) report: Option<RequestCost>,
    /// What was charged to the flow for this request, in nanoseconds: the
    /// reported cost clamped between the view's first frame and the observed
    /// time, the observed time when there was no report, or the request's own
    /// wall time when a view bailed out of a `5xx` it had answered (`R19`). This
    /// is the `cost_ns` of the `request` event (§8.1).
    pub(super) charged_ns: u64,
    /// The flow has been charged for this request. Exactly one of the path that
    /// concludes the request and the release does it: the charge is idempotent,
    /// so whichever runs second is a no-op (D5).
    pub(super) charged: bool,
    /// When the response ended — the last byte, the abort or the failure —
    /// which is where `duration_ms` stops even though the event waits for the
    /// `DONE` that carries the cost report (D5).
    pub(super) finished_at: Option<Instant>,
    /// When the view sent its first response frame (D5): the floor under what
    /// this request may be charged, because it is the one part of the request
    /// the view cannot shorten without making its own client wait.
    pub(super) head_at: Option<Instant>,
    /// A `RESP_HEAD` was forwarded to http.
    pub(super) head_sent: bool,
    /// The `RESP_HEAD` http was sent came **from the view**, not from the
    /// scheduler's own `respond`.
    ///
    /// `head_sent` answers "is the response under way"; this answers "did the
    /// view answer this request". They are one and the same for an app's
    /// response and different for every `5xx` the scheduler wrote itself (the
    /// `502` of a dead view, a `504` timeout, a `503` refusal), which is what
    /// [`answered_a_bailout`] and [`completed_outcome`] have to know: a view
    /// that later bails out of a request the scheduler already answered must
    /// not turn that answer into the `R19` crash bill.
    pub(super) view_answered: bool,
    /// The view's handler bailed out of this request rather than returning
    /// (`DONE`'s `bailed`): a contained `exit()`/`die()`/fatal.
    ///
    /// This is a different fact from the status it may have answered. A `5xx`
    /// an app answers itself (a framework's `500`, a script's
    /// `http_response_code(500)`) is an answer; only this flag is the `R19`
    /// crash that is charged its wall time.
    pub(super) bailed: bool,
    /// The response to http is over (`RESP_END`, `RESP_ABORT` or a status the
    /// scheduler sent itself).
    pub(super) complete: bool,
    /// The `request` event has been emitted.
    pub(super) emitted: bool,
    /// The request passed `BLASTIT_REQUEST_TIMEOUT`.
    pub(super) timed_out: bool,
    /// This request holds one of the global worker slots.
    pub(super) slots: bool,
    /// Request-body bytes http may still send before the view grants more.
    pub(super) upload_credit: u32,
    /// Response-body bytes the view may still send before http grants more.
    pub(super) download_credit: u32,
}
impl Scheduler {
    /// Routes a new request whose head was not decoded, then dispatches or
    /// queues it (§2.4).
    ///
    /// Only `http.chan` reaches here (see [`Scheduler::on_http`]). The head is
    /// parsed just far enough to route it and to resolve its fair-queue flow,
    /// and the raw payload is kept for the view: everything the scheduler does
    /// not read — scheme, server name and port, and every header the app sees —
    /// reaches the view byte for byte, and no header is copied into an owned
    /// `Vec` on the way.
    pub(super) fn begin_raw(
        &mut self,
        stream_id: u32,
        raw: RawFrame,
    ) -> Result<(), SchedulerError> {
        if self.requests.contains_key(&stream_id) {
            // http's contract is that a stream id is not reused while it is in
            // flight; anything else would let one request inherit another's
            // worker slot, so it is fatal rather than guessed at.
            return Err(SchedulerError::protocol(format!(
                "http.chan reused stream id {stream_id} while it was still in flight"
            )));
        }
        // http encodes from a typed `Frame`, so a head that does not parse is a
        // bug in this build, not a peer's fault (§3.1).
        let head = ReqHeadView::parse(&raw.payload).map_err(SchedulerError::Frame)?;
        let request_id = self.next_request_id();
        // The event's one clock: `arrived` and the flow the queue stamps are
        // read from it, so a request cannot age against two clocks at once.
        let now = Instant::now();
        let resolution = self.resolve(&head);
        // D5: the flow is resolved once per request, from the raw peer and the
        // headers, and it is the *same* resolution the front end used for
        // `REMOTE_ADDR`. It is resolved for a routed request before the request
        // is queued or refused, exactly as before. The queue is given this
        // event's clock, so what it stamps and what it later compares are one
        // clock.
        let flow = match &resolution {
            Resolution::Routed { .. } => self.queue.resolve_view(head.peer_addr(), &head, now),
            _ => None,
        };
        // `R12`: the head alone decides whether this route may have a body at
        // all, and `R16`: `raw` streams with zero buffering. It is planned from
        // the view, before the payload is moved into the request, so the
        // headers are never copied out to be read.
        let plan = self.plan_request_body(&head, &resolution);
        // Design §4.3 steps 3–4, and §4.4's `has_body`: on a route with
        // `[route.static]` the head is also decoded to segments here — before a
        // slot, a queue entry or any descriptor — so a `..` or a dotfile is
        // answered without ever reaching `admit_ready`.
        #[cfg(feature = "static")]
        let static_head = self.static_head(&head, &resolution);
        // The few head fields routing and the `request` event need, copied out
        // before the raw payload is moved into the request.
        let facts = RequestHead {
            method: head.method().to_vec(),
            target: head.target().to_vec(),
            remote_addr: head.remote_addr().to_string(),
        };
        let mut request = Request {
            stream_id,
            request_id,
            head: facts,
            raw: Some(raw),
            route: None,
            rule: None,
            path_info: None,
            arrived: now,
            queued_since: None,
            dispatched_at: None,
            deadline: None,
            status: 0,
            outcome: Outcome::Ok,
            bytes_in: 0,
            bytes_out: 0,
            body: Vec::new(),
            body_plan: BodyPlan::AllowEmptyOnly,
            #[cfg(feature = "static")]
            body_plan_error: None,
            body_ready: true,
            static_pending: false,
            #[cfg(feature = "static")]
            static_mount: None,
            static_parts: None,
            #[cfg(feature = "static")]
            static_has_body: false,
            #[cfg(feature = "static")]
            static_if_none_match: None,
            #[cfg(feature = "static")]
            static_body: None,
            pumping: false,
            learn_fallback: false,
            end_seen: false,
            cancelled: false,
            view: None,
            flow: None,
            report: None,
            charged_ns: 0,
            charged: false,
            finished_at: None,
            head_at: None,
            head_sent: false,
            view_answered: false,
            bailed: false,
            complete: false,
            emitted: false,
            timed_out: false,
            slots: false,
            upload_credit: INITIAL_WINDOW,
            download_credit: INITIAL_WINDOW,
        };
        match resolution {
            Resolution::Bad => {
                self.requests.insert(stream_id, request);
                self.reject(
                    stream_id,
                    400,
                    "Bad Request",
                    "bad request\n",
                    Outcome::Unmatched,
                    None,
                )
            }
            #[cfg(feature = "static")]
            Resolution::MountRefused => {
                // §9.2 step 0: a shape the resolver refuses is a `400` before
                // any mount or view is consulted.
                self.requests.insert(stream_id, request);
                self.refuse_static(stream_id, 400, None)
            }
            #[cfg(feature = "static")]
            Resolution::Hidden => {
                // §9.2 step 0: the full path is a dotfile or a PHP source
                // before any mount or view is consulted, and no view may answer
                // it.
                self.requests.insert(stream_id, request);
                self.refuse_static(stream_id, 404, None)
            }
            Resolution::MethodNotAllowed { allow } => {
                // §9.2 step 4: a view's pattern matched and no view declared
                // this method.
                self.requests.insert(stream_id, request);
                self.refuse_static(stream_id, 405, Some(allow.as_bytes()))
            }
            Resolution::Health => {
                self.requests.insert(stream_id, request);
                self.respond(stream_id, 200, "OK", "ok", None)?;
                self.finish_immediately(stream_id, Outcome::Ok)
            }
            Resolution::Unrouted { path, .. } => {
                request.path_info = Some(path);
                // §9.2 step 1/3: a mount that claims the path answers its file
                // or its `fallback` even when no view matched. It takes a slot
                // exactly like a routed static request, and a miss that reaches
                // step 4 is a `404` (or the `--learn` fallback).
                #[cfg(feature = "static")]
                if let Some(StaticHead::Resolve {
                    mount,
                    segments,
                    has_body,
                    if_none_match,
                }) = static_head
                {
                    request.static_pending = true;
                    request.static_mount = Some(mount);
                    request.static_parts = Some(segments);
                    request.static_has_body = has_body;
                    request.static_if_none_match = if_none_match;
                    request.body_plan = BodyPlan::AllowEmptyOnly;
                    request.body_ready = true;
                    self.requests.insert(stream_id, request);
                    return self.admit_ready(stream_id);
                }
                self.requests.insert(stream_id, request);
                if self.cfg.learn_mode {
                    // `--learn` never short-circuits an unrouted request: it is
                    // served by the app one at a time so its SQL can be
                    // attributed to `ObsTarget::Unrouted` (`R26`).
                    let retry_after = retry_after_seconds(self.cfg.queue_timeout);
                    if self.learn_fallback_queue.len() >= self.cfg.queue_size {
                        return self.reject(
                            stream_id,
                            503,
                            "Service Unavailable",
                            "queue full\n",
                            Outcome::QueueFull,
                            Some(retry_after),
                        );
                    }
                    if let Some(request) = self.requests.get_mut(&stream_id) {
                        request.queued_since = Some(now);
                    }
                    self.learn_fallback_queue.push_back(stream_id);
                    self.pump_learn_fallback()
                } else {
                    self.reject(
                        stream_id,
                        404,
                        "Not Found",
                        "not found\n",
                        Outcome::Unmatched,
                        None,
                    )
                }
            }
            Resolution::Routed {
                route,
                rule,
                path_info,
                ..
            } => {
                request.route = Some(route);
                request.rule = Some(rule);
                request.path_info = Some(path_info);
                // D5: the flow was resolved once, from the raw peer and the
                // headers of this request's head, before it was moved here —
                // and it is the *same* resolution the front end used for
                // `REMOTE_ADDR`, stamped by the queue with this event's clock.
                request.flow = flow;
                // §9.2 steps 1–2: a mount's file answers first and a miss falls
                // through to this view. The request is admitted at the head; the
                // plan is still computed (a miss that forwards needs it) but its
                // error is *stored*, not answered: the mount's method/body gate
                // outranks a codec error on a path a file answers.
                #[cfg(feature = "static")]
                if let Some(StaticHead::Resolve {
                    mount,
                    segments,
                    has_body,
                    if_none_match,
                }) = static_head
                {
                    request.static_pending = true;
                    request.static_mount = Some(mount);
                    request.static_parts = Some(segments);
                    request.static_has_body = has_body;
                    request.static_if_none_match = if_none_match;
                    request.body_plan = match plan {
                        Ok(plan) => plan,
                        // Nothing is buffered or transcoded for a static
                        // answer; the plan's error is what the view answers
                        // when the mount misses.
                        Err(_) => BodyPlan::AllowEmptyOnly,
                    };
                    request.body_plan_error = plan.err();
                    request.body_ready = true;
                    self.requests.insert(stream_id, request);
                    return self.admit_ready(stream_id);
                }
                // `R12`/`R16`: `plan` was decided from the same head, and `Err`
                // carries the status a body this route will not take is
                // answered with.
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(status) => {
                        self.requests.insert(stream_id, request);
                        return self.refuse_body(stream_id, status);
                    }
                };
                request.body_plan = plan;
                // `R12` from the other end: only a `raw` route may see its head
                // before `REQ_END`, because it forwards every byte as it
                // arrives (`R16`). An `AllowEmptyOnly` route that reaches a
                // view holds no body at all, and nothing in the head can prove
                // a chunked — or not yet arrived — body empty: `REQ_END` is the
                // one proof. Until it arrives the request is not body-ready, so
                // it cannot be dispatched and no view can answer the head this
                // request must instead `413` on the first body byte. A
                // `Buffer` body is collected until it transcodes or overflows.
                // Both wait under `queued_since`, which bounds a body that
                // never ends (`expire_queue`, `503 queue timeout`) instead of
                // parking the request — and both are counted against
                // `queue_size` here, because a request waiting for a body is a
                // request a peer is holding open, exactly like one in the
                // admission queue. Without that count a peer could park an
                // unbounded number of heads, each holding its raw payload.
                let waiting = !matches!(plan, BodyPlan::StreamPassthrough);
                request.body_ready = !waiting;
                if waiting {
                    request.queued_since = Some(now);
                }
                self.requests.insert(stream_id, request);
                let over_capacity = if !waiting {
                    false
                } else if matches!(plan, BodyPlan::Buffer { .. }) {
                    // A structured body is buffered up to
                    // `BLASTIT_MAX_BODY_BYTES` and transcoded before the view
                    // sees a byte.
                    self.buffering_count() >= self.cfg.queue_size
                } else {
                    // An empty-only request holds no bytes, but it holds its
                    // stream: the same bound, for the same reason.
                    self.awaiting_end_count() >= self.cfg.queue_size
                };
                if over_capacity {
                    return self.reject(
                        stream_id,
                        503,
                        "Service Unavailable",
                        "queue full\n",
                        Outcome::QueueFull,
                        Some(retry_after_seconds(self.cfg.queue_timeout)),
                    );
                }
                if waiting {
                    return Ok(());
                }
                self.admit_ready(stream_id)
            }
        }
    }
}
impl Scheduler {
    /// Turns the request target into a routing decision (§2.4, §6).
    pub(super) fn resolve(&self, head: &ReqHeadView<'_>) -> Resolution {
        let path = path_of_target(head.target());
        let Some(decoded) = percent_decode(path) else {
            return Resolution::Bad;
        };
        if !decoded.starts_with(b"/") {
            return Resolution::Bad;
        }
        // NUL and the C0 controls cannot be routed on (and must never reach an
        // event), so they are a `400` like any other unusable target.
        if decoded.iter().any(|byte| is_control_byte(*byte)) {
            return Resolution::Bad;
        }
        let Ok(path) = std::str::from_utf8(&decoded) else {
            return Resolution::Bad;
        };
        // An unrouted request reports the path exactly as decoded, before any
        // mount prefix is stripped: that is what the client asked for.
        let request_path = path.to_string();
        let Ok(method) = std::str::from_utf8(head.method()) else {
            return Resolution::Bad;
        };
        // Only a GET or a HEAD is a health check; anything else is a request
        // the app may well want to see (§2.4).
        if path == self.cfg.health_path && (method == "GET" || method == "HEAD") {
            return Resolution::Health;
        }
        let path = match self.script_name.as_deref() {
            Some(prefix) => match strip_script_name(path, prefix) {
                Some(rest) => rest,
                None => {
                    return Resolution::Unrouted {
                        path: request_path,
                        #[cfg(feature = "static")]
                        mount: None,
                    };
                }
            },
            None => path,
        };
        // §9.2 step 0: the decode and the path-shape refusal, plus the dotfile
        // and PHP-source rules, all run on the **full** path before the prefix
        // is stripped, so a mount can neither hide a refusal nor manufacture
        // one. They only apply when a mount claims the path: an app is free to
        // serve a dotfile of its own.
        #[cfg(feature = "static")]
        let mount = match self.mounts.lookup(path) {
            None => None,
            Some(found) => {
                let Ok(parts) = segments(path) else {
                    return Resolution::MountRefused;
                };
                if is_dotfile(&parts) {
                    return Resolution::Hidden;
                }
                if parts.last().is_some_and(|segment| is_php_source(segment)) {
                    return Resolution::Hidden;
                }
                let below = segments(&self.cfg.mounts[found.mount].prefix)
                    .map(|prefix| prefix.len())
                    .unwrap_or(0);
                let relative = parts.get(below..).unwrap_or_default();
                Some(MountHead {
                    mount: found.mount,
                    segments: relative.iter().map(|part| (*part).to_string()).collect(),
                })
            }
        };
        match self.table.route(method, path) {
            Some(matched) => Resolution::Routed {
                route: matched.route,
                rule: u32::try_from(matched.rule).unwrap_or(u32::MAX),
                path_info: path.to_string(),
                #[cfg(feature = "static")]
                mount,
            },
            None => {
                // §9.2 step 4: a path a view's pattern matched but whose method
                // no view declared is a `405`, not an unrouted `404`.
                if let Some(allow) = self.table.methods_for(path) {
                    return Resolution::MethodNotAllowed {
                        allow: allow.join(", "),
                    };
                }
                Resolution::Unrouted {
                    path: request_path,
                    #[cfg(feature = "static")]
                    mount,
                }
            }
        }
    }

    /// Puts a request in the admission queue: at the back of the FIFO, or into
    /// its flow (D5).
    pub(super) fn enqueue(&mut self, stream_id: u32) {
        let now = Instant::now();
        let route = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.route)
            .and_then(|route| u32::try_from(route).ok())
            .unwrap_or(u32::MAX);
        let flow = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.flow);
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.queued_since = Some(now);
        }
        self.queue.push(stream_id, route, flow, now);
    }

    /// Records request body bytes: forwards them when the request is
    /// dispatched, holds them while it waits.
    ///
    /// Credit is what bounds the held bytes: http may not send more than the
    /// view has granted (§3.1), so a queued request can hold at most
    /// `INITIAL_WINDOW` bytes and other streams keep flowing.
    pub(super) fn request_body(
        &mut self,
        stream_id: u32,
        body: Vec<u8>,
    ) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        match request.body_plan {
            // A route that accepts no body may see `REQ_END`, never a byte
            // (`R12`, `ADR-0006`).
            BodyPlan::AllowEmptyOnly => {
                if body.is_empty() {
                    return Ok(());
                }
                self.refuse_body(stream_id, 413)
            }
            BodyPlan::StreamPassthrough => {
                let view = request.view;
                let allowed = request.upload_credit as u64;
                if body.len() as u64 > allowed {
                    return self.http_credit_violation(stream_id);
                }
                let count = body.len() as u64;
                if let Some(request) = self.requests.get_mut(&stream_id) {
                    request.bytes_in += count;
                    request.upload_credit -= body.len() as u32;
                }
                match view {
                    Some((key, id)) => self.view_push(key, id, Frame::ReqBody(body)),
                    None => {
                        if let Some(request) = self.requests.get_mut(&stream_id) {
                            request.body.push(body);
                        }
                        Ok(())
                    }
                }
            }
            // `R14`/`R15`: the whole body is validated and re-encoded before a
            // view sees it, so it is buffered here — bounded, and with the
            // consumed credit handed back so an upload may exceed one window.
            BodyPlan::Buffer { .. } => {
                let allowed = request.upload_credit as u64;
                if body.len() as u64 > allowed {
                    return self.http_credit_violation(stream_id);
                }
                let total = request.bytes_in.saturating_add(body.len() as u64);
                if total > self.cfg.max_body_bytes {
                    return self.refuse_body(stream_id, 413);
                }
                let credit = u32::try_from(body.len()).unwrap_or(u32::MAX);
                let count = body.len() as u64;
                if let Some(request) = self.requests.get_mut(&stream_id) {
                    request.bytes_in += count;
                    request.upload_credit = request
                        .upload_credit
                        .saturating_sub(credit)
                        .saturating_add(credit);
                    request.body.push(body);
                }
                // The scheduler consumed those bytes, so the client may send
                // that much more while the body is still being collected.
                if credit > 0 {
                    self.http_push(stream_id, Frame::Window { bytes: credit })?;
                }
                Ok(())
            }
        }
    }

    /// Forwards `REQ_END`, or remembers it for a queued request.
    pub(super) fn request_end(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete || request.end_seen {
            return Ok(());
        }
        if let BodyPlan::Buffer { codec } = request.body_plan {
            // The body is complete: canonicalise it before dispatch so the view
            // only ever sees valid, re-encoded bytes and no stale framing
            // headers (`R14`, `R15`).
            //
            // The head reached here as the raw payload (see
            // [`Scheduler::begin_raw`]). Canonicalising replaces the framing
            // headers the client sent, so this is the one place that has to
            // decode it — the re-encoded head is what the view then reads.
            // Routes with no ingress configured take the streaming path below
            // and never touch their head's bytes.
            let Some(raw) = request.raw.as_ref() else {
                // A buffered request keeps its raw head until dispatch; losing
                // it would be a bug in this build, not a peer's fault.
                return Err(SchedulerError::protocol(
                    "a buffered request reached REQ_END without its raw head",
                ));
            };
            let mut head = match raw.decode().map_err(SchedulerError::Frame)? {
                Frame::ReqHead(head) => head,
                _ => {
                    return Err(SchedulerError::protocol(
                        "a request's raw head did not decode as REQ_HEAD",
                    ));
                }
            };
            let id = raw.id;
            let method = String::from_utf8_lossy(&head.method).into_owned();
            let content_type = header_text(&head, b"content-type");
            let raw_body: Vec<u8> = request.body.iter().flatten().copied().collect();
            if raw_body.len() as u64 > self.cfg.max_body_bytes {
                return self.refuse_body(stream_id, 413);
            }
            let transcoded =
                match transcode_buffered(codec, &method, content_type.as_deref(), &raw_body) {
                    Ok(body) => body,
                    Err(reject) => return self.refuse_body(stream_id, reject.status()),
                };
            apply_transcoded_headers(
                &mut head.headers,
                transcoded.content_type,
                transcoded.bytes.len(),
            );
            let head = Frame::ReqHead(head)
                .to_raw(id)
                .map_err(SchedulerError::Frame)?;
            // A request already waiting for a slot keeps its one queue entry,
            // its `queued_since` and its timeout: `enqueue` would list a FIFO
            // id a second time (the `VecDeque` does not dedupe), so the
            // transcoded body is what the walk will hand over, not a second
            // admission (design §4.6: "no re-drive").
            let queued = self.queue.contains(stream_id);
            if let Some(request) = self.requests.get_mut(&stream_id) {
                request.raw = Some(head);
                request.body = if transcoded.bytes.is_empty() {
                    Vec::new()
                } else {
                    vec![transcoded.bytes]
                };
                request.end_seen = true;
                request.body_ready = true;
                if !queued {
                    request.queued_since = None;
                }
            }
            if queued {
                return Ok(());
            }
            return self.admit_ready(stream_id);
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.end_seen = true;
        }
        let Some((view, held)) = self
            .requests
            .get(&stream_id)
            .map(|request| (request.view, !request.body_ready))
        else {
            self.dropped += 1;
            return Ok(());
        };
        // An `AllowEmptyOnly` request was held back at its head precisely so a
        // view could not answer it before the `413`: this empty `REQ_END` is the
        // proof it is empty, so it is here — and nowhere earlier — that it
        // becomes ready and joins admission. Its head-arrival stamp stops
        // counting now: `enqueue` starts the queue clock for a request that has
        // to wait for a slot.
        match view {
            Some((key, id)) => self.view_push(key, id, Frame::ReqEnd),
            None => {
                if !held {
                    return Ok(());
                }
                if let Some(request) = self.requests.get_mut(&stream_id) {
                    request.body_ready = true;
                    request.queued_since = None;
                }
                self.admit_ready(stream_id)
            }
        }
    }

    /// The client went away (§3.2): forward `CANCEL` and keep the slot.
    ///
    /// The request reports the status it had reached — `0` when the response
    /// never started — with the `client_gone` outcome (§8.1).
    pub(super) fn client_cancel(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        let view = request.view;
        if let Some((key, id)) = view {
            self.view_push(key, id, Frame::Cancel)?;
        }
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.cancelled = true;
            request.outcome = Outcome::ClientGone;
            request.complete = true;
        }
        self.mark_finished(stream_id);
        // The response ends here, so its span is final: charge before the event
        // that reports it. The fair-queue debit lands here, while
        // the slot is still held to the view's `DONE`, not at the later
        // release: the span billed is the request's own and the debit belongs
        // to the charge ([`Scheduler::charge`]). `charge` is
        // idempotent, so that `DONE` charges nothing more.
        self.charge(stream_id);
        self.emit_request(stream_id);
        if view.is_none() {
            // Never dispatched: it leaves the queue, and no slot was ever held.
            // A static read that is still parked keeps both the slot and the
            // descriptor until it returns (design §4.6).
            if self.static_read_parked(stream_id) {
                return Ok(());
            }
            self.release_static_slot(stream_id);
            self.release(stream_id);
        }
        Ok(())
    }

    /// http granted the stream more response-body credit (§3.1): the view gets
    /// to send that much more.
    ///
    /// A static answer has no view to tell: the credit is what the pump was
    /// waiting for, so it is woken directly.
    pub(super) fn client_window(
        &mut self,
        stream_id: u32,
        bytes: u32,
    ) -> Result<(), SchedulerError> {
        let Some(request) = self.requests.get(&stream_id) else {
            self.dropped += 1;
            return Ok(());
        };
        if request.complete {
            return Ok(());
        }
        let view = request.view;
        if let Some(request) = self.requests.get_mut(&stream_id) {
            request.download_credit = request.download_credit.saturating_add(bytes);
        }
        match view {
            Some((key, id)) => self.view_push(key, id, Frame::Window { bytes }),
            None => self.pump_static(stream_id),
        }
    }

    /// http sent more request-body bytes than its credit allowed (§3.1): the
    /// stream is failed on both sides.
    pub(super) fn http_credit_violation(&mut self, stream_id: u32) -> Result<(), SchedulerError> {
        let view = self
            .requests
            .get(&stream_id)
            .and_then(|request| request.view);
        if let Some((key, id)) = view {
            self.view_push(key, id, Frame::Cancel)?;
        }
        self.fail_request(stream_id, Outcome::ViewDied, 502)?;
        // The stream is over on both sides: http stops reading this client's
        // socket for a stream the scheduler will not answer.
        self.http_push(stream_id, Frame::Cancel)
    }

    // ------------------------------------------------------------------ pool
}
