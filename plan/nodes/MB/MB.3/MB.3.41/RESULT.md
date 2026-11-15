# Result MB.3.41

## Summary

A completed `5xx` now says *why* it failed, and `view_died` is back to meaning
only "the view process died mid-request".

**The rule (one rule, R19 as written — not a second rule).** `MB.3.38` inferred
intent from the status code and mapped *every* completed `5xx` to `view_died`.
The missing fact is not on `RESP_HEAD`: a `500` an app answers itself and a
`500` a dying handler leaves behind are byte-identical there. It *is* known to
the SAPI shim (`contained`), so it travels on the completion path that already
encodes how a request ended: **`DONE`'s `outcome` gains one value,
`DoneOutcome::Bailed` (wire code `3`)**, set by `blastit-view`'s PHP runtime when
the shim reports a contained `zend_bailout`. No frame changed size; `DONE`'s
payload is still one `u8`, and `RESP_END_COST`'s pinned 9-or-17-byte rule is
untouched.

Three shapes, one rule each (`crates/blastit-scheduler/src/scheduler.rs`):

| Shape | `outcome` | `cost_ns` |
|---|---|---|
| an app **answers** a `5xx` itself (Django's `500`, `http_response_code(500)`, a Node/JVM error) | `ok`, with that `status` — the crate's own gloss for `Ok` is "the view answered" | the documented clamp, exactly like a `200` |
| a **contained bailout** answers a `5xx` and never returns (`exit()`, `die()`, a fatal) | `bailed` (new; `DoneOutcome::Bailed`) | the request's own wall time, arrival→last byte (`R19`'s crash ceiling at one live thread); the view's report is not believed |
| a `5xx` the **scheduler** wrote itself (`502`/`504`/`503`) | its own value (`view_died`/`timeout`/`queue_full`) | dispatch→completion — **no queue-wait bill** |

So: an app's own `500` keeps a completed-app outcome (`ok` + `status: 500`,
level `warn`), and a bailout is distinguishable from it — and from a dead
process — in the `request` event. `view_died` was not widened; it was narrowed
back.

**Why `DONE` and not `RESP_END_COST`.** The completion frame was the brief's
strong preference and it is the right seam: `DONE` is the frame whose only job
is "how the worker thread finished", its payload has room for one more enum
value without changing a byte count, and the 9-or-17-byte `RESP_END_COST` table
(plus its C-layout twin in `blastit-php-sapi`, a protected file outside these
paths) stays frozen. Because `DONE` arrives *after* `RESP_END`, a `5xx` a view
answered is **labelled and charged at its `DONE`** (one frame later, same
channel) instead of at `RESP_END`; `release` still charges and emits if that
`DONE` never comes, so a lost frame costs the request its label, never its
event. Every other response keeps charging and emitting at `RESP_END`, as
before.

**Why R19 needed no amendment.** R19's text is "crashes/`504` charge
`ceiling.max(kernel_floor_ns)`". A contained bailout *is* the crash, and a
`504` is the `504`; MB.3.38's mistake was calling an app's own `500` a crash.
The rule needed to be applied as written, not rewritten, so
`docs/design/backend-runtimes.md` is untouched (it is also outside this node's
paths — see Follow-ups for an optional one-clause clarification).

**Flake owned and fixed (`crates/blastit-view/src/php.rs`, CI red).**
`a_response_body_before_the_head_is_a_protocol_error` wrote `REQ_END` to a peer
that could already be gone (the view refuses the head-less body and retires the
moment `REQ_HEAD` arrives) → sporadic `EPIPE` at the *test peer's* `write_all`,
line 1138. Fixed by **ordering, not tolerance**: the test now joins the worker
first (its `ViewError::Protocol` is asserted **by name** —
`RESP_BODY before RESP_HEAD`), then writes `REQ_END` and asserts the write
**fails** — the closed channel is the evidence the view ended. That one call
site uses a new `Peer::send_to_a_gone_peer`, and one helper
(`peer_is_gone`) names the whole tolerated set (`BrokenPipe`, `ConnectionReset`,
`UnexpectedEof` — an AF_UNIX peer that exits with queued data reports the last
two as readily as the first, `LESSONS.md`). `Peer::send`'s panic is unchanged
everywhere else, because a closed peer there is a real failure. This path writes
**no** `DONE`: `serve_forever` returns the recorded protocol error before it can
write `RESP_END_COST`/`DONE`, so the deterministic half asserted is the returned
error, not an invented `DoneOutcome`.

**Scope check (README).** `README.md` is **not** in this diff, and was never
edited here: `git diff d1a016b --name-only` lists only the 9 source/doc files
plus the plan's own `JOURNAL.md`/`LOG.md`/`NODE.md`, and `git diff -- README.md`
is empty. Nothing to revert.

## Evidence

### Protected guard (authorization for the one protected edit)

```console
$ uv run --no-project tools/critical_guard.py --base d1a016b --node MB.3.41
critical-guard: d1a016b..working tree, 12 changed file(s)
node MB.3.41: paths ['crates/blastit-scheduler/src/**', 'crates/blastit-view/src/**',
  'crates/blastit-php-sapi/src/**', 'crates/blastit-frame/src/**', 'crates/blastit-events/src/**',
  'ARCHITECTURE.md', 'crates/blastit/tests/critical_php_embed.rs']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.41

$ make critical-skip-check
protected critical tests hold no skip construct
```

`crates/blastit-php-sapi/tests/critical_shim_contract.rs` and
`crates/blastit-scheduler/tests/critical_*.rs` were **not** touched and needed no
change (the `contained` flag already exists; the shim contract pins the ABI, not
the `DoneOutcome` vocabulary).

### Red before green (each new assertion)

Frame — `DoneOutcome::Bailed` (code 3) and its `every_frame()` round trip:

```console
$ cargo test -p blastit-frame --locked --lib
error[E0599]: no variant, associated function, or constant named `Bailed` found for enum
             `frame::DoneOutcome` in the current scope
   --> crates/blastit-frame/src/frame.rs:251:1
```

Events — `Outcome::Bailed` (`"bailed"`), the byte-pinned line and the decoder:

```console
$ cargo test -p blastit-events --locked --lib
error[E0599]: no variant, associated function, or constant named `Bailed` found for enum `Outcome`
   --> crates/blastit-events/src/json.rs:1421:17
```

Scheduler — `completed_outcome(head_sent, status, bailed)` /
`answered_a_bailout(..)` and the 502/504 charge test:

```console
$ cargo test -p blastit-scheduler --locked --lib
error[E0432]: unresolved import `super::answered_a_bailout`
error[E0061]: this function takes 1 argument but 3 arguments were supplied (×6)
```

View — a contained bailout is `DoneOutcome::Bailed`, a script's own `500` is
`Completed`:

```console
$ cargo test -p blastit-view --locked --features php --lib
error[E0425]: cannot find function `ended` in this scope (×4)
error[E0599]: no variant ... named `Bailed` found for enum `blastit_frame::DoneOutcome`
```

Flake fix — both new assertions are load-bearing (each flipped once, then
restored):

```console
# expected substring changed to a wrong one:
thread 'php::tests::a_response_body_before_the_head_is_a_protocol_error' panicked at
crates/blastit-view/src/php.rs:1902:9:
a body before the head is the transport's protocol error: Err(Protocol("RESP_BODY before RESP_HEAD"))

# `gone.is_err()` flipped to `gone.is_ok()`:
thread '...a_response_body_before_the_head_is_a_protocol_error' panicked at
crates/blastit-view/src/php.rs:1907:9:
the view ended, so its end of the channel is closed: Err(Os { code: 32, kind: BrokenPipe,
message: "Broken pipe" })
```

### The flake, 50× and both thread counts

```console
$ for i in $(seq 1 50); do cargo test -q -p blastit-view --locked --features php --lib \
    a_response_body_before_the_head_is_a_protocol_error 2>&1 | grep -q "1 passed" \
    || { echo "RED at run $i"; exit 1; }; done; echo "50/50 green"
50/50 green

$ cargo test -p blastit-view --locked --features php
test result: ok. 75 passed; 0 failed; 0 ignored; ... (lib, php feature)
test result: ok. 10 passed; ... / 4 / 12 / 7 / 1 / 0 (1 ignored) / 30 / 1

$ cargo test -p blastit-view --locked --features php -- --test-threads=1
test result: ok. 75 passed; 0 failed; 0 ignored; ...
test result: ok. 10 passed; ... / 4 / 12 / 7 / 1 / 0 (1 ignored) / 30 / 1
```

(`cargo test -p blastit-view --locked` with **default** features does not build
`php.rs` at all — the module is behind `feature = "php"` — so the php-feature
run above is the one that exercises the flake; `make check`'s workspace run
covers it through `blastit`'s default `php` feature.)

### Acceptance commands

```console
$ cargo test -p blastit-scheduler --locked
exit=0, 16 × "test result: ok", 0 failed
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.49s; merged doctests compilation took 0.48s

$ cargo test -p blastit --locked
exit=0, 13 × "test result: ok", 0 failed

$ cargo test -p blastit --locked --test critical_php_embed
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.05s

$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-py v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.88s

$ make check
make-check exit=0, 123 × "test result: ok", 0 failed
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

### The wire + shim witness (real NTS `libphp` + real shim, in the CI image)

```console
$ make rs-test-php-image
exit=0
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
usleep(): status 500  outcome=bailed  cpu_ns=109815   wall_ns=75769096  cost_ns=75769097  retired pid 130 ...
fatal:    status 500  outcome=bailed  cpu_ns=2692539  wall_ns=18553876  cost_ns=18553876  retired pid 169 ...
exit():   status 500  outcome=bailed  cpu_ns=1086289  wall_ns=19002104  cost_ns=19002104  retired pid 274 ...
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.52s
```

Each bailout leg now reports `outcome=bailed` with `cost_ns >= wall_ns`
(`critical_php_embed.rs` asserts `outcome == "bailed"` and keeps
`cost_ns >= wall_ns`), and the worker is respawned — the end-to-end proof that
`contained` reached the `request` event. On a PHP-free host those legs take the
refusal branch, so 19 pass locally for the other reason; this image is the CI
witness (5 + 19 green).

## Files

- `crates/blastit-frame/src/frame.rs` — `DoneOutcome::Bailed` (code `3`),
  `from_code` accepts it, the `DONE`/outcome paragraphs in the module doc.
- `crates/blastit-frame/src/lib.rs` — `every_frame()` + `from_code(3)`/`code()`
  (tests).
- `crates/blastit-events/src/lib.rs` — `Outcome::Bailed` (`"bailed"`), `Ok`'s
  gloss ("a `5xx` an app answers itself is still an answer"), `ViewDied`'s
  gloss, `RequestInfo::cost_ns`'s three charge rules.
- `crates/blastit-events/src/json.rs` — `parse_outcome("bailed")`, the
  byte-pinned `request` line, and a `bailed` decode round trip.
- `crates/blastit-view/src/php.rs` — `ended(&ResultV1, head_sent)` (the one
  place `contained` becomes a `DoneOutcome`), `bailed_out` import, unit test;
  plus the flake fix (`Peer::send_to_a_gone_peer` + `peer_is_gone` + the
  ordered test).
- `crates/blastit-view/src/runtime.rs` — `RequestEnd`'s doc names `Bailed`.
- `crates/blastit-scheduler/src/scheduler.rs` — `Request::bailed`, deferred
  label+charge for a completed `5xx` (`view_end`/`view_done`),
  `completed_outcome(head_sent, status, bailed)`,
  `answered_a_bailout(head_sent, status, bailed)`, module/`charge`/`Charge`
  docs, and the three tests (`a_completed_response_is_a_bailed_5xx_or_an_answer`,
  `only_a_bailed_5xx_a_view_answered_is_a_crash`,
  `a_bailed_500_pays_its_wall_and_a_scheduler_502_or_504_does_not`).
- `crates/blastit/tests/critical_php_embed.rs` (authorized) — the containment
  leg asserts `outcome == "bailed"` instead of `!= "ok"`, plus its docs.
- `ARCHITECTURE.md` — "What a request costs" (three rules), "A lying view is
  bounded on both sides" (the bailout exception), §3.1 framing (`DONE`'s
  vocabulary, why a `5xx` is finalized at `DONE`), §8.1's `outcome` list and the
  `cost_ns` paragraph. No paragraph claims the old "every `5xx` is a crash"
  clamp.

## Follow-ups

- **`docs/reference/log-events.md:39-40` is now stale** (it lists `ok`,
  `unmatched`, `queue_full`, `queue_timeout`, `timeout`, `view_died`,
  `client_gone`). It is a public user page and outside this node's paths, so it
  needs one word added by the doc owner: `bailed` — "the handler bailed out of a
  `5xx` it had answered (a contained `exit()`/`die()`/fatal); the view process
  is still alive".
- **`docs/design/backend-runtimes.md` R19** needs no amendment (see Summary);
  if the doc owner wants it literally spelled out, the one-clause addition is:
  "a `5xx` a view *answered* is not a crash; its report is clamped like any
  other".
- **Coverage gap in the scheduler harness**: `crates/blastit-scheduler/tests/common/mod.rs`'s
  `answer()`/`answer_with_cost()` only ever send `200`, so no scheduler
  integration test can produce a *view-answered* `5xx`. The deferred
  label-and-charge path is therefore pinned only by
  `src/scheduler.rs`'s unit tests plus `critical_php_embed.rs` on a real pair.
  A `Script::AnswerStatus(u16)` + a `DONE(Bailed)` twin in `tests/common` would
  close it — that file is outside this node's paths.
- **One shape, three flakes**: `supervise`'s `EPIPE`, `Toolchain::probe`'s, and
  this one are the same "write to an AF_UNIX peer that may already be gone".
  No `LESSONS.md` line was added: `docs/knowledge/LESSONS.md:12` already records
  it ("classify `EPIPE`/`ECONNRESET` as a lost peer on **both** the read and the
  write side"), and `LESSONS.md` is outside this node's paths.
- `ARCHITECTURE.md` §8.1's example `request` line is unchanged (`ok`/`200`); a
  second example line for `bailed` would document the new vocabulary on the
  page that has the sample, if the docs pass wants one.
