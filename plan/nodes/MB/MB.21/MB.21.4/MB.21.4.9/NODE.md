---
id: MB.21.4.9
title: "Refuse an AllowEmptyOnly request from its head and never dispatch one before REQ_END: close the 413-vs-200 race behind the dev suite's flake"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.8]
paths:
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/**"
  - "crates/blastit-view/src/**"
  - "crates/blastit-view/tests/**"
  - crates/blastit/tests/critical_dev_and_fail_closed.rs
  - crates/blastit/tests/critical_raw_ingress.rs
  - docs/knowledge/LESSONS.md
attempts: 2
max_attempts: 4
escalation: none
created: "2026-11-16T16:30"
updated: "2026-11-16T17:19"
---

# Goal

Close the `413`-vs-`200` race behind `dev_boots_five_unprivileged_roles_and_strips_secrets`: a
`POST /env` on a view with no `[route.ingress]` is `413` (`R12`, design §10.1/§10.2), and today it is
sometimes the app's `200`. Two halves, in one commit:

1. **Refuse from the head.** `plan_request_body` (`crates/blastit-scheduler/src/scheduler.rs:1550-1600`)
   early-returns `Ok(BodyPlan::AllowEmptyOnly)` for a routed request with no ingress policy *without
   reading `Content-Length`*, so a head that declares `Content-Length: 5` is admitted as an empty plan.
   A head that declares a non-empty body on a route that takes none must be `413` **in `begin_raw`,
   before the request is inserted, marked body-ready, queued or dispatched** — never after a view has
   the head.
2. **No view-bound `AllowEmptyOnly` request before `REQ_END`.** `body_ready = !matches!(plan,
   BodyPlan::Buffer { .. })` (`:1487`) plus `admit_ready` (`:1507`) dispatch an `AllowEmptyOnly`
   request from its head, and `blastit-view` starts the WSGI app on `REQ_HEAD`
   (`crates/blastit-view/src/lib.rs:474`, `:501`). A body byte then reaches `request_body`'s
   `refuse_body(413)` (`:1901`/`:1907`) after `view_head` (`:4154`) already forwarded the app's
   `200`. An `AllowEmptyOnly` request that can reach a view must not be `body_ready` or dispatchable
   until its `REQ_END`; the first non-empty `REQ_BODY` is still the `413`.

Both halves are needed: (1) is the synchronous answer when the head states a length; (2) covers a
chunked / not-yet-arrived body, where only `REQ_END` (an empty end) proves emptiness.

# Context

- **Measured, do not re-derive** (`MB.21.4.8` §1 and §5.1): head, sleep **1 ms**, 5 body bytes against a
  real `blastit dev` → `200` **5/5** (`bytes_in: 0` in the scheduler's event — the body never reached
  it); in the suite the two writes are back-to-back, so the `413` normally wins and only CPU contention
  makes the scheduler deschedule between them (4 failures/20 and 2/40 at 87 % host memory). 4.8 fixed
  the suite's client pairing and left the protected `assert_eq!(rejected.status, 413)` untouched; this
  leaf owns the product race.
- **Why existing coverage passes today.** `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs::a_non_empty_body_on_an_ingress_less_route_is_413`
  sends head, body and `REQ_END` back-to-back on a cold view, so the byte is processed in the same read
  as the head and the `413` wins; it proves the status, not that dispatch cannot happen first.
  `a_non_empty_body_on_a_none_route_is_413` (`:316`) **relies on head dispatch**: it waits
  `until(|world| !request_ids(0).is_empty())` for the view to start, then sends the byte. That wait is
  the defect written as a barrier — drop the wait, keep every assertion (the `413`, no `ReqBody`, the
  empty view log).
- **Allocation contract.** `crates/blastit-scheduler/tests/head_allocs.rs:179-260` asserts a
  policy-less route copies **none** of the `content-type` / `content-encoding` / `content-length`
  `String`s (`ingress == plain + 3 * iterations`) and its `per_head`/`warm_up` send a **head only** and
  wait for the `request` event. Read the length with the existing header scan + `u64` parse (no `String`
  copies, fail closed to `u64::MAX` when unparsable) and send `REQ_END` in `warm_up`/`per_head`; the
  `+3` delta must survive unchanged.
- **Where the wait lives.** `request_end` (`:1958`) sets `end_seen` and, for a non-`Buffer` request with
  no view, does nothing (`None => Ok(())`) — after the invariant it must set `body_ready = true` and
  `admit_ready`. The bound already exists: `begin_raw`'s `Buffer` branch (`:1497-1506`) sets
  `body_ready = false` and `queued_since = Some(now)`, and `expire_queue` (`:2885`, waiting filter
  `!body_ready && queued_since elapsed && view.is_none()`) plus `next_deadline` (`:971`) answer a body
  that never completes with `503 queue timeout`. Mirror that for `AllowEmptyOnly` so a head with no
  `REQ_END` is bounded, not parked.
- **Exempt paths.** Requests the scheduler answers itself never reach a view and keep `body_ready: true`
  with `AllowEmptyOnly`: `Resolution::{Bad, MountRefused, Hidden, MethodNotAllowed, Health}` and the
  unrouted non-learn `404` (`begin_raw`'s insert sites, default `:1313`); the static branch
  (`static_head`/`admit_static`, `:1460-1480` and `:1360-1375`) admits by slot alone (§9.2/§4.6) and
  `respond_static` (`:2573`) is a scheduler answer; the `--learn` fallback overrides to
  `StreamPassthrough` with `body_ready = true` (`:1768`). Do not change those; prove them with
  `critical_static_*`, `critical_blastfile_parity` and the learn cases.
- **HTTP already ends a bodyless request immediately:** `crates/blastit-http/src/conn.rs:446` sends
  `REQ_END` right after `REQ_HEAD` for `BodyFraming::None | ContentLength(0)`, so a `GET` waits one
  frame, not for the client. Scheduler suites that send a head *only* and then wait for a view head,
  response or `request` event must now send `ReqEnd` (they use `send_head` alone: `flow.rs`,
  `routing.rs`, `isolation.rs`, `critical_rewrite.rs`, `critical_blastfile_parity.rs`, `head_allocs.rs`).
  Never weaken an assertion to make one pass — the request really is not admitted until it ends.
- **View:** no production change is expected (the scheduler must stop handing it a head early).
  `crates/blastit-view/**` is in `paths` so a test there can pin the contract, and so a defect found at
  `lib.rs:474`/`:501` has a home. If no view file changes, say so in `RESULT.md`.
- **Protected files:** `critical_ingress_enforcement.rs` (new case + the two barrier changes above),
  `critical_dev_and_fail_closed.rs` and `critical_raw_ingress.rs` only if something there changes
  (additions are always allowed; `critical_raw_ingress.rs`'s cases are `body raw` =
  `StreamPassthrough`, so it is expected to stay untouched — read it and say so). The dev suite's
  `assert_eq!(rejected.status, 413)` is **not** edited.
- **Host:** this machine may be above `BLASTIT_MEM_HIGH_WATERMARK` during the run, so `blastit dev`
  refuses to boot and two host-memory cases in `critical_dev_and_fail_closed.rs` fail for reasons that
  are **not this leaf's**. Reproduce those at `BASE` and report the refusal text; the required
  deterministic regression is scheduler-level precisely because it needs no `blastit dev`.

# Acceptance

```console
$ BASE=$(git rev-parse HEAD)                       # captured before the first edit
$ cargo test -p blastit-scheduler --locked --test critical_ingress_enforcement -- --exact <new_test>
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit-view --locked
$ cargo test -p blastit --features python --locked --test critical_dev_and_fail_closed -- --exact dev_boots_five_unprivileged_roles_and_strips_secrets --nocapture
$ make test-critical
$ make check
$ uv run --no-project tools/critical_guard.py --base "$BASE" --node MB.21.4.9
$ make critical-skip-check
$ cargo fmt --all -- --check
$ uv run --no-project tools/plan.py validate
```

1. **A deterministic regression that fails on `BASE` and passes with the fix**, in
   `critical_ingress_enforcement.rs`, unprivileged and with no `blastit dev`: a warm view (one completed
   `GET`), then `HEAD` = `POST /app/` with `Content-Length: 5`, a 1 ms `tokio::time::sleep`, then the 5
   bytes → `413`, `ended`, `world.log.request_ids(0)` empty and no `ReqBody` for that id (4.8's 5/5 race
   shape). Paste the failing run from `BASE` and the passing run. A head-only leg (`Content-Length: 5`,
   no byte, no `REQ_END`, bounded `timeout`) that expects the `413` is a good second shape if the 1 ms
   leg is awkward.
2. **No view-bound `AllowEmptyOnly` request is body-ready before `REQ_END`:** a `Content-Length: 0` head
   (and a `Transfer-Encoding: chunked` head with no byte) is served only after `REQ_END` — prove the
   ordering with a barrier request that completes, not with a sleep — and a chunked head plus one byte
   before `REQ_END` is `413` with no `ReqHead`. A head with no `REQ_END` at all is a bounded
   `503 queue timeout`, not a parked request.
3. **The head refusal is synchronous:** the `Content-Length: 5` request is answered before any byte and
   before `REQ_END`, `status_of == 413`, `ended(frames)`, and the scheduler logs no `request` event with
   `bytes_in > 0` for it.
4. **The dev suite's 413 assertion is unchanged** (`git diff "$BASE" -- crates/blastit/tests/critical_dev_and_fail_closed.rs` removes no `assert`; if untouched, the diff is empty) and
   `dev_boots_five_unprivileged_roles_and_strips_secrets` passes under load (e.g. 20 runs with
   `nproc/2` `yes > /dev/null`), or the only failure is the documented host-memory refusal.
5. `make test-critical` and `make check` exit 0 — or, above the watermark, exit non-zero **only** on the
   two named host-memory cases, with the identical failure shown at `BASE`.
6. `critical_guard.py --base "$BASE" --node MB.21.4.9` reports every protected change authorized by
   this node; `make critical-skip-check` passes (no `#[ignore]`).
7. `RESULT.md` states the mechanism in one paragraph: what made the `413` lose (the no-ingress early
   return, then head dispatch), what now guarantees the refusal comes first (the head-level decision and
   the `REQ_END` gate), with final line numbers. Add one `docs/knowledge/LESSONS.md` bullet: an
   invariant enforced on the first body frame is a race when the request was dispatched from its head.

# Notes

- No dependency changes (`Cargo.toml`/`Cargo.lock` are not in `paths`). No `blastit-ingress` change:
  `plan_body` already returns `Err(BodyNotAllowed)` → `413` for a positive length, so
  `plan_request_body` can express both rules from the head it already holds.
- Keep the leaf small. If the head-only `send_head` sweep needs more than ~8 scheduler test files, stop
  and report `needs_split` with the list instead of half-applying the invariant.
- This closes `MB.21.4.8` §5.1. Out of scope: `crates/blastit/tests/smoke.rs:315`'s copy of the old
  reader — record it, do not edit it.
