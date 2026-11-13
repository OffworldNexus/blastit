# Result MB.2.5.2 — dispatch a static answer and pump its body (design MB.2.5b)

Base `b8e8806`, branch `release/0.2.0`. Two files, both in `paths`:

| File | +/− |
|---|---|
| `crates/blastit-scheduler/src/scheduler.rs` | +845 / −2 |
| `crates/blastit-scheduler/tests/critical_rewrite.rs` | +895 / −0 (672 → 1,567 lines, inside the shared 1,600 ceiling) |

## What landed

* **Boot.** `run` opens one `Arc<StaticRoot>` per route that declares
  `[route.static]`, before a request is served, from `SchedulerConfig::{manifest_dir,
  static_max_bytes}`; a failure is `SchedulerError::Protocol("static root <root>: <source>")`
  (`StaticError`'s own `Display`), which exits the role. `Scheduler::static_roots` is
  `Vec<Option<Arc<StaticRoot>>>`; `None` for every other route.
* **Head work (`begin_raw`).** A route with a policy computes `declares_body` from the
  borrowed header list and calls `static_asset::head_segments` on the raw target
  (`from_utf8` → `Refused(Encoding)`, never `from_utf8_lossy`). A steps 3–4 refusal is
  answered **there**, through `decide` → the one `refuse_static` helper, before
  `admit_ready`: no slot, no queue entry, no descriptor. `Ok(segments)` sets
  `static_pending`/`static_parts`/`static_has_body`/`static_if_none_match`, stores the
  `plan_body` error in `body_plan_error` and sets `body_ready = true` (design §4.4).
* **Admission.** `admit_ready` takes the static branch first (`static_pending` →
  `admit_static`): `can_dispatch_static()` (`slots < max_workers`, no view) → dispatch,
  else the same bounded queue / `queue_timeout` / `503` + `Retry-After`. Both
  `dispatch_queued` walks take a queued static request through `dispatch_static` while a
  slot is free; `Scheduler::start` and `try_spawn` refuse a route the responder answers
  on its own (`spawnable`), so a pure static route is never forked, and a forward-capable
  one keeps the ordinary lazy spawn (`spawn_for_static`). The fair walk admits statics
  before its view-budget order (see Deviations).
* **`dispatch_static`.** The one function that sets `slots`/`view = None`/`dispatched_at`/
  `deadline`, then `resolve` + `decide` (steps 5–11) and one of: `serve_static` (head from
  `StaticRoot::respond`, then the pump), `forward_static` (`release_static_slot`, clear
  `static_pending`/`dispatched_at`/`deadline`, `queue.remove`, answer the stored
  `body_plan_error` or record a `Buffer` wait, else `admit_ready` — the ordinary view path),
  `refuse_static` (slot back **before** the status; `canned` + `Allow` only on 405 +
  `Cache-Control: no-store` on all four).
* **The pump.** `pump_static` = design §4.6 steps 1–3: `remaining == 0` → `RESP_END` and
  the slot back **before any read**; `pumping` or zero credit → wait; otherwise one
  `spawn_blocking` `pread` of `min(remaining, credit, max_payload, STATIC_CHUNK_BYTES)`
  whose `(stream_id, StaticBody, want, io::Result<Vec<u8>>)` returns over a 64-deep
  `static_in` channel with its own `select!` arm. `client_window` wakes the pump, so no
  second write loop and no per-request thread. No reader counter and no re-drive queue:
  one read per slot and `slots <= max_workers` is the reader and descriptor bound.
* **Release exactly once.** `slot_releasable(holds, pumping) = holds && !pumping` is the
  only predicate; `release_static_slot` is the only decrement and is idempotent.
  `finish_static`, `abort_static` and the read result call it and then `release`;
  `expire_requests` and `client_cancel` release a view-less request only when no read is
  parked — a parked `pread` keeps the slot and the descriptor until its result returns
  (a timed-out/cancelled request stays `complete` meanwhile).
* **Fidelity.** A short read or a read error is `RespAbort { reason: "static: read
  failed" }`, never a truncated body under a promised `Content-Length`. `Forward` behaves
  byte-identically to the pre-node app path; the whole existing suite is unchanged and
  green.
* **The static `405`** now answers `Method Not Allowed` / `method not allowed\n`
  (`canned`'s `405` arm — see Deviations; this was in lane).

## Flow table (behaviour → asserted)

| Behaviour (design §) | Where asserted |
|---|---|
| `Serve` reaches the wire with the responder's own head, header for header, and the exact file bytes (§4.5, §7.2 "+" exact hit) | `a_serve_reaches_the_wire_with_the_responders_own_bytes_and_headers` (compares `wire.headers == respond(...).headers`, plus `Content-Type`, `Cache-Control: immutable`, `nosniff`, `Content-Length` by name) |
| one `request` event with `route`, `rule`, `outcome == ok`; no view fork; no `REQ_HEAD` to any view (§7.2 `R12`) | same test (`requests().len() == 1`, `request_routes() == ["site"]`, `spawns.is_empty()`, `received.is_empty()`) |
| `HEAD` = the GET head with `Content-Length` and **no** `RESP_BODY`; `bytes_out == 0` | `a_head_answers_the_get_head_and_never_a_body_frame` |
| `304` from the head's `If-None-Match`, no body, no `Content-Length`, cache repeated (§4.5) | `an_if_none_match_from_the_head_answers_304_without_a_body` |
| zero-length file = ordinary `200`, `Content-Length: 0`, no body frame, **before any read** (§4.6 step 1) | `a_zero_length_file_is_an_ordinary_200_with_no_body_frame` |
| `400`/`404`/`405`/`413` refused with `no-store`, `Allow` only on the 405, exactly `GET, HEAD`; `outcome == unmatched`; refusal never masked by `spa`/`app` (§4.4) | `a_static_refusal_is_one_scheduler_answer_with_no_store` |
| the `405` is a real `Method Not Allowed` / `method not allowed\n` | same test (status + body + reason + `Allow`) |
| `Serve` outranks the codec error; `Forward` answers the stored `body_plan_error` and forking stays out of it (§4.4) | `a_serve_outranks_the_codec_error_a_forward_stores` (`POST` + `text/plain` on a file = 405; on a miss = 400; `GET` + `Content-Length: 1` = 413 both ways) |
| more than `max_workers` static streams all complete **in waves**; slot held until the read result; ≤ `max_workers` descriptors; two streams pump concurrently (§4.6, `R10`) | `more_than_max_workers_static_streams_complete_in_waves_inside_the_slot_bound` (4 requests, `max_workers = 2`: exactly 2 heads, both with a full window and no end, `/proc/self/fd` into the tree == root dirfd + 2, the queued streams frameless, then `.grant` one → `[2, 3]` in flight, then the fourth answers) |
| a short read aborts and frees the slot **exactly once** (§4.6) | `a_short_read_aborts_the_response_and_frees_the_slot_exactly_once` (`set_len(0)` under an open body → `RespAbort("static: read failed")`, body < promised length, then exactly one of the two waiters is admitted, the other still frameless) |
| a client that stops granting credit is aborted at `request_timeout`, and the slot comes back (§4.6, §7.2) | `a_client_that_stops_granting_credit_is_aborted_at_request_timeout` (`RespAbort("request timeout")`, `bytes_out == INITIAL_WINDOW`, `outcome == timeout`, next request answered) |
| a `Forward` hands the slot back and re-enters the view path, spawn included (`max_workers = 1`: `Serve`, `Forward`, `Forward`, all `200`) (§4.6) | `a_forward_hands_the_slot_back_and_re_enters_the_view_path` (`spawns == [0]`, the view saw exactly the two forwarded heads, no `503`) |
| `slot_releasable` table: `(true,false) → true`, `(true,true) → false`, `(false,_) → false` (§4.6) | `scheduler::tests::a_static_slot_is_releasable_only_when_its_read_is_not_parked` (`--lib`) |

**A pump that released the slot at `RESP_HEAD` fails the concurrency test, and this is
how.** `more_than_max_workers_...` asserts, while the streams are parked, that
`in_flight == [1, 2]` for `max_workers = 2`, that streams 3 and 4 have sent *no frame at
all*, and that `/proc/self/fd` holds exactly the root's dirfd plus 2 descriptors. Release
at the headers lets stream 3 (and then 4) dispatch immediately: the test's first
`until_pumped` never reaches two heads (three arrive), the frameless assertion fails, and
the descriptor count is 3. Verified by mutating `can_dispatch_static` to
`slots < max_workers + 1` — the same two stream tests fail in 5.9 s with three heads
(`condition never became true; heads in flight: …`), and every test passes again with the
mutation reverted.

## Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
```

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked
test scheduler::tests::a_static_slot_is_releasable_only_when_its_read_is_not_parked ... ok
test task::tests::a_burst_of_queued_frames_leaves_in_one_write ... ok
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ make rs-lint
cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.1.2 (.../crates/blastit-scheduler)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.48s
```

```console
$ make rs-doc            # the brief's `cargo doc -D warnings`, workspace-wide
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
 Documenting blastit-scheduler v0.1.2 (.../crates/blastit-scheduler)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.67s
   Generated .../target/doc/blastit/index.html and 12 other files
```

```console
$ cargo fmt --all -- --check
FMT CLEAN (no diff)
```

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
15 × "test result: ok" (lib + 13 integration binaries + doctests); 0 failed;
lib 46, critical_rewrite 23, critical_static_assets 11, critical_static_resolve 14,
admission 11, critical_ingress_enforcement 9, ctl_channel 9, fairness 17, flow 6,
head_allocs 2, isolation 13, lifecycle 18, memory 14, routing 14, doc 1
```

```console
$ uv run --no-project tools/critical_guard.py --base b8e8806 --node MB.2.5.2
node MB.2.5.2: paths ['crates/blastit-scheduler/src/scheduler.rs', 'crates/blastit-scheduler/tests/critical_rewrite.rs']
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [authorized by ...]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.2.5.2
```

(`make critical-guard BASE=develop NODE=MB.2.5.2` is the same tool; `develop` does not
contain this branch's base `b8e8806`, so the lead's brief pins `--base b8e8806`.)

`make check` / `make e2e` were **not** run, per the brief (the lead gates them and a
doc-writer is editing `docs/deploy` + `e2e`).

## Deviations (explicit)

1. **No `Request.static_target` field.** Design §4.2 describes `decide`'s `Serve` moving
   the `StaticFile` into `request.static_target` until `dispatch_static` moves it into
   `respond`. Because `resolve` also runs inside `dispatch_static` (§4.6: a queued request
   holds no descriptor), the file goes `decide` → `respond` inside one function and never
   needs a home on the request: the by-value chain is uninterrupted, nothing is `dup`ed,
   and `release` drops `static_body` (its descriptor) exactly as §4.6 asks. Adding a field
   that is always `None` would have been dead state.
2. **`Request.static_if_none_match` added.** `respond` takes the request's
   `If-None-Match` and `Request` keeps no header list, so `begin_raw` copies that one
   value alongside the `declares_body` bit. Without it `R9`'s `304` is unreachable through
   the scheduler (pinned by `an_if_none_match_from_the_head_answers_304_without_a_body`).
3. **The fair walk admits statics before its view-budget order.** The fair queue orders
   *view* budgets; a static request holds no view, and the fair queue cannot say whether a
   route's head is static (`peek` returns a flow, not a stream id). `first_static_queued`
   therefore takes a queued static request first, in the same loop, while
   `slots < max_workers` — the slot bound is what keeps it from starving the view path. The
   FIFO walk needs no such exception (design §4.6's own order).
4. **The static `405`.** `canned` gained `405 => ("Method Not Allowed", "method not
   allowed\n")`; it is the only status in that table the app path never uses, so the app
   path is byte-identical. This is the design's own line (`§4.4`: "a proper `Method Not
   Allowed` arm is `MB.2.5.2`'s") and the flow half pins status, reason, body and `Allow`.
   It was in lane (`scheduler.rs`).
5. **"Log only status + header names".** This crate has no log sink (events only) and
   `StaticResponse` has no `Debug`; nothing formats the response, and the `request` event
   carries only the status. `serve_static`'s doc records the constraint so no later
   `{:?}` can creep in.
6. **Changed-line budget.** `scheduler.rs` is +845 (budget ≤ 600); the test file is
   +895 of its shared 1,600-line ceiling (design §8 lets the test file grow and forbids
   trimming a `MB.2.5.1` oracle for a budget). Nothing was dropped to save lines; the
   overrun is the documented "raise the budget through a Follow-up" case — see Follow-ups.

## Follow-ups

1. **Budget:** raise `MB.2.5.2`'s `scheduler.rs` changed-line budget from 600 to ~900
   (scheduler.rs +845, of which ~196 are doc-comment lines in this crate's house style).
   No behaviour was cut to fit; the design's §8 escape hatch applies.
2. **`BLASTIT_SCRIPT_NAME` and static routes (design-level, not this leaf).** §4.3 calls
   `head_segments(target)` on the **raw** target, so with a mount prefix the segments
   would include it (`/mount/assets/x.js` → `root/mount/assets/x.js`). I followed the
   design; if a prefixed deployment is meant to serve statics, `MB.2.6`/`MB.2.9` need a
   decision (strip the prefix before `head_segments`, or document that a static root is
   only served at `/`). No test covers a prefix + static route today.
3. **`304` `Content-Length` for `HEAD`+`If-None-Match`** is `respond`'s existing rule (no
   `Content-Length` on a `304`), already pinned by `MB.2.4.2`; the scheduler simply
   forwards the head. No action, recorded so the flow/e2e readers know where it lives.

## Review fixups (attempt 2)

Totals against `b8e8806` after these fixups: `scheduler.rs` +875/−3; `critical_rewrite.rs`
+1046 (672 → **1,718** lines).

**1 [MAJOR] buffered `Forward` whose `REQ_END` arrives while queued — fixed in both
halves.** (a) `forward_static` now arms the "wait for the body" state only for
`matches!(body_plan, Buffer { .. }) && !end_seen`; a request whose body already ended falls
through to `admit_ready`, so nothing has to re-drive it. (b) `request_end`'s `Buffer`
branch no longer re-admits a request that is still in the admission queue: it keeps its one
entry and its `queued_since` (so the queue timeout still bounds it) and returns without
`enqueue`, which the FIFO `VecDeque` would have listed twice. Pinned by
`a_buffered_forward_that_ends_while_queued_is_still_answered` (json front controller, one
parked static stream, body + `REQ_END` while queued → `200 "hello"`, exactly one forwarded
`REQ_HEAD`, and the view's body is the canonical `{"a":1}`). The row fails on the old arm
with the reviewer's exact symptom:

```console
$ (mutation: drop `&& !request.end_seen`) cargo test -p blastit-scheduler --test critical_rewrite --locked buffered_forward
thread 'a_buffered_forward_that_ends_while_queued_is_still_answered' panicked at .../critical_rewrite.rs:1636:
assertion `left == right` failed: [RespHead(RespHead { status: 503, headers: ["Content-Type", "Content-Length", "Retry-After"], .. }), RespBody { len: 14 }, RespEnd]
  left: 503
 right: 200
test result: FAILED. 0 passed; 1 failed; ... ; reverted → 25 passed
```

**2 [MINOR] the half-body cap.** The buffered-forward wait now applies the app path's own
`buffering_count() >= queue_size → 503` + `Retry-After` guard before arming
`body_ready = false`, so a static front controller cannot hold more half-bodies than any
other route, and the route's bound is the same one (§4.6 "the same queue policy").

**3 [MINOR] the two missing rows.** `a_full_static_queue_answers_503_with_retry_after`
(`max_workers = 1`, `queue_size = 1`, one parked stream + one waiter + one more request →
`503`, `queue full\n`, `Retry-After: 5`, `outcome == queue_full`, no fork; the admitted
waiter is asserted still unanswered) and the buffered-forward row above.

**4 [MINOR, budget] the `scheduler.rs` ceiling is raised.** 875 changed lines landed against
the node's 600 (of which ~200 are rustdoc in this crate's house style). **Nothing was cut to
fit**: no §7.2 case, oracle or invariant was trimmed, and the pump/dispatch/release code is
what the design's §4.6 asks for. Recorded so the next reader raises the budget instead of
shrinking the pump.

**The shared test file's ceiling is crossed, and why:** `critical_rewrite.rs` is 1,718 lines
against the recorded 1,600. Design §8 gives that file to `MB.2.5.1` (672) and `MB.2.5.2`
and forbids trimming a `MB.2.5.1` oracle for a budget; the review added two §7.2 rows
finding 1 slipped through (a positive buffered-forward case and the `503` + `Retry-After`
row NODE.md's Notes require), and both are kept in full. The excess is the review's two
rows (+151 lines), not a re-write of the pure half. A follow-up should raise the recorded
ceiling to ~1,750.

### Acceptance after the fixups

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
test result: ok. 46 passed ... (lib)
test result: ok. 25 passed ... (critical_rewrite)
test result: ok. 11 · 9 · 11 · 14 · 9 · 17 · 6 · 2 · 13 · 18 · 14 · 14 · 1 (the other binaries + doctests)
0 failed, 0 ignored — 15/15 targets
```

```console
$ make rs-lint
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.18s
```

```console
$ make rs-doc            # `cargo doc -D warnings`, workspace-wide
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    Finished `dev` profile ... ; Generated .../target/doc/blastit/index.html and 12 other files
```

```console
$ cargo fmt --all -- --check
FMT CLEAN
```

```console
$ uv run --no-project tools/critical_guard.py --base b8e8806 --node MB.2.5.2
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.2.5.2
```
