# MB.26.8 — a queued request is never handed to a PHP view that retired

## Summary

**Root cause.** A `libphp` worker that contains an `exit()`/`die()`/fatal answers the request
(`RESP_HEAD`/`RESP_END_COST`/`DONE { Bailed }`) and then **retires by design** (`php.rs`'s
`should_recycle`). The scheduler learns that from the ordered `view.chan` close (or init's
`Ctl::Exited`) — both *after* `DONE`, and `pump` runs between them. `DONE` releases the worker's slot
(`View::inflight` back to 0), and the very next `dispatch_queued` found the same worker `ready`,
`inflight == 0`, not `stopping`, not `exited`, channel still open — so it dispatched the queued head
into a process that had already answered and was going away. When the socket then closed,
`view_gone` failed that request `502`/`view_died`. That is the measured `MB.21.7` shape, and it had
nothing to do with the queue order or the pool size: it is one `DONE`-to-close window per delivered
answer.

**Fix** (`crates/blastit-scheduler/src/scheduler.rs`, 115 added / 14 removed lines, 27 of the added
lines being comment text from the polish pass):

- `View::retiring: Option<Instant>` — the worker answered **its own** request with
  `DONE { Bailed }` and its channel has not closed yet. `View::admits` refuses it, so
  `has_dispatchable`/`reserve` cannot pick it; `dispatch_queued` then grows a replacement (charged
  and accounted by the existing `grow`/`SPAWN` path — the new test asserts at least one
  `process.spawned` per request) and the queued request is served, never answered `502`.
- `View::leaving_at()` folds `exited` and `retiring` into the one instant `next_deadline` and
  `expire_exited_views` bound, so a worker that says "bailed" and then never closes its channel is
  removed `request_timeout` later instead of pinning the route's process slot.
- `evictable_view`/`oldest_idle_view` no longer name a worker that is on its way out: it frees
  itself, and killing it would report an `lru_cap`/`memory_pressure` retirement for a process that
  had already answered its last request.
- The mark is **narrowed to a request the worker concluded itself** (`!timed_out && !cancelled`,
  and — structurally — to a stream the scheduler has not released; see follow-up 1 for the
  `Errored`/`Aborted` shapes that this leaves to `expire_exited_views`).
  A `DONE { Bailed }` for a request the scheduler already answered is `LateDone`'s shape
  (`MB.3.45`), whose slot was freed by that answer; `critical_charge`'s protected
  `a_scheduler_answered_504_stays_out_of_the_bailout_bill` pins that the *same* worker takes the
  next request there, so the mark must not fire for it. No protected leg was edited.

**Which shape reproduces, which only guards.**

- **Reproduces (`critical_php_embed.rs`, real `libphp` 8.4.27 NTS + shim):** the front controller
  answers and then calls `exit(0)` — the design retire of every Symfony/Laravel-shaped
  `public/index.php`. With `BLASTIT_MAX_WORKERS=1` (one process, one slot) and the default
  `BLASTIT_VIEW_MAX_REQUESTS=0`, **every** answer retires its worker, so 8 concurrent requests are
  seven retirements wide. RED at the parent source; GREEN on this tree.
- **Guards only:** the same batch with `BLASTIT_VIEW_MAX_REQUESTS=1` and a page that does not exit
  is **green at the parent too** (`{200: 160}`, measured before any `src/**` change): `reserve` sets
  `stopping` at dispatch time for `max_requests`, so the scheduler knows in advance and never
  dispatches into it. It is not the bug and is not presented as one.
- **Deterministic scheduler leg (`crates/blastit-scheduler/tests/pool.rs`, no `libphp`):**
  `a_bailed_worker_is_never_given_the_request_that_queued_behind_it` drives the same interleaving
  with a scripted `view.chan` (a worker that answers `200` + `DONE { Bailed }` and keeps its
  channel open), reads the *view's own* request log to prove the queued request was not dispatched,
  then closes the worker and proves a replacement served it `200` with no `view_died` anywhere.
  RED at the parent, GREEN here.

Measured before → after, verbatim:

```text
parent source, assertion paused, 20 batches × 8:  DIAG statuses {200: 80, 502: 80}
parent source, first failing batch:                batch 0: request 1 -> "HTTP/1.1 502 Bad Gateway"
this tree:                                         queued behind retired workers: 20 batches × 8 =
                                                   160 requests, statuses {200: 160}, 160 workers
                                                   spawned, every outcome ok
```

## Evidence

### The RED proof (parent source: `git stash push -- crates/blastit-scheduler/src/scheduler.rs`)

The only `src/**` this node changes is `scheduler.rs`; `a4b930e` and `68a6400` differ only in plan
bookkeeping, so the stash is exactly the parent behaviour with the new tests in place.

Both tails below are re-captured against the **delivered** test files (the leg is
`a_queued_request_is_never_handed_to_a_retiring_view`; an earlier revision of this section quoted the
pre-rename spelling and its line numbers — see `## Polish pass`).

```console
$ BLASTIT_PHP_SHIM_DIR=target/php-sapi cargo test -p blastit --features python \
    --test critical_php_embed a_queued_request_is_never_handed_to_a_retiring_view -- --nocapture
running 1 test

thread 'a_queued_request_is_never_handed_to_a_retiring_view' panicked at
crates/blastit/tests/critical_php_embed.rs:2873:5:
assertion `left == right` failed: every one of the 160 queued requests must be a 200 with its own
body; the responses said {200: 80, 502: 80}
batch 0: request 1 was handed to the worker it queued behind instead of being served by a
replacement (status 502): {"bytes_in":0,"bytes_out":12,"component":"scheduler","cost_ns":36536,
"cpu_ns":null,"duration_ms":71.667612,"event":"request","level":"warn","method":"GET",
"outcome":"view_died","path":"/q/1","queued_ms":71.631759,"remote_addr":"127.0.0.1",
"request_id":"0000000b00000004","route":"app","rule":0,"status":502,
"ts":"2026-11-17T09:04:31.038Z"}
HTTP/1.1 502 Bad Gateway
Content-Type: text/plain; charset=utf-8
Content-Length: 12
Connection: close

bad gateway

  left: Some(80)
 right: Some(160)
test a_queued_request_is_never_handed_to_a_retiring_view ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 26 filtered out; finished in 3.12s
```

```console
$ cargo test -p blastit-scheduler --locked --test pool \
    a_bailed_worker_is_never_given_the_request_that_queued_behind_it -- --nocapture
thread 'a_bailed_worker_is_never_given_the_request_that_queued_behind_it' panicked at
crates/blastit-scheduler/tests/pool.rs:404:5:
assertion `left == right` failed: the request queued behind the retirement was dispatched before
the worker closed: [1, 2]
  left: 2
 right: 1
test a_bailed_worker_is_never_given_the_request_that_queued_behind_it ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 19 filtered out; finished in 0.00s
```

### GREEN on this tree

```console
$ BLASTIT_PHP_SHIM_DIR=target/php-sapi cargo test -p blastit --features python \
    --test critical_php_embed a_queued_request_is_never_handed_to_a_retiring_view -- --nocapture
running 1 test
queued behind retiring workers: 20 batches × 8 = 160 requests, statuses {200: 160}, 160 workers spawned, every outcome ok
test a_queued_request_is_never_handed_to_a_retiring_view ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out; finished in 8.74s
```

`make rs-test-php` (the three protected PHP suites against the host's real pair), re-run on this tree:

```console
$ make rs-test-php
BLASTIT_PHP_SHIM_DIR=target/php-sapi cargo test -p blastit --locked --test critical_php_backend \
  --test critical_php_embed --test critical_parallel_views -- --nocapture
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.69s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s
queued behind retiring workers: 20 batches × 8 = 160 requests, statuses {200: 160}, 160 workers spawned, every outcome ok
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.87s
RS_TEST_PHP_EXIT=0
```

`make rs-test-php-image` — the image was **not** present in this checkout, so it was built once with
`make php-sapi-e2e PHP_SAPI_KEEP=1` (exit `0`: `libblastit_php84-x86_64-gnu.so` + `libphp8.4.so`,
four probe shapes `ok`), and then the target CI runs:

```console
$ make php-sapi-e2e PHP_SAPI_KEEP=1     # once, builds blastit-php-sapi-e2e:local
  ok  php 8.4 abi 1: status 200, cpu_ns 10922016 <= wall_ns 13448415, exit()+fatal contained, next request served
PHP_SAPI_E2E_EXIT=0

$ make rs-test-php-image
test result: ok. 9 passed; 0 failed; ...                 # critical_php_backend (image root)
queued behind retired workers: 20 batches × 8 = 160 requests, statuses {200: 160}, 160 workers spawned, every outcome ok
test result: ok. 27 passed; 0 failed; ...                # critical_php_embed (image root)
init pid 9: no libphp/shim; above the view 0 map(s) content-checked, 2 denied (EACCES), 0 gone, of 3 children; euid 1000
test result: ok. 1 passed; 0 failed; ...                 # the unprivileged PR_SET_DUMPABLE(0) re-run
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
RS_TEST_PHP_IMAGE_EXIT=0
```

That image transcript predates the wording change in the leg's summary line (`retired` → `retiring`
workers); the counts are the image's own. `make rs-test-php-image` was **not** re-run for the
comment-only polish pass, per the reviewer's green run on the same tree.

The acceptance's own command, **verbatim** (the suite resolves the shim from the cargo target's
`php-sapi/` via the test binary's ancestors — `shim_path`'s third candidate), `9 + 27` legs, the new
leg among them:

```console
$ cargo test -p blastit --features python --test critical_php_embed --test critical_php_backend
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.55s
```

The same pair with `BLASTIT_PHP_SHIM_DIR` set (`make rs-test-php`'s spelling) is equally green:

```console
$ BLASTIT_PHP_SHIM_DIR=target/php-sapi cargo test -p blastit --features python \
    --test critical_php_embed --test critical_php_backend
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s
test a_queued_request_is_never_handed_to_a_retiring_view ... ok
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.04s
```

The whole scheduler crate, including the protected `critical_charge` legs that the `LateDone` narrowing
had to leave untouched (`20` legs in `pool`, `7` in `critical_charge`):

```console
$ cargo test -p blastit-scheduler --locked
test result: ok. 49 passed; ... 11 passed; ... 10 passed; ... 7 passed; ... 16 passed; ...
             32 passed; ... 14 passed; ... 6 passed; ... 18 passed; ... 9 passed; ...
             17 passed; ... 6 passed; ... 2 passed; ... 13 passed; ... 18 passed; ...
             14 passed; ... 4 passed; ... 20 passed; ... 14 passed; ... 1 passed; ...
0 failed everywhere
```

### Lints and formatting

```console
$ cargo clippy -p blastit-scheduler -p blastit --features python --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.90s

$ cargo fmt --all -- --check && echo FMT-CLEAN
FMT-CLEAN
```

### `make check`

```console
$ make check ; echo MAKE_CHECK_EXIT=$?
...
FAIL  docs/reference/cli.md  fence 1  dev-flags             2.2s  blastit dev exited before ready; stdout:

stderr:
blastit dev: unknown argument `--no-reload`
...
48 fences: 30 executed, 18 accounted, 1 failed · 28.7s
make: *** [Makefile:343: docs-test] Error 1
MAKE_CHECK_EXIT=2
```

That is the one **known pre-existing** failure (`--no-reload` is `MB.23.5`'s and is not implemented on
this branch — no fence touched by this node, no file of this node in `docs/`). Every target before
`docs-test` passed (make stops at the first failing prerequisite); the targets *after* it were run
explicitly — and `make check` was re-run once more after the last (test-only) edit with the same
single failure and `169` `test result: ok` lines and no `test result: FAILED`:

```console
$ make rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test \
       doc-tone manifest-purity zed-check plan-validate
...
zed-check: OK (42 properties checked)
Ran 19 tests in 8.180s
OK
uv run --no-project tools/plan.py validate
plan OK
TAIL_EXIT=0
```

### Protected-test guard

```console
$ uv run --no-project tools/critical_guard.py --base a4b930e --node MB.26.8
critical-guard: a4b930e..working tree, 13 changed file(s)
node MB.26.8: paths ['crates/blastit-scheduler/**', 'crates/blastit-view/**', 'crates/blastit-php-sapi/**', 'crates/blastit/tests/critical_php_embed.rs']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.26.8
```

`critical_php_embed.rs` is `+233 / −0`: the existing 26 legs are byte-identical (its diff is one
module-doc paragraph and the new leg and its three helpers). No `#[ignore]`, `skip`, `xfail`, no
assertion loosened, and `critical_charge.rs`/`critical_parallel_views.rs` are untouched.

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/src/scheduler.rs` | `+115 / −14`: `View::retiring`, `View::leaving_at`, the `DONE { Bailed }` mark in `view_done`, the `retiring`/`leaving_at` reads in `admits`/`next_deadline`/`expire_exited_views`, and the two eviction predicates; module doc and the `view_done` narrowing comment (27 of the added lines, from the comment-only polish pass). |
| `crates/blastit-scheduler/tests/pool.rs` | `+107 / −1`: the deterministic scripted-AF_UNIX-view leg, its `dispatched` helper, the `SETTLE` constant and the `DoneOutcome` import. |
| `crates/blastit/tests/critical_php_embed.rs` | `+233 / −0`: `a_queued_request_is_never_handed_to_a_retiring_view` (8 × 20 at one process), `QUEUED_PHP`/`QUEUED_CONCURRENCY`/`QUEUED_BATCHES`/`QUEUED_MARKER`, and the module-doc paragraph. |

No `Cargo.toml`/`Cargo.lock` byte changed (`git diff --numstat` lists only the three files above);
no dependency, no knob, no flag, no manifest key. No `unwrap`/`expect`/`panic`/indexing was added to
`scheduler.rs` (the two `expect`s in the test are the file's existing test-harness idiom, already
under its `#![allow(clippy::unwrap_used, clippy::expect_used)]`).

## Follow-ups

1. **`Errored` and client-cancelled retires still have the window; the real fix is an explicit
   `RETIRING` frame.** The PHP worker retires after answering in three shapes, not one:
   `DONE { Bailed }` (marked here), `DONE { Errored }` (a shim/protocol error, `php.rs`'s error arm),
   and a **bailing handler whose client disconnected** — `php.rs` sets `worker.bailed |= retire` from
   the bail, then forces `DONE { Aborted }` because `io.cancelled`, so `recycle` still retires the
   process. The mark cannot cover either of the last two: it only fires while the scheduler still
   holds the stream *and* has not concluded it (`view_done` returns early once `release` dropped the
   dispatch entry, and its `!timed_out && !cancelled` guard covers a cancel/timeout of a dispatched
   request), so a `DONE` that arrives after a timeout or a cancel can never mark — by construction,
   not by luck. Either worker is bounded only by `expire_exited_views` one `request_timeout` later,
   and a request that queues in that gap is still dispatched into it and still answered
   `502`/`view_died`. `Errored` additionally cannot be inferred safely: it does not imply retirement
   for the WSGI view, so the scheduler has no per-runtime datum to key on. Both shapes need the
   transport to say "I am retiring" explicitly on the wire (a `DONE` bit or a `RETIRING` frame)
   instead of the scheduler inferring it from an outcome it may never see. That is a protocol change
   and belongs to its own node.
2. **Worth a `LESSONS.md` line** (not appended: `docs/` is outside this node's `paths`): a request
   is dispatched in the pump *between* a view's last `DONE` and its ordered `view.chan` close, so any
   scheduler decision that reuses a worker must be based on what the worker said, not on
   `inflight == 0`; and `DONE { Bailed }` means "own request concluded", never "scheduler already
   answered this id" — `critical_charge`'s `LateDone` leg pins the difference.
3. **The `php-sapi` image was missing and is now built** (`blastit-php-sapi-e2e:local`): this
   checkout had only the old `blastit-php-sapi-e2e:mb358-php82` tag, so `make rs-test-php-image`
   refused until `make php-sapi-e2e PHP_SAPI_KEEP=1` built it (exit `0`; the image also carries the
   compiled suites, as `MB.3.58` designs). Anyone re-running this node on a fresh checkout must
   build it first; a later node should not have to.
4. **An unexplained uncommitted block was found in the working tree and removed.** At 09:29 the tree
   carried a `// MUTATION (MB.26.8 evidence)` block inside `Scheduler::retire` that failed the
   route's whole admission queue on any retirement. It was not part of this attempt (it appeared
   after this session's first edits), and it turned three pre-existing `pool.rs` legs red
   (`a_queued_request_survives_the_worker_it_queued_behind`,
   `a_spawn_sent_before_a_reload_is_retired_on_arrival`,
   `a_colliding_spawned_retires_the_held_pid`). It was removed before the evidence above; the final
   `scheduler.rs` diff contains no such block. If another session wrote it, that session must not
   share this worktree.
5. **`critical_parallel_views` is still outside the image leg** (`MB.24.4`'s filed follow-up): the
   image compiles only `critical_php_backend`/`critical_php_embed`, so `make rs-test-php` is the only
   place the new leg and the parallel-burst promise are proven together.

## Polish pass (reviewer PASS, four MINOR findings)

Comment-and-documentation only: no `retiring` semantics, frame, field, test or assertion changed, and
the behaviour the reviewer passed is byte-for-byte the same. `git diff --numstat` on `crates/` is
`115/14` (`scheduler.rs`, 27 of its added lines being comment text from this pass), `107/1`
(`pool.rs`, untouched by this pass) and `233/0` (`critical_php_embed.rs`, untouched by this pass).

1. **Stale RED tails (fixed).** Both RED proofs were re-run against the **delivered** test files with
   the parent source (`git stash push -- crates/blastit-scheduler/src/scheduler.rs` = `68a6400` +
   these tests), and the transcripts in `## Evidence` are now those runs:
   - `crates/blastit-scheduler/tests/pool.rs:404:5` — `left: 2`, `right: 1`, request ids `[1, 2]`;
   - `crates/blastit/tests/critical_php_embed.rs:2873:5` — `the responses said {200: 80, 502: 80}`,
     `left: Some(80)`, `right: Some(160)`, with the `view_died` `request` event and the
     `HTTP/1.1 502 Bad Gateway` body.

   The earlier revision of this file quoted a **pre-rename** leg (`a_queued_request_is_never_handed_to_a_retired_view`,
   `pool.rs:397:5`, `critical_php_embed.rs:2855:9`). The delivered leg is
   **`a_queued_request_is_never_handed_to_a_retiring_view`**: running the old spelling now filters
   `0` tests out of `27` (`running 0 tests ... 27 filtered out`), which is how the drift surfaced, and
   every command line in this file now names the leg that exists. The `make rs-test-php` and
   `critical_php_embed + critical_php_backend` transcripts were re-run for the same reason (the leg's
   own summary string now says `queued behind retiring workers`); the image transcript is annotated
   because it was not re-run, per the reviewer's green run on this tree.
2. **`Aborted`/cancelled retires named (fixed).** `Scheduler::view_done`'s comment now states the
   narrowing structurally rather than as a preference: the mark fires only while the scheduler still
   holds the stream *and* has not concluded it — `release` drops the `(key, id)` dispatch entry, so a
   repeat `DONE` returns early as dropped, while a cancel or a timeout of a dispatched request keeps
   the entry (the view keeps its slot to its `DONE`) and is stopped by the `!timed_out && !cancelled`
   guard. It says plainly that a client disconnect during a bailing handler leaves an unmarked
   retiring worker (`php.rs`'s `worker.bailed |= retire` + the forced `DoneOutcome::Aborted` for
   `io.cancelled`). Follow-up 1 now names `Errored` **and** `Aborted`/cancelled-by-client, and names
   the explicit `RETIRING` wire frame (a `DONE` bit) as the real fix.
3. **Module doc scoped (fixed).** The bullet promises only the `DONE { Bailed }` retirement and states
   that `Errored`/`Aborted` retires are covered only by the bounded `expire_exited_views` path, one
   `request_timeout` later, not at the next dispatch.
4. **Host-saturation flake** in `a_probe_that_hangs_is_killed_and_refuses_the_boot`: not touched, as
   instructed.

### Verification of this pass

`make check`, `make e2e` and `make rs-test-php-image` were **not** re-run (the reviewer's green run on
the same tree stands; no code line changed). Every command below was run at the end of this pass:

```console
$ cargo fmt --all -- --check
FMT_EXIT=0

$ cargo clippy -p blastit-scheduler -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.73s
CLIPPY_EXIT=0

$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-scheduler --no-deps --locked
   Generated .../target/doc/blastit_scheduler/index.html
DOC_EXIT=0
```

```console
$ cargo test -p blastit-scheduler --locked --test pool \
    a_bailed_worker_is_never_given_the_request_that_queued_behind_it
running 1 test
test a_bailed_worker_is_never_given_the_request_that_queued_behind_it ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 19 filtered out; finished in 0.00s
```

```console
$ cargo test -p blastit --features python --test critical_php_embed \
    a_queued_request_is_never_handed_to_a_retiring_view -- --nocapture
running 1 test
queued behind retiring workers: 20 batches × 8 = 160 requests, statuses {200: 160}, 160 workers spawned, every outcome ok
test a_queued_request_is_never_handed_to_a_retiring_view ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out; finished in 7.14s
```

The one deviation from the dispatch brief's spelling is the leg's name: the brief's
`a_queued_request_is_never_handed_to_a_retired_view` filters `0` tests on this tree because the
delivered leg is `..._a_retiring_view` (same leg, same assertions, and the same run as the GREEN tail
above). Renaming the leg was not worth touching a test file in a comment-only pass.
