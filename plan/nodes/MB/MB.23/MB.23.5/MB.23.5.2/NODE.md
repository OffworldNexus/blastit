---
id: MB.23.5.2
title: "Wire reload into blastit dev: the policy, the supervisor loop, --no-reload, and the preload/Blastfile re-run with :8080 held open"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.5.1]
paths:
  - crates/blastit/src/dev_reload.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/src/child.rs
  - crates/blastit/src/zygote.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/tests/critical_dev_reload.rs
  - "crates/blastit/tests/common/**"
  - Makefile
  - "scripts/check-no-test-seam.sh"
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/dev-fanotify-reload.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:37"
updated: "2026-11-17T12:37"
---

# Goal

Wire the watcher and the loaded-file reports into `blastit dev` (design §8's **5b-i +
5b-ii**): `crates/blastit/src/dev_reload.rs` (union, debounce policy, reload cycle), the
supervisor's one dev-only `Source` and armed deadline, `--no-reload` with the boot line, the
`preload`/`./Blastfile` re-run with `:8080` held open, and `critical_dev_reload.rs`. This
leaf also ends `make check` and `make docs-test` **fully green**, closing the one failure
`MB.23.5.1` recorded.

# Context

- Read `docs/design/dev-fanotify-reload.md` §4.1 (boot order), §4.2 (seam 1), §4.3 (seam 2),
  §4.4 (the union), §4.5 (debounce), §4.6 (both transactions), §4.7 (the ladder), §4.8
  (listener and proxies), §4.10 (budgets) and §8's 5b-i/5b-ii lines. `MB.23.3`
  (`blastit-sys::fanotify`, `WatcherError::of`) and `MB.23.4`
  (`blastit-view` codec, `LoadedReport`/`LoadedReporter`, `serve_reporting`) are `done`;
  `MB.23.5.1` shipped the frame, the events, `RetireReason::Reload` and `ARCHITECTURE.md`.
- **Trigger rule:** the watched set is the union of the `preload` phase's and every view
  worker's loaded files; any change to a tracked loaded file — including an editable
  package or template **outside** cwd — → exactly one `Ctl::Reload` for every view; a new
  file in a marked directory is treated the same way (conservative).
  `FAN_Q_OVERFLOW` → all views with one log line.
- **Debounce from the last event:** 50 ms quiet, 250 ms hard max, 500 ms global floor
  between two reloads. `deadline(now) = max(last_fire + 500 ms, min(last + 50 ms, first +
  250 ms))`. The policy takes the clock as a parameter, so tests assert exact windows; zero
  sleeps decide a verdict.
- `./Blastfile` or a `preload` module re-runs the preload/manifest cycle **without closing
  the listening fd** (stage 0 hands the restart argv over once via a stage-0-only variable
  stripped from views); an invalid intermediate `Blastfile` save logs **one**
  `Blastfile:<line>:<col>` error and keeps the previous topology serving.
- **Failure ladder:** `reload disabled: fanotify is not available here (<errno>)` for
  `ENOSYS`/`EPERM`/`EINVAL` via `WatcherError::of(&err)` — **not** `raw_os_error()`, which
  is `None` here; the filesystem sentence for `EOPNOTSUPP` from `name_to_handle_at`;
  "stop marking, keep the marks" for `ENOSPC`; never a second watcher, never a retry.
- `--no-reload` is **dev only and the only knob**; reload is ON by default. The boot line is
  `reload: on (N directories)`, or the one warning instead. `--dev-reload`,
  `--dev-adopt-fd`, `--dev-policy-fd`, `--dev-reload-since` and `--dbproxy-policy-fd` are
  internal and never appear in `--help`.
- **Two corrections to `docs/design/dev-fanotify-reload.md` §4.2** (that page is in this
  node's paths for exactly this): "no separate error enum" and "`err.raw_os_error()`" are
  both wrong for the delivered `WatcherError`. Fix the two sentences to the shipped
  type/method; change nothing else in the design.
- Structural wiring, deliberately small: `crates/blastit/src/supervise.rs` gains **one**
  dev-only `Source` (the reporting socket) and a `Some(deadline)` poll timeout while a
  reload window is armed; `crates/blastit/src/child.rs` keeps the reporting fd;
  `crates/blastit/src/zygote.rs` passes `serve_reporting(..., Some(LoadedReport{socket,
  source}))` (python) and `Some(PhpLoadedReport{..})` (PHP) with the worker fd in the keep
  list and a **fresh** reporter per boot; `crates/blastit/src/stage1.rs`'s `dev.is_some()`
  gate on the hook must be cleared so `--no-reload` disables the hook too;
  `crates/blastit/src/lib.rs` gains the module. No second event loop.
- Stage-1 adoption and the reload exec are 5b-ii: no bind, no `http`/dbproxy fork, the new
  scheduler on the inherited `http.chan` end, the adoption document, `Policy::reconcile`
  after the boot report, the `reloading` gate in `component_exit`/`ctl_closed`, and step 0's
  `--learn` skip (§4.6). `crates/blastit/src/dev_learn.rs` is **unchanged**.

# Acceptance

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload
$ cargo clippy -p blastit -p blastit-frame -p blastit-scheduler --all-targets --locked --features python -- -D warnings
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
$ make check
$ make docs-test
$ uv run --no-project tools/critical_guard.py --base $(git log --format=%H -1 --grep '^\[MB.23.5.1\]') --node MB.23.5.2
$ git diff --quiet -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
```

1. **All views, one event:** editing `app/views/cart.py` produces exactly one `dev.reload`
   whose `routes` names **every** view route; every view pid changes; the next request
   returns the new body.
2. **No mixed versions:** editing a shared `models.py` produces the same single all-view
   reload, and both routes serve the new body afterwards — a test that would fail if only
   one view were restarted.
3. **Outside cwd:** editing a file in an editable package or a template the app loaded from
   outside the working directory triggers the same single all-view reload; a change in
   `.git/`, `node_modules/` or a log file triggers none.
4. **Atomic save:** a temp-write + `rename` produces exactly one reload (the 50 ms window
   absorbs the burst) — no N-reload storm.
5. **No loop:** a worker writing a framework cache file (an excluded trigger dir) causes
   **zero** further reloads.
6. **`--no-reload`:** zero `dev.reload`, zero marks, and the old body is still served; also
   no `dev.reload.unavailable`, no fanotify link, no reporting socket, no audit hook.
7. **`Blastfile` and `preload`:** an invalid intermediate `Blastfile` save logs one
   `Blastfile` error and keeps answering `200`; the next valid save reloads the topology
   once; editing a `preload` module re-runs the preload cycle once — and a keep-alive
   connection opened before the save still gets its response on `:8080` (the listener never
   drops, and the supervisor/dbproxy/`http` pids are unchanged while the scheduler pid
   changed).
8. **Overflow:** a `FAN_Q_OVERFLOW` readout reloads every view exactly once and logs once.
9. **Clock:** debounce and global-floor assertions drive the injected clock; no `sleep`
   decides a verdict (a bounded poll loop with a deadline is allowed only for real process
   startup).
10. **The multi-file burst (conductor's explicit requirement):** 12 tracked files modified
    back-to-back (Save-All / `git checkout` / a multi-file agent edit) coalesce into **ONE**
    `dev.reload` — asserted both on the injected-clock state machine and end-to-end in
    `critical_dev_reload.rs`; never 12 reloads.
11. `make check` is now **fully green** (the `--no-reload` fence `MB.23.5.1` allowlisted is
    closed) and `make docs-test` is 100 % green, with the fence executed live.

# Notes

- `Cargo.toml` and `Cargo.lock` stay **byte-identical**: no new dependency, no feature, no
  workspace member. `libc`/`rustix` are already there; no raw syscall numbers and no
  hand-written structs.
- No new knob or config key beyond `--no-reload`: no environment variable, no `Blastfile`
  field, no `BLASTIT_DEV_RELOAD=poll` (a second stat-based path is rejected by `ADR-0014`).
- Never a second watcher, never a retried boot, never a second event loop — one `Reload`,
  one `Source`, one deadline.
- Production and `check` never construct `dev_reload`; the module is reachable only from
  the `dev` supervisor path and carries no production caller.
- `crates/blastit-sys/src/seccomp.rs` is untouched; `dev_reload` widens no filter.
- No test may assert a per-route or surgical restart. A surviving "only one view restarted"
  or "this pid is unchanged across a reload" assertion is a bug: delete it.
- The two debug-only seams (`BLASTIT_TEST_RELOAD_PAUSE`, `BLASTIT_TEST_RELOAD_FAULT`) are
  declared in `scripts/check-no-test-seam.sh` beside `BLASTIT_TEST_SWAP_POINT` and compiled
  out of release builds; the guard must stay green for the release artifact.
- Keep `supervise.rs`'s change to one `Source` plus one deadline; if it wants a second loop,
  stop and file a follow-up rather than redesigning the supervisor here.
- `crates/blastit/src/php_view.rs` is **not** in this node's paths: pass the PHP report
  through `PhpViewSpawn` from `supervise.rs`/`zygote.rs` only if no edit there is needed;
  otherwise record it under `## Follow-ups` for `MB.23.5.3`.
- `crates/blastit-view/**` is `MB.23.4`'s and `crates/blastit/src/dev_learn.rs` must stay
  unchanged; the new image calls the existing `Learn::adopt` with the adopted descriptors.
- `critical_dev_reload.rs` is a **new** protected file, so adding it is allowed; it must not
  carry `#[ignore]` or a weakened assertion, and every row above must be red-capable.
- If the diff would exceed ~8 files / ~650 lines beyond the design's own cut, land rows
  above plus the policy and file the remainder as a `## Follow-ups` child — do not widen
  this leaf into an epic.
