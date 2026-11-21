# MB.3.45 — the random `php-sapi` release-blocker is gone, and a scheduler's
# own answer is charged and labelled honestly

## Summary

All seven binding items landed (`LOG.md` 2026-11-15T05:20 added item 7 and its
order; (4)+(5) came from `MB.3.46`). **No `src/**` outside the scheduler, no
`crates/blastit/tests/**`, no `supervise.rs`, `Makefile` or workflow file was
touched.**

1. **Item 7 — the flake (`scheduler.rs:3387` `view_exited`, `:3415`
   `expire_exited_views`).** `Ctl::Exited` no longer fails a view's in-flight
   requests: it marks the view `exited` (nothing more is dispatched to it) and
   the failure is left to `ViewIn::Closed`, which `read_frames` sends **after**
   every frame it decoded. A designed retire (`RESP_HEAD`/`RESP_END_COST`/
   `DONE{Bailed}` then exit) is therefore read first and stays `bailed` with its
   cost; a process that really died mid-request still closes its channel and is
   still `view_died`. The wait is bounded by `request_timeout`
   (`next_deadline` + `expire_exited_views`) so a lost descriptor cannot hang a
   request, and a view with no channel to wait for (already retired, still
   spawning) is handled exactly as before. **Measured in the real-pair image
   under 8 host spinners: 1 fail in 20 with the fix reverted (the `MB.3.48`
   rate, head-sent `view_died`/500 variant), 0 in 40 with it.**
2. **Item 5 — `view_answered` (`:622`, set only in `view_head`).** A `5xx` the
   scheduler wrote no longer counts as "a view answered this":
   `answered_a_bailout` (`:4229`), `completed_outcome` (`:4200`) and the `DONE`
   relabel guard (`a_done_may_relabel_a_completed_5xx`, unit-tested at `:4686`)
   all read the new bit. A `504` the scheduler answered whose view later sends
   `DONE{Bailed}` stays `timeout` and keeps the dispatch→completion charge. One
   honest limit is recorded under Follow-ups 1: after item 4 the `request` event
   is already out when the late `DONE` arrives, so the *false* branch of the bit
   has no end-to-end oracle; it is pinned at the predicate and by the leg's
   property assertions.
3. **Item 4 — charge before emit.** Every path that emits a `request` event now
   charges first (`expire_requests:2851`, `reject:3584`, `finish_immediately:
   3630`, `fail_request:3671`, plus `client_cancel:2003`, `refuse_static:2717`,
   `refuse_body:3610`, `view_abort:4126`, which had the same shape). A
   `debug_assert!(request.charged)` in `emit_request` (`:3812`) makes the
   ordering an invariant, not a convention — it is what reddens first if anyone
   emits before charging again. A scheduler-written `504` now reports its real
   `cost_ns`.
4. **Item 3 — the release claim.** **Comment narrowed**, not code: releasing a
   completed request in `client_cancel` would free a worker slot whose thread is
   still alive, which §2.4 counts until `DONE`. `view_end`'s doc now names the
   three paths that reach `release` and says a live view that loses its `DONE`
   holds the request *and its event* until one of them. `RESULT` says what a
   test would catch (Follow-up 3).
5. **Items 1 + 6 — the public page** (`docs/reference/log-events.md:39-46`):
   `outcome` is the eight-name set with the `bailed` gloss, and `cost_ns` says
   what a scheduler-answered request is charged (`0` only when it never reached
   a worker; dispatch→completion otherwise, never the queue wait) — in agreement
   with item 4's code, item 5's bit and `ARCHITECTURE.md:1540`.
6. **Item 2 — `php.rs:1902`** asserts `ViewError::Protocol` **by type and
   message**.

`critical_charge.rs` (protected) gained three legs and lost nothing: `MB.3.46`'s
three are byte-identical in intent, and `critical_guard --node MB.3.45` passes.

## Evidence

### Item 7 — the two directions, pinned (`tests/critical_charge.rs:570`)

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
running 6 tests
test a_scheduler_written_504_is_charged_before_its_request_event ... ok
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... ok
test a_scheduler_answered_504_stays_out_of_the_bailout_bill ... ok
test a_view_that_answers_then_retires_is_billed_for_its_answer_not_a_death ... ok
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... ok
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

The leg sends `Ctl::Exited` on `ctl.chan` **before** the view writes anything,
asserts nothing was failed, then lets the view write `RESP_HEAD(500)`,
`RESP_END_COST` and `DONE{Bailed}` and close: the response is complete, the
event is `bailed`, `cpu_ns == Some(1 ms)`, `cost_ns >= 18 ms` and `>=
duration_ms`. The second half of the same test holds with `answer: None`: the
channel closes with nothing written, the client gets the canned `502`, and the
event is `view_died` with a non-zero charge.

**Red — the old `Ctl::Exited` (`view_exited` → `view_gone`):**

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge a_view_that_answers_then_retires
thread 'a_view_that_answers_then_retires_is_billed_for_its_answer_not_a_death' panicked at crates/blastit-scheduler/tests/critical_charge.rs:593:5:
a view init only reported gone is not failed before the frames it wrote are read
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out
```

### Item 7 — the flake, found the way it was found (`real pair`, image, 8 spinners)

`flake-before` = working tree with only the item-7 fix reverted, embed suite in
`blastit-php-sapi-e2e:local`, 8 busy host spinners:

```console
$ /tmp/opencode/mb345/flake-loop.sh /tmp/opencode/mb345/flake-before 20 "BEFORE (item-7 fix reverted)"
FAIL run 19 exit 101
thread 'a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time' panicked at crates/blastit/tests/critical_php_embed.rs:2315:5:
assertion `left == right` failed: usleep(): a 500 contained bailout is a handler that never returned:
{"…","cost_ns":63256569,"cpu_ns":null,"…,"outcome":"view_died","path":"/crash","queued_ms":26.587634,…,"status":500,…}
test result: FAILED. 18 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.01s
BEFORE (item-7 fix reverted): fails=1 of 20 embed suite runs (real pair, image, 8 spinners)
```

1 in 20 — the rate `MB.3.48` recorded — and the gate's own head-sent variant
(500 + `view_died`). After the fix, same binary rebuilt, 40 runs:

```console
$ /tmp/opencode/mb345/flake-loop.sh /tmp/opencode/mb345/flake-after 40 "AFTER (MB.3.45)"
AFTER (MB.3.45): fails=0 of 40 embed suite runs (real pair, image, 8 spinners)
```

(An earlier 40-run loop on the same fixed binary was also 0/40.) The whole-suite
witness on the real pair is `make rs-test-php-image` below.

### Item 4 — a scheduler-written `504`'s event carries its charge (`:457`)

The leg queues the request for 5 ms behind a held slot, lets it time out, and
asserts the event's `cost_ns > 0`, `>= request_timeout`, `<= duration_ms × 1e6`
and `< duration_ms × 1e6` (the queue wait is not billed), with `cpu_ns: null`.

**Red A — restore emit-before-charge in `expire_requests`:** the new invariant
fires first:

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge a_scheduler_written_504 -- --nocapture
thread '…' panicked at crates/blastit-scheduler/src/scheduler.rs:3811:9:
a request event is emitted only after the request was charged
```

**Red B — same, with the `debug_assert!` also removed (the exact shape
`MB.3.46` probed):**

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge a_scheduler_written_504
thread 'a_scheduler_written_504_is_charged_before_its_request_event' panicked at crates/blastit-scheduler/tests/critical_charge.rs:469:5:
a scheduler-written 504 held a worker, so its event cannot say 0ns: 0
```

### Item 5 — a `504` stays out of the bailout bill (`:509`)

Same queue-wait shape, view scripted to `DONE{Bailed}` after the timeout: the
event is `timeout`, `cost_ns > 0`, and `cost_ns < duration_ms × 1e6` (the 5 ms
queue wait is not billed); after the `DONE` arrives the same event is unchanged
and the slot is freed (a third request is dispatched). The predicate table
(`scheduler::tests::a_done_cannot_label_a_5xx_the_scheduler_answered`) pins the
bit's two directions.

**Red (and the honest limit).** Feeding the bit from `head_sent` (i.e.
`respond_with` also setting `view_answered`) leaves all six legs green:

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge   # with the conflation restored
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

That is not a gap in the leg: once item 4 charges at the scheduler's answer, the
`request` event is emitted *before* the late `DONE` can relabel anything, so the
wall bill the bit prevents no longer reaches the event. The pre-item-4 tree is
where the reviewer found it, and there the leg's first cost assertion is what
fails (Red B above: `cost_ns: 0`). Follow-up 1 records the residue.

### Items 1 + 6 — the public page

```console
$ grep -n '`outcome` is one of' -A3 docs/reference/log-events.md
39:`outcome` is one of `ok`, `unmatched`, `queue_full`, `queue_timeout`, `timeout`,
40-`view_died`, `bailed`, `client_gone`. `bailed` — the handler exited, died or
41-fataled out of a `5xx` it had answered; the view process is still alive.
42-`cost_ns` is what the request was charged for fair queuing; `cpu_ns` is `null`
$ make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.30s
```

**Red:** before the edit the same grep printed nothing (the set closed at seven);
the page now says `0` only for a request that never reached a worker, matching
`:2851`'s charge, which is what the leg above asserts, and matching
`ARCHITECTURE.md:1540`'s "a `5xx` the scheduler wrote itself … is charged
dispatch to completion".

### Item 2 — the protocol error by type and message

```console
$ cargo test -p blastit-view --locked --features php --lib a_response_body_before_the_head_is_a_protocol_error
test php::tests::a_response_body_before_the_head_is_a_protocol_error ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 74 filtered out
```

**Red:** the expected string spelled `"RESP_HEAD before RESP_BODY"`:

```console
thread 'php::tests::a_response_body_before_the_head_is_a_protocol_error' panicked at crates/blastit-view/src/php.rs:1904:9:
a body before the head is the transport's protocol error: Err(Protocol("RESP_BODY before RESP_HEAD"))
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 74 filtered out
```

### Acceptance commands

```console
$ cargo test -p blastit-view --locked --features php
test result: ok. 75 passed … 10 passed … 4 passed … 12 passed … 7 passed … 1 passed … 0 passed (1 ignored) … 30 passed … 1 passed
$ cargo test -p blastit-scheduler --locked
17 "test result: ok" lines, 0 failed
$ cargo test -p blastit --locked
194 passed (1 ignored) … 19 passed … 5 passed … 19 passed … 31 passed … 0 failed
$ make docs-check          → No issues found (exit 0)
$ make rs-test-php-image
usleep(): status 500  outcome=bailed  cpu_ns=142046  wall_ns=76384838  cost_ns=76384838  retired pid 125  respawned pid 136 (ppid 94) uid 1000  live=[136]
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
exit=0
$ uv run --no-project tools/plan.py validate
plan OK
```

### `make check` (the final run, after the last doc edit)

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

(`/tmp/opencode/mb345/make-check.final2.log`; clippy with `--features python`
over `--workspace --all-targets` is part of it and was also run standalone:
`Finished … -D warnings`, exit 0.)

### Protected-critical gates

```console
$ uv run --no-project tools/critical_guard.py --base 88874a4 --node MB.3.45
node MB.3.45: paths ['crates/blastit-view/src/php.rs', 'crates/blastit-scheduler/src/scheduler.rs',
  'crates/blastit-scheduler/tests/critical_charge.rs', 'crates/blastit-scheduler/tests/common/mod.rs',
  'docs/reference/log-events.md']
  M  crates/blastit-scheduler/tests/critical_charge.rs  [authorized by crates/blastit-scheduler/tests/critical_charge.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.45
$ make critical-skip-check
protected critical tests hold no skip construct
```

`MB.3.46`'s three legs are untouched (the file only grew, 442 → 677 lines).
`cargo fmt --all -- --check` is clean throughout.

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/src/scheduler.rs` | +226/−29: `View::exited` + `view_exited`/`expire_exited_views` (item 7); `Request::view_answered` + `a_done_may_relabel_a_completed_5xx` (item 5); charge-before-emit on every event path + `debug_assert!` (item 4); `view_end` doc narrowed (item 3); a unit table for the relabel predicate |
| `crates/blastit-scheduler/tests/common/mod.rs` | +74/−3: `Script::LateDone`, `Script::SilentThenExit`, `Log::handles`, `World::stop_view`, and `ctl_exit` now closes the fake process's channel too (a dead process does); no existing arm or helper changed |
| `crates/blastit-scheduler/tests/critical_charge.rs` | +297/−2: `queued_second_request` + three legs (items 7, 4, 5) and the module doc; `MB.3.46`'s three legs unchanged |
| `crates/blastit-view/src/php.rs` | 4 lines: the assertion is `matches!(… Err(ViewError::Protocol(msg)) if msg == "RESP_BODY before RESP_HEAD")` |
| `docs/reference/log-events.md` | +7/−4: the eight-name `outcome` set with the `bailed` gloss, and the `cost_ns` rule for a scheduler-answered request |

## Follow-ups (outside this node's `paths`, or deliberately not taken)

1. **Item 5's false branch has no end-to-end oracle after item 4** (see the red
   above). What *would* catch it in a tier that can see the debit: a leg that
   reads the fair-queue charge (the harness has no such view — `MB.3.46`'s
   Follow-up 1) or a scheduler that re-charged at release. The invariant is
   documented in `answered_a_bailout`'s doc, enforced by the predicate table,
   and the end-to-end property (a scheduler-answered `504` never pays the queue
   wait, even when the view bails afterwards) is pinned by the new leg.
2. **`docs/knowledge/LESSONS.md`** deserves two lines this node could not add
   (not in `paths`): (a) "a harness that simulates a reaped process must close
   the fake process's channel too; otherwise an ordered-close fix looks like a
   hang" — `World::ctl_exit` now does; (b) "`tokio::time::advance(d)` only fires
   timers due at or before `now + d`, and a deadline armed a millisecond later
   than the advance misses it — advance past it, and assert bounds not
   equalities".
3. **Item 3's residue, and what a test would catch.** With the comment narrowed,
   a *live* view that loses one `DONE` for a completed `5xx` still holds the
   request and its event until `view_gone` or a `stuck_threads` retire, because
   `client_cancel:1955` returns early and `expire_requests` only marks it
   overdue. A test would: complete a `5xx`, drop the `DONE`, keep the view alive
   and the request not `timed_out`, cancel the client, and assert no `request`
   event appears — which today it would not. Releasing there is what would fix
   it, and it is risky (the worker slot would go back while its thread is still
   alive, which §2.4 counts until `DONE`), so it needs its own decision.
4. **`expire_exited_views` is a backstop only.** An *in-flight* request always
   hits its own `request_timeout` first (its deadline precedes the exit), so the
   bound is reached only by a request that is already complete but unreleased. A
   leg for it would need a completed `5xx` whose channel never closes.
5. **`docs/reference/log-events.md`'s `process.retired` `reason` list** is
   `idle`, `max_requests`, `stuck_threads`, `memory_pressure`, while the code
   also emits `lru_cap` and `Other(credit_overflow|unexpected_spawn)`
   (`RetireReason::as_str`). Pre-existing, unrelated to this node's items; worth
   one sentence when that page is next touched.
