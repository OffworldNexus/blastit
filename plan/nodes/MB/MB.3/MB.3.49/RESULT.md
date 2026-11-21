# MB.3.49 — `can_dispatch`'s guard has a leg, and the four stale claims are true again

## Summary

All six items landed. `crates/blastit-scheduler/src/scheduler.rs` changed **comments
only** (no code), `critical_charge.rs` only grew (plus the one `eprintln!` deletion),
and the two `bailed` glosses now say the same thing.

1. **Item 5 — the hole, closed first.** New leg
   `a_view_init_reported_gone_is_never_given_a_second_request`
   (`tests/critical_charge.rs`, +95/−1 with the module paragraph and `SETTLE`).
   A `Script::Hold` view takes request 1
   with `max_workers = 2`, init's `Ctl::Exited` is sent while the view's channel is
   still open (it is still reading — the ctl-before-frames window), and request 2
   then arrives with a worker slot free. The leg asserts for 256 scheduler turns
   that the view's own log holds only `[1]`, i.e. the exited view was given nothing;
   after that the held answer is released, the view's channel is closed, and the
   queued request is served by the **replacement** process (`pids == [1001, 1002]`,
   `200`) — which shows the queue was not simply dropped. `Hold` (not
   `SilentThenExit`) is what makes the dispatch observable: a view parked in
   `step` never reads the second `REQ_HEAD`, so a log-based assertion could pass
   in the broken build; `Hold` keeps reading while it holds, so the red fails on
   the intended assertion. Red and green pasted below.
2. **Item 4** — `eprintln!("DEBUG first dispatched")` deleted from
   `queued_second_request`; it printed on every run of the three queue-wait legs.
   `MB.3.46`'s and `MB.3.45`'s six legs are otherwise byte-identical (the diff is
   additions plus that one deletion; `critical_guard` below).
3. **Item 3** — the `bailed` gloss in `docs/reference/log-events.md` and
   `crates/blastit-events/src/lib.rs`'s `Outcome::Bailed` doc now reads "it
   answered before it went away, which is not a death mid-request". The old
   sentence ("the view process is still alive") is false for the designed retire
   `MB.3.45` itself pins.
4. **Item 6 — choice: state the endpoint, do not defer the debit.** `charge`'s doc
   now records that the fair-queue debit lands at the first charge — for
   `client_cancel` and `refuse_body` while the slot is still held to the view's
   `DONE` — and why: the span billed is the request's own and is final at the
   cancel/refusal; splitting the debit from the charge would need a residual
   carried to `release` and would weaken the `request.charged`/`emit_request`
   ordering invariant; and the flow still leaves the table at `release`, so the
   order is debit-then-release, not a changed ordering rule. A debit-at-release
   would have been a fair-queue semantics change, which this node rules out. The
   same sentence is at both call sites.
5. **Items 1 + 2** — `charge`'s "called from two places … at release" paragraph is
   rewritten (every concluding path charges before its event; `release` is the
   uncharged fallback: the deferred `5xx` and a view that died with nothing
   written). `view_end`'s charge comment no longer says release charges a `504` or
   a cancellation. `only_a_bailed_5xx_a_view_answered_is_a_crash` now names
   `view_answered` as the "a `RESP_HEAD` came from the view" flag and says
   `respond` sets `head_sent` too. The module header's "at release for everything
   else" was the same stale claim and was corrected in passing.

No semantics change: the `view.exited.is_none()` predicate is restored byte-for-byte
(the red run below is the only diff that ever removed it). No dependency, no
`Cargo.*`, no `unsafe`.

## Evidence

### Item 5 — red with the predicate deleted, green with it restored

Red (`scheduler.rs` `can_dispatch`: `&& view.exited.is_none()` removed):

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge a_view_init_reported_gone
running 1 test
test a_view_init_reported_gone_is_never_given_a_second_request ... FAILED

---- a_view_init_reported_gone_is_never_given_a_second_request stdout ----
thread 'a_view_init_reported_gone_is_never_given_a_second_request' (1233954) panicked at crates/blastit-scheduler/tests/critical_charge.rs:739:9:
a view init reported gone was given the second request: [1, 2]
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 6 filtered out
```

Green (predicate restored, whole protected file):

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
running 7 tests
test a_scheduler_written_504_is_charged_before_its_request_event ... ok
test a_scheduler_answered_504_stays_out_of_the_bailout_bill ... ok
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... ok
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... ok
test a_view_init_reported_gone_is_never_given_a_second_request ... ok
test a_view_that_answers_then_retires_is_billed_for_its_answer_not_a_death ... ok
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### Items 1–4/6 — the diffs, as the reader sees them

```console
$ git diff crates/blastit-scheduler/tests/critical_charge.rs
@@ -34,6 +34,11 @@
 //! still `view_died` (the `MB.3.48` flake).
+//!
+//! `MB.3.49` pins the half `MB.3.45` left to its own words: between `Ctl::Exited`
+//! and the ordered close, `can_dispatch`'s `view.exited.is_none()` is what
+//! keeps a second request off a process init has already reported gone, even
+//! with a free worker slot.
...
@@ -101,7 +113,6 @@
     world.until(|world| dispatched(world, 0) == 1).await;
-    eprintln!("DEBUG first dispatched");
     let first = world.http.response(1).await;
         assert!(
...
+async fn a_view_init_reported_gone_is_never_given_a_second_request() {
+    ...   // the 75-line leg pasted in full in the file
```

```console
$ grep -n "bailed" docs/reference/log-events.md | head -2
40:`view_died`, `bailed`, `client_gone`. `bailed` — the handler exited, died or
41:fataled out of a `5xx` it had answered: it answered before it went away, which
42:is not a death mid-request.
$ sed -n '199,203p' crates/blastit-events/src/lib.rs
    /// Distinct from [`Ok`](Self::Ok) because the handler never returned (the
    /// request is a failed one, and `R19` bills it its wall time) and from
    /// [`ViewDied`](Self::ViewDied) because it answered before it went away —
    /// not a death mid-request.
```

Item 6 and items 1/2 are the `charge`, `client_cancel`, `refuse_body` and
`view_end` comments plus the `answered_a_bailout` test doc; `git diff
crates/blastit-scheduler/src/scheduler.rs` shows no `fn` body changed.

### Acceptance commands

```console
$ cargo test -p blastit-scheduler --locked
17 "test result: ok" lines, 0 failed (new count in critical_charge: 7)
$ cargo test -p blastit-view --locked --features php
test result: ok. 75 … 10 … 4 … 12 … 7 … 1 … 0 (1 ignored) … 30 … 1; 0 failed
$ cargo test -p blastit --locked
test result: ok. 194 (1 ignored) … 4 … 19 … 11 … 5 … 19 … 11 … 1 … 3 … 31 … 0; 0 failed
$ make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.26s
$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.75s
$ uv run --no-project tools/critical_guard.py --base 5ee6a57 --node MB.3.49
node MB.3.49: paths ['crates/blastit-scheduler/src/scheduler.rs',
  'crates/blastit-scheduler/tests/critical_charge.rs',
  'docs/reference/log-events.md', 'crates/blastit-events/src/lib.rs']
  M  crates/blastit-scheduler/tests/critical_charge.rs  [authorized by crates/blastit-scheduler/tests/critical_charge.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.49
$ make critical-skip-check
protected critical tests hold no skip construct
```

### `make check`

```console
$ make check; echo exit=$?
…
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

`cargo fmt --all -- --check` is clean throughout.

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/tests/critical_charge.rs` | +95/−1: the new `can_dispatch` leg, `SETTLE`, a module-doc paragraph, and the `eprintln!` deletion; `MB.3.46`'s and `MB.3.45`'s six legs untouched |
| `crates/blastit-scheduler/src/scheduler.rs` | comments only: `charge` (+ `client_cancel`/`refuse_body` call sites) state the fair-queue debit endpoint and the real charge-before-emit map; `view_end` and the `answered_a_bailout` test doc corrected; module header's "at release for everything else" corrected |
| `docs/reference/log-events.md` | 2 lines: the `bailed` gloss |
| `crates/blastit-events/src/lib.rs` | 3 lines: the same gloss in `Outcome::Bailed`'s doc |

## Follow-ups (outside this node's `paths`, or deliberately not taken)

1. **The debit-endpoint choice could get an oracle.** The harness has no view of
   the fair queue (`MB.3.46`/`MB.3.45` Follow-up 1), so item 6 is prose only. A
   fair-queue test that cancels a request whose view is still alive and asserts
   the flow's `used` moved at the cancel (rather than at the `DONE`) would pin the
   documented endpoint; it belongs to the fair-queue suite, not here.
2. **`plan/JOURNAL.md` and `plan/nodes/MB/MB.3/MB.3.45/LOG.md` were already
   modified in the worktree before this node started** (6 changed files in the
   guard's view, 4 owned by this node). Left untouched.
3. `crates/blastit-view/src/php.rs`'s comments around the retire path could repeat
   the old "still alive" phrasing; a grep for `bailed` + `alive` outside this
   node's paths found none in `docs/`, so this is a note, not a finding.
