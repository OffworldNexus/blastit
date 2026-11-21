# RESULT: MB.3.50 — sweep the stale claims instead of patching three more

## Summary

Comments, one doc marker, one fixture and one LESSONS line. No behaviour change, no
assertion weakened, no dependency, no plan-status edit.

The sweep was grep-driven (`charge`, `charged`, `release`, `emit_request`, `bailed`,
`view_died`, `head_sent`, `view_answered`, `queue`, `Ctl::Exited`, `ViewIn::Closed`) over
`crates/blastit-scheduler/src/scheduler.rs`, and every doc paragraph around a hit was read
against the code that ships it.

**Inspected: 52 claims. Changed: 20** (17 in `scheduler.rs`, 2 in
`manifest/src/model.rs`, 1 in the protected `--learn` fixture), plus the `MB.3.21` stale
command and the new `LESSONS.md` line.

**Attempt 2** closed the one MAJOR the reviewer found: `charge_ns`'s doc still said a
scheduler-written `502`/`504` "never held the worker its wall spans" — the very claim this
sweep proved false at :3756-3762, and one I had wrongly listed as confirmed in attempt 1. It
was the last surviving instance in the crate (`grep -rn 'never held\|wall span\|held the
worker' crates/` now returns only `critical_charge.rs`'s legitimate definition of the event's
own wall span). The `release`-fallback wording also lost "head-only": the `view_done` guard is
`head_sent && !complete`, so a response can lose only its `RESP_END` after head *and* body.

Nothing found where the **code** is wrong: every false claim was a comment describing an
older code shape (`MB.3.41`/`.45`/`.49` moved completion, charging and the retire leg under
them).

## Changed claims, old sentence → what is true now

### `crates/blastit-scheduler/src/scheduler.rs`

1. **Module header, view outcomes (:47-52)** — old: "A `RESP_ABORT` … and so is a response a
   view **completed** with a `5xx`: a contained `exit()`/`die()`/fatal answers `500` and
   keeps its worker, but the request failed." True now: `RESP_ABORT` **and** `DONE { Errored }`
   are `view_died` (the closed list has no "the app failed"); a completed `5xx` is `ok` when
   the handler returned and `bailed` when a contained exit/die/fatal answered it — and that
   worker **retires** (`php.rs::ended` returns `retire = true`), so it does not "keep its
   worker".
2. **Module header, `view_died` (:117-120)** — old: "`view_died` is left for a view process
   that actually went away mid-request." True now: it is the event table's gloss *and* the
   closed list's label for a response that died under a live view (`RESP_ABORT`,
   `DONE { Errored }`) and for a stream failed on both sides (`http_credit_violation`); a
   completed `5xx` is never it.
3. **`Request::report` field (:594-599)** — old: "…and drops it for a `5xx`." True now: the
   report is ignored only when the view **bailed out** of a `5xx`; an app's own `500`
   returned and keeps the clamp.
4. **`Request::charged_ns` field (:600-605)** — old: "…or the request's own wall time when the
   view failed it." True now: only when a view bailed out of a `5xx` it had answered; any
   other failure charges the observed span.
5. **`Request::charged` field (:606-609)** — old: "Exactly one of the response end and the
   release does it (D5)." Now: "exactly one of the path that concludes the request and the
   release does it: the charge is idempotent, so whichever runs second is a no-op" — the
   response does not always "end" (a cancellation) and `release` is not the only other path.
6. **`Scheduler::charge` doc, scheduler-written `5xx` (:3756-3763)** — old: "…it never had the
   worker it would have been charged for." False for a `502`/`504`, which *did* hold a slot.
   True now: the crash bill's arrival→last-byte span would charge a **queue wait** the request
   never spent on a worker.
7. **`Scheduler::charge` doc, `release` fallback (:3767-3775)** — old: "`release` charges again
   as the fallback … the completed `5xx` whose `DONE` decides its label (`MB.3.41`), and a view
   that went away before any of the paths above could run." The second example is false:
   `view_gone` → `fail_request` **charges at :3686** before `release`, so a gone view's
   request is not charged at release. The three real paths are: a deferred `5xx` whose `DONE`
   labels it, a deferred `5xx` whose channel closes before that `DONE` (`fail_request` returns
   early on a complete request), and a `DONE` that answers a response whose head is out but
   whose `RESP_END` never came (`view_done`'s `head_sent && !complete` arm — not "head-only":
   the head *and* body can be out, so `echo …; exit;` → `DONE{Bailed}` lands here too).
8. **`Scheduler::charge` doc, fair debit endpoint (:3775-3783)** — the surprising claim is
   true and stays (`debit` happens inside `charge`; `release_flow` only decrements
   `in_flight`), now qualified: the slot is held to the view's `DONE` **when the request
   already has a view**; a view-less request releases in the same turn, so debit and release
   coincide (`client_cancel`/`refuse_body`'s `if view.is_none()` arms).
9. **`Scheduler::view_done` doc, duplicate/late `DONE` (:3920-3926)** — old: "A `DONE` that
   arrives twice, or one that arrives after the response was already answered by a timeout,
   finds no stream to release the first time and is simply dropped the second." Backwards and
   conflated: the **first** `DONE` releases (removing the dispatch entry), the **second** is
   counted as dropped; a `DONE` for an already-completed request still frees the slot and can
   only change the label of a `5xx` the view itself answered.
10. **`Scheduler::view_done` doc, `view_died` (:3931-3933)** — old: "a `5xx` a live view
    completed is never that." Now: "a `5xx` a view **completed** is never that, whether its
    handler returned (`ok`) or bailed (`bailed`)" — "live" was the wrong axis after the retire
    leg.
11. **`Scheduler::view_end` doc, where the deferred `5xx` finally charges (:4071-4080)** — old:
    "…the request's `DONE`, the ordered close of the view's channel (a view that went away), or
    a `stuck_threads` retire. A live view that simply loses that one frame holds the request —
    and its event — until one of those happens: neither `client_cancel` … nor
    `expire_requests` (which only marks it overdue) releases it." Now: the `DONE` or the
    ordered close — and a `stuck_threads` retire **ends through that close** (`retire` drops
    `view.out`), so it is not a third kind of path; `expire_requests` never releases it
    directly, and the retire it may emit ends it through the close.
12. **`Scheduler::view_end` inline comment (:4117-4122)** — old: "…— this deferred `5xx` once
    its `DONE` labels it, and a view that died with nothing written." The second example is
    false (that path is charged by `fail_request`). Now: this deferred `5xx` once its `DONE`
    labels it **or its view's channel closes first**, and a `DONE` for a response whose head is
    out but whose `RESP_END` never came.
13. **`completed_outcome` doc (:4231-4238)** — old: "it is not `view_died` either, because the
    process is alive and takes the next request." Falsified by `MB.3.45`'s retire leg. Now,
    in `Outcome::Bailed`'s own words: it answered before it went away, so it is no death
    mid-request; the worker retires after that request (a bailout leaves the interpreter
    unusable, `crates/blastit-view/src/php.rs`), and its ordered `view.chan` frames are read
    before the close that would fail what it owes (`Scheduler::view_exited`).
14. **`completed_outcome` doc marker (:4240)** — the paragraph explaining why `view_answered`
    is required was a `//` line, not `///`. Now `///`, so the paragraph belongs to the item.
15. **`answered_a_bailout` doc, `view_answered` bullet (:4261-4266)** — old: "it never had the
    worker it would have been charged for, and must not pay the queue wait." Same falsehood as
    (6): a `502`/`504` did hold a worker. Now: the view never reported for it, and the crash
    bill's arrival→last-byte span would charge a queue wait it never spent on a worker.
16. **`a_completed_response_is_a_bailed_5xx_or_an_answer` test doc (:4570-4573)** — old:
    "`view_died` is left for a view process that actually went away mid-request." Now: it is
    the gloss and the closed list's label for a response that died under the view, so a
    completed response is never it. (Assertions untouched.)
17. **`charge_ns` doc, scheduler-written `5xx` (:4322-4326, the attempt-2 MAJOR)** — old:
    "…and a `502` or `504` the scheduler wrote itself, **which never held the worker its wall
    spans**." False for the same reason as (6): a *dispatched* `502`/`504` did hold a slot.
    Now, in the :3760-3762 wording: "the view never reported for it, and the crash bill's
    arrival→last-byte span would charge a queue wait the request never spent on a worker."
    This was the last surviving instance of that sentence in the crate; it was listed as
    "confirmed accurate" in attempt 1, which is why the sweep needed this pass.

### `crates/blastit-manifest/src/model.rs`

18. **`Route::with_static` (:415)** — old: "Parsing is the only caller." False:
    `edit.rs:943` calls it in `expected_routes`. Now: parsing sets it, and so must any rebuild
    of an existing route — `edit.rs`'s postcondition chains it so a block the writer cannot
    reproduce **fails** the postcondition instead of vanishing (`MB.3.21`).
19. **`Route::with_php` (:425, was :422 before the item above grew two lines)** — same claim,
    same second caller at `edit.rs:944`; same
    correction. (`with_app`, `with_preload` and `with_route_lines` still say "parsing is the
    only caller" and **are** only called from `parse.rs`; left as they are.)

### `crates/blastit/tests/critical_learn_executor_blocks.rs` (protected, authorized)

20. **Fixture and its doc** — old: "one WSGI route with `[route.static] root = "public"` and
    **no `[route.db]`**, so the request's `SELECT 1` is ungranted", and the final assertion
    `queries == ["SELECT 1"]`. Now the route also carries `[route.db] queries = ['SELECT 42']`
    and the assertion is `["SELECT 42", "SELECT 1"]`, so the case covers the **extend-array
    splice** in `patch_grant`/`append_to_array` (one-line list, `was_empty == false` →
    `, ` separator), not just the create-a-table path. Strengthened, not weakened, in three
    places: the learned statement must sit **beside** the seed grant, exactly one `[route.db]`
    table may exist, and the parsed list must be both entries in order.
    The `[route.static]`/`[route.db]` multiplicity assertions now count **table header lines**
    (`table_headers`) instead of substring matches: the new comment legitimately names
    `[route.db]`, and a comment must not be able to satisfy or break a header count.

## Claims re-read and confirmed accurate (left as they are)

29 in `scheduler.rs`, each checked against the code it describes (30 in attempt 1; the
`charge_ns` doc below it moved to the changed list after attempt 2's review):

- queue/writer rules (:16-21), `request_timeout` from dispatch and `queued_ms` (:35-36),
  credit-overflow retire reason (:43-46), `DRAIN` → `503`/`queue_full` (:53-55), LRU cap only
  retires a `READY`, idle, unqueued view (:64-67);
- fair-queue ordering and "every removal goes through `Admission::remove`" (:84-95) — the
  fair walk's `pop` removes the same `entries`, and `dispatch` still calls `remove`;
- the clamp, its two bounds and "no report → observed" (:95-105, :3722-3736), the bailout
  exception and its `duration_ms` span (:105-113, :3739-3751), the app's-own-`500` rule
  (:113-117), the event's emission points (:120-125, :3831-3834);
- `View.exited` / `view_exited` / `view_gone` / `expire_exited_views`: frames are read before
  the close, and only a missing close is bounded (:283-291, :3384-3394, :3424);
- `Admission` FIFO/fair and `remove`'s idempotence (:323-333, :405-406);
- the static slot: a parked read owns the release, never a timeout or a cancellation
  (:578, :2666-2670);
- `head_at`/`head_sent`/`view_answered`/`bailed` field docs (:614-637), `view_push`'s
  full-queue recycle (:1161-1163), `abort_static` → `view_died` (:2648-2650);
- `charge`'s clamp rationale and idempotence (:3722-3762), `view_done`'s `MB.3.41` paragraph
  (:3927-3930), `view_end`'s "charged at `RESP_END` unless the view answered a `5xx`"
  (:4066-4071), `view_abort`'s "not a bailout" (:4136-4139), `retry_after_seconds`
  (:4209-4219);
- `answered_a_bailout`'s other two bullets (:4252-4259), `a_done_may_relabel_a_completed_5xx`
  (:4281-4289), `Charge`'s field docs (:4293-4311), and the three `charge`/relabel test docs
  (:4616-4623, :4644-4650, :4726-4732).

## Also changed (same defect class, in the other two files)

- `plan/nodes/MB/MB.3/MB.3.21/RESULT.md:38-41` — deleted the literal stale acceptance command
  the LOG forbids repeating (the LOG says "never the stale one"); it now cites
  `REVIEW FAIL (attempt 1)` (`2026-11-15T06:43`, MAJOR 1) as the correction of record. Its
  bullet about this fixture also gained one clause, because item 20 makes "no `[route.db]`"
  read as current when it is historical.
- `docs/knowledge/LESSONS.md` — the requested line: a postcondition rewrite built with
  `Route::new` must chain every defaulted field, or the comparison silently answers a
  different question than the writer asked.

## Acceptance evidence

```console
$ cargo test -p blastit-scheduler --locked
test result: ok. … (17 suites, 242 passed; 0 failed)

$ cargo test -p blastit --features python --locked --test critical_learn_executor_blocks
test dev_learn_writes_the_query_and_keeps_the_static_block_byte_for_byte ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s

$ cargo test -p blastit-manifest --locked
test result: ok. … (251 passed; 0 failed) + 5 doc-tests passed

$ make check
… default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
make check exit=0   # incl. RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
docs-check exit=0

$ cargo fmt --all -- --check
fmt exit=0

$ uv run --no-project tools/critical_guard.py --base deb80c8 --node MB.3.50
node MB.3.50: paths [...] 
  M  crates/blastit/tests/critical_learn_executor_blocks.rs  [authorized by crates/blastit/tests/critical_learn_executor_blocks.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.50

$ make critical-skip-check
protected critical tests hold no skip construct
skip-check exit=0
```

Attempt 2 re-ran every command above after the `charge_ns`/`release`-fallback edits: identical
results (`cargo fmt` 0, scheduler 242/0, manifest 251/0, the protected `--learn` 1/0,
`make check` exit 0, docs-check "No issues found", guard OK, skip-check clean).

## For the planner (do not edit `NODE.md` here)

`testing` §2 requires a node's `# Goal` to say why a protected critical file is edited, and
today that reason lives only in the LOG. The sentence `NODE.md`'s `# Goal` should carry:

> The protected `crates/blastit/tests/critical_learn_executor_blocks.rs` is edited because its
> fixture is the node's evidence: it is the only case where a real `blastit dev --learn`
> process writes a grant, and it currently seeds no `[route.db]`, so it covers only the
> create-a-table splice; adding `[route.db] queries = ['SELECT 42']` beside `SELECT 1` makes
> the same run prove the extend-array splice too, and no assertion is weakened (the parsed
> list must now contain both entries, in order).

## Follow-ups

- **Judged not a defect by the attempt-1 reviewer (kept, not changed):** a deferred `5xx`
  whose view's channel closes before its `DONE` is emitted with the outcome the request still
  carries (`Outcome::Ok`, status `500`) — `view_gone` → `fail_request` early-returns on a
  complete request, `release` charges, and `bailed` would assert a `DONE` fact that never
  arrived while `view_died` would be a lie.
- The removed literal was the only acceptance target in `MB.3.21/RESULT.md`; `NODE.md` for
  `MB.3.21` still names it. Not editable here (not in this node's paths).
