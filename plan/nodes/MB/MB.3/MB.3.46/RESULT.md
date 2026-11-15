# RESULT MB.3.46 — the `bailed` label and its `R19` charge, witnessed without `libphp`

## Summary

`MB.3.41`'s `bailed` label and its wall charge had no unprivileged witness: the
scheduler harness could only ever answer `200`, so deleting `request.bailed =
bailed`, deleting `view_end`'s `deferred` half, or replacing a `DONE`'s outcome
with `Completed` left `make check` green and only reddened the `libphp` Docker
job (audit `MB.3.13` closure, gap **N1**).

Delivered, in `crates/blastit-scheduler/tests/**` only:

- **The harness can now say `5xx` and `bailed`.** `Script::AnswerStatus {
  status, body, cost, outcome, gate }` answers any status, reports any cost, and
  sends a `DONE` carrying a chosen `DoneOutcome`; with a `gate`, the response
  end and then the `DONE` each wait for a permit, so a test places both on the
  paused clock. `Script::RawDone { status, code }` puts an **outcome byte the
  typed encoder refuses** (`4`) on the wire, which is the only way to drive a
  hostile view. `Http::await_head` is the new client-side wait for the *head*
  (see Follow-up 4 for why the existing `Log::received` cannot be used for it).
  `answer()`, `answer_with_cost()` and every existing `Script` arm are
  untouched: all 16 existing suite files stay green unmodified.
- **A new protected suite**, `tests/critical_charge.rs` (3 tests, 4 arms, 382
  lines, 0.01 s), on a real `AF_UNIX` socketpair view and
  `tokio::time::pause()`, so the three spans a request has — the view's report,
  the scheduler's observed dispatch→`DONE`, and the event's arrival→last-byte
  `duration_ms` — are exact, distinct numbers:
  1. `a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp`
     — `500` + a 3.2 ms report + `DONE { Bailed }` at 18 ms is `bailed`,
     `cost_ns >= duration_ms × 1e6`, `>= 18 ms`, `> 3.2 ms`, `cpu_ns ==
     Some(1 ms)`. Twin, byte-for-byte the same frames with `DONE { Completed }`:
     `ok`, status `500`, the report clamped (`>= 3.2 ms`, `< 18 ms`).
  2. `a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done` — the same
     `5xx` reported at `RESP_END` at 18 ms, `DONE` at 28 ms: the event does not
     exist before the `DONE`, then the charge is `>= 18 ms` and `< 28 ms`, i.e.
     the wall the client waited, never the `DONE`'s own delay.
  3. `an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it`
     — outcome byte `4` after a `500` head: the client gets `RESP_ABORT
     (view_died)`, the event is `view_died`/`500` **once**, the channel closes,
     and the next request is served by a *new* process (a `_ => Completed`
     fallback would have freed the slot and labelled the request `ok`). Twin:
     the same frames with byte `0` complete the response, the request is `ok`,
     and the *same* process takes the next request.

All three mutations the node names redden a named leg (evidence below), plus the
decoder fail-open that leg 3 exists for. **One node leg was NOT delivered** —
"a scheduler-written `5xx` never pays the queue wait" — because that charge never
reaches the `request` event at all; see Follow-up 1. Two production findings came
out of writing it (Follow-ups 1 and 2).

No `src/**`, no `Cargo.toml`, no `docs/`, no existing `critical_*` file was
touched. No `#[ignore]`, no skip, no weakened assertion.

## Evidence

### The new suite

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
running 3 tests
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... ok
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... ok
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

(5 consecutive runs: 3 passed each, 0.00–0.01 s — the paused clock leaves nothing
to jitter.)

### The whole crate, including the four `critical_*.rs` files that share the harness

```console
$ cargo test -p blastit-scheduler --locked
… 17 "test result: ok" lines, 0 failed …
```

### `make check`

```console
$ make check
…
protected critical tests hold no skip construct
cargo test -p blastit-scheduler  --test critical_charge
running 3 tests
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... ok
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... ok
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
…
clippy      -p blastit{,-view} (standalone)
…
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0   (real 1m57s)
```

### Clippy

```console
$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-view v0.2.0 (…)
    Checking blastit-py v0.2.0 (…)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.06s
```

### Plan + protection gates

```console
$ uv run --no-project tools/plan.py validate
plan OK

$ uv run --no-project tools/critical_guard.py --base da1eb77 --node MB.3.46
critical-guard: da1eb77..working tree, 1 changed file(s)
node MB.3.46: paths ['crates/blastit-scheduler/tests/**']
  no change to a protected critical test

$ make critical-skip-check
protected critical tests hold no skip construct
```

(`critical_charge.rs` is untracked, so the guard sees only the harness edit; a
**new** `critical_*.rs` is always allowed, and `make rs-test-critical` picks it
up by glob — log line 3024 of the `make check` run above.)

### The three mutations, one red run each, restored afterwards

Each was applied to a copy of the working tree, run, then restored; the
`git diff --stat` after the last one is exactly the two files of this node
(`common/mod.rs` +115/−2, and the untracked new suite).

**1. Drop `request.bailed = bailed;` in `view_done`'s completed-`5xx` arm**
(`crates/blastit-scheduler/src/scheduler.rs`, the `else if` at 3819–3827):

```console
$ uv run --no-project /tmp/opencode/mutate.py crates/blastit-scheduler/src/scheduler.rs …
mutated …: 'if !request.cancelled && head_sent && status >= 500 {' -> '…'
 crates/blastit-scheduler/src/scheduler.rs | 1 -
$ cargo test -p blastit-scheduler --locked --test critical_charge
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... FAILED
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... FAILED
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... ok

thread '…pays_its_wall_where_the_same_frames_returned_pay_the_clamp' panicked at crates/blastit-scheduler/tests/critical_charge.rs:150:5:
3200000ns charged for a request the event says lasted 18000000ns
thread '…a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done' panicked at crates/blastit-scheduler/tests/critical_charge.rs:245:5:
the span to the last byte is paid: 3200000ns
test result: FAILED. 1 passed; 2 failed; 0 ignored
--- restored; scheduler.rs diff: 0 lines ---
```

`MB.3.38`'s exact defect, in the fast tier: 3.2 ms charged for an 18 ms request.

**2. Drop the `deferred` half of `view_end`** (`scheduler.rs` 3918–3929,
`deferred = request.status >= 500;` and its `if !deferred`):

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... FAILED
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... FAILED
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... ok

thread '…pays_its_wall_where_the_same_frames_returned_pay_the_clamp' panicked at crates/blastit-scheduler/tests/critical_charge.rs:142:5:
assertion `left == right` failed: a `DONE` that says the handler bailed is `bailed`, not `ok`
  left: Ok
 right: Bailed
thread '…a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done' panicked at crates/blastit-scheduler/tests/critical_charge.rs:233:5:
a completed `5xx` waits for its `DONE` before it is labelled or billed
test result: FAILED. 1 passed; 2 failed; 0 ignored
--- restored; scheduler.rs diff: 0 lines ---
```

**3a. The producer writes `Completed` where the outcome belongs** — the shape of
`crates/blastit-view/src/php.rs`'s `serve_forever` writing `DoneOutcome::Completed`
instead of `end.outcome`. That producer is not linkable here (it needs a real NTS
`libphp`), so the mutant was applied to the harness's `AnswerStatus` arm, which is
the same frame on the same wire:

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... FAILED
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... FAILED
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... ok

thread '…pays_its_wall_where_the_same_frames_returned_pay_the_clamp' panicked at crates/blastit-scheduler/tests/critical_charge.rs:142:5:
assertion `left == right` failed: a `DONE` that says the handler bailed is `bailed`, not `ok`
  left: Ok
 right: Bailed
thread '…a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done' panicked at crates/blastit-scheduler/tests/critical_charge.rs:243:5:
assertion `left == right` failed
  left: Ok
 right: Bailed
test result: FAILED. 1 passed; 2 failed; 0 ignored
--- restored; common/mod.rs diff: 115 insertions(+), 2 deletions(-) ---
```

This is the half of mutation 3 the scheduler tier *can* see, and it is why leg 1
carries its twin: with the label never reaching the wire, the two arms become the
same request, and only the pair catches it. The `php.rs` producer itself stays
witnessed by `crates/blastit/tests/critical_php_embed.rs` on the `libphp` job.

**3b. The decoder reads an out-of-set outcome as `Completed`** — the `_ =>
Completed` fallback leg 3 exists for (`crates/blastit-frame/src/frame.rs:300–311`):

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
test an_out_of_set_done_outcome_takes_the_view_down_where_a_defined_one_completes_it ... FAILED
test a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done ... ok
test a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp ... ok

thread '…an_out_of_set_done_outcome_takes_the_view_down…' panicked at crates/blastit-scheduler/tests/critical_charge.rs:287:5:
a view that broke the protocol never completed this response: [RespHead(RespHead { status: 500, headers: ["Content-Length"], .. }), RespEnd]
test result: FAILED. 2 passed; 1 failed; 0 ignored
--- restored ---
 M crates/blastit-scheduler/tests/common/mod.rs
?? crates/blastit-scheduler/tests/critical_charge.rs
```

### After restoring: green again, `git diff --stat` unchanged

```console
$ cargo test -p blastit-scheduler --locked --test critical_charge
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

$ git status --short
 M crates/blastit-scheduler/tests/common/mod.rs
?? crates/blastit-scheduler/tests/critical_charge.rs
$ git diff --stat
 crates/blastit-scheduler/tests/common/mod.rs | 117 +++++++++++++++++++++++++-
 1 file changed, 115 insertions(+), 2 deletions(-)
```

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/tests/critical_charge.rs` | **new** (382 lines): the three protected legs of this node, 4 arms, all with an accepted twin |
| `crates/blastit-scheduler/tests/common/mod.rs` | +115/−2: `Script::AnswerStatus`, `Script::RawDone`, `Http::await_head`, `send_raw_done`, `wait_gate`; no existing arm or helper changed |

## Follow-ups

1. **The node's leg 2 could not be written (blocked, needs `src/**`).** "A
   scheduler-written `5xx` never pays the queue wait" is not observable at this
   tier: every scheduler-written answer (`respond` → `reject` /
   `finish_immediately` / `fail_request` / `expire_requests`) calls
   `emit_request` **before** `release` charges it, and `emit_request` reads
   `charged_ns`, so the event says `cost_ns: 0`. Probed directly on a `504`:

   ```console
   PROBE 504: outcome=Timeout status=504 cost=0 duration=21 queued=0 cpu=None
   {"…","status":504,"outcome":"timeout","queued_ms":0.0,"duration_ms":21.0,…,"cost_ns":0,"cpu_ns":null,…}
   ```

   The charge does reach `queue.debit`, but the harness exposes no view of the
   fair queue, so a leg would have to be a two-world ordering differential with
   millisecond margins — a flaky protected test, not an oracle. Owner of the
   seam: `MB.3.45` (`src/scheduler.rs`). The two things that would make it
   writable: charge before the emit where the span is already final, and/or hand
   the leg the flow's debit.
2. **Production finding (`MB.3.45`'s `src/scheduler.rs`, read, not observed).**
   `respond()` sets `request.head_sent = true` for a head the *scheduler* wrote,
   so `answered_a_bailout`'s documented rule — "`head_sent` is set only when a
   `RESP_HEAD` came from the view" (`scheduler.rs` 4069–4073) — is false for the
   scheduler's own `502`/`503`/`504`. A request timed out at `504` whose view
   later sends `DONE { Bailed }` therefore reaches `view_done`'s completed-`5xx`
   arm with `head_sent == true`, gets `request.bailed = true`, and
   `answered_a_bailout(true, 504, true)` bills it its **arrival→finished** wall,
   queue wait included, instead of dispatch→completion. (Its `outcome` is
   relabelled `bailed` too, but by then the event was already emitted as
   `timeout`.) A `5xx` the scheduler wrote needs its own "a view answered this"
   bit, distinct from "a head went out".
3. **Doc consequence of the same finding** (`docs/reference/log-events.md`, not
   in this node's paths — `MB.3.45` item 1 is already rewriting that page):
   `cost_ns` is `0` for every request the scheduler answered itself, including a
   `504` that held a worker for 20 ms. Worth one sentence next to the bailout
   rule.
4. **Harness note for the next author.** `Log::received` is written by the fake
   view *before* it answers, so it cannot be used to place the request's floor:
   a test that waits on it and then advances the clock moves the view's first
   frame with it (that mistake cost one debugging round here — the clamp then
   charged the full 18 ms). Wait on `Http::await_head` instead, which is the
   head actually on the wire.
