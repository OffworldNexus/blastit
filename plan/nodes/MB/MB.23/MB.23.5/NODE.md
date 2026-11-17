---
id: MB.23.5
title: "Wire reload into blastit dev: Ctl::Reload 0x45 with no payload (all views retire together), supervisor fanotify loop, --no-reload, preload/Blastfile re-run with :8080 held open, Python+PHP live-reload critical tests"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.3, MB.23.4, MB.24.4]
paths:
  - "crates/blastit-frame/src/frame.rs"
  - "crates/blastit-frame/src/ctl.rs"
  - "crates/blastit-scheduler/src/ctl.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-events/src/lib.rs"
  - "crates/blastit-events/src/json.rs"
  - "crates/blastit-events/src/pretty.rs"
  - crates/blastit/src/dev_reload.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/src/child.rs
  - crates/blastit/src/zygote.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/stage0.rs
  - "crates/blastit-dbproxy/src/lib.rs"
  - "crates/blastit-dbproxy/src/config.rs"
  - "crates/blastit-dbproxy/src/learn.rs"
  - "crates/blastit-dbproxy/src/state.rs"
  - "crates/blastit-dbproxy/src/server.rs"
  - ARCHITECTURE.md
  - crates/blastit/tests/critical_dev_reload.rs
  - "crates/blastit/tests/common/**"
  - Makefile
  - "scripts/check-no-test-seam.sh"
  - python/tests/runnable_docs/inventory.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T23:13"
updated: "2026-11-17T09:18"
---

# Goal

Wire the watcher and the loaded-file reports into `blastit dev`, so the documented
behaviour from `MB.23.1` is real:

- `--no-reload` (dev only, default OFF for the flag = reload ON) in `cli.rs`, and a boot
  line that says `reload: on (N directories)` or the one warning.
- `crates/blastit/src/dev_reload.rs`: the startup walk + marks derived from the **union**
  of the loaded-file reports, the skip/cache lists, the debounce state machine, the
  all-worker reload transaction, the `preload`/`./Blastfile` re-run, and the two events.
- `Ctl::Reload` (`0x45`, **no route payload**) in `blastit-frame` + `blastit-scheduler`,
  retiring **every active view's idle workers together** with the reason `reload` and
  marking every view stale, so no two views can serve different versions of a shared or
  preloaded module. init never `SIGKILL`s a view mid-request — the scheduler keeps that
  decision — and the `:8080` listener and the proxies are never retired or re-bound.
- `supervise.rs` gains one dev-only `Source` (the reporting socket) and a `Some(deadline)`
  poll timeout while a reload window is armed; `child.rs` keeps the reporting fd. A
  `preload` module or `./Blastfile` change re-runs the preload/manifest cycle with the
  listening fd handed over once, so `:8080` accepts throughout.

# Context

- Trigger rule from the research (§3), widened to one unit: the watched set is the union
  of the `preload` phase's and every view worker's loaded files; any change to a tracked
  loaded file (including an editable package or template **outside** cwd) → exactly one
  `Ctl::Reload` for every view; a new file in a marked directory is treated the same way
  (conservative). `FAN_Q_OVERFLOW` → all views with one log line. `./Blastfile` or a
  `preload` module → re-run the preload/manifest cycle (stage 1 only knows its `--stage=1`
  argv, so stage 0 hands the restart argv over once via a stage-0-only variable that is
  stripped from views' environment) **without** closing the listening socket.
- Debounce from the last event: **50 ms** quiet, **250 ms** hard max, **500 ms** global
  floor between two reloads. The policy takes the clock as a parameter, so the tests
  assert exact windows.
- `Blastfile` mid-edit: re-parse first; on a `Blastfile:<line>:<col>` error log it once and
  **keep serving the previous topology**; on the next valid save reload the topology once.
  A `preload` module that is mid-write gets the same treatment: the old preload stays
  serving until the file parses and imports cleanly.
- The failure ladder from the research (one sentence, one event, then serve):
  `reload disabled: fanotify is not available here (<errno>)` for `ENOSYS`/`EPERM`/`EINVAL`,
  the filesystem sentence for `EOPNOTSUPP` from `name_to_handle_at`, and "stop marking,
  keep the marks" for `ENOSPC`. Never a second watcher.
- Production and `check` never construct `dev_reload`; the module is reachable only from
  the `dev` supervisor path, and `seccomp.rs` is untouched.
- Events (field names frozen by `MB.23.1`): `dev.reload` with `routes` (every view route
  reloaded together), `files`, `reason` (`loaded`|`preload`|`blastfile`|`overflow`);
  `dev.reload.unavailable` with `errno`, `filesystem`; `process.retired`'s `reason` list
  gains `reload`.

# Acceptance

```console
$ cargo test -p blastit-frame --locked ctl
$ cargo test -p blastit-scheduler --locked reload
$ cargo test -p blastit --locked --features python --test critical_dev_reload
$ cargo clippy -p blastit -p blastit-frame -p blastit-scheduler --all-targets --locked --features python -- -D warnings
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
$ make check
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.23.5
```

`critical_dev_reload.rs` (protected, new file) must prove, with a fake watcher and an
injected clock wherever possible:

1. **All views, one event:** editing `app/views/cart.py` produces exactly one
   `dev.reload` whose `routes` names **every** view route; every view pid changes; the next
   request returns the new body.
2. **No mixed versions:** editing a shared `models.py` produces the same single all-view
   reload, and both routes serve the new body afterwards — a test that would fail if only
   one view were restarted.
3. **Outside cwd:** editing a file in an editable package or a template the app loaded
   from outside the working directory triggers the same single all-view reload; a change in
   `.git/`, `node_modules/` or a log file triggers none.
4. **Atomic save:** a temp-write + `rename` produces exactly one reload (the 50 ms window
   absorbs the burst) — no N-reload storm.
5. **No loop:** a worker writing a framework cache file (an excluded trigger dir) causes
   **zero** further reloads.
6. **`--no-reload`:** zero `dev.reload`, zero marks, and the old body is still served.
7. **`Blastfile` and `preload`:** an invalid intermediate `Blastfile` save logs one
   `Blastfile` error and keeps answering `200`; the next valid save reloads the topology
   once; editing a `preload` module re-runs the preload cycle once — and a keep-alive
   connection opened before the save still gets its response on `:8080` (the listener
   never drops).
8. **Unavailable:** a fake watcher returning `Unavailable{EPERM}` yields exactly one
   `dev.reload.unavailable` and `200`s for the rest of the run.
9. **Overflow:** a `FAN_Q_OVERFLOW` readout reloads every view exactly once and logs once.
10. **PHP:** editing an included `.php` reloads all views together and the next request
    returns the new body; a warm compiled template edit needs no restart (the framework
    revalidates it). The row runs in the PHP-enabled configuration, using the probe the
    existing `critical_php_embed` suite established so a host without `libphp` says so in
    one line instead of passing hollow.
11. **Clock:** debounce and global-floor assertions drive the injected clock; no `sleep`
    decides a verdict (a bounded poll loop with a deadline is allowed only for real process
    startup).

# Notes

- `supervise.rs`'s structural change is small on purpose: one `Source`, one deadline. Keep
  it that way; a second event loop is a redesign.
- The scheduler change is one transaction, not a selection algorithm: `Ctl::Reload` carries
  no route, and every active view's idle worker retires. Do not reintroduce a route map, a
  "reload one view" path, or a per-route test that pins a pid unchanged — that is the
  behaviour this epic deliberately dropped.
- If the diff would exceed ~8 files / ~650 lines, land the Python rows plus the policy and
  file the PHP row as `MB.23.5.1` in the `RESULT.md` `## Follow-ups` — do not widen this
  leaf into an epic.
- `MB.23.3` and `MB.23.4` must be `done` before this starts: the watcher and the reports
  are its inputs.
