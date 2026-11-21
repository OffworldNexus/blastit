---
id: MB.23.5.2.1
title: "Supervise the reload: one dev Source, an armed deadline, fresh per-boot reporters, the boot line and the live-reload critical suite"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - crates/blastit/src/dev_reload.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/src/child.rs
  - crates/blastit/src/zygote.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/tests/critical_dev_reload.rs
  - "crates/blastit/tests/common/**"
  - Makefile
  - "scripts/check-no-test-seam.sh"
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/dev-fanotify-reload.md"
attempts: 5
max_attempts: 6
escalation: none
created: "2026-11-17T13:44"
updated: "2026-11-17T16:31"
---

# Goal

Land design §8's commit **5b-i** on top of the policy that already exists: one dev-only
`dev_reload::Reload` inside the supervisor's existing `poll(2)`, its armed deadline, one
fresh per-boot report socket wired from `spawn_now` through `zygote.rs` into every view,
the `reload: on (N directories)` boot line (or the one warning), the hook gate that makes
`--no-reload` install nothing at all, and `crates/blastit/tests/critical_dev_reload.rs`
(new, protected) proving the live behaviour end to end. No `execve`, no adoption document,
no `stage0.rs` restart variable — that is `MB.23.5.2.2`.

# Context

- **Already on disk from `MB.23.5.2` attempt 1 (base `54ad5d9` + working tree) — verify, do
  not rewrite:** `crates/blastit/src/dev_reload.rs` (new, ~1 905 lines: `ReloadWatcher` /
  `FanotifyWatcher` / `FakeWatcher`, `Clock` / `SystemClock` / `TestClock`,
  `ReloadConfig`, `Class` / `Entry` / `Report` / `Drop`, `ReportTable`, `Policy`
  (`deadline`, `observe`, `apply_report`, `tick`, `next_deadline`, `pending_dirs`,
  `stop_marking`, `kill`, `reconcile`), `Effect`, `MarkOutcome`, and
  `Reload::{boot, policy, now, poll_fds, on_fd, tick, sync_marks, next_deadline}`),
  `cli.rs`'s `--no-reload` (parser, usage line, `blastit dev --help` block, the test that
  the `--dev-*` tokens stay non-dev arguments), `lib.rs`'s `pub mod dev_reload;`, and the
  two §4.2 corrections in `docs/design/dev-fanotify-reload.md`. Read the node's
  `RESULT.md` first: it records exactly what landed and what it deliberately did not.
  Touch `dev_reload.rs` only if the supervisor genuinely needs an accessor nobody added;
  prefer zero edits, and never change a policy verdict the existing unit tests pin.
- **`supervise.rs` — one field, one deadline, no second loop.** Build
  `reload: Option<dev_reload::Reload>` only on the `dev` path with reload on:
  `Reload::boot(cfg, Box::new(FanotifyWatcher::new()?), dup, Box::new(SystemClock))`,
  where `dup` is the `F_DUPFD_CLOEXEC` clone of the group fd taken **before** the box goes
  in (§4.1: a clone without `FD_CLOEXEC` would outlive the image). In `poll_once`, append
  `reload.poll_fds(&mut out)` after the existing sources and keep `source_base`'s index
  arithmetic correct; the timeout becomes `min(shutdown deadline, reload.next_deadline())`
  and stays `None` while nothing is armed. A ready reload fd → `on_fd(fd, now)`; the
  deadline waking the poll → `tick(now)`. Map `Effect::Reload(fire)` to **exactly one**
  `dev.reload` (routes = every view route in manifest order, `fire.files`,
  `fire.reason`) **and exactly one** `Ctl::Reload`; map `Effect::Stopped(why)` to exactly
  one `dev.reload.unavailable` warn and then drop the `Reload` — no retry, no second
  watcher. Register the preload source `0xffff` before the first import and each view
  source at `spawn_now`; `reap_children` revokes it. While a transaction is not yet armed
  there is nothing to do: init never `SIGKILL`s a view on this path.
- **`child.rs` / `zygote.rs` — the report fd, one per boot.** Put the write end in
  `ChildIo.keep` (one extra descriptor, nothing else). `fork_view` creates one
  `socketpair(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK)` per spawn and passes
  `Some(LoadedReport { socket, source })` for Python and `Some(PhpLoadedReport { .. })`
  for PHP through `serve_reporting`, with a **fresh** reporter per boot (the second
  `boot()` refusal in `loaded.rs` is the oracle). `php_view.rs` is **not** in this node's
  paths: route it through `PhpViewSpawn` from `supervise.rs` / `zygote.rs` only if no edit
  there is needed, else record it under `## Follow-ups` for `MB.23.5.3`.
- **Boot line.** Pretty stdout only (§4.9: not an event), `reload: on (N directories)`
  after the boot marks, or the ladder's single warning instead. `--no-reload` prints
  neither, and both the `--dev-reload`/`--dev-adopt-fd`/`--dev-policy-fd`/
  `--dev-reload-since` tokens stay absent from `--help`.
- **`stage1.rs` — one gate.** The loaded-file hook gate stops being `dev.is_some()` and
  becomes the report fd's presence (§5 `S3`), so `--no-reload` installs no hook, opens no
  reporting socket and places no marks. Nothing else in stage 1 changes here.
- `Makefile`, `scripts/check-no-test-seam.sh` and `python/tests/runnable_docs/inventory.toml`
  are in scope only if the harness needs a line (`rs-test-critical` already globs
  `crates/*/tests/critical_*.rs`; the `dev-flags` fence already runs `--no-reload`). Do
  **not** declare `BLASTIT_TEST_RELOAD_PAUSE` / `_FAULT` here — `MB.23.5.2.2` owns them.

# Acceptance

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
$ cargo fmt --all -- --check
$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
$ make check
$ make docs-test
$ uv run --no-project tools/critical_guard.py --base $(git log --format=%H -1 --grep '^\[MB.23.5.1\]') --node MB.23.5.2.1
$ git diff --quiet -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs && echo LOCK_AND_SECCOMP_IDENTICAL
```

1. **All views, one event (row 1):** editing `app/views/cart.py` yields exactly one
   `dev.reload` whose `routes` names **every** view route; every view pid changes; the next
   request returns the new body.
2. **No mixed versions (row 2):** editing a shared `models.py` yields the same single
   all-view reload and both routes then serve the new body — the assertion that goes red
   if one view were left on the old module.
3. **Outside cwd (row 3):** a changed file in an editable package or a template loaded from
   outside the working directory triggers the same single all-view reload; a change under
   `.git/`, `node_modules/` or a log file triggers none.
4. **Atomic save (row 4):** a temp-write + `rename` produces exactly one reload.
5. **No loop (row 5):** a worker's framework cache write causes **zero** further reloads.
6. **`--no-reload` (row 6):** zero `dev.reload`, zero marks, old body still served, no
   `dev.reload.unavailable`, no fanotify link, no reporting socket, no audit hook.
7. **Overflow (row 8):** one `FAN_Q_OVERFLOW` readout reloads every view exactly once and
   logs one line (injected-clock/policy assertion plus the end-to-end path where the fake
   watcher can script it).
8. **Clock (row 9):** the 50 / 250 / 500 ms windows are asserted through the injected
   clock; no `sleep` decides a verdict (a bounded startup poll is the only allowed wait).
9. **Burst, end-to-end half (row 10):** 12 tracked files saved back to back coalesce into
   **one** `dev.reload`.
10. **Green gate (row 11):** `make check` fully green (it is green at the base commit —
    keep it that way) and `make docs-test` 100 %, with the `dev-flags` fence executed.

# Notes

- `Cargo.toml` and `Cargo.lock` stay **byte-identical**: no new dependency, no feature, no
  workspace member. `libc`/`rustix` and `blastit-view::loaded` are already linked.
- No knob beyond `--no-reload`: no environment variable, no `Blastfile` field, no
  `BLASTIT_DEV_RELOAD=poll`, no config key, no second stat-based path (`ADR-0014`).
- Never a second watcher, never a retry, never a second event loop: one `Reload`, one poll
  duplicate, one `Source` per live process, one deadline. If the supervisor wants another
  loop, stop and file a follow-up rather than redesigning it here.
- Production, `check` and `test` never construct `dev_reload`: the module has no
  production caller, and `seccomp.rs` is untouched — this leaf widens no filter.
- **No test may assert a surgical per-route restart or an unchanged pid for another
  route.** A surviving "only this view restarted" or "this pid did not change" assertion is
  a bug: delete it. Every view pid changes on a reload; that is the contract.
- `critical_dev_reload.rs` is a **new** protected file, so creating it is allowed; it must
  carry no `#[ignore]`, no skip construct (R42) and no weakened assertion, and every row
  above must be red-capable.
- `docs/design/dev-fanotify-reload.md` is **not** in this node's paths: the design is
  frozen after attempt 1's two §4.2 corrections. A design contradiction goes to
  `## Follow-ups`, not into an edit.
- Keep `dev_learn.rs` and `crates/blastit-view/**` untouched; the preload report is the
  `0xffff` source the policy already knows.
- `make check` already runs `guard`, `rs-fmt`, `rs-lint`, `rs-doc`, `rs-test`,
  `test-critical`, `seam-check`, `docs-check`, `docs-test` and `plan validate`: a failure
  there is this leaf's, not a pre-existing one.
- If the diff wants to exceed ~650 lines across more than ~8 files, land the supervisor,
  child/zygote wiring, the boot line and rows 1–6 + 8–10, then file the remainder as a
  `## Follow-ups` child — do not widen this leaf.
- Record the base commit in `RESULT.md` and pass its hash to `critical_guard`; the
  `--grep '^\[MB.23.5.1\]'` form above resolves `54ad5d9` on this branch.
