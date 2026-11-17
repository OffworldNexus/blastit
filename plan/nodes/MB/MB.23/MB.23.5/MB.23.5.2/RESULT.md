# RESULT — MB.23.5.2

## STATUS

needs_split

The leaf mixes design §8's **5b-i** (the policy, the union, the supervisor's one `Source`
and armed deadline, `--no-reload`, the boot line, `critical_dev_reload.rs` rows 1–6/9/10)
with **5b-ii** (the adoption document, the stage-1 `execve`, stage-1 adoption, stage-0's
restart variable, `reconcile`, the `--learn` skip, rows 7–8). Measured instead of guessed:
`dev_reload.rs` is 1 905 lines (~900 production + the two seams, ~1 000 its own oracles),
the supervisor/zygote/child wiring is ~350 more, stage 1's adoption is ~250 and
`critical_dev_reload.rs` (a real boot, pids, keep-alive, an invalid `Blastfile`, a `preload`
re-run) is ~700 — well past the node's "~10 files / ~900 lines" stop line.

This attempt landed the half that is fully decided, has no consumer to fight with, and is
provable offline with zero sleeps: **the policy engine plus the one user-facing flag**.
Nothing in the tree is red.

## Summary

Landed:

- **`crates/blastit/src/dev_reload.rs` (new)** — design §4.2/§4.3/§4.4/§4.5/§4.7 as code:
  the `ReloadWatcher` seam with its one real adapter (`FanotifyWatcher` over
  `blastit_sys::fanotify::Watcher`) and a scripted `FakeWatcher`; the `Clock` seam with
  `SystemClock` and `TestClock`; `ReloadConfig`; `Class`/`Entry`/`Report`/`Drop`; the
  `ReportTable` (fd-keyed register/revoke/`on_datagram`/`recv`, per-source `loaded::Sequence`
  sets, the union with class precedence `Blastfile > Preload > Loaded`, `dirs_to_mark` minus
  the skip list, the 16 384-path cap); the `Policy` debounce state machine
  (`deadline = max(last_fire + 500 ms, min(last + 50 ms, first + 250 ms))` — exactly §4.5's
  formula), `observe`/`apply_report`/`tick`/`next_deadline`/`pending_dirs`/`stop_marking`/
  `reconcile`; the failure ladder's classification (`Unavailable::of` reads
  **`WatcherError::of(&err)`**, never `raw_os_error()`), `errno_symbol` (`E<number>` outside
  the ladder), `MarkOutcome` and the mid-run `Effect::Stopped` latch; and `Reload`
  (`boot`/`poll_fds`/`on_fd`/`tick`/`sync_marks`/`next_deadline`), dead on `EIO`, `ENOSPC`
  as "stop marking, keep the marks", never retried, never a second watcher.
- **`Policy`'s two run-loop entry points are tested through the real codec**: `report_pair` +
  `send_batch` drive `loaded::Batch` over a real `SOCK_DGRAM` socketpair, so the datagram path
  (`decode`, `source` self-check, `seq` gap rebuild, boot vs resync) is the shipped one.
- **`crates/blastit/src/cli.rs`** — `--no-reload`: dev only, default `false` (reload on), in
  `DevOptions`, in the usage line and in `blastit dev --help`'s block, with a test that the
  internal `--dev-reload=*`/`--dev-adopt-fd`/`--dev-policy-fd` tokens stay **not** dev
  arguments. This is `T29`'s landing step and the only `make check` failure `MB.23.5.1`
  recorded.
- **`crates/blastit/src/lib.rs`** — `pub mod dev_reload;`.
- **`docs/design/dev-fanotify-reload.md` §4.2** — the node's two corrections, nothing else:
  there **is** a typed `WatcherError` (`Unavailable{op,errno}`/`NoFileHandle{fs_type}`,
  carried in the `io::Error`), and the policy classifies with
  `WatcherError::of(&err)?.errno()`, not `err.raw_os_error()` (which is `None` here).

Not landed (the split below): the supervisor's `Reload` field / `Some(deadline)` poll
timeout / report `Source`s, the `child.rs` keep-list and the `zygote.rs`
`serve_reporting(..., Some(LoadedReport{..}))` / `PhpLoadedReport` wiring, the boot line
`reload: on (N directories)`, stage 1's adoption and `stage0.rs`'s restart variable, and
`crates/blastit/tests/critical_dev_reload.rs`. Consequently `--no-reload` currently parses
and boots identically to today's `dev` — the flag is honest (nothing is watched either way)
but the default `reload: on` line is not printed yet, which is 5b-i's supervisor change.

One deliberate, documented deviation from §4.5's illustrative struct: `Fire` carries
`cycle: bool` (+ `needs_cycle()`), because §4.5's own rule — "the action is chosen by the
union entry" — cannot be applied by the supervisor from `{files, reason}` alone (`reason =
loaded` fires both actions). §4.5's snippet is a sketch, so the design page is unchanged,
as the node requires.

## Evidence

### Policy + CLI unit tests (the injected clock decides every verdict)

```console
$ cargo test -p blastit --locked --features python --lib dev_reload
running 16 tests
test dev_reload::tests::a_boot_batch_does_not_rearm_but_a_resync_does ... ok
test dev_reload::tests::a_burst_of_twelve_files_is_one_reload ... ok
test dev_reload::tests::a_framework_cache_write_never_triggers ... ok
test dev_reload::tests::a_mid_run_watcher_error_stops_once_and_is_never_retried ... ok
test dev_reload::tests::a_seq_gap_rebuilds_the_source_set ... ok
test dev_reload::tests::a_view_report_marks_its_parent_through_the_wakeup ... ok
test dev_reload::tests::atomic_save_is_one_reload ... ok
test dev_reload::tests::debounce_edges_and_the_floor ... ok
test dev_reload::tests::enospc_stops_marking_and_the_marks_keep_firing ... ok
test dev_reload::tests::modify_is_never_a_trigger ... ok
test dev_reload::tests::overflow_reloads_every_view_once ... ok
test dev_reload::tests::path_refusals_are_counted_once ... ok
test dev_reload::tests::skipped_missing_and_untracked_paths_do_not_fire ... ok
test dev_reload::tests::the_union_cap_refuses_the_last_path_with_one_warning ... ok
test dev_reload::tests::two_windows_four_hundred_ms_apart_respect_the_floor ... ok
test dev_reload::tests::unavailable_errno_mapping_and_the_sentence ... ok
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 254 filtered out
```

Covers design `T2` (`FAN_MODIFY` alone never fires), `T6` (temp-write + `MOVED_TO` = one
fire), `T7` (no fire at 49 ms, fire at 50 ms; a 10 ms stream fires at the 250 ms hard max; a
second window 400 ms later fires at the 500 ms floor), the node's row 10 (`12` files ⇒ one
`Fire` with all 12), `T8` (every ladder errno through the typed `WatcherError`, `E9999`, one
`Stopped` and no retry), `T9` (one `overflow` fire, `cycle`), `T10`/`T11`-policy (a
`storage/framework/views` and a `.next` write fire nothing though both are marked; `.git`,
`node_modules`, an untracked log fire nothing; an outside-cwd template does fire; relative /
missing / 4 097-byte / non-UTF-8 paths are refused once and never marked), `T15` (the union
cap, a wrong-source datagram, a `seq` gap), `T30` (boot does not re-arm, resync does),
`T32` (`ENOSPC` stops marking and a placed mark still fires) and the wakeup invariant that a
report's new parent is marked in the same `on_fd` (§4.4: "every later source registers at
`spawn_now` … the union grows only from reports the supervisor has already marked").

### Whole crate

```console
$ cargo test -p blastit --locked --features python --lib
test result: ok. 268 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.40s
```

### `cargo clippy` (both feature sets) and `cargo fmt`

```console
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.33s
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.27s
$ cargo fmt --all -- --check
FMT_OK
```

### Watcher guard on the standalone binary

```console
$ cargo build -p blastit --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.91s
$ scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 456 source files (one fanotify group, one Watcher::new)
EXIT=0
```

### `make docs-test` — 100 % green, the `--no-reload` fence executed

```console
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  2.3s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 2  check-flags  4.9s  each check flag runs
...
48 fences: 30 executed, 18 accounted, 0 failed · 26.4s
```

The `dev-flags` fence is `MB.23.5.1`'s one `make check` failure; it now boots `blastit dev
--no-reload` and stops on `SIGTERM` like the other three lines of that fence.

### `cargo test --test critical_dev_reload` — not yet a target (the split)

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload
error: no test target named `critical_dev_reload` in package `blastit`.
       Test targets available: ... critical_php_embed, critical_raw_ingress, ...
EXIT=101
```

Rows 1–8/10 of the node are end-to-end rows (a live `blastit dev`, view pids, a keep-alive
connection, an invalid `Blastfile`, a `preload` re-run) and cannot be written honestly
before 5b-i/5b-ii exist. The policy half of rows 3–6, 9 and 10 is proven above through the
same `observe`/`tick` entry points the supervisor will call.

### `make check`

```console
$ make check
...  (see /tmp/opencode/make-check-mb2352-definitive.log; the earlier run on the
      same tree at /tmp/opencode/make-check-mb2352.log also finished EXIT=0)
EXIT=0
```

Every target, all green: guard, rs-fmt, rs-lint (`cargo clippy --workspace … -D warnings`),
rs-doc (`RUSTDOCFLAGS="-D warnings" cargo doc`), rs-test + test-critical,
rs-no-python, py-lint, docs-unit, docs-check, docs-test (the `--no-reload` fence),
seam-check, platform-matrix-check, workflow-check, packaging-gates-test, doc-tone,
manifest-purity, zed-check, plan validate.

### A release build carries neither seam

```console
$ cargo check -p blastit --release --locked
    Finished `release` profile [optimized] target(s) in 31.21s
```

No warning, and `FakeWatcher`/`TestClock`/`report_pair`/`send_batch` are absent from a
non-`debug_assertions` image (even the `Batch` import is behind the seam's `cfg`), so the
`seam-check` target and the wheel's shipped-symbol test stay green.

### Protected tests, lock file, seccomp

```console
$ uv run --no-project tools/critical_guard.py --base 54ad5d9 --node MB.23.5.2
critical-guard: 54ad5d9..working tree, 14 changed file(s)
node MB.23.5.2: paths [...]
  no change to a protected critical test
$ git diff --quiet -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs && echo LOCK_AND_SECCOMP_IDENTICAL
LOCK_AND_SECCOMP_IDENTICAL
```

No dependency, no feature, no workspace member, no config key, no `seccomp.rs` byte.

## Files

- `crates/blastit/src/dev_reload.rs` (new)
- `crates/blastit/src/cli.rs`
- `crates/blastit/src/lib.rs`
- `docs/design/dev-fanotify-reload.md` (§4.2 only)

## Split (proposed)

1. **`MB.23.5.2` (remainder, 5b-i)** — `supervise.rs`: the one dev-only `Reload` field, its
   `poll_fds` (poll duplicate first, then each report end) folded into the existing poll
   array without a second loop, `Some(deadline)` while `next_deadline()` is `Some`,
   `Effect::Reload ⇒ one Ctl::Reload` plus the `dev.reload` event, `Effect::Unavailable`/
   `Stopped ⇒` the one event/warning; `child.rs` keeps the reporting fd; `zygote.rs` creates
   the socketpair per view and passes `LoadedReport{socket, source}` /
   `PhpLoadedReport{..}` with a **fresh** reporter per boot (the second `boot()` refusal in
   `loaded.rs` is the oracle), registering/revoking sources at `spawn_now`/`reap_children`;
   the boot line `reload: on (N directories)`; `crates/blastit/tests/critical_dev_reload.rs`
   rows 1–6, 9, 10 end-to-end (one `dev.reload` naming every route, every view pid changed,
   new body; shared module; outside cwd; atomic save; zero-reload cache write; `--no-reload`;
   injected-clock edges; the 12-file burst).
2. **`MB.23.5.2.1` (5b-ii)** — the reload cycle: validate-then-`execve`, the adoption
   document, the `--dev-*` internal tokens, stage 1's no-bind/no-fork adoption on the
   inherited `http.chan`, `stage0.rs`'s stage-0-only restart variable (stripped from views),
   `Policy::reconcile` after the boot report, the `reloading` gate in
   `component_exit`/`ctl_closed`, the `--learn` skip, `BLASTIT_TEST_RELOAD_PAUSE`/`FAULT` in
   `scripts/check-no-test-seam.sh`, and rows 7–8 (invalid `Blastfile`, `preload` re-run with
   `:8080` and a pre-save keep-alive, `FAN_Q_OVERFLOW` through the real supervisor).

## Follow-ups

- `stage1.rs`'s `dev.is_some()` hook gate is still only `dev.is_some()`: the design gates
  the hook on the report fd being present (§5 `S3`), and that fd exists only after 5b-i's
  zygote wiring, so clearing the gate without it would leave a hook with nothing to talk to.
  The flag needs no `Config` field (`config.rs` is deliberately untouched): presence of the
  fd is the gate `--no-reload` produces.
- `scripts/check-no-test-seam.sh` is unchanged on purpose: `BLASTIT_TEST_RELOAD_PAUSE` and
  `BLASTIT_TEST_RELOAD_FAULT` gate 5b-ii's supervisor path. Declaring them now would claim a
  seam no code reads.
- `Fire` gained `cycle` (see Summary). §4.5's sketch could name it in `MB.23.6`'s design
  audit; this node was told to change nothing in the design but §4.2.
- The node's Context line "a new file in a marked directory is treated the same way
  (conservative)" contradicts design §4.4 ("An untracked new file is **not** a tracked
  change and does not reload") and `T11`, and §6 makes the design/`T11` win; the policy
  follows the design. If the epic really wants the conservative reading, it needs a design
  edit plus a `T11` rewrite, not a code change here.
- Host flake, not a regression: `critical_php_embed::an_egress_route_lets_the_probe_open_a_socket`
  failed once under full `make check` load with "the probe child … did not load `libphp` …
  within 250 ms", passes alone and in the next full `make check`. Worth a
  `docs/knowledge/LESSONS.md` line (a 250 ms probe budget is tight under a full gate);
  `LESSONS.md` is outside this node's paths so it was not edited.

## Gate

Both children are `done`: **`MB.23.5.2.1`** (commit `83c6bb9` — the policy, the supervisor's
single source and armed deadline, real `--no-reload`, the 14-leg live-reload suite) and
**`MB.23.5.2.2`** (commit `bdd23c5` — validate-then-`execve`, the adoption document, the cycle
and its abandon path, the three seams, five more legs).

| Command | Result |
|---|---|
| `cargo test -p blastit --test critical_dev_reload` | `19 passed; 0 failed` (repeated serial and under all cores busy) |
| `cargo test -p blastit --lib` | `282 passed, 2 pre-existing ignored` |
| `make check` | `exit 0, 0 FAIL, 0 FAILED` |
| `make docs-test` | `48 fences, 30 executed, 0 failed, dev-flags EXECUTED` |

`make seam-check` proves three debug seams present in a debug build and all three absent from a
release build; `Cargo.toml`, `Cargo.lock` and `crates/blastit-sys/src/seccomp.rs` are
byte-identical to the base. The gates that used to be red are green: this container is what
closed `docs/reference/cli.md`'s `--no-reload` fence for the whole tree.

**ONE OPEN UNKNOWN, stated plainly:** during `MB.23.5.2.2` attempt 3 a single
`critical_dev_reload` run failed early and its panic was destroyed by piping through `tail -2`, so
it could never be triaged. It did not reproduce in 90+ subsequent runs, including many with every
core busy, and it is recorded as an unexplained unknown rather than attributed to host load. Any
future failure must be captured to a full log.
