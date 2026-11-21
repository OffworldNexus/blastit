# RESULT — MB.23.5.2.1

## STATUS

done

## Summary

Attempt 2 finished the leaf attempt 1 had already implemented: **the only code change was one
doc-comment sentence** in `crates/blastit/tests/critical_dev_reload.rs`. `make check` was red at
`critical-skip-check` because the suite's header documentation *quoted* the forbidden skip token
while explaining that it never uses one; `CRITICAL_SKIP_PATTERN`
(`\#[[:space:]]*ignore|...`) greps every `crates/*/tests/critical_*.rs` and cannot tell prose
from an attribute. Reworded to "no test here is marked ignored (R42)" — the same point, without
the token. The pattern, `tools/critical_guard.py`, and every assertion are untouched. Nothing
else in the tree was edited in this pass; the feature below was verified, not rewritten.

What this node delivers (base `54ad5d9`):

- **`crates/blastit/src/dev_reload.rs` (new, 1 937 lines)** — design §4.2–§4.9: the
  `ReloadWatcher` seam (`FanotifyWatcher` over `blastit_sys::fanotify::Watcher`, plus a scripted
  `FakeWatcher`), the `Clock` seam (`SystemClock`, `TestClock`), `ReloadConfig`,
  `Class`/`Entry`/`Report`/`Drop`, `ReportTable` (fresh per-boot reporter registration,
  `forget_source` on reap, forged-source refusal), `Policy`
  (`deadline = max(last_fire + 500 ms, min(last + 50 ms, first + 250 ms))`,
  `observe`/`apply_report`/`tick`/`next_deadline`/`pending_dirs`/`stop_marking`/`kill`/`reconcile`),
  the `Effect::{Reload,Stopped}` resolution and `Reload::{boot, open, policy, now, poll_fds,
  on_fd, tick, sync_marks, next_deadline}`. `Reload::open` keeps the single `Watcher::new(` inside
  the guarded module so `scripts/check-no-inotify.sh` still passes.
- **`crates/blastit/src/supervise.rs` (+406/−…)** — one dev-only `reload: Option<Reload>` built by
  `boot_reload`; its fds appended to the existing `poll_once` after the other sources; the timeout
  is `min(shutdown deadline, reload.next_deadline())` and stays `None` while nothing is armed;
  a ready fd → `on_fd`, the deadline → `tick`; `Effect::Reload` → exactly one `dev.reload`
  (every view route in manifest order, `fire.files`, `fire.reason`) **and** exactly one
  `Ctl::Reload`; `Effect::Stopped` → one `dev.reload.unavailable` warn and the `Reload` is
  dropped (never retried, never a second watcher). Per-spawn report sockets with
  `forget_source` and revoke-on-reap, `open_preload_report` on source `0xffff`, and
  `announce_reload` printing the pretty-only `reload: on (N directories)` boot line.
- **`crates/blastit/src/zygote.rs` (+35)** — `serve_reporting(..., Some(LoadedReport{..}))` with a
  fresh `socketpair(AF_UNIX, SOCK_DGRAM|SOCK_CLOEXEC|SOCK_NONBLOCK)` per boot (the second
  `boot()` refusal in `loaded.rs` is the oracle).
- **`crates/blastit/src/stage1.rs` (+83)** — the loaded-file hook gate is the report fd's
  presence, not `dev.is_some()` (§5 `S3`), so with no report there is no hook, no socket and no
  mark. `crates/blastit/src/child.rs` needed **no** edit (the keep-list plumbing landed through
  `zygote.rs`), which is why it is absent from the diff stat.
- **`crates/blastit/src/cli.rs` (+26) / `lib.rs` (+4)** — `--no-reload` (dev only, default off
  i.e. reload on, usage line, `blastit dev --help` block, and the test that the internal
  `--dev-*` tokens stay non-dev arguments) and `pub mod dev_reload;`.
- **`crates/blastit/tests/critical_dev_reload.rs` (new, protected, 1 194 lines, 10 tests)** — rows
  1, 2, 3, 4, 5, the boot line, overflow, the exact 50/250/500 ms edges through `TestClock`, the
  boot-batch-arms-nothing rule, and forged-source refusal. No skip construct; no assertion pins a
  per-route restart or an unchanged pid for another route.
- The absolute-root fix: the root **and** the manifest are resolved absolute before the root mark
  is placed, so a relative `./Blastfile` cannot shadow the absolute root mark.

`Cargo.toml`, `Cargo.lock` and `crates/blastit-sys/src/seccomp.rs` are byte-identical to the base
commit: no new dependency, no feature, no workspace member, no widened filter.

## Evidence

All commands run in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`, branch
`release/0.2.0`, after the doc-comment edit. Base commit `54ad5d9`.

### `cargo test -p blastit --locked --features python --test critical_dev_reload`

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload
running 10 tests
test a_queue_overflow_is_one_all_views_reload ... ok
test a_forged_source_is_dropped_and_never_tracked ... ok
test a_boot_report_replaces_a_set_and_arms_nothing ... ok
test the_debounce_windows_are_exact_through_the_injected_clock ... ok
test the_boot_line_is_one_pretty_line_and_never_an_event ... ok
test saving_a_view_reloads_every_view_once_and_the_new_body_is_served ... ok
test a_worker_cache_write_never_triggers_a_reload ... ok
test saving_a_shared_module_leaves_no_view_on_the_old_version ... ok
test an_atomic_save_is_one_reload ... ok
test outside_the_project_reloads_and_vcs_dependencies_and_logs_do_not ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.00s
```

### `cargo clippy -p blastit --all-targets --locked --features python -- -D warnings`

```console
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.59s
EXIT=0   # zero warnings, zero errors
```

### `cargo fmt --all -- --check`

```console
$ cargo fmt --all -- --check
EXIT=0   # no output: every file already formatted
```

### `cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit`

```console
$ cargo build -p blastit --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.52s
$ scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
```

### `make critical-skip-check` (the TASK A regression, proven green)

```console
$ make critical-skip-check
protected critical tests hold no skip construct
```

### `make check`

```console
$ make check
...
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

all doctests ran in 0.22s; merged doctests compilation took 0.21s
protected critical tests hold no skip construct
...
cargo test -p blastit --features python --test critical_dev_reload
     Running tests/critical_dev_reload.rs (target/debug/build/blastit/6ba8c066bfd6efe4/out/critical_dev_reload-6ba8c066bfd6efe4)

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.12s
...
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.72s      # critical_lsp
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.51s       # critical_parallel_views
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s     # critical_php_backend
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.29s     # critical_php_embed
Ran 53 tests in 15.844s                                                                             # python/tests/critical
...
Ran 19 tests in 2.600s    # test_check_zed_extension
uv run --no-project tools/plan.py validate
plan OK
```

`grep -cE "FAIL|FAILED"` over the full log: **0**. `EXIT=0`.
No flake tripped in this run — `critical_php_embed::an_egress_route_lets_the_probe_open_a_socket`
and `critical_parallel_views::serialized_then_parallel_php` both passed inside the gate, so
neither needed an isolated re-run. (The `1 ignored` in the 244-test suite is a pre-existing
non-critical unit test, untouched by this leaf; the critical tier itself reports
`0 failed; 0 ignored` everywhere.)

### `make docs-test`

```console
$ make docs-test
PASS  docs/frameworks/static-and-spa.md  fence 6  presets     0.0s  each preset exists and fails closed without capabilities
PASS  docs/reference/cli.md  fence 1  dev-flags               4.4s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 2  check-flags             8.2s  each check flag runs
PASS  docs/reference/cli.md  fence 3  serve-flags             0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                   0.1s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                       0.0s  statically checked every command, flag and snippet (...)

48 fences: 30 executed, 18 accounted, 0 failed · 33.0s
EXIT=0
```

The `dev-flags` fence is **executed** (4.4 s, not accounted) and it is the fence that boots
`blastit dev --no-reload` from `docs/reference/cli.md`:

```console
$ blastit dev
$ blastit dev --port 9000
$ blastit dev --learn
$ blastit dev --no-reload
```

### `cargo check --release -p blastit --locked`

```console
$ cargo check --release -p blastit --locked
    Checking blastit-view v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit-view)
    Checking blastit v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit)
    Finished `release` profile [optimized] target(s) in 2.70s
EXIT=0   # `grep -c warning`: 0
```

No test seam (`BLASTIT_TEST_RELOAD_PAUSE`/`_FAULT` are not declared in this leaf at all) reaches a
release check; the module has no production caller.

### `uv run --no-project tools/critical_guard.py --base 54ad5d9 --node MB.23.5.2.1`

```console
$ uv run --no-project tools/critical_guard.py --base 54ad5d9 --node MB.23.5.2.1
critical-guard: 54ad5d9..working tree, 49 changed file(s)
node MB.23.5.2.1: paths ['crates/blastit/src/dev_reload.rs', 'crates/blastit/src/supervise.rs', 'crates/blastit/src/child.rs', 'crates/blastit/src/zygote.rs', 'crates/blastit/src/cli.rs', 'crates/blastit/src/lib.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/tests/critical_dev_reload.rs', 'crates/blastit/tests/common/**', 'Makefile', 'scripts/check-no-test-seam.sh', 'python/tests/runnable_docs/inventory.toml']
  no change to a protected critical test
EXIT=0
```

### `git diff --stat 54ad5d9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs`

```console
$ git diff --stat 54ad5d9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
EXIT=0   # empty output: the three files are byte-identical to the base commit
```

## Files

Changed by this finishing pass (1 file, 3 lines of prose):

- `crates/blastit/tests/critical_dev_reload.rs` — header sentence reworded so the file keeps
  making the R42 point without the literal `#[ignore]` token that `CRITICAL_SKIP_PATTERN` greps.

Landed by attempt 1 and verified here (not edited in this pass):

- `crates/blastit/src/dev_reload.rs` (new, 1 937 lines)
- `crates/blastit/src/supervise.rs` (+406/−…)
- `crates/blastit/src/zygote.rs` (+35/−…)
- `crates/blastit/src/stage1.rs` (+83/−…)
- `crates/blastit/src/cli.rs` (+26/−…), `crates/blastit/src/lib.rs` (+4)
- `crates/blastit/tests/critical_dev_reload.rs` (new, protected, 1 194 lines, 10 tests)
- `docs/design/dev-fanotify-reload.md` (attempt 1's two §4.2 corrections; frozen since)

`crates/blastit/src/child.rs`, `Makefile`, `scripts/check-no-test-seam.sh` and
`python/tests/runnable_docs/inventory.toml` are in `paths` but needed **no** change: the report
fd rides `zygote.rs`, and the existing `rs-test-critical` glob, `critical-skip-check` and
`dev-flags` fence already cover this leaf.

## Follow-ups

Recorded verbatim as owed by `MB.23.5.2.2` / `MB.23.5.3`, not softened:

1. **Row 6 (`--no-reload`) end to end is IMPOSSIBLE in this leaf.** `stage0.rs` belongs to
   `MB.23.5.2.2` and must emit `--dev-reload=off`. Until then the flag parses and is **inert**
   while `docs/local.md` already claims "nothing is watched", so 5b-ii owes **both** the wiring
   and the doc truth. This leaf lands only the gate both halves need (hook + report socket +
   watcher exist in the same boot, §5 `S3`).
2. **`Class::Preload` never fires** in the live path: a source carries one class, so
   `reason=preload` needs per-path classification, which 5b-ii owns.
3. **`crates/blastit/src/php_view.rs` carries no report**, so PHP slots never reload — owed by
   5b-ii / `MB.23.5.3`.
4. **`docs/knowledge/LESSONS.md` (outside this node's `paths`) deserves a line:** a *relative*
   `Blastfile` parent resolves to `.` and shadows the absolute root mark, so the root **and** the
   manifest must both be made absolute before marks are placed. Not written here because the file
   is out of scope.
5. **Design §4.7's frozen sentence prints `reload: on (1 directories)`** — the pluralisation is
   the design's wording, not a code bug; the code prints the count verbatim as frozen.

## Attempt 2

Attempt 2 reviewed the six MAJOR + three MINOR findings below as already fixed and verified
them; this pass changed **one binding** and one formatting chain. Base commit `54ad5d9`,
branch `release/0.2.0`, worktree `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`.

### The code change (TASK 1): `polled_sources` hoisted, ordering untouched

`Supervisor::poll_once` had a `let mut polled_sources = 0;` placeholder assigned later inside the
poll-set block; clippy's `unused_assignments` reproached the dead initial value. The binding is
now computed where it is used and returned with the flags, so there is no sentinel:

```rust
let (ready, polled_sources): (Vec<rustix::event::PollFlags>, usize) = {
    ...
    // The number of log sources **in the poll set**, read here while it
    // is being built: `read_ctl` below can fork a view and push two
    // more, and the reload's descriptors sit after the ones that were
    // polled, not after the ones init now owns (a same-wakeup `SPAWN`
    // used to shift them, so report datagrams were never drained).
    let polled_sources = self.sources.len();
    ...
    (flags, polled_sources)
};
```

The capture still happens **inside** the block, after the poll set is built and before
`rustix::event::poll`/`read_ctl`: that ordering is the defect fix and is not undone. Behaviour is
identical — same value, same instant, same `first = base + polled_sources` index base.

Formatting also had to be repaired: `cargo fmt --all -- --check` arrived **red**, with exactly
one diff, in `crates/blastit/tests/critical_dev_reload.rs:521` (a `self.live_views().iter().any(..)`
chain left over a line break by this attempt's `wait_replaced` helper). `rustfmt` was run on that
one file — a reflow only; no assertion, bound, tolerance or test name changed, and the suite is
still 12/12.

**Files in this pass:** `crates/blastit/src/supervise.rs` (the hoist: −4/+6 lines, behaviour
identical) and `crates/blastit/tests/critical_dev_reload.rs` (rustfmt reflow only). Nothing else in
the tree was touched by attempt 2's finishing pass.

### Findings F1–F9 (review order in `LOG.md`)
- **F1 (MAJOR, `--no-reload` was inert).** The flag is now forwarded across the `execve` and gates
  the whole watcher: with it stage 1 constructs no `Reload`, opens no fanotify group, places no
  mark, creates no report socket and therefore installs no audit hook. Guard:
  `critical_dev_reload::no_reload_watches_nothing_and_serves_the_old_body` (zero `dev.reload`,
  zero marks, `fanotify_fds() == 0`, one fewer view descriptor than the reload-on control, old
  body and unchanged pid after a save, no boot line, no `dev.reload.unavailable`).
- **F2 (MAJOR, a reused source leaked its previous report read end and a dead end spun the
  poll).** `ReportTable::register` drops the source's previous raw end before inserting the new
  one, and `Reload::on_fd` revokes the source in the same wakeup on a zero-length read or a read
  error (`EAGAIN` stays the idle race), so a hung-up descriptor never stays level-triggered.
  Guards: `dev_reload::registering_a_source_again_releases_the_previous_end` and
  `dev_reload::a_closed_report_end_is_revoked_on_its_own_wakeup`.
- **F3 (MAJOR, `ENOSPC` latched with no warning and the `MAX_MARKS` cap silently broke).** The
  policy latches `marking_stopped` plus a one-shot cap flag; `Reload::boot` keeps serving on the
  `ENOSPC` row but hands the owed warning to the caller through `take_latch()` (so the supervisor
  emits exactly one `dev.reload.unavailable` instead of `reload: on`), and a cap hit during
  `pending_dirs` reports `MarkOutcome::Exhausted` like a real `ENOSPC`. Guards:
  `dev_reload::enospc_stops_marking_and_the_marks_keep_firing` and
  `dev_reload::the_mark_cap_latches_like_enospc_and_reports_once`.
- **F4 (MAJOR, `dirs_to_mark` re-`stat`ed every union path on every datagram and tick).** The
  `is_dir` bit is decided once by the absorb's single `stat` and stored on `Entry`; the marking
  pass only walks the union and never touches the filesystem. Guard:
  `dev_reload::dirs_to_mark_never_restats_the_union`.
- **F5 (MAJOR, `sync_marks`' `Dead` outcome was swallowed).** `Reload::boot` now returns
  `Err(why)` for `MarkOutcome::Dead` instead of booting a watcher that can never mark (stage 1
  turns it into the ladder's one warning and serves without reload), and `fold_marks` maps a
  mid-run `Dead` to `Effect::Stopped` — one warning, then the `Reload` is dropped. Honest gap: the
  `Dead`-from-marking arm has **no dedicated test** (the closest is its `ENOSPC` sibling above);
  recorded as follow-up 6.
- **F6 (MAJOR, the `reloading` gate could stay set forever with no live view).** The gate is
  `opens_reload_gate(fire, live_views) = fire.needs_cycle() && live_views > 0` and is cleared at
  the expected scheduler exit, so a later genuine scheduler death is never excused. Guard:
  `supervise::a_reload_with_no_live_view_leaves_the_gate_clear`.
- **F7 (MINOR, the cycle was discarded).** `Fire` carries `cycle` (`needs_cycle()`), so the gate
  is keyed on the reload's own verdict instead of being inferred from the event; the same test as
  F6 pins both the `cycle` and all-views halves.
- **F8 (MINOR, the overflow leg witnessed only the `Effect`).** The end-to-end leg now goes
  through the real supervisor: `critical_dev_reload::a_queue_overflow_reloads_every_view_once_and_logs_one_line`
  (kernel `FAN_Q_OVERFLOW` under a `SIGSTOP`/`SIGCONT` window, one `dev.reload`, every route
  named, every view pid replaced, one log line, nothing twice).
- **F9 (MINOR, blocking socketpairs).** `dev_reload::report_socket()` is the single constructor
  and clears blocking on **both** ends; spawn (`supervise.rs:888`) and the preload
  (`supervise.rs:1419`) use it, so neither a reporting worker nor the draining supervisor can
  wait on the other. Guard: the end-to-end critical suite (a worker reports while init is
  stopped — the overflow leg above), via the same constructor; no separate `O_NONBLOCK`
  assertion exists.

### Acceptance (all re-run after the edit)

All commands below ran in the worktree above; tails are real.

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload
running 12 tests
test no_reload_watches_nothing_and_serves_the_old_body ... ok
test a_queue_overflow_reloads_every_view_once_and_logs_one_line ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.97s
EXIT=0
```

```console
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.31s
EXIT=0   # zero warnings (the `polled_sources` unused-assignment warning is gone)
```

```console
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.39s
CLIPPY_PLAIN_EXIT=0
```

```console
$ cargo fmt --all -- --check
FMT_EXIT=0   # no output
```

```console
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.28s
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
BUILD_EXIT=0
```

```console
$ cargo check --release -p blastit --locked
    Checking blastit v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit)
    Finished `release` profile [optimized] target(s) in 3.25s
REL_EXIT=0   # `grep -c '^warning'`: 0
```

```console
$ make check
cargo test -p blastit --features python --test critical_dev_reload
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.03s
test result: ok. 42 passed; 0 failed; 0 ignored ... (critical_lsp)
test result: ok. 7 passed; 0 failed; 0 ignored ... (critical_parallel_views, 5.52s)
test result: ok. 9 passed; 0 failed; 0 ignored ... (critical_php_backend)
test result: ok. 27 passed; 0 failed; 0 ignored ... (critical_php_embed, 9.65s)
test result: ok. 250 passed; 0 failed; 1 ignored ... (unit tier; the `1 ignored` is pre-existing)
Ran 53 tests in 15.158s          (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 40.4s
test result: ok. 43 passed ... / Ran 19 tests in 3.244s  (zed-check)
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0   # `grep -cE 'FAIL|FAILED'` over the whole log: 0
```

The only `warning:` line in the full log is cargo's pre-existing
`doc tests are not supported for crate type(s) cdylib in package blastit-py`. No flake tripped:
`critical_php_embed::an_egress_route_lets_the_probe_open_a_socket` and
`critical_parallel_views::serialized_then_parallel_php` both passed inside the gate, so neither
needed an isolated re-run, and there was no intermittent hang (see the `polled_sources` note).

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags             2.3s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 3  serve-flags           0.0s  production refuses closed; help lists the commands and presets
PASS  docs/security.md  fence 1  static                     0.0s  statically checked every command, flag and snippet
48 fences: 30 executed, 18 accounted, 0 failed · 34.8s
DOCS_TEST_EXIT=0
```

The `dev-flags` fence runs in **2.3 s**, i.e. it is EXECUTED (not accounted), and it is the fence
that boots `blastit dev --no-reload`.

```console
$ uv run --no-project tools/critical_guard.py --base 54ad5d9 --node MB.23.5.2.1
critical-guard: 54ad5d9..working tree, 53 changed file(s)
node MB.23.5.2.1: paths ['crates/blastit/src/dev_reload.rs', 'crates/blastit/src/supervise.rs', 'crates/blastit/src/child.rs', 'crates/blastit/src/zygote.rs', 'crates/blastit/src/cli.rs', 'crates/blastit/src/lib.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/tests/critical_dev_reload.rs', 'crates/blastit/tests/common/**', 'Makefile', 'scripts/check-no-test-seam.sh', 'python/tests/runnable_docs/inventory.toml']
  no change to a protected critical test
GUARD_EXIT=0
```

```console
$ git diff --stat 54ad5d9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty)
```

### The authorised `stage0.rs` exception

`crates/blastit/src/stage0.rs` is **not** in this node's `paths`, and it is edited on purpose, as
the only such file: `exec_stage1_dev(..., no_reload)` gained one `bool` parameter, `run_dev` passes
`options.no_reload` to it, and the function pushes `--dev-reload=off` into stage 1's argv when the
user asked for it. That is the whole exception — **15 insertions, 1 deletion, nothing else in the
file** (`git diff --numstat 54ad5d9 -- crates/blastit/src/stage0.rs` → `15	1`). It is
unavoidable: the flag is parsed in
stage 0 and stage 1 is a different image, so the only way to mean "install nothing" is to carry
the token across the `execve`. `MB.23.5.2.2` owns the rest of the stage-0 restart variable.

### The extra defect found in attempt 2: `polled_sources` read too late

`poll_once` used to index the reload descriptors from `self.sources.len()` *after* `read_ctl`.
A `SPAWN` handled in the same wakeup forks a view and pushes two log sources, so the reload
descriptors' poll indices shifted and the ready flags were read against the wrong slots: report
datagrams were never drained, the window never armed from a report, and the suite hung
intermittently. That hang was this bug, **not a host flake**. The capture is now taken while the
poll set is built (before `read_ctl`), which is why the block returns it with the flags; the
F2/F8 guards and every live-row test exercise it.

### Follow-ups (attempt 1's list, still accurate, plus one honest gap)

1. The reload **cycle** itself (validate-then-`execve`, the adoption document) is `MB.23.5.2.2`'s:
   a `Blastfile`/preload save currently emits `reason=blastfile` without re-reading anything.
2. `Class::Preload` still needs per-path classification before `reason=preload` can fire.
3. `crates/blastit/src/php_view.rs` carries no report, so PHP slots do not reload yet
   (`MB.23.5.2.2` / `MB.23.5.3`).
4. `docs/knowledge/LESSONS.md` (outside `paths`) deserves a line: a *relative* `Blastfile` parent
   resolves to `.` and shadows the absolute root mark, so the root **and** the manifest must be
   made absolute before marks are placed.
5. Design §4.7's frozen sentence prints `reload: on (1 directories)`; the pluralisation is the
   design's wording, and the code prints the count verbatim as frozen.
6. **New:** F5's `Dead`-from-marking arm has no dedicated test (only the `ENOSPC`/cap siblings);
   a one-assert unit test (`FakeWatcher { mark_failure: Some(EPERM) }` ⇒ `Reload::boot` is
   `Err`/`Effect::Stopped`) belongs to whichever node next touches `dev_reload.rs`.

## Attempt 3

Attempt 3 fixed the one BLOCKER, two MAJOR and three MINOR findings of the attempt-2 review.
The blocker was real and was the reason `make check` died while the suite passed alone. Base
commit `54ad5d9`, branch `release/0.2.0`, worktree
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`.

### FIX 1 (BLOCKER) — the log-source loop read past the poll set

`poll_once` iterated `0..self.sources.len()`, while `ready` carries only `polled_sources`
source slots, with the reload's descriptors after them. A `Ctl::Spawn` handled by `read_ctl`
in the same wakeup forks a view and pushes two log pipes, so the loop read a reload
descriptor's ready flag as if it were a source's and then `read_source`ed the forked view's
empty pipe — and those pipes are blocking, so init parked in `read(fd, 16 KiB)` for good. That
is exactly the panic attempt 2 saw inside `make check` (`saving_a_view...`, init in `S` /
`pipe_read`); it was never a host flake.

The bound is now a named, tested invariant rather than an expression nobody checks:

```rust
// A same-wakeup `SPAWN` adds pipes `ready` has no slot for; reading them
// indexes the reload descriptors and blocks in `read(2)` on an empty pipe.
fn polled_source_range(polled_sources: usize, live_sources: usize) -> Range<usize> {
    0..polled_sources.min(live_sources)
}
fn reload_base(base: usize, polled_sources: usize) -> usize { base + polled_sources }
```

Red-capability, proven, not asserted:

- `supervise::a_wakeup_reads_only_the_sources_its_poll_set_carried` is the deterministic pin:
  with `polled_sources = 4` and six live sources it requires `0..4`, and `reload_base(2, 4)`
  must be `6`. Changing the helper to `0..live_sources` makes it fail with
  `left: 0..6, right: 0..4` (verified by hand, then reverted).
- `critical_dev_reload::a_spawn_and_a_report_in_one_wakeup_never_wedge_init` drives the live
  path: three saves, each awaited only through its `dev.reload`, so every round's watcher
  event lands next to the previous reload's `SPAWN` and the fresh views' boot reports, with a
  request in between that can only be answered by a live init.

### FIX 2 (MAJOR) + FIX 6 (MINOR) — no gate this leaf cannot honour; `fire.cycle` deleted

Nothing in this leaf stops the scheduler, so a gate opened by a `blastfile`/preload/overflow
save could only ever stay open and swallow the next genuine `scheduler-died`. The truthful
state is therefore **nothing opens it**: the `reloading` field, `opens_reload_gate`,
`ChildTable::len` and the two `expected` arms in `component_exit`/`ctl_closed` are gone, and
both paths are back to the base rule — a component exit outside a shutdown is fatal (§8),
emits `shutdown` with `<component>-died` and `_exit(1)`. `Fire::cycle`/`needs_cycle()` had no
remaining reader, so they are deleted (FIX 6) and the two overflow assertions that named them
are gone with the field; nothing else in those tests changed.

Red-capability, proven: `critical_dev_reload::a_scheduler_exit_after_a_reload_still_stops_the_boot`
saves the `Blastfile` (reason `blastfile`, every route), waits for the replacement views,
SIGKILLs the scheduler, and requires init to exit non-zero, name the death
(`scheduler-died` or `ctl.chan: the scheduler closed the channel`) and leave no view behind
(`kill_all`). With the gate simulated (`component_exit` returning early for the scheduler and
`ctl_closed` non-fatal) the test fails with the boot naming `http-died` and no scheduler
death; it passes 4/4 with the real code. The scheduler is identified by init's child list:
`http` is forked first, the scheduler second (`stage1.rs`), and a component's own `/proc`
entry is unreadable by design (`PR_SET_DUMPABLE` cleared), so a wrong pick fails the
naming assertion instead of passing silently.

### FIX 3 (MINOR) — a closed peer, not an empty datagram

`Reload::on_fd` takes the poll flags now (`on_fd(fd, flags, now)`), because the old test's
premise was false and so was the comment: on Linux an `AF_UNIX` `SOCK_DGRAM` end whose writer
exited reports **no** hangup, no readability and no zero-length read (`unix_release_sock`
releases only stream and seqpacket peers) — a blocking `recv` after the peer closes just
blocks. So the datagram EOF the old test "drove" was an empty batch, and `Reporter::send`
never emits one. Now:

- `a_closed_report_end_is_revoked_on_its_own_wakeup` drops the peer and drives
  `on_fd(fd, PollFlags::HUP, now)` — the flags `poll` reports when a transport does report
  one — and requires the source to leave the poll set in that wakeup;
- `a_zero_length_datagram_is_not_a_hangup` is the negative that pins the corrected reading: an
  empty batch keeps the end registered instead of silently unwatching a route;
- the live mechanism is named in both comments and in design §4.5: a view's end is really
  revoked by `reap_children`, and the flags arm closes anything a transport does report.
- `Ok(0)` no longer revokes, a read error other than `EAGAIN` still does.

### FIX 4 (MINOR) — the dead-after-preload marking row has a test

The post-preload decision is now a value, `stage1::after_preload_marks(outcome, stopped,
dirs) -> BootLine::{Announce, Silent, Unavailable, Stopped}`, which is what `serve` acts on.
Grey-box test `stage1::a_watcher_that_dies_after_the_preload_keeps_the_boot_line_off` builds
the real `Reload` over a real temp tree with a `FakeWatcher` that marks the app root (call 1,
the boot) and fails `EIO` from call 2 (the preload pass), absorbs one loaded file so the pass
has a parent to mark, drives `sync_marks` into `MarkOutcome::Dead`, and requires
`BootLine::Stopped(EIO)` — never `Announce` — plus the two positive controls
(`Marked, false -> Announce(3)`, `Marked, true -> Silent`). `FakeWatcher` gained
`fail_from: (call, errno)` for exactly this ("a later pass dies").

### FIX 5 (MINOR) — the design edits are authorised and stated

`docs/design/dev-fanotify-reload.md` is in this leaf's `paths` as of the plan amendment, so
the attempt-2 corrections stay, and attempt 3 adds one clause for FIX 3. The authorised edits,
in order of the file:

1. §4.1 boot order: `--no-reload` is carried across the `execve` as the internal token
   `--dev-reload=off`, and every step is skipped entirely.
2. §4.1 step 5: a boot whose marking latched `ENOSPC` (or hit the marks cap) prints its one
   warning **instead** of `reload: on (N directories)`, because the line is a claim that
   marking succeeded.
3. §4.2: the real watcher's frozen surface carries one typed error,
   `fanotify::WatcherError`, read with `WatcherError::of(&err)?.errno()`, never
   `raw_os_error` (which is `None` for every error that module returns).
4. §4.5: `on_fd(fd, flags, now)` — the flags travel because a datagram report end's dead
   peer is invisible to `read` on Linux, so `POLLHUP`/`POLLERR` closes the source when a
   transport reports one (this is FIX 3's clause).
5. §4.5: `Exhausted(why)` is the `ENOSPC` row with **one** `dev.reload.unavailable`, the same
   row the §4.10 marks cap hits — a cap is never a silent `break`; `Dead` stays the mid-run
   latch.

`stage0.rs` (now in `paths` too) is unchanged in attempt 3: its `exec_stage1_dev(...,
no_reload)` forwarding of `--dev-reload=off` is still the whole 15-insertion/1-deletion
exception.

### Acceptance (all re-run after the edits)

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload   # 3x in a row
run 1 exit=0  test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.55s
run 2 exit=0  test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.65s
run 3 exit=0  test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.51s
```

```console
$ # once under load: 8 busy loops on 8 cpus, then the same suite
under-load exit=0  test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.19s
```

```console
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.03s
EXIT=0
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.26s
EXIT=0
$ cargo fmt --all -- --check
FMT=0
```

```console
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.94s
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
BUILD_EXIT=0
$ cargo check --release -p blastit --locked
    Finished `release` profile [optimized] target(s) in 2.97s
REL_EXIT=0   # `grep -c '^warning'`: 0
```

```console
$ make check
cargo test -p blastit --features python --test critical_dev_reload
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.95s
test result: ok. 42 passed; 0 failed; 0 ignored ... (critical_lsp)
test result: ok. 7 passed; 0 failed; 0 ignored ... (critical_parallel_views, 5.41s)
test result: ok. 9 passed; 0 failed; 0 ignored ... (critical_php_backend)
test result: ok. 27 passed; 0 failed; 0 ignored ... (critical_php_embed, 8.35s)
test result: ok. 251 passed; 0 failed; 1 ignored ... (unit tier; the `1 ignored` is pre-existing)
Ran 53 tests in 11.422s          (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 26.9s
Ran 19 tests in 1.745s           (zed-check)
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0   # `grep -cE 'FAIL|FAILED'` over the whole log: 0
```

The **first** `make check` of this attempt failed, and honestly:
`critical_php_embed::a_probe_that_hangs_is_killed_and_refuses_the_boot` panicked with "no probe
shape was written", after the log showed "the probe child … did not load `libphp` … within
250 ms". That is the known 250 ms probe-budget flake on this host, in a suite this node does
not touch (`git diff --stat 54ad5d9 -- crates/blastit/tests/critical_php_embed.rs` is empty);
run alone it passed **2/2**, and the next full `make check` was green end to end. No timeout
was raised, no test disabled, nothing reordered.

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags             0.2s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
48 fences: 30 executed, 18 accounted, 0 failed · 25.4s
DOCS_TEST_EXIT=0
```

```console
$ uv run --no-project tools/critical_guard.py --base 54ad5d9 --node MB.23.5.2.1
critical-guard: 54ad5d9..working tree, 54 changed file(s)
node MB.23.5.2.1: paths [... 'crates/blastit/src/stage0.rs', 'crates/blastit/src/stage1.rs', 'crates/blastit/tests/critical_dev_reload.rs', ... 'docs/design/dev-fanotify-reload.md']
  no change to a protected critical test
GUARD_EXIT=0
```

```console
$ git diff --stat 54ad5d9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty)
```

### Files (attempt 3)

- `crates/blastit/src/supervise.rs` — `polled_source_range`/`reload_base` with the bound at the
  call site; the `reloading` gate, `opens_reload_gate` and `ChildTable::len` removed;
  `component_exit`/`ctl_closed` back to the base fatal rule; `on_fd` call passes the flags;
  the wakeup unit test.
- `crates/blastit/src/dev_reload.rs` — `on_fd(fd, flags, now)` (revoke on hangup/error, not on
  an empty batch); `Fire::cycle`/`needs_cycle` deleted; `FakeWatcher::fail_from`; the two
  report-end tests.
- `crates/blastit/src/stage1.rs` — `BootLine` + `after_preload_marks`, `serve` acting on it,
  and the grey-box EIO test.
- `crates/blastit/tests/critical_dev_reload.rs` — two new tests (spawn/report wakeup;
  scheduler exit after a reload), `PollFlags` at the `on_fd` call sites, the dead-cycle
  assertion removed.
- `docs/design/dev-fanotify-reload.md` — FIX 5's authorised clauses (above).

### Follow-ups

1. The reload **cycle** (validate-then-`execve`, the adoption document) is `MB.23.5.2.2`'s, and
   it is also where the gate may come back: a cycle that stops the scheduler must open it, and
   this leaf ships none. A `Blastfile`/preload save still emits `reason=blastfile` without
   re-reading anything.
2. `Class::Preload` still needs per-path classification before `reason=preload` can fire.
3. `crates/blastit/src/php_view.rs` carries no report, so PHP slots do not reload yet
   (`MB.23.5.2.2` / `MB.23.5.3`).
4. `docs/knowledge/LESSONS.md` (outside `paths`) deserves two lines: (a) a *relative*
   `Blastfile` parent resolves to `.` and shadows the absolute root mark; (b) an `AF_UNIX`
   `SOCK_DGRAM` peer's exit is invisible to `read`/`poll` on Linux, so a report end is closed
   by the reap path and never by an EOF.
5. Design §4.7's frozen sentence prints `reload: on (1 directories)`; the pluralisation is the
   design's wording, and the code prints the count verbatim as frozen.
6. The `Dead`-from-marking arm is pinned for the post-preload row (FIX 4); the mid-run
   `fold_marks` arm still rides on the read-path test
   `a_mid_run_watcher_error_stops_once_and_is_never_retried`.

## Attempt 4

Attempt 4 changed **no production behaviour**. Base commit `54ad5d9`, branch `release/0.2.0`,
worktree `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`. Two defects in this
leaf's own protected test, one grammar fix in the design, and one bookkeeping note.

### FIX A (MAJOR, flake) — the test waited on a table that retires lazily

`a_scheduler_exit_after_a_reload_still_stops_the_boot` waited for `live_views().len() == 2`
while issuing **no** request. This design retires views lazily, so a route whose view the
reload retired has no live view until someone asks for it; the table could legitimately sit
at 1 and the wait burned the whole 30 s `EVENT_TIMEOUT` (the reviewer's 1-in-9 failure). The
length assertion is gone. The set the test needs is now the pids that **answered**:

```rust
let cart = body_of(&dev.get("/cart/x"));
let products = body_of(&dev.get("/products/x"));
assert_ne!(cart.pid, products.pid, "two routes, two views");
let views = vec![("cart", cart.pid), ("products", products.pid)];
```

Both requests make their views live before they answer, so the set is exact on the **first
poll**: no wait, no added timeout, and nothing depends on how many views the table holds.

### FIX B (MAJOR, wrong assertion) — accept any named `<component>-died`, and say what the test witnesses

The old assertion demanded `scheduler-died` or the `ctl.chan` message. Under load the
observed stage was `http-died`: killing the scheduler closes `ctl.chan`, http exits with it,
and http's death is fatal in every version — so the test was both too narrow **and** not a
witness of the deleted gate at all. Now the transcript is parsed and any shutdown stage that
names a component is accepted:

```rust
.filter(|event| event["event"] == "shutdown")
.filter_map(|event| event["stage"].as_str().map(str::to_string))
.filter(|stage| stage == "scheduler-died" || stage == "http-died")
```

(named stages, not a substring match; the `ctl.chan: the scheduler closed the channel` line
stays as the second legitimate path). The doc comment now states plainly what it does and does
not witness: it proves **"a component dying during a reload is still fatal and leaves no
view"**, and it says so explicitly — it does **not** prove "no gate exists", because http's
exit is fatal in every version, so a re-introduced scheduler gate would still satisfy its
non-zero-exit and `kill_all` assertions. A true gate witness needs the cycle that can stop the
scheduler, and belongs to `MB.23.5.2.2`; none is faked here.

### FIX C (MINOR) — the design's grammar

`docs/design/dev-fanotify-reload.md` §4.5 had a dangling `The` left by attempt 3's inserted
sentence ("…in the same wakeup. The" / "report fds change…"). Rewrapped so the paragraph reads
"…closes the source in the same wakeup. The report fds change as views are born and reaped…".
No meaning changed; `make docs-check` still builds clean.

### FIX D (MINOR) — bookkeeping: the deleted `cycle`, and the unfulfilled doc row

`Fire::cycle`, `Fire::needs_cycle()` and `ChildTable::len` are deleted in this leaf (attempt 3,
FIX 2/FIX 6), because nothing here can stop the scheduler and carrying the flag would be dead
state. Design §4.5/§4.6 still choose between the *transaction* and the *cycle* with exactly
that flag (`cycle: true` for a `Blastfile`, a `preload` path or an overflow), so
`MB.23.5.2.2` must **re-add** `Fire::cycle`/`needs_cycle()` together with the cycle and the
gate that consumes it (and re-pin the two overflow assertions that went with it).

Likewise `docs/local.md`'s live-reload table still promises, for `./Blastfile` or a `preload`
module, "**re-reads what changed**, then reloads the whole topology". That row is **not**
fulfilled by this build: a manifest/preload save fires a single all-views reload with
`reason=blastfile` and re-reads nothing — there is no `execve`, no adoption document and no
re-parse. It stays true only after `MB.23.5.2.2`. The doc is outside this node's `paths`, so
it is recorded here rather than edited.

### Acceptance (all re-run after the edits)

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload   # 5 consecutive
run 1 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.94s
run 2 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.97s
run 3 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.04s
run 4 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.00s
run 5 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.94s

$ # 3 more with four busy loops on this 8-cpu host
load run 1 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.13s
load run 2 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.09s
load run 3 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.43s

# 9 full-suite runs in this attempt (5 serial + 3 loaded + 1 earlier single-run check): 0 failures
```

```console
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.26s
$ cargo fmt --all -- --check
FMT=0
```

```console
$ make check
test result: ok. 14 passed; 0 failed; 0 ignored ... (critical_dev_reload)
test result: ok. 42 passed; 0 failed; 0 ignored ... (critical_lsp)
test result: ok. 7 passed; 0 failed; 0 ignored ... (critical_parallel_views, 5.35s)
test result: ok. 9 passed; 0 failed; 0 ignored ... (critical_php_backend)
test result: ok. 27 passed; 0 failed; 0 ignored ... (critical_php_embed, 8.16s)
test result: ok. 251 passed; 0 failed; 1 ignored ... (unit tier; the `1 ignored` is pre-existing)
Ran 53 tests in 11.478s          (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 26.4s
Ran 19 tests in 1.748s           (zed-check)
plan OK
MAKE_CHECK_EXIT=0   # `grep -cE 'FAIL|FAILED'`: 0
```

The `critical_php_embed` probe test did **not** trip in this run; no timeout was raised,
nothing disabled, nothing reordered.

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags             2.2s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
48 fences: 30 executed, 18 accounted, 0 failed · 25.5s
DOCS_TEST_EXIT=0
```

```console
$ uv run --no-project tools/critical_guard.py --base 54ad5d9 --node MB.23.5.2.1
critical-guard: 54ad5d9..working tree, 78 changed file(s)
node MB.23.5.2.1: paths [... 'crates/blastit/tests/critical_dev_reload.rs' ... 'docs/design/dev-fanotify-reload.md']
  no change to a protected critical test
GUARD_EXIT=0
```

```console
$ git diff --stat 54ad5d9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty)
```

### Files (attempt 4)

- `crates/blastit/tests/critical_dev_reload.rs` — the answered-pid set (FIX A), the
  any-named-`<component>-died` assertion and the honest doc comment (FIX B).
- `docs/design/dev-fanotify-reload.md` — the grammar fix (FIX C).

No production file changed in this attempt (`crates/blastit/src/**` is byte-identical to
attempt 3's tree).

## Attempt 5

Attempt 5 fixed the last MAJOR, which was a flaw in **how the test asserts**, not in the
feature: the test enumerated stderr wordings for a fatal exit. Base commit `54ad5d9`, branch
`release/0.2.0`, worktree `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`.
`crates/blastit/src/**` is byte-identical to attempt 4's tree — proven, not asserted:
`find crates/blastit/src -type f -name '*.rs' | sort | xargs sha256sum | sha256sum` is
`27d96f55f2b329be7a0f5d67372af2b1b6a352b5eecfe08405836b1dbec21160` before and after this pass.

### The finding

Under load init legitimately died through `send_ctl`'s **write** failure and printed
`{"event":"app.output","stream":"stderr","line":"ctl.chan: Broken pipe (os error 32)"}` with
**no** `shutdown` stage, so the enumerated `died` vector was empty and the accepted literal
sentence was false — a panic in 1 of 3 loaded runs. The enumeration could only ever chase
phrasings: the read-EOF arm, a `<component>-died` stage and the write-failure arm are the
**same** contract (any component dies during a reload ⇒ `kill_all` + `_exit(1)`, no surviving
view), and which one wins is a race.

### The fix: assert the contract, never the wording

The enumeration (`died` vec, the exact-sentence `closed` check, and the `shutdown`-event
requirement) is deleted. What remains:

```rust
let status = dev.wait_exit();
assert!(!status.success(), "any component dying during a reload is fatal, ...");

// ... for every (route, pid) in views: /proc/<pid> must be gone before the deadline
assert!(!Path::new(&format!("/proc/{pid}")).exists(),
        "route {route}'s view {pid} outlived init: `kill_all` did not run \
         (last fatal arm seen: {witness:?})");
```

(a) the boot exited non-zero, and (b) **no view process survives** — the real evidence, and
exactly what breaks if `kill_all` were skipped. A log witness is still *recorded* (`(c)`: any
`ctl.chan:`-prefixed stderr line or any `-died` shutdown stage), but it is **never required**:
it is only carried into the surviving-view assertion's message, so `None` is a pass.

The code carries a comment saying why, so nobody re-narrows it:

```rust
// Deliberately wording-agnostic; do not re-narrow it. A component death
// during reload traffic surfaces through whichever fatal arm wins the race —
// the reaped `<component>-died` shutdown stage, the `ctl.chan` read-EOF arm,
// or `send_ctl`'s write failure (`ctl.chan: Broken pipe`) — and all three
// are the same contract: `kill_all` and `_exit(1)`. Demanding one phrasing
// turns this into a wording lottery (it has already flaked on each arm in
// turn), so the asserted evidence is the non-zero exit above and the
// emptied view set below. A log witness is recorded for whoever reads a
// failure, and `None` is a pass.
```

The doc comment keeps the honest scope: it witnesses "a component dying during a reload is
still fatal, kills every view and exits non-zero", and explicitly **not** "no gate exists" —
a re-introduced scheduler gate would still satisfy it, and the true gate witness belongs to
`MB.23.5.2.2`. Nothing is faked.

### Acceptance

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload   # 3 serial
serial 1 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.95s
serial 2 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.96s
serial 3 exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.01s

$ # 5 more with all 8 cpus in busy loops (the reviewer's failure was 1 in 3 loaded runs)
load 1 (cpus=8 busy) exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.71s
load 2 (cpus=8 busy) exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.60s
load 3 (cpus=8 busy) exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.02s
load 4 (cpus=8 busy) exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.64s
load 5 (cpus=8 busy) exit=0 :: test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.94s

# 8 full-suite runs this attempt, 5 of them under load, 0 failures
```

```console
$ cargo fmt --all -- --check
FMT=0
```

```console
$ make check
cargo test -p blastit --features python --test critical_dev_reload
test a_scheduler_exit_after_a_reload_still_stops_the_boot ... ok
test result: ok. 14 passed; 0 failed; 0 ignored ... (critical_dev_reload, twice: build script + Makefile)
test result: ok. 42 passed; 0 failed; 0 ignored ... (critical_lsp)
test result: ok. 7 passed; 0 failed; 0 ignored ... (critical_parallel_views, 5.30s)
test result: ok. 9 passed; 0 failed; 0 ignored ... (critical_php_backend)
test result: ok. 27 passed; 0 failed; 0 ignored ... (critical_php_embed, 8.13s)
test result: ok. 251 passed; 0 failed; 1 ignored ... (unit tier; the `1 ignored` is pre-existing)
Ran 53 tests in 11.162s          (python/tests/critical)
48 fences: 30 executed, 18 accounted, 0 failed · 22.6s
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
Ran 19 tests in 1.766s           (zed-check)
plan OK
MAKE_CHECK_EXIT=0   # `grep -cE 'FAIL|FAILED'`: 0
```

No timeout was raised, nothing disabled, nothing reordered, and the `critical_php_embed` probe
test did not trip in this run.

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags             0.2s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
48 fences: 30 executed, 18 accounted, 0 failed · 22.3s
DOCS_TEST_EXIT=0
```

```console
$ uv run --no-project tools/critical_guard.py --base 54ad5d9 --node MB.23.5.2.1
  no change to a protected critical test
GUARD_EXIT=0

$ git diff --stat 54ad5d9 -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty)
```

### Files (attempt 5)

- `crates/blastit/tests/critical_dev_reload.rs` — the wording enumeration replaced by the fatal
  contract (non-zero exit + no surviving view), the recorded-but-optional log witness, the
  wording-agnostic comment and the honest doc comment.

No production file and no other test changed in this attempt.
