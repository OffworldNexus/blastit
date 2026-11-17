---
id: MB.23.5.2.2
title: "Reload the preload cycle and the Blastfile without dropping :8080: the adoption document, the exec, stage-1 adoption and the two debug seams"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.23.5.2.1]
paths:
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/child.rs
  - crates/blastit/src/dev_reload.rs
  - crates/blastit/tests/critical_dev_reload.rs
  - "crates/blastit/tests/common/**"
  - Makefile
  - "scripts/check-no-test-seam.sh"
  - python/tests/runnable_docs/inventory.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:45"
updated: "2026-11-17T13:45"
---

# Goal

Land design §8's commit **5b-ii**: a `Blastfile` / `preload` / `overflow` change re-runs
the preload cycle on a fresh interpreter image with `:8080` held open — validate first,
then `execve` the same stage-1 prefix with one adoption document; stage 1 adopts that
document (no bind, no `http`/dbproxy fork, a new scheduler on the inherited `http.chan`
end); stage 0's stage-0-only restart variable carries the restart argv once and is stripped
from views; `Policy::reconcile` closes the no-watcher gap; `--learn` skips the manifest
cycle; the debug-only `BLASTIT_TEST_RELOAD_PAUSE` seam exists beside
`BLASTIT_TEST_SWAP_POINT`; and `critical_dev_reload.rs` gains rows 7 and the ladder row.

# Context

- **Depends on `MB.23.5.2.1`:** the supervisor already owns one dev-only `Reload`, its
  armed deadline, the report sockets, the boot line and rows 1–6/8–10. This leaf adds the
  cycle on top; it must not restructure the poll loop. `dev_reload.rs` already carries
  `Policy::reconcile(since, now)` and `Fire::needs_cycle()` — **call them, do not
  reimplement them**.
- **`supervise.rs` — the cycle, validate-then-`execve` (§4.6).** `Fire::needs_cycle()` →
  (0) under `--learn` emit one `manifest.warning` ("the Blastfile changed; restart
  `blastit dev --learn` to apply it") and keep the previous topology serving; `loaded`
  changes still reload views. (1) Validate before touching anything: `read_manifest` +
  `parse_blastfile`, and for a `preload` reason `compile(source, path, "exec")` per
  changed module. An error emits **one** `manifest.warning` with the file's own
  `<path>:<line>:<col>: …` sentence, no `dev.reload`, no retire, no exec, and the floor
  restarts from that moment. (2) One `dev.reload` with the **new** manifest's routes.
  (3) One `Ctl::Reload`, then `reloading = true`; wait for `views.is_empty()` as a state,
  never a sleep. (4) `kill(sched_pid, SIGTERM)` **only the scheduler**; while `reloading`
  is set, `component_exit` treats the scheduler's `EXITED` as expected and `ctl_closed`
  does not fail — one flag for both, and no error-level `process.exited` / `shutdown`
  event appears during a reload. (5) The debug-only `BLASTIT_TEST_RELOAD_PAUSE` rendezvous
  (`<path>.ready` / bounded `<path>.go`), compiled under `debug_assertions` only.
  (6) `execve` the stage-1 prefix this image was started with — `/proc/self/cmdline` up to
  the first `--stage=1` token, at most 8 tokens, re-checked non-empty, **not**
  `/proc/self/exe` — then `--stage=1`, `--config-fd=N` re-supplying the exact configuration
  bytes this image received, `--dev-root`, `--dev-host`, `--dev-reload=on`,
  `--dev-reload-since=<unix nanos>` (the instant the cycle began) and the adoption
  document on `--dev-adopt-fd`. `FD_CLOEXEC` is cleared on exactly the configuration pipe,
  the adoption pipe, the policy pipe when present, and every descriptor the document
  names. An `execve` that returns prints and `_exit(1)` — a half-started reload must never
  keep serving. The listening socket is **not** among those descriptors: `http` owns it.
- **The adoption document** (§4.6 step 6), UTF-8 JSON, ≤ 4096 bytes, unknown/missing key or
  an unopened descriptor refuses the boot:
  `{"version":1,"http":{"pid":…,"stdout":…,"stderr":…,"events":…},"chan":…,`
  `"dbproxy":{"pid":…,"events":…,"stdout":…,"stderr":…},"learn":{"obs":…,"ctl":…},`
  `"policy":…}` — `learn` present exactly when the boot had `--learn`, `policy` exactly
  when reload is on.
- **`stage1.rs` — adopt it.** No bind (`:8080` is already `http`'s), no `http`/dbproxy
  fork, a new scheduler on the inherited `http.chan` end, the new manifest's private view
  directories, `Learn::adopt(obs, ctl)` when the document carries `learn` (dev_learn.rs is
  **unchanged**), one `Policy::reconcile(reload_since, now)` **after** the boot report,
  then the usual import/preload/warmup/immortalize and no banner / no `boot` / no `ready`
  repeat. Its own group is created fresh (the old one died with the image).
- **`stage0.rs` — hand the argv over once.** A stage-0-only restart variable carries the
  restart argv to the `preload`/`Blastfile` cycle and is stripped from every view's
  environment by the existing `BLASTIT_*` strip; the listening fd is handed over **once**,
  never re-bound. The dev policy socketpair and `--dbproxy-policy-fd` are
  `MB.23.5.3`'s; if this leaf parses `--dev-policy-fd` it must not claim the swap.
- **`cli.rs` — internal tokens only.** `--dev-reload=on|off`, `--dev-adopt-fd`,
  `--dev-policy-fd`, `--dev-reload-since`, rejected unless `--stage=1` + `--dev-root` by
  the existing `any_dev` gate, **never** in `--help`. `--no-reload` stays the only
  user-facing knob.
- `Makefile`, `scripts/check-no-test-seam.sh` and `python/tests/runnable_docs/inventory.toml`:
  only `BLASTIT_TEST_RELOAD_PAUSE` is declared beside `BLASTIT_TEST_SWAP_POINT`, and only
  because T31's test drives it. Do not claim `BLASTIT_TEST_RELOAD_FAULT` if no code reads
  it — an unread seam is a dead name in a shipped-image guard.

# Acceptance

```console
$ cargo test -p blastit --locked --features python --test critical_dev_reload
$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
$ cargo fmt --all -- --check
$ cargo check -p blastit --release --locked
$ make check
$ make docs-test
$ uv run --no-project tools/critical_guard.py --base $(git log --format=%H -1 --grep '^\[MB.23.5.2.1\]') --node MB.23.5.2.2
$ git diff --quiet -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs && echo LOCK_AND_SECCOMP_IDENTICAL
```

1. **Blastfile mid-edit (row 7a):** an invalid intermediate `Blastfile` save logs exactly
   one `Blastfile:<line>:<col>` sentence, emits no `dev.reload`, and the server keeps
   answering `200`.
2. **Blastfile valid (row 7b):** the next valid save reloads the topology exactly once
   (one `dev.reload` with the new routes), and a keep-alive connection opened **before**
   the save still gets its response on `:8080`.
3. **Pids (row 7c):** across that cycle the supervisor, `http` and dbproxy pids are
   unchanged while the scheduler pid changed; the listener never dropped. No assertion pins
   a *view* pid as unchanged, and none names a per-route restart.
4. **`preload` (row 7d):** editing a `preload` module re-runs the preload cycle exactly
   once; a half-written module keeps the old preload serving with one warning.
5. **Ladder (row 8):** a fake watcher returning `Unavailable { EPERM }` yields exactly one
   `dev.reload.unavailable` and `200` for the rest of the run — no retry, no second
   watcher, no reload.
6. **`--learn`:** a `Blastfile` save under `--learn` emits one `manifest.warning`, runs no
   exec, and still reloads views for a `loaded` change.
7. **Release image:** `cargo check --release` is warning-free and the pause seam exists
   only under `debug_assertions` (`make seam-check` green in both directions).
8. **Green gate:** `make check` fully green (it is green at the base commit) and
   `make docs-test` 100 %.

# Notes

- `Cargo.toml` and `Cargo.lock` stay **byte-identical**: no new dependency, no feature, no
  workspace member (`libc`/`rustix` already cover every descriptor and `execve`).
- No knob beyond `--no-reload`: no environment variable, no `Blastfile` field, no config
  key. `--dev-reload=on|off` is internal stage-1 argv, rejected outside `--stage=1` +
  `--dev-root`, invisible in `--help`.
- Never a second watcher, never a retried boot, never a second event loop. The reload
  re-creates exactly one group in the new image; the old one died with the previous one.
- Production, `check` and `test` never construct `dev_reload`, never see an adoption
  document and never read a restart variable; `seccomp.rs` is untouched and no filter is
  widened for `execve`.
- **No test may assert a surgical per-route restart or an unchanged pid for another
  route.** "Only one view restarted" and "this pid did not change" are bugs: delete them.
  Every view pid changes; the *non-view* pids (supervisor, `http`, dbproxy) are the ones
  that must not.
- `critical_dev_reload.rs` is protected: adding rows is allowed, weakening one is not — no
  `#[ignore]`, no skip construct (R42), every row red-capable.
- The invalid-`Blastfile` and keep-alive legs need real processes; a bounded poll loop with
  a deadline is allowed for startup only, never to decide a debounce verdict.
- Do not implement the dbproxy policy refresh: `Learning::replace_route_policies`, the
  length-prefixed document and the one-byte ack are `MB.23.5.3`'s. This leaf must not
  invent a substitute path or a warning about grants it cannot apply.
- `dev_learn.rs` and `crates/blastit-view/**` stay untouched; the reload image calls the
  existing `Learn::adopt` with the adopted descriptors.
- `php_view.rs` remains out of scope: pass a PHP report through `PhpViewSpawn` only if no
  edit there is needed, else `## Follow-ups` for `MB.23.5.3`.
- Record the base commit in `RESULT.md` and pass its hash to `critical_guard`; the
  `--grep '^\[MB.23.5.2.1\]'` form above resolves that commit on this branch.
- If the cycle cannot land green without touching dbproxy or `php_view.rs`, land rows 7a/7b
  plus the adoption document and file the rest as a `## Follow-ups` child — do not widen
  this leaf.
