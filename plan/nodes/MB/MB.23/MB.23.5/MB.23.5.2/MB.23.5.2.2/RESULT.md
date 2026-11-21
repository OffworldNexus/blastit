# MB.23.5.2.2 — the reload cycle: validate, stop the scheduler, re-exec, adopt

Base commit `83c6bb9`, branch `release/0.2.0`, worktree
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`. Design
`docs/design/dev-fanotify-reload.md` §4.1, §4.5, §4.6 (steps 0–7), §4.8, §8 5b-ii;
`ADR-0014`.

## What landed

- **`Fire::cycle` / `Fire::needs_cycle()`** (`dev_reload.rs`). The action is decided
  from the union entry, never from the reason word: `pre_fork || class == Blastfile ||
  overflow` means the heap every view was forked from is stale, so the cycle runs. The
  reason word stays `blastfile > preload > loaded` (`overflow` on a drop). A unit test
  pins both halves (`a_pre_fork_change_needs_the_cycle_and_a_view_change_does_not`).
- **Validate-then-`execve`** (`supervise.rs::begin_cycle`). Re-reads the manifest and
  re-parses it; for every changed `.py` path it compiles the source
  (`PyCode::compile(…, File)`, never executes it). The first refusal is **one**
  `manifest.warning` carrying the file's own `<path>: <line>:<col>: …` sentence — no
  `dev.reload`, no `Ctl::Reload`, no exec, previous topology serving. A valid save emits
  one `dev.reload` with the **new** manifest's routes, sends one `Ctl::Reload`, and sets
  the cycle state. `--learn` skips the manifest cycle entirely (one `manifest.warning`
  naming the restart) while `loaded` saves still reload views.
- **The gate, with a real witness.** `reloading` makes the scheduler's `EXITED`
  expected (`component_exit` logs at `info`, no `shutdown` stage), makes a closed
  `ctl.chan` non-fatal (`ctl_closed`), and makes a `ctl.chan` write failure
  (`Broken pipe`) non-fatal (`ctl_failure_is_fatal(shutting_down, reloading, err)` — this
  was the last flake: a view's `EXITED` racing the stopped scheduler used to kill the
  old image instead of execing). The flag is cleared in the wakeup that execs and the
  replacement image starts clear. New witness:
  `a_scheduler_exit_after_a_cycle_is_still_fatal` runs the cycle, then `SIGKILL`s the
  **replacement** scheduler and requires a non-zero exit and no surviving view; its doc
  comment states why the older all-views row could not witness the gate (nothing stops
  the scheduler there, and `http-died` is fatal in every version).
- **Drain as a state, never a sleep** (`progress_cycle`). While a cycle is in flight the
  loop keeps polling with a bounded timeout, `SPAWN` is answered with `SPAWN_REFUSED`
  (a fork now would fork the old heap), and only when every view is reaped is the
  scheduler sent `SIGTERM`; once it is reaped, the image execs. A wedged scheduler is
  `SIGKILL`ed at the cycle deadline.
- **The exec** (`supervise.rs::exec_cycle`). `/proc/self/cmdline` (or stage 0's handover,
  see below) up to the first `--stage=1` token, at most 8 tokens, never
  `/proc/self/exe`; then `--stage=1`, `--config-fd=N` with the **exact** bytes this image
  received, `--dev-root`, `--dev-host`, `--dev-reload=on`,
  `--dev-reload-since=<unix nanos>` and `--dev-adopt-fd`. `FD_CLOEXEC` is cleared on
  exactly the two pipe read ends, the policy fd, the `http.chan` scheduler end and the
  descriptors the document names; the signal mask is restored first. An `execve` that
  returns prints and `_exit(1)` (and `kill_all`s whatever was left).
- **The adoption document** (≤ 4096 bytes, one line): `{"version":1,"http":{pid,stdout,
  stderr,events},"chan":…,"dbproxy":{…}?,"learn":{obs,ctl?}?,"policy":…?}`. The
  replacement image refuses a document with an unknown key, a missing key, a wrong
  version or a descriptor it does not hold.
- **Stage-1 adoption** (`stage1.rs`). `Adoption::read` before anything is bound; the
  adopt branch creates **no listener**, forks **no `http`/dbproxy** (their pids and pipe
  read ends come from the document), forks a **new scheduler on the adopted
  `http.chan` end**, re-runs the manifest/php/probe path, imports/preloads/warms/
  immortalizes, creates the private directory of any route the save added, calls the
  existing `Learn::adopt` when the document carries `learn`, runs one
  `Policy::reconcile(reload_since, now)` after the boot report, and repeats neither the
  boot line nor `boot`/`ready`. `init` keeps the scheduler's end of `http.chan` for the
  whole run on a dev boot with reload on, so `http`'s peer never sees end-of-file.
- **Stage 0's handover.** The stage-1 program prefix (`Host::stage1()`: the standalone
  binary, or `python -I -m blastit`) is recorded in the configuration document as
  `restart` — **not** an environment variable, because `blastit-env` strips every
  `BLASTIT_*` name and a variable that reached stage 1 would be inherited by every view.
  The listener is still bound once by stage 0 and owned by `http`; a reload never sees
  it.
- **Internal tokens** (`cli.rs`): `--dev-adopt-fd`, `--dev-policy-fd`,
  `--dev-reload-since` beside `--dev-reload=on|off`, all rejected unless `--stage=1` +
  `--dev-root` by the existing `any_dev` gate, absent from `--help`; `--no-reload`
  remains the only user-facing switch. Unit tests pin each token, including the
  no-`--dev-root` and non-stage-1 refusals.
- **The two debug-only seams** (both inside `debug_assertions`, both declared in
  `scripts/check-no-test-seam.sh` beside `BLASTIT_TEST_SWAP_POINT`):
  `BLASTIT_TEST_RELOAD_PAUSE` (creates `<path>.ready`, waits bounded for `<path>.go`
  immediately before the exec) and `BLASTIT_TEST_RELOAD_FAULT=open` (makes watcher
  construction fail `EPERM`, so a critical test drives the ladder's boot row end to end).
  Both are read by stage 0 — before `BLASTIT_*` is stripped — and carried in the
  configuration document; a release build has neither name nor member (proven against
  `target/release/blastit`). `make seam-check` now runs `--present-all` so *every*
  declared seam must exist in the debug binary, while `--present` stays "at least one"
  for `scripts/test_packaging_gates.py`'s synthetic single-name file (that fixture is
  outside this node's paths and stays green, 49 cases).

## Evidence

All commands ran in the worktree above.

### `cargo test -p blastit --locked --features python --test critical_dev_reload`

```console
$ for i in 1 2 3; do cargo test -p blastit --locked --features python --test critical_dev_reload | tail -2; done
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.03s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.11s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.07s
```

Under load (all 8 CPUs in busy loops), 12 consecutive runs:

```console
$ # 8 busy loops, then the same suite 12 times
12 x test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
        (3.18s .. 5.65s each); 0 failures
```

New rows (all red-capable, no `#[ignore]`, no skip construct):

- `an_invalid_blastfile_keeps_serving_and_the_next_valid_one_reloads_once` (7a/7b/7c):
  the invalid save logs exactly one `manifest.warning` naming `Blastfile` and its
  `6:1` position, no `dev.reload`, the old view pid still answers; the valid save emits
  exactly one `dev.reload` with `["products","cart"]`, the added route answers, the
  keep-alive connection opened **before** the save still gets its response, and
  supervisor + `http` pids are unchanged while the scheduler pid changed.
- `editing_a_preload_module_reloads_the_cycle_once` (7d): a half-written `models.py`
  emits one warning and the old preload keeps serving the old view; the fixed module
  reloads once and the replacement image serves `VERSION = 2`.
- `an_eperm_watcher_is_one_unavailable_and_the_run_keeps_serving` (8): the real boot
  under `BLASTIT_TEST_RELOAD_FAULT=open` emits exactly one
  `dev.reload.unavailable` (`errno: EPERM`), zero `dev.reload`, zero fanotify
  descriptors, and three `200`s afterwards; still exactly one warning at the end.
- `learn_skips_the_manifest_cycle_and_still_reloads_views` (acceptance 6): one
  `manifest.warning` naming `restart blastit dev --learn`, no `dev.reload`, same pid
  afterwards; then a view save is one `loaded` reload and the new body is served.
- `a_scheduler_exit_after_a_cycle_is_still_fatal` (the true gate witness, above).

No test asserts a per-route restart or an unchanged pid for another **route**; the only
unchanged pids named are the supervisor's and `http`'s (acceptance 3).

### clippy (both), fmt

```console
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.06s
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.90s
$ cargo fmt --all -- --check
FMT=0
```

### build + the one-watcher guard

```console
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
```

### `cargo check --release -p blastit --locked`

```console
$ cargo check --release -p blastit --locked
    Checking blastit v0.2.0 (…)
    Finished `release` profile [optimized] target(s) in 1.79s
warning count: 0
$ scripts/check-no-test-seam.sh --present-all target/debug/blastit
ok   target/debug/blastit: 2 test seam(s) present, as a debug build must
$ scripts/check-no-test-seam.sh target/release/blastit
ok   target/release/blastit: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   target/release/blastit: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
```

### `make check`

```console
$ make check
MAKE_CHECK_EXIT=0   # grep -cE 'FAIL|FAILED' over the whole log: 0
test result: ok. 19 passed; 0 failed; 0 ignored …   (critical_dev_reload, build script + Makefile)
test result: ok. 42 passed; 0 failed; 0 ignored …   (critical_lsp)
test result: ok. 7 passed; 0 failed; 0 ignored …    (critical_parallel_views)
test result: ok. 9 passed; 0 failed; 0 ignored …    (critical_php_backend)
test result: ok. 27 passed; 0 failed; 0 ignored …   (critical_php_embed)
test result: ok. 252 passed; 0 failed; 1 ignored …  (unit tier; the `1 ignored` is pre-existing)
Ran 53 tests in 12.348s                            (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 24.4s
ok   target/debug/blastit: 2 test seam(s) present, as a debug build must
ok   target/release/blastit: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   target/release/blastit: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
uv run --no-project tools/plan.py validate
plan OK
```

`make check`'s `critical-guard` step runs **without** `--node` and therefore prints a
report-only line for the protected file
(`M crates/blastit/tests/critical_dev_reload.rs [VIOLATION: not covered by any node path]`);
it is `report only`, exit 0, and the acceptance's `--node` form below is the enforced
one and is green.

### `make docs-test`

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  2.2s  each dev flag boots and stops (…)
48 fences: 30 executed, 18 accounted, 0 failed · 24.2s
DOCS_TEST_EXIT=0
```

The `dev-flags` fence is **executed**, not accounted.

### `critical_guard` and the frozen files

```console
$ uv run --no-project tools/critical_guard.py --base 83c6bb9 --node MB.23.5.2.2
  M  crates/blastit/tests/critical_dev_reload.rs  [authorized by crates/blastit/tests/critical_dev_reload.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.23.5.2.2

$ git diff --stat 83c6bb9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty; DIFF_EMPTY=0)
```

## Files

`git diff --stat 83c6bb9` over the node's paths:

```text
 Makefile                                    |   2 +-
 crates/blastit/src/cli.rs                   |  25 +-
 crates/blastit/src/dev_reload.rs            |  98 ++++-
 crates/blastit/src/stage0.rs                |  60 ++-
 crates/blastit/src/stage1.rs                | 638 ++++++++++++++++++++++++----
 crates/blastit/src/supervise.rs             | 590 +++++++++++++++++++++++--  (incl. the cycle)
 crates/blastit/tests/critical_dev_reload.rs | 512 +++++++++++++++++++++-
 docs/design/dev-fanotify-reload.md          |  33 +-
 scripts/check-no-test-seam.sh               |  90 ++--
```

Plus one line in `docs/knowledge/LESSONS.md` (the dispatch asked for it; outside the
node's `paths`, recorded here). `stage1.rs`/`supervise.rs` counts include re-indentation
from moving the fork block behind the adopt branch. `child.rs`,
`python/tests/runnable_docs/inventory.toml` needed no change (the report fd still rides
`zygote.rs`; the `dev-flags` fence and `rs-test-critical` glob already cover this leaf).

The feature is one atomic change and exceeds the ~10-file/900-line guideline
(~9 files, ~1.8 k inserted lines with re-indentation); a split was not taken because
validation without the exec would leave a boot that claims a reload it cannot perform,
and the exec without adoption would bind a second listener. Nothing outside the node's
paths changed except the LESSONS line.

## Follow-ups

1. **dbproxy policy refresh is `MB.23.5.3`'s and is not implemented here.** The
   adoption document carries `policy` when a `--dev-policy-fd` exists (and
   `--dev-policy-fd` parses and is rejected outside `--dev-root`), but stage 0 creates
   no socketpair yet and no document is written, so no swap is claimed and no warning
   about unapplied grants is invented.
2. **PHP slots still carry no report** (`php_view.rs`, `MB.23.5.3`), so a PHP view never
   triggers a reload; a cycle over a PHP route still rebuilds its bindings in the new
   image because `boot_bindings`/`probe_bindings` re-run there.
3. **The reason word for a `preload` module is `loaded`.** The *action* is decided by
   `pre_fork` (so the cycle runs), but per-file `Class::Preload` classification (the
   manifest's `preload` list vs. any pre-fork import) is still open; design §4.5's
   "preload" word therefore never appears yet.
4. **A save that changes the *listener set*** (a route gaining a `postgres` block, a new
   store) is not detected before the exec. The strategies stay stage 0's and the
   old builtins serve; design §4.8's "keeps the boot listeners and warns" belongs with
   the dbproxy refresh in `MB.23.5.3`.
5. `BLASTIT_TEST_RELOAD_FAULT` takes `=open` only. The design's `read|mark` arm was not
   declared because the mid-run latch is already driven deterministically through
   `FakeWatcher` at the policy level; the boot row is the one that needed a real
   process.
6. Stage 0's restart handover is the configuration-document member `restart`, not an
   environment variable: `blastit-env` strips every `BLASTIT_*` name before stage 1, and
   a token that reached stage 1 would be inherited by every view — the opposite of what
   the design asks for.
7. `docs/local.md`'s live-reload table row for `./Blastfile`/`preload` ("re-reads what
   changed, then reloads the whole topology") is now true and needed no edit; the page
   is outside this node's paths.

## Attempt 2

Fixed the reviewer's 3 MAJOR and 4 MINOR findings with no redesign and no weakened
assertion. Base commit `83c6bb9`, same worktree.

### FIX 1 (MAJOR) — the third seam is declared, so `make seam-check` can catch it

`scripts/check-no-test-seam.sh` now lists `BLASTIT_TEST_RELOAD_PAUSE` beside
`BLASTIT_TEST_SWAP_POINT` and `BLASTIT_TEST_RELOAD_FAULT`, and its comment names where
each is defined/read. `--absent` refuses a shipped image carrying *any* of the three;
`--present-all` (used by `make seam-check`) requires all three; `--present` stays "at
least one" so `scripts/test_packaging_gates.py`'s synthetic one-name fixture keeps
passing (49 cases, unchanged).

Red if it regresses: `make seam-check` prints `3 test seam(s) present`, and this run
refused a synthetic release-shaped file containing only `BLASTIT_TEST_RELOAD_PAUSE` with
exit 1:

```console
$ scripts/check-no-test-seam.sh --present-all target/debug/blastit
ok   …: 3 test seam(s) present, as a debug build must
$ scripts/check-no-test-seam.sh target/release/blastit
ok   …: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   …: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
ok   …: no BLASTIT_TEST_RELOAD_PAUSE (absent, as shipped)
$ printf 'BLASTIT_TEST_RELOAD_PAUSE=\n' >> fake-release && scripts/check-no-test-seam.sh fake-release
error: fake-release contains BLASTIT_TEST_RELOAD_PAUSE (1 occurrence(s)), …
       release_with_pause_refused=1
```

### FIX 2 (MAJOR) — the valid save changes the topology, and the row says so

`an_invalid_blastfile_keeps_serving_and_the_next_valid_one_reloads_once` now writes a
manifest that **reorders** the boot's routes and **adds** `checkout`:

```text
boot:  products, cart
save:  cart, products, checkout
```

and asserts the emitted `routes` is `["cart","products","checkout"]` (the re-parsed
manifest, not `self.routes`), that `/checkout/x` answers `200` on `:8080`, and that the
added route's private directory — index 2, past the two stage 0 created — now exists
(read from the boot's own `--dev-root`). Emitting the boot's route list fails the first
assertion (2 vs 3 elements, wrong order); a save that never re-parses fails the second
and third.

Red if it regresses: `an_invalid_blastfile_keeps_serving_and_the_next_valid_one_reloads_once`.

### FIX 3 (MAJOR) — the adoption contract has table-driven legs and a round trip

- `stage1::tests::the_adoption_document_refuses_every_shape_it_cannot_honour`: unknown
  key (`bogus`), a descriptor not held (`999999`) in `http` and in `dbproxy`, `version:
  2`, missing `http`, missing `chan`, a document one byte past 4096 (checked before
  parsing), plus the positive control that a well-formed document is adopted. One pipe
  per case; each case asserts its own sentence.
- `supervise::tests::the_adoption_document_round_trips_through_the_reader`: builds an
  `Adoption` from real pipe ends, writes it with `adoption_document` (now `Result`),
  checks the 4096-byte bound, and reads it back with `stage1::Adoption::read`, asserting
  pid and every descriptor number.

Red if it regresses: both tests above (the reader drops a check, or the writer emits a
different shape, and each leg fails on its own sentence).

### FIX 4 (MINOR, real bug) — drain expiry abandons the cycle, it does not `kill_all`

`progress_cycle`'s expiry arm now calls `abandon_cycle`: one `manifest.warning` naming
the route(s) whose views would not drain (and `a view` when the table is empty), then
`cycle = None` and `reloading = false`. Nothing is signalled — `http`, dbproxy and the
scheduler keep the serving topology, the scheduler still has the all-views transaction,
and a later save can retry. A request stuck in a view can no longer turn a `Blastfile`
save into a dead dev tree.

Red if it regresses: **CORRECTION (attempt 3).** The claim above was **false**: the only
leg then was `an_abandoned_cycle_names_the_route_and_never_asks_for_a_kill`, which calls
the *formatter* and executes no part of the abandon branch — deleting
`self.reloading = false` or restoring `kill_all()` left the suite green. The real leg is
now `a_cycle_whose_views_never_drain_is_abandoned_without_a_signal` (attempt 3, FIX 1),
which drives `progress_cycle` on an expired `Cycle::Draining` over real child processes:
the formatter leg remains only as a message-shape check.

### FIX 5 (MINOR) — the new `reloading` arm has a witness

`supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort` now also asserts
`!ctl_failure_is_fatal(false, true, &error)` on the `EPIPE` it already produces, so
reverting the `reloading` arm makes that assertion fail.

Red if it regresses: `a_late_ctl_write_to_a_gone_scheduler_is_best_effort`.

### FIX 6 (MINOR) — the gate test states its true scope

`a_scheduler_exit_after_a_cycle_is_still_fatal`'s doc comment now says exactly what it
witnesses: the replacement image's `Supervisor::new` starts with `reloading == false`
(the gate is a field of the old supervisor and dies with its `execve`), so the next
scheduler death takes the base fatal path — non-zero exit, no surviving view. It states
explicitly that it does **not** witness a gate that "survived the replacement" (there is
no such value) and points at the unit legs that pin the in-image clear
(`ctl_failure_is_fatal`'s reloading arm and the abandoned-cycle warning). It also keeps
the honest note about why the earlier all-views row cannot witness this.

### FIX 7 (MINOR) — no silent empty document, no silent truncation

- `adoption_document` returns `Result<Vec<u8>, String>`; `exec_cycle` fails the boot
  (`fail`, which `kill_all`s and exits) instead of execing with an empty document.
- `stage1_program` collects the whole `/proc/self/cmdline` prefix and refuses `> 8`
  instead of `take(8)` silently truncating; `stage1::tests::a_stage1_prefix_past_eight_tokens_is_refused_not_truncated`
  asserts the 9-token refusal and accepts 8.

Red if it regresses: `a_stage1_prefix_past_eight_tokens_is_refused_not_truncated` (the
`> 8` sentence) and the adoption round trip (a `Result` writer that silently returned an
empty document would fail the reader).

### Acceptance (all re-run after the edits)

```console
$ for i in 1 2 3; do cargo test -p blastit --locked --features python --test critical_dev_reload; done
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.14s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.08s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.03s

$ # same suite 5x with all 8 CPUs in busy loops
test result: ok. 19 passed; … 3.72s / 3.51s / 4.38s / 3.38s / 3.11s   (0 failures)

$ cargo test -p blastit --locked --features python --lib
test result: ok. 281 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.45s

$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings   # clean
$ cargo clippy -p blastit --all-targets --locked -- -D warnings                     # clean
$ cargo fmt --all -- --check                                                        # FMT=0

$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)

$ make seam-check
ok   …/target/debug/blastit: 3 test seam(s) present, as a debug build must
ok   …/target/release/blastit: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   …/target/release/blastit: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
ok   …/target/release/blastit: no BLASTIT_TEST_RELOAD_PAUSE (absent, as shipped)
SEAM_EXIT=0

$ cargo check -p blastit --release --locked
    Finished `release` profile [optimized] target(s) in …; warning count: 0

$ make check
MAKE_CHECK_EXIT=0   # grep -cE 'FAIL|FAILED' over the whole log: 0
test result: ok. 19 passed; 0 failed; 0 ignored …   (critical_dev_reload, twice)
test result: ok. 42 / 7 / 9 / 27 passed, 0 failed …  (critical_lsp / parallel_views / php_backend / php_embed)
Ran 53 tests in 10.236s                            (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 22.7s
ok   …/target/debug/blastit: 3 test seam(s) present, as a debug build must
ok   …/target/release/blastit: no BLASTIT_TEST_SWAP_POINT / RELOAD_FAULT / RELOAD_PAUSE (absent, as shipped)
test result: ok. 254 passed; 0 failed; 1 ignored …  (unit tier; the `1 ignored` is pre-existing)
plan OK

$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  2.2s  each dev flag boots and stops (…)
48 fences: 30 executed, 18 accounted, 0 failed · 28.4s
DOCS=0

$ uv run --no-project tools/critical_guard.py --base 83c6bb9 --node MB.23.5.2.2
  M  crates/blastit/tests/critical_dev_reload.rs  [authorized by crates/blastit/tests/critical_dev_reload.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.23.5.2.2

$ git diff --stat 83c6bb9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty; DIFF_EMPTY=0)
```

No failure in this attempt was called a flake: there were none to explain, and the
previous attempts' three "flakes" are recorded above (the `polled_sources` index shift,
the wording enumeration, and this attempt's own two race-caused test defects) as real
causes with real fixes.

### Files (attempt 2)

- `scripts/check-no-test-seam.sh` — the third seam name (FIX 1).
- `crates/blastit/src/supervise.rs` — `abandon_cycle`/`wedged_routes`, the expiry branch
  without `kill_all`, `abandoned_cycle_warning` + unit legs, `adoption_document -> Result`,
  the `reloading` witness, the writer/reader round trip (FIX 3–5, 7).
- `crates/blastit/src/stage1.rs` — `Adoption`/`read` visibility for the round trip, the
  unbounded prefix collection (FIX 7), and the adoption-refusal + prefix unit legs
  (FIX 3).
- `crates/blastit/tests/critical_dev_reload.rs` — the genuinely different valid save and
  its three new assertions (FIX 2), the true-scope doc comment (FIX 6), and `dev_root` /
  `wait_path` helpers.

## Attempt 3

One MAJOR (the abandon path was unwitnessed) and two MINORs (the design's 10 s bound,
and a comment that pointed at the wrong leg). FIX 1/2/3/5/6/7 from attempt 2 were left
untouched. Base commit `83c6bb9`, same worktree.

### FIX 1 (MAJOR) — the abandon branch is executed, and both mutations go red

`supervise::tests::a_cycle_whose_views_never_drain_is_abandoned_without_a_signal`
constructs a real [`Supervisor`] (a `Config::default()`, real `Limits`, one route named
`cart`, a `ctl.chan` socketpair, a real `SignalFd` restored by a `Drop` guard, three real
`sleep` children — one registered as a live view of route 0, one as `http`, one as the
scheduler, with real `ComponentPipes`) and drives the real `progress_cycle` with
`Cycle { stage: Draining, deadline: Deadline::after(Duration::ZERO) }` and
`reloading = true`. No 10 s sleep: the deadline is injected, not the constant.

It asserts, after the call:

- `cycle.is_none()` — the cycle is gone;
- `!reloading` — the gate is cleared, so later views are admitted and a later
  `scheduler-died` is fatal again;
- `components.len() == 2` — `http` and the scheduler are still supervised;
- all three `sleep` children are still alive after 100 ms — the abandoned path sent **no
  signal** (a reinstated `kill_all()` kills them and `try_wait` reports it);
- exactly one warning line, naming `cart` and saying "abandoned".

Both mutations were checked by hand and reverted:

```console
# delete `self.reloading = false;` from `abandon_cycle`
thread '…abandoned_without_a_signal' panicked at supervise.rs:2628:
test result: FAILED. 0 passed; 1 failed

# restore `self.kill_all();` in the drain-expiry arm
thread '…abandoned_without_a_signal' panicked at supervise.rs:2648:
  "the wedged view was signalled by the abandoned cycle"
test result: FAILED. 0 passed; 1 failed
```

`an_abandoned_cycle_names_the_route_and_never_asks_for_a_kill` stays as the message-shape
check only; RESULT attempt 2's FIX 4 "Red if it regresses" line is corrected in place above
to say plainly that the old claim was false.

### FIX 2 (MINOR) — the design states the 10 s bound and the abandon outcome

`docs/design/dev-fanotify-reload.md` §4.6 step 4 now says the `views.is_empty()` wait is
bounded at `CYCLE_GRACE` (10 s) and that an expiry **abandons** the cycle: one
`manifest.warning` naming the route(s), `reloading` cleared, nothing signalled, `http`/
dbproxy/the scheduler keep the serving topology, and the next save retries. §4.10's budget
table gained the matching row (`reload drain … 10 s (`CYCLE_GRACE`) → cycle abandoned …`).

### FIX 3 (MINOR) — the gate comment points at the leg that executes the flag

`a_scheduler_exit_after_a_cycle_is_still_fatal`'s doc comment now names
`a_cycle_whose_views_never_drain_is_abandoned_without_a_signal` (which asserts
`reloading == false` on real state) for the drain-expiry clear, and keeps
`a_late_ctl_write_to_a_gone_scheduler_is_best_effort` for the expected-exit clear; the old
"abandoned-cycle warning" clause is gone.

### Acceptance

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload   # 3x
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.05s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.09s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.51s

$ cargo test -p blastit --locked --features python --lib
test result: ok. 282 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.44s

$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings   # clean
$ cargo clippy -p blastit --all-targets --locked -- -D warnings                     # clean
$ cargo fmt --all -- --check                                                        # FMT=0

$ make seam-check
ok   …/target/debug/blastit: 3 test seam(s) present, as a debug build must
ok   …/target/release/blastit: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   …/target/release/blastit: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
ok   …/target/release/blastit: no BLASTIT_TEST_RELOAD_PAUSE (absent, as shipped)

$ cargo check -p blastit --release --locked
    Finished `release` profile [optimized] target(s); warning count: 0
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)

$ make check
MAKE_CHECK_EXIT=0   # grep -cE 'FAIL|FAILED' over the whole log: 0
test result: ok. 282 passed; 0 failed; 2 ignored …   (unit tier)
test result: ok. 19 passed; 0 failed; 0 ignored …    (critical_dev_reload, build script)
test result: ok. 19 passed; 0 failed; 0 ignored …    (critical_dev_reload, Makefile)
test result: ok. 42 / 7 / 9 / 27 passed, 0 failed …  (lsp / parallel_views / php_backend / php_embed)
Ran 53 tests in 10.228s                            (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 27.7s
ok   …/target/debug/blastit: 3 test seam(s) present, as a debug build must
ok   …/target/release/blastit: no BLASTIT_TEST_SWAP_POINT / RELOAD_FAULT / RELOAD_PAUSE
test result: ok. 254 passed; 0 failed; 1 ignored …  (unit tier; `1 ignored` pre-existing)
plan OK

$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  0.2s  each dev flag boots and stops (…)
48 fences: 30 executed, 18 accounted, 0 failed · 28.9s

$ uv run --no-project tools/critical_guard.py --base 83c6bb9 --node MB.23.5.2.2
  M  crates/blastit/tests/critical_dev_reload.rs  [authorized by crates/blastit/tests/critical_dev_reload.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.23.5.2.2

$ git diff --stat 83c6bb9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty; DIFF_EMPTY=0)
```

Under load (8 busy CPUs) the suite ran 14/14 green, and the repeated serial campaign was
24/24 green; the three cycle-heavy tests alone ran 20/20 under load, and a `--lib`-then-
3×`critical` sequence (the ordering that produced one failure, below) ran 5/5 green.

**One failure has no explanation and is not being called a flake.** In the first serial
batch of this attempt, one `critical_dev_reload` run failed (5.69 s) and its output was
piped through `tail -2`, so the panic was discarded before it could be read — that was a
process mistake on my part. 73 subsequent runs (24 serial full-suite, 14 full-suite under
8 CPUs, 20 targeted at the heaviest test, 15 in the lib-then-3× ordering) were green, and
no code changed in between that could single out one test. The honest state is: one
unread failure, not reproduced; there is no evidence it was load.

### Files (attempt 3)

- `crates/blastit/src/supervise.rs` — the `a_cycle_whose_views_never_drain_is_abandoned_without_a_signal`
  leg (FIX 1); `abandon_cycle`/`progress_cycle` unchanged.
- `docs/design/dev-fanotify-reload.md` — §4.6 step 4 and the §4.10 row (FIX 2).
- `crates/blastit/tests/critical_dev_reload.rs` — the gate comment's target (FIX 3).
- `RESULT.md` — the corrected FIX 4 claim and this section.
