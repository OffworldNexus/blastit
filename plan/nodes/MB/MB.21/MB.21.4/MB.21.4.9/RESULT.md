# MB.21.4.9 — result

**STATUS: done.** The `413`-vs-`200` race behind `dev_boots_five_unprivileged_roles_and_strips_secrets`
is closed at both ends: a head that **declares** a body on a route that takes none is refused
**synchronously in `begin_raw`**, and an `AllowEmptyOnly` request that can reach a view is not
body-ready (so not dispatchable) until its `REQ_END` proves the body empty.

- **BASE:** `eeccd9f` (`LOG.md` "base eeccd9f"); `HEAD` at the start of this attempt was `1d92c6a`,
  the plan-only commit that changed no code, so `eeccd9f..` and `1d92c6a..` are the same tree for
  every file here.
- **Changed:** `crates/blastit-scheduler/src/scheduler.rs`,
  `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` (protected, edited in place),
  `crates/blastit-scheduler/tests/head_allocs.rs`, `docs/knowledge/LESSONS.md`.
- **No `blastit-view` change** (production or test): `git diff --stat eeccd9f -- crates/blastit-view/`
  is empty. The scheduler is what handed the view a head it must not have; `lib.rs:474`/`:501` stay as
  they are. `crates/blastit/tests/critical_raw_ingress.rs` is untouched too — every case there is
  `body raw` (`StreamPassthrough`), which still dispatches from its head (`R16`), so nothing in it
  changed (read, not edited).
- **Protected files edited in place (never weakened):**
  `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` — one existing case's "wait for the
  view to start" barrier removed and every assertion kept plus one added, and five new cases.
  `crates/blastit/tests/critical_dev_and_fail_closed.rs` is **untouched**
  (`git diff eeccd9f -- crates/blastit/tests/critical_dev_and_fail_closed.rs | wc -l` → `0`), so
  `assert_eq!(rejected.status, 413)` is exactly as `MB.21.4.8` left it.

## 1. The mechanism, final line numbers

**What made the `413` lose.** `plan_request_body` (`scheduler.rs:1594`) early-returned
`Ok(BodyPlan::AllowEmptyOnly)` for a routed request with no `[route.ingress]` **without reading the
`Content-Length` the head already carried**; `begin_raw` then set `body_ready = !matches!(plan,
BodyPlan::Buffer { .. })` and called `admit_ready`, so the request was dispatched from its head.
`blastit-view` starts the WSGI app on `REQ_HEAD` (`crates/blastit-view/src/lib.rs:474`/`:501`), the app
answers without reading `wsgi.input`, and a body byte arriving a millisecond later reached
`request_body`'s `refuse_body(413)` (`scheduler.rs:1966`) *after* `view_head` had already forwarded the
app's `200` — so the byte's refusal wrote nothing and the client saw `200`. In a suite the head and the
byte share one read, which is why the defect only surfaced under load (4/20 and 2/40 at 87 % host
memory) and was **deterministic at a 1 ms gap** (5/5 against a real `blastit dev`, `MB.21.4.8` §1/§5.1).

**What now guarantees the refusal comes first.**

1. **The head decides.** `plan_request_body` asks `plan_body` for the policy-less route too, with only
   the head's declared length read (`scheduler.rs:1609-1613`, `declared_content_length` at `:4615` — the
   header list is walked in place, so no `String` is copied and an unparsable value is `u64::MAX`). A
   `Content-Length: 5` is `BodyNotAllowed` → `413` in `begin_raw`'s `Err(status)` arm (`:1487-1490`)
   **before** the request is body-ready, queued or dispatched, and with no byte and no `REQ_END` in
   existence.
2. **`REQ_END` is the only thing that admits an empty-only request.** In `begin_raw`
   (`scheduler.rs:1509-1533`) `body_ready` is `false` for every plan except `StreamPassthrough`,
   `queued_since` is stamped, and the request is counted against `queue_size` by
   `awaiting_end_count` (`:1767`) exactly as a half-collected `Buffer` body is by
   `buffering_count` (`:1744`, now filtered to `Buffer`, so an empty-only wait never takes a buffer
   slot). `request_end` (`:2022`) sets `body_ready = true`, clears the head's stamp and calls
   `admit_ready` (`:2120-2127`) when the `REQ_END` arrives empty — so a `Content-Length: 0` or chunked
   head is served only after its end, and a chunked head's first byte is the `413` while the view has
   never seen a `REQ_HEAD`.
3. **The wait is bounded, twice.** `expire_queue`/`next_deadline` already wake on
   `!body_ready && queued_since + queue_timeout` (`:2995-3005`), which answers a head that never ends
   with `503 queue timeout`; and the new `queue_size` count answers one head too many with
   `503 queue full` instead of parking an unbounded number of streams, each holding its raw payload.

Static mounts, the `Resolution::{Bad, MountRefused, Hidden, MethodNotAllowed, Health}` answers and the
`--learn` passthrough are untouched, because all of them insert with `body_ready = true` and take the
paths they took before (`critical_static_*`, `critical_blastfile_parity`, the learn cases: green).

## 2. The deterministic regression: fails on BASE, passes now

`critical_ingress_enforcement.rs:360`, `a_declared_body_on_an_ingress_less_route_is_413_from_the_head`:
a warm view (one completed `GET /app/warm`), then `POST /app/refused` with `Content-Length: 5`, a
**1 ms** `tokio::time::sleep`, and the `413` asserted complete **inside that gap** — before the five
bytes are sent and with no `REQ_END` ever sent. It then sends `hello` and asserts the view's log still
holds only `/app/warm`, no `ReqBody` reached it, the late byte produced no further frame, and the
`request` event is `413` with `bytes_in == 0`.

**It fails on BASE.** Reconstructed without touching the working tree or its history: `git archive`
of `eeccd9f` into a scratch tree, the two test files copied over it, and a separate
`CARGO_TARGET_DIR` — so the pre-fix `scheduler.rs` meets the post-fix tests and nothing else moves:

```console
$ git archive eeccd9f | tar -x -C /tmp/mb2149-base
$ cp <worktree>/crates/blastit-scheduler/tests/{critical_ingress_enforcement,head_allocs}.rs /tmp/mb2149-base/crates/blastit-scheduler/tests/
$ cd /tmp/mb2149-base && CARGO_TARGET_DIR=/tmp/mb2149-base-target cargo test -p blastit-scheduler --locked \
    --test critical_ingress_enforcement -- --exact a_declared_body_on_an_ingress_less_route_is_413_from_the_head
running 1 test
test a_declared_body_on_an_ingress_less_route_is_413_from_the_head ... FAILED
panicked at crates/blastit-scheduler/tests/critical_ingress_enforcement.rs:393:5:
assertion `left == right` failed
  left: 200
 right: 413
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 13 filtered out; finished in 0.01s
```

The whole file at BASE fails on **all five** new cases and on nothing else — the exact shape the
`MB.21.4.8` measurement predicted, each one a `200`/a view that should not have the head:

```console
$ (same scratch tree) cargo test -p blastit-scheduler --locked --test critical_ingress_enforcement
test an_ingress_less_head_that_waits_holds_one_queue_slot_and_no_more ... FAILED   left: 200  right: 503  (:583)
test a_chunked_ingress_less_head_cannot_reach_the_view_before_req_end ... FAILED   left: 200  right: 413  (:510)
test an_ingress_less_request_is_served_only_after_req_end ... FAILED
      left: ["/app/gated", "/app/chunked", "/app/barrier"]   right: ["/app/barrier"]                   (:466)
test a_declared_body_on_an_ingress_less_route_is_413_from_the_head ... FAILED   left: 200  right: 413  (:393)
test an_ingress_less_head_that_never_ends_is_a_bounded_503 ... FAILED          left: 200  right: 503  (:542)
test result: FAILED. 9 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

**…and passes with the fix** (the acceptance command, plus ten consecutive whole-file runs):

```console
$ cargo test -p blastit-scheduler --locked --test critical_ingress_enforcement \
    -- --exact a_declared_body_on_an_ingress_less_route_is_413_from_the_head
test a_declared_body_on_an_ingress_less_route_is_413_from_the_head ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out; finished in 0.01s

$ for i in $(seq 1 10); do cargo test -p blastit-scheduler --locked \
    --test critical_ingress_enforcement; done
10 runs × "test result: ok. 14 passed; 0 failed"        # no flakes
```

Item 2's other shapes, same protected file: `:434` `an_ingress_less_request_is_served_only_after_req_end`
proves the ordering with a **barrier request that completes**, never a sleep — a second request's `200`
lands on the same `http.chan`, which (the scheduler being single-threaded) proves every earlier frame
was handled while a `Content-Length: 0` head *and* a `Transfer-Encoding: chunked` head are both still
not at the view; each is then admitted by its own empty `REQ_END`, and the view sees all three, in
order. `:487` proves a chunked head plus one byte before `REQ_END` is `413` with no `ReqHead`. `:519`
proves a head with no `REQ_END` at all is a bounded `503 queue timeout`. `:552` proves a waiting head
takes one `queue_size` slot and no more (`503 queue full`), and that the refusal does not disturb the
request inside the allowance.

## 3. The barrier that encoded the defect, and the head-only sweep

- `critical_ingress_enforcement.rs:326`, `a_non_empty_body_on_a_none_route_is_413`: the
  `until(|world| !request_ids(0).is_empty())` wait (a wait only a *dispatched* head could satisfy — the
  defect written down as a barrier) and the trailing `until(… Cancel)` are gone; the `413`, `ended` and
  "no `ReqBody`" assertions are kept and `request_ids(0).is_empty()` is **added** — the view must never
  see that head. Nothing was weakened.
- **The head-only sweep was one file, not six.** `flow.rs`, `routing.rs`, `isolation.rs`,
  `critical_rewrite.rs` and `critical_blastfile_parity.rs` needed nothing: their `send_head` sites are
  `body raw` routes (`StreamPassthrough`, still head-dispatched) or are followed by a `REQ_END` in the
  same test (e.g. `routing.rs:18-19`, `critical_blastfile_parity.rs:93-95`). All five suites stayed
  green, untouched. The only suite that sent a head and then waited for something only dispatch could
  produce was `head_allocs.rs`: `warm_up` (`:231-235`) and `per_head` (`:256-273`) now send the `REQ_END`
  with the head, and the allocation contract is unchanged — the policy-less branch still copies **no**
  header `String`, so the delta is still exactly three:

```console
$ cargo test -p blastit-scheduler --locked --test head_allocs -- --nocapture
allocations per 8-header request: decoded 47, raw 6
allocations per head: no policy 67.02, body = "none" 70.02     # +3.00, unchanged
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;
```

## 4. Acceptance evidence

```console
$ cargo test -p blastit-scheduler --locked                                  exit=0
  18 targets (unit + integration) + doctests, 19 × "test result: ok", 0 × FAILED
$ cargo test -p blastit-view --locked                                      exit=0
  9 test targets ("test result: ok", 0 × FAILED)
$ cargo test -p blastit --features python --locked --test critical_dev_and_fail_closed \
    -- --exact dev_boots_five_unprivileged_roles_and_strips_secrets --nocapture
test dev_boots_five_unprivileged_roles_and_strips_secrets ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 19 filtered out; finished in 0.20s
```

Item 4's load run (`nproc` = 10, five `yes > /dev/null` held for the whole loop, the test binary run
directly so no build competes with it):

```console
$ pass=0; fail=0; for i in $(seq 1 20); do "$bin" --exact \
    dev_boots_five_unprivileged_roles_and_strips_secrets >/tmp/run-$i.log 2>&1 && pass=$((pass+1)) || fail=$((fail+1)); done
pass=20 fail=0                                    # no 200, no host-memory refusal
$ git diff eeccd9f -- crates/blastit/tests/critical_dev_and_fail_closed.rs | wc -l
0
```

```console
$ make test-critical                                                       exit=0
  … 41 Rust suites ok;  python -m unittest discover -s python/tests/critical → "Ran 50 tests … OK"
$ make rs-lint                                                             exit=0
$ make rs-doc                                                              exit=0
$ make rs-test                                                             exit=0
  122 × "test result: ok", 0 × FAILED
$ make rs-no-python                                                        exit=0
$ make docs-check                                                          exit=0
$ make seam-check                                                          exit=0
$ make platform-matrix-check                                               exit=0
$ make workflow-check                                                      exit=0
$ make packaging-gates-test                                                exit=0
$ make doc-tone                                                            exit=0
$ make manifest-purity                                                     exit=0
$ make plan-validate                                                       exit=0
$ cargo fmt --all -- --check                                               exit=0
$ uv run --no-project tools/plan.py validate
plan OK
$ uv run --no-project tools/critical_guard.py --base eeccd9f --node MB.21.4.9
critical-guard: eeccd9f..working tree, 27 changed file(s)
node MB.21.4.9: paths [...]
  M  crates/blastit-scheduler/tests/critical_ingress_enforcement.rs  [authorized by crates/blastit-scheduler/tests/**]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.21.4.9   exit=0
$ make critical-skip-check
protected critical tests hold no skip construct                            exit=0
```

`make check` is **green on every target that this node can influence** (163 × `test result: ok`, zero
build/lint/doc failures before `docs-unit`), and stops only at `docs-unit` (`docs-test` depends on it)
on two files another node's planner created in `plan/` **after** the code was finished — `plan/` is not
in this node's `paths`:

```console
$ make check
FAILED python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_php_vocabulary
  plan/nodes/MB/MB.21/MB.21.4/MB.21.4.5/MB.21.4.5.2/NODE.md:41: 'fallback': a PHP fallback is refused…
  plan/nodes/MB/MB.21/MB.21.4/MB.21.4.5/MB.21.4.5.4/NODE.md:43: 'fallback': a PHP fallback is refused…
1 failed, 95 passed, 65 subtests passed
make: *** [Makefile:315: docs-unit] Error 1
```

Those two files are **untracked** (`git status --short plan/` shows `?? …MB.21.4.5.2/`, `?? …MB.21.4.5.4/`)
and are live briefs for `MB.21.4.5`, whose prose legitimately uses `fallback` about a `[route.static]`
mount — the guard cannot tell that from the retired PHP transport. Proof it is the whole cause and
nothing of mine: this tree's own five `docs-unit` files pass as soon as those two directories are out
of the scan, and the same guard test passes on a tree that never had them.

```console
$ rsync -a --exclude target --exclude .git --exclude 'MB.21.4.5.*' ./ /tmp/mb2149-probe/
$ (cd /tmp/mb2149-probe && UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
     python/tests/test_runnable_docs.py python/tests/runnable_docs/test_harness.py \
     python/tests/test_docs_assets.py python/tests/test_release_scripts.py scripts/test_doc_tone.py -q)
96 passed, 65 subtests passed in 1.53s

$ (cd /tmp/mb2149-base && UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
     python/tests/test_docs_assets.py -q -k retired_php_vocabulary)
1 passed, 23 deselected in 0.20s
```

Every other `make check` target was therefore also run individually on the final tree (above, all
`exit=0`), and the scheduled work was re-run at the end: `scheduler`, `view`, the dev case, the ten
whole-file runs, `head_allocs`, `test-critical`, `rs-test`, `rs-lint`, `rs-doc`, `rs-no-python`, the
guard and the fmt/plan checks.

No host-memory caveat applies: the machine was at ~4 GiB / 48 GiB used during the gate runs, so
`a_host_whose_page_cache_is_above_the_default_still_boots_and_serves` and the dev memory advisory both
pass, and `blastit dev` never refused to boot.

*Note for the gate:* this worktree was shared with a concurrent session during the attempt (it ran
`cargo build`/`make rs-no-python` in the same `target/`, and at one point rewrote
`crates/blastit-scheduler/src/scheduler.rs` from a git object for its own BASE comparison). Every
number above was re-measured on the final tree, and `sha256sum` of the two edited scheduler files was
checked before and after the last `make check` to confirm no third party moved them mid-run.

## 5. Files

- `crates/blastit-scheduler/src/scheduler.rs` — the head refusal in `plan_request_body` (`:1594`,
  policy-less branch `:1609-1613`), the `REQ_END` gate and its `queue_size` bound in `begin_raw`
  (`:1509-1533`), `request_end` (`:2022`, admit at `:2120-2127`), `buffering_count` (`:1744`) now
  counting only `Buffer` bodies, `awaiting_end_count` (`:1767`), the shared `declared_content_length`
  (`:4615`), and the `body_ready`/`queued_since`/`plan_request_body` rustdoc.
- `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` — five new cases (`:360`, `:434`,
  `:487`, `:519`, `:552`), the barrier dropped in `:326` (+ one assertion added), the `view_paths`
  helper (`:737`) and `ingress_less_manifest` (`:39`).
- `crates/blastit-scheduler/tests/head_allocs.rs` — `warm_up`/`per_head` send `REQ_END` with the head;
  the `+3` allocation delta is asserted unchanged.
- `docs/knowledge/LESSONS.md` — one bullet: an invariant enforced on the first body frame is a race
  when the request was dispatched from its head; measure the head-to-first-byte gap; a "wait for the
  view to start" barrier is the defect written down; prove the regression in a `git archive` scratch
  tree instead of stashing the worktree.

## 6. Follow-ups (out of `paths`)

1. `crates/blastit/tests/smoke.rs:315` keeps its own read-to-EOF-and-parse-a-prefix client
   (`MB.21.4.8` §5.2). Recorded, not edited.
2. `python/tests/test_docs_assets.py`'s retired-vocabulary guard scans `plan/` (`live_briefs`), so a
   planner writing "PHP fallback" into a live node body fails a *code* gate (`make docs-unit`, and with
   it `make check`/`docs-test`). A guard that reads only the shipped tree (`docs/`, `README.md`,
   `crates/`, `python/`) would not tie the docs tier to the plan tier — the flags above are
   `MB.21.4.5.2`/`MB.21.4.5.4`, not this node.
