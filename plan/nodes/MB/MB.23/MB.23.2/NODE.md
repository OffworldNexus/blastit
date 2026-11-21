---
id: MB.23.2
title: "Design the fanotify reload: watcher + clock seams, handle table, debounce, worker datagram, Ctl::Reload (ADR-0014) with security/implementability/testability/consistency critics"
kind: task
status: done
owner: designer
priority: 1
depends_on: [MB.23.1]
paths:
  - "docs/design/dev-fanotify-reload.md"
  - "docs/design/dev-fanotify-reload.review-security.md"
  - "docs/design/dev-fanotify-reload.review-implementability.md"
  - "docs/design/dev-fanotify-reload.review-testability.md"
  - "docs/design/dev-fanotify-reload.review-consistency.md"
  - "docs/decisions/ADR-0014-dev-fanotify-live-reload.md"
  - docs/decisions/README.md
  - docs/design/README.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T23:12"
updated: "2026-11-17T06:13"
---

# Goal

Translate the accepted DX from `MB.23.1` into one technical design and one ADR, precise
enough that `MB.23.3`–`MB.23.5` can be written without a second design decision:

- `docs/design/dev-fanotify-reload.md` — module boundaries, the two test seams, the wire
  format, the watched-set union, the all-worker reload transaction, the preload/manifest
  re-run that keeps `:8080`, the policy state machine, the failure ladder, budgets, and the
  security invariants.
- `docs/decisions/ADR-0014-dev-fanotify-live-reload.md` — the decisions and the rejected
  alternatives.
- Four goldfish reviews: `dev-fanotify-reload.review-{security,implementability,testability,consistency}.md`.

`ADR-0013` belongs to `MB.22.2`; use `0014` and never renumber, even if `0013` has not
landed yet.

# Context

- Ground truth is `docs/research/2026-11-16-dev-fanotify-reload.md`. The design **cites**
  it by section and **must not** change its measured constants: init flags `0x0c03`,
  `O_RDONLY`; mark mask `FAN_CLOSE_WRITE|FAN_MOVED_TO|FAN_MOVED_FROM|FAN_CREATE|FAN_DELETE|FAN_ONDIR|FAN_EVENT_ON_CHILD`;
  read buffer 16 KiB `#[repr(C, align(8))]`; name area `hdr.len - 20 - handle_bytes`,
  stop at NUL; handle key `(fsid [u32;2], handle_type i32, handle bytes)`; `AT_HANDLE_FID`
  with `flags = 0` fallback; debounce 50 ms quiet / 250 ms max / 500 ms global floor
  between two reloads; 64-path chunks; 4096-byte path cap; skip list `.git .hg .svn
  node_modules .venv venv __pycache__ .mypy_cache .pytest_cache .ruff_cache .tox target`;
  framework cache dirs marked but excluded from triggers.
- `MB.23.1` froze the observable surface: events `dev.reload` (`routes`, `files`,
  `reason` ∈ `loaded|preload|blastfile|overflow`) and `dev.reload.unavailable` (`errno`,
  `filesystem`), plus the `reload` value in `process.retired`'s `reason`. The design
  implements exactly those names.
- **One reload unit, no route map.** The supervisor unions the `preload` phase's and every
  view worker's reported files into one exact watched set (parent directories get marked;
  a path outside the working directory is still watched because it was loaded). A change
  to any tracked loaded file sends one `Ctl::Reload` with **no route payload**; the
  scheduler retires every active view's idle workers with reason `reload` so the next
  request forks from the warm immortal parent. The `:8080` listener and the proxies are
  never retired or re-bound. The design must also specify how a `preload` module or
  `./Blastfile` change re-runs the preload/manifest cycle (stage 0 hands the listening fd
  over once) **without** closing the listener, and how a worker mid-request is never
  `SIGKILL`ed. A rejected alternative is a per-route dependency map with selective
  single-view restarts: it lets two views run different versions of a shared or preloaded
  module and needs extra bookkeeping for files loaded before the fork.
- The design must specify **two seams** and nothing more:
  1. `trait ReloadWatcher { fn mark_dir(&mut self, dir: &Path) -> io::Result<()>;
     fn read(&mut self, out: &mut Vec<Event>) -> io::Result<Readout>; }` with the real
     implementation `blastit_sys::fanotify::Watcher` and a test fake. There is one real
     implementation; the trait exists so the policy is tested without a kernel quirk.
  2. an injected clock (`Instant` passed in, or a `trait Clock`), so debounce and the
     global reload floor are asserted exactly, with zero `sleep` in the policy tests.
- Security invariants to state and defend: reload is constructed only on the `dev`
  supervisor path; the reporting socket is a dev-only `AF_UNIX SOCK_DGRAM` handed through
  the loader scope like `--dev-obs-fd` and stripped from views' environment by stage 0;
  no `BLASTIT_*` variable is introduced; no seccomp filter is widened (production keeps
  its refusal list as it is); no `open_by_handle_at`; no path from an event is trusted as
  UTF-8 or joined without a bound; no logging of file contents, only counts and names
  already inside the app root; every failure degrades to "serve without reload".
- Name the exact new/changed files so each leaf's `paths` is checkable:
  `crates/blastit-sys/src/fanotify.rs`, `crates/blastit-view/src/loaded.rs`,
  `crates/blastit/src/dev_reload.rs`, `crates/blastit-frame/src/ctl.rs` (`0x45`),
  `crates/blastit-scheduler/src/{ctl,scheduler}.rs`,
  `crates/blastit-php-sapi/c/blastit_php_sapi.c` (the `dlsym`-resolved sink).
- Record the rejected alternatives: `inotify` (owner ban: there is one watcher and no
  second one), per-route dependency maps + selective single-view restarts (mixed module
  versions across views; preload files belong to no route), non-FID group
  (`EPERM` unprivileged), `FAN_MARK_MOUNT`/`FILESYSTEM` (`EPERM`), `event->fd` +
  `/proc/self/fd`, replacing PHP's plain file wrapper (`php_resolve_path` compares wrapper
  identity; `open_basedir` lives inside the plain opener), `FAN_REPORT_FD_ERROR`, and the
  deferred stat-poll env var (not in `0.2.0`; one honest path plus a warning).
- Testability table (one row per planned test, naming its file): CloseWrite coalescing,
  `FAN_MODIFY` carried-but-not-requested, atomic `rename` = one all-view reload, overlayfs
  `AT_HANDLE_FID` handle equality (a regression to `flags = 0` fails with `EOPNOTSUPP`),
  tmpfs handle equality, `Unavailable` errno mapping, reload-during-reload (a worker's own
  cache write causes zero further reloads; one reload per quiet window), `--no-reload` =
  zero fds, invalid intermediate `Blastfile` survives, a `preload` module edit re-runs the
  preload cycle once with the listener still accepting, a shared module edit leaves no
  view on the old version, Python `.pyc → .py`, PHP `EG(included_files)` across
  8.3/8.4/8.5, and the "an audit hook that raises never aborts the app's `open`" case.

# Acceptance

```console
$ test -s docs/design/dev-fanotify-reload.md && test -s docs/decisions/ADR-0014-dev-fanotify-live-reload.md
$ for lens in security implementability testability consistency; do tail -2 "docs/design/dev-fanotify-reload.review-$lens.md"; done
$ grep -c "^## " docs/design/dev-fanotify-reload.md
$ grep -nE "0x0c03|AT_HANDLE_FID|50 ms|250 ms|500 ms|64-path|16 KiB" docs/design/dev-fanotify-reload.md
$ grep -nE "Ctl::Reload|all (active )?view|preload cycle|listener" docs/design/dev-fanotify-reload.md
$ grep -rniE "per-route (floor|dependency|selection)" docs/design/dev-fanotify-reload.md   # only the rejected-alternative row
$ grep -rn "inotify" docs/decisions/ADR-0014-dev-fanotify-live-reload.md   # only the "rejected" row
$ make docs-check
$ uv run --no-project tools/guard.py
$ uv run --no-project tools/plan.py validate
```

1. The design doc names every file in the epic's `paths` map, every frozen constant, both
   seams, the datagram protocol byte layout, the union watched set and all-worker reload
   transaction (`Ctl::Reload` with no route payload; preload/manifest re-run with the
   listener held open), the failure ladder with the one warning sentence, and the security
   invariants; a reader can write each leaf's diff from it.
2. Each of the four reviews ends `VERDICT: PASS` with no BLOCKER and no MAJOR finding;
   findings that survive as MINOR are listed and either fixed or filed as follow-ups.
3. The ADR records `inotify` only as a rejected alternative (one row), states the kernel
   and filesystem floor (`5.13` for the FID group, `6.5` for `AT_HANDLE_FID`, older
   kernels fall back to `flags = 0`), and states what happens when neither works.
4. `docs-check`, `guard.py` and `validate` are green; the design doc and ADR are excluded
   from the public site like every other `docs/design`/`docs/decisions` file.

# Notes

- The `security` critic must be able to answer "can a view widen its own reload reach?"
  from the design alone — say which process holds the fanotify fd (the supervisor, never a
  view), and which process can write the reporting socket.
- The design's "budgets" section is what `MB.23.6` audits: path count, path length, batch
  size, marks per group, and the one-group rule.
- The design must be able to answer "why not reload just that view?" in one paragraph:
  two views on different versions of a shared `models.py`, and `preload` modules that no
  route owns, are the two failure modes the all-worker unit removes.
- Do not design a `--watch <dir>` flag, a config file, a per-route override, or any
  "restart one view" escape hatch. One default and one off switch is the whole surface
  `MB.23.1` documented.
