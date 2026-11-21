# MB.21.4.9 — result

**STATUS: done** (attempt 2: review finding `[MAJOR]` on the `forward_static` path fixed and pinned,
`[MINOR]` line numbers re-cited).

The `413`-vs-`200` race behind `dev_boots_five_unprivileged_roles_and_strips_secrets` is closed by one
rule, enforced at **every** point where a request can reach a view: **a view may only be handed a head
whose body this route may actually hold.** A head that *declares* a body on a route that takes none is
a synchronous `413` in `begin_raw`, before the request is body-ready, queued or dispatched; and a body
whose emptiness is not yet proved keeps the request un-dispatchable until `REQ_END`. There are three
places a request becomes dispatchable — `begin_raw` (the ordinary view path), `request_end` (the
proof), and `forward_static` (a mount miss that falls through to a view) — and all three apply the same
rule.

- **BASE:** `1d92c6a` (captured before the first edit).
- **Changed:** `crates/blastit-scheduler/src/scheduler.rs`,
  `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` (protected, edited in place),
  `crates/blastit-scheduler/tests/head_allocs.rs`, `docs/knowledge/LESSONS.md`.
- **No `blastit-view` change** (production or test): the scheduler is what handed the view a head it
  must not have; `crates/blastit-view/**` is untouched, and so is
  `crates/blastit/tests/critical_raw_ingress.rs` — every case there is `body raw`
  (`StreamPassthrough`), which still dispatches from its head (`R16`), so nothing in it changed (read,
  not edited).
- **Protected files edited in place (never weakened):**
  `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` — one existing case's "wait for the
  view to start" barrier removed and its assertions kept plus one added, and **seven** new cases.
  `crates/blastit/tests/critical_dev_and_fail_closed.rs` is **untouched**
  (`git diff 1d92c6a -- …` is empty, 0 lines), so `assert_eq!(rejected.status, 413)` is exactly as
  `MB.21.4.8` left it.

## 1. The mechanism and the invariant, final line numbers

**What made the `413` lose.** `plan_request_body` early-returned `Ok(BodyPlan::AllowEmptyOnly)` for a
routed request with no `[route.ingress]` **without reading the `Content-Length` the head already
carried**, and `begin_raw` then set `body_ready = !matches!(plan, BodyPlan::Buffer { .. })`, so the
request was dispatched from its head — and `blastit-view` starts the WSGI app on `REQ_HEAD`
(`crates/blastit-view/src/lib.rs:474`/`:501`). The app answers without reading `wsgi.input`, so a body
byte arriving a millisecond later reached `request_body`'s `refuse_body(413)` *after* `view_head` had
forwarded its `200`. The same hole existed a second time on the mount path: a routed head a `static`
mount claimed was admitted (`body_ready = true`), and when the mount missed,
`mount_missed` → `forward_static` → `admit_ready` handed the view a head whose `AllowEmptyOnly` body
was still unproved.

**What now guarantees the refusal comes first — on all three paths.**

1. **Head-time decision, `plan_request_body` (`scheduler.rs:1594`; the refusal at `:1609-1612`).** A
   policy-less route now asks `plan_body` too, with only the head's declared length read
   (`declared_content_length`, `:4633`; no `String` copies, unparsable ⇒ `u64::MAX`). A positive
   `Content-Length` is `BodyNotAllowed` → `413` in `begin_raw` (`:1257`) before the request is marked
   ready, queued or dispatched, and before any byte or `REQ_END` exists.
2. **The `REQ_END` gate, `begin_raw` (`:1509-1538`).** Only `StreamPassthrough` is admitted at its
   head (`R16`). An `AllowEmptyOnly` request gets `body_ready = false` and `queued_since = Some(now)`
   (bounded by `awaiting_end_count() >= queue_size` → `503 queue full`, `:1767`), and `request_end`
   (`:2022`; the setter at `:2103-2128`) is the only thing that makes it ready again — while
   `expire_queue`/`next_deadline` answer a head that never ends with `503 queue timeout`. The first
   byte is still the `413` (`request_body`, `:1951`), and no view has the head.
3. **The forward path, `forward_static` (`:2680`).** A mount that cannot answer hands the request to
   the view, and it now hands it over under the same rule: a plan that is not `StreamPassthrough` and
   whose `REQ_END` has not arrived sets `body_ready = false`, is counted against `queue_size`
   (`buffering_count` for `Buffer` at `:1744`, `awaiting_end_count` for `AllowEmptyOnly` at `:1767`,
   decision at `:2719-2726`) and waits under `queued_since` (`:2741`); `request_end` then admits it
   and `expire_queue` bounds it. So a front controller's mount miss cannot reopen the race: a chunked
   byte there is a `413` with no `REQ_HEAD` and no view started, and an empty `REQ_END` is the app's
   `200`.

The exemptions are only the requests the scheduler answers itself, which never reach a view:
`Resolution::{Bad, MountRefused, Hidden, MethodNotAllowed, Health}`, the unrouted non-learn `404`, a
mount that *answers* (`serve_static`, `:2626`, which resets the plan to `AllowEmptyOnly` and never
forwards), and the `--learn` fallback's forced `StreamPassthrough` (`:1829`). Those keep
`body_ready: true` and are proved by `critical_static_*`, `critical_blastfile_parity` and the learn
cases, all green.

## 2. The deterministic regressions (scheduler-level, no `blastit dev`)

All in `critical_ingress_enforcement.rs`, all failing on BASE and passing with the fix.

**a. `:426` `a_declared_body_on_an_ingress_less_route_is_413_from_the_head`** — a warm view (one
completed `GET /app/warm`), then `POST /app/refused` with `Content-Length: 5`, a **1 ms**
`tokio::time::sleep`, then the 5 bytes: the `413` must be complete *inside that gap*, before any byte
and with no `REQ_END`; the view log still holds only `/app/warm`, no `ReqBody` reached it, the late
byte produced no further frame, and the `request` event is `413` with `bytes_in == 0`.

**b. `:585` `a_mount_miss_with_a_body_before_req_end_is_413_without_a_view`** — the review's probe:
`static "/" { root "public" }` + `view app GET, POST "/*"`, `POST /gated-miss` chunked, 1 ms, one
byte ⇒ `413`, the view log empty **and `Log::spawns` empty** (no view was started at all).

**c. `:619` `a_mount_miss_reaches_the_view_only_after_an_empty_req_end`** — the allowed twin of (b):
same miss, no byte, and the view's `200` for `the-view` after the barrier request (`/barrier-miss`)
completes — the barrier, not a sleep.

**d.** The remaining shapes: `:500` `Content-Length: 0` **and** a chunked head are served only after
their `REQ_END` (barrier again); `:553` a chunked head plus one byte before `REQ_END` is `413` with no
`ReqHead`; `:651` a head with no `REQ_END` at all is a bounded `503 queue timeout`; `:684` waiting
heads take a `queue_size` slot and no more.

**Failing at BASE** (scheduler.rs restored from `1d92c6a`, nothing else changed):

```console
$ cargo test -p blastit-scheduler --locked --test critical_ingress_enforcement -- --exact <case>
=== a_declared_body_on_an_ingress_less_route_is_413_from_the_head      left: 200  right: 413
=== an_ingress_less_request_is_served_only_after_req_end
      left: ["/app/gated", "/app/chunked", "/app/barrier"]  right: ["/app/barrier"]
=== a_chunked_ingress_less_head_cannot_reach_the_view_before_req_end   left: 200  right: 413
=== an_ingress_less_head_that_never_ends_is_a_bounded_503              left: 200  right: 503
=== an_ingress_less_head_that_waits_holds_one_queue_slot_and_no_more   left: 200  right: 503
=== a_mount_miss_with_a_body_before_req_end_is_413_without_a_view      left: 200  right: 413
=== a_mount_miss_reaches_the_view_only_after_an_empty_req_end
      left: ["/gated-miss", "/barrier-miss"]  right: ["/barrier-miss"]
test result: FAILED. 0 passed; 1 failed …            (every case, 7/7)
```

(b) and (c) are the review's exact probe: on BASE the mount-miss chunked byte reads **`200`** and the
view saw the gated head before any byte.

**…and passing with the fix:** the whole file `16 passed` (repeated), and the exact acceptance command
for (a) `1 passed`.

## 3. The suite whose barrier encoded the defect, and the `send_head` sweep

- `:392` `a_non_empty_body_on_a_none_route_is_413`: the `until(|world| !request_ids(0).is_empty())`
  wait (and the trailing `until(… Cancel)` wait that only a dispatched request could satisfy) are
  **gone**; the `413`, `ended` and "no `ReqBody`" assertions are kept and `request_ids(0).is_empty()`
  is added — the view must never see that head.
- **The head-only sweep was one file, not six.** `flow.rs`, `routing.rs`, `isolation.rs`,
  `critical_rewrite.rs` and `critical_blastfile_parity.rs` needed nothing: their `send_head` sites are
  `body raw` routes (`StreamPassthrough`) or are followed by a `REQ_END`. The only failing suite in the
  crate was `head_allocs.rs`: `warm_up` and `per_head` sent a head and waited for the `request` event,
  so they now send `REQ_END` with it (`head_allocs.rs:231`, `:267`) and the allocation contract is
  unchanged — the `+3` delta survives exactly, because the policy-less branch still copies no header
  `String`:

```console
$ cargo test -p blastit-scheduler --locked --test head_allocs -- --nocapture
allocations per 8-header request: decoded 47, raw 6
allocations per head: no policy 67.02, body = "none" 70.02
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
```

## 4. Acceptance evidence (attempt 2, final tree)

```console
$ export PYTHONHOME=… PYO3_PYTHON=…python3.14 LD_LIBRARY_PATH=…   # make's RUST_ENV
$ cargo test -p blastit-scheduler --locked
… 19 suites … "test result: ok." ×19                           exit=0
$ cargo test -p blastit-view --locked
…  9 suites … "test result: ok." ×9                             exit=0
$ make test-critical
… 41 × "test result: ok"; python: Ran 50 tests … OK             exit=0
$ make check
… 166 × "test result: ok"; 0 × FAILED; guard · critical-guard · rs-fmt · rs-lint · rs-doc ·
  test-critical · py-lint · docs-unit · docs-check · docs-test · rs-no-python · seam-check ·
  platform-matrix-check · workflow-check · packaging-gates-test · doc-tone · manifest-purity ·
  plan validate …                                             exit=0
$ uv run --no-project tools/critical_guard.py --base 1d92c6a --node MB.21.4.9
critical-guard: 1d92c6a..working tree, 25 changed file(s)
  M  crates/blastit-scheduler/tests/critical_ingress_enforcement.rs  [authorized by crates/blastit-scheduler/tests/**]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.21.4.9
$ make critical-skip-check          protected critical tests hold no skip construct       exit=0
$ cargo fmt --all -- --check                                                               exit=0
$ uv run --no-project tools/plan.py validate            plan OK                           exit=0
```

`make check` is **fully green** on this run: the `docs-unit` failure another session's untracked
`plan/nodes/…/MB.21.4.5.{2,4}/NODE.md` caused earlier is gone (those files were removed outside this
node), so `dev_boots_five_unprivileged_roles_and_strips_secrets` and both host-memory cases pass — this
host was at 9 GiB / 48 GiB used.

Earlier in this attempt, with those files still present, `docs-unit` was the only failure and it was
proved unrelated: `git archive 1d92c6a` (where the guard passes, `24 passed`) and a copy of this tree
with only the six `MB.21.4.5.*` dirs excluded (`docs-unit`: `96 passed`).

Item 4's load run (attempt 1, unchanged product behaviour for a mountless dev view: `nproc` = 10, five
`yes > /dev/null`, load average 11.29):

```console
$ for i in $(seq 1 20); do cargo test -p blastit --features python --locked \
    --test critical_dev_and_fail_closed -- --exact dev_boots_five_unprivileged_roles_and_strips_secrets; done
runs_ok=20 failures=0
$ git diff 1d92c6a -- crates/blastit/tests/critical_dev_and_fail_closed.rs | wc -l
0
```

## 5. Files (final line numbers)

- `crates/blastit-scheduler/src/scheduler.rs` — the head refusal in `plan_request_body` (`:1594`,
  `:1609-1612`), the `REQ_END` gate and its `queue_size` bound in `begin_raw` (`:1509-1538`),
  `request_end` (`:2022`, `:2103-2128`), `buffering_count` (`:1744`) now counting only `Buffer`
  bodies, `awaiting_end_count` (`:1767`), the same gate in `forward_static` (`:2680`, `:2692-2746`),
  the shared `declared_content_length` (`:4633`), and the `body_ready`/`plan_request_body` rustdoc.
- `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` — seven new cases
  (`:426`, `:500`, `:553`, `:585`, `:619`, `:651`, `:684`), the barrier dropped in `:392`, the
  `StaticTree`/`MOUNT_BLASTFILE`/`mount_world` fixture (`:52-104`) and the `view_paths` helper
  (`:869`).
- `crates/blastit-scheduler/tests/head_allocs.rs` — `warm_up`/`per_head` send `REQ_END` with the head.
- `docs/knowledge/LESSONS.md` — one bullet: an invariant enforced on the first body frame is a race
  when the request was dispatched from its head; measure the head-to-first-byte gap; a "wait for the
  view to start" barrier is the defect written down; and the *forward* path from a mount miss is a
  second dispatch site for the same rule.

## 6. Follow-ups (out of `paths`)

1. `crates/blastit/tests/smoke.rs:315` keeps its own read-to-EOF-and-parse-a-prefix client
   (`MB.21.4.8` §5.2). Recorded, not edited.
2. `python/tests/test_docs_assets.py`'s retired-vocabulary guard scans `plan/`, so a planner writing
   "PHP fallback" into a node body fails a *code* gate. A guard that only reads the shipped tree
   (`docs/`, `README.md`, `crates/`, `python/`) would not tie the docs tier to the plan tier.
3. The LESSONS bullet is from attempt 1; attempt 2's forward-path rule belongs in the same entry and
   is described in §1.3 above rather than as a second bullet, because it is the same invariant seen at
   a second dispatch site.
