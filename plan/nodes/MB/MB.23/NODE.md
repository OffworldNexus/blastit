---
id: MB.23
title: "Reload what changed in blastit dev: unprivileged fanotify + exact preload/worker loaded-file tracking, all views reload together on any change, on by default with --no-reload"
kind: epic
status: done
priority: 1
depends_on: [MB.21.5.2]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T23:12"
updated: "2026-11-17T21:09"
---

# Goal

`blastit dev` reloads what changed, by default, with **no new config key and no new
`Blastfile` field**. Save any file the app actually loaded — a view, a shared module, an
editable package, a template — and **every view worker retires together**, so two views can
never serve different versions of a shared or preloaded module; the next requests fork
fresh workers from the warm immortal parent, and the `:8080` listener and its proxies stay
up the whole time. Save a `preload` module or `./Blastfile` and the dev supervisor re-runs
the preload/manifest cycle cleanly, also without dropping `:8080`. `--no-reload` turns all
of it off (zero fds, zero marks, zero audit hooks, zero cost).

The only watcher is raw Linux **`fanotify`**, an unprivileged file-handle group
(`FAN_CLOEXEC|FAN_NONBLOCK|FAN_REPORT_DFID_NAME` = `0x0c03`, `FAN_MARK_ADD` per directory,
`name_to_handle_at(AT_HANDLE_FID)` to resolve the event's parent handle back to a path).
**`inotify` is banned outright** — no `inotify_init`, no `inotify_add_watch`, no `notify`
or `inotify` crate, nowhere in the workspace, and a permanent guard proves the built
binaries carry zero `inotify` symbols.

Attribution comes from the workers themselves: the `preload` phase plus every Python
(`sys.modules` and the `open` audit hook) and PHP (`EG(included_files)` and opened streams)
view worker reports what it loaded, over a dev-only socket. The **union** of those reports
is the exact watched file set — which is how an editable package or template outside the
working directory is caught, while `.git/`, `node_modules/`, logs and caches are skipped.
There is no per-route dependency map and no selective single-view restart: one changed
tracked file reloads all views together.

Where the kernel, the container profile or the filesystem refuses unprivileged fanotify,
`blastit dev` prints **one** structured warning (`dev.reload.unavailable`) and keeps
serving. It never crashes in a container and never silently degrades to a second watcher.

Production `blastit` and `blastit check`/`blastit test` never create a fanotify group and
never widen a seccomp filter.

Mandatory for `0.2.0`; `depends_on: [MB.21]`.

# Context

- **The research is the source of truth:** `docs/research/2026-11-16-dev-fanotify-reload.md`.
  Every kernel fact below is measured there (`6.12`, `libc 0.2.191`, unprivileged, xfs +
  tmpfs + overlayfs): the `0x0c03` init flags, the byte-identical parent-dir handle across
  `name_to_handle_at(AT_HANDLE_FID)`, `(fsid, handle_type, handle bytes)` as the only valid
  key, `hdr.len` roundup4 + trailing NUL, the 16 KiB/aligned read buffer,
  `FAN_EVENT_ON_CHILD` being immediate-children-only, `pid == 0` for events a view did not
  cause, `FAN_CLOSE_WRITE|FAN_MOVED_TO|FAN_MOVED_FROM|FAN_CREATE|FAN_DELETE|FAN_ONDIR`,
  `FAN_Q_OVERFLOW`, `FAN_MODIFY`/`FAN_RENAME`/`FAN_REPORT_TARGET_FID`/`FAN_CLASS_CONTENT`
  deliberately not requested, and the `ENOSYS`/`EPERM`/`EINVAL`/`EOPNOTSUPP`/`ENOSPC`
  ladder. Do not re-derive it; a design or brief that contradicts it needs a new
  measurement, not an opinion.
- **Loaded-file sources are measured too:** Python `__spec__.origin` (never `__file__` on
  frozen modules), the `open`-only audit hook (`path` may be an `int`, `mode` may be
  `None`, `abspath` inside the hook is banned), `.pyc → .py` via `source_from_cache`,
  Django's unconditional `cached.Loader` (a template edit needs the restart); PHP
  `EG(included_files)` snapshotted between `php_execute_script` and
  `php_request_shutdown`, compiled Twig/Blade artifacts carrying their source path, and
  the rejected stream-wrapper substitution.
- **Code sites:** `crates/blastit-sys/src/{lib,seccomp}.rs`,
  `crates/blastit/src/{supervise,child,cli,stage1,php_view,dev_learn}.rs`,
  `crates/blastit-view/src/{runtime,php}.rs`, `crates/blastit-frame/src/ctl.rs`,
  `crates/blastit-scheduler/src/{ctl,scheduler}.rs`, `crates/blastit-events/src/lib.rs`,
  `crates/blastit-php-sapi/c/blastit_php_sapi.c`, `crates/blastit-php-sapi/c/include/`.
- **No new dependency.** `libc 0.2.191` and `rustix 1.1.5` are already in `Cargo.lock`;
  `Cargo.toml` and `Cargo.lock` must stay byte-identical for this whole epic. No raw
  syscall numbers, no hand-written `fanotify` structs.
- **Docs order is not optional.** `MB.22.1` also owns `docs/local.md` and
  `docs/reference/cli.md`, so `MB.23.1` depends on it and lands after it; the two epics
  never edit those pages concurrently.
- **Two events and one word, not a new vocabulary:** `dev.reload` (info: one reload of all
  views, the `routes` it replaced, the `files` that triggered it, and `reason` ∈
  `loaded|preload|blastfile|overflow`), `dev.reload.unavailable` (warn: errno + filesystem
  + what still works), and `reload` added to the existing `process.retired` reason list.
- **One reload unit, deliberately:** the union of the `preload` phase's and every view
  worker's loaded files is the watched set; a change to any of them reloads every active
  view worker together, so two views can never run different versions of a shared or
  preloaded module. Per-route dependency maps and selective single-view restarts are
  rejected (recorded in `ADR-0014`), as is a per-route override.
- **Out of scope, deliberately:** `BLASTIT_DEV_RELOAD=poll` and any stat-based second
  path (deferred, recorded as rejected in `ADR-0014`); watching `.venv`/`node_modules`/
  framework caches as *triggers*; Node/JVM/static asset reload (a static file is read per
  request, so nothing needs restarting); non-Linux hosts; the retired manifest format and
  any PHP alternative transport (never named in this epic, in either language).

# Acceptance

```console
$ uv run --no-project tools/plan.py validate
$ uv run --no-project tools/plan.py tree --under MB.23
$ make check
$ make docs-check
$ make docs-test
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ uv run --no-project tools/guard.py
$ scripts/check-no-inotify.sh target/debug/blastit
$ cargo tree -p blastit -e normal --locked | grep -Ei '\b(notify|inotify)\b'   # no output
$ git diff --quiet -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
```

1. Every child `RESULT.md` exists, every `# Acceptance` command is recorded as run, and
   each stated exit status matches.
2. On a Flask fixture: editing `app/views/cart.py` produces exactly one `dev.reload`
   naming **every** view route, every view pid changes, and the next request returns the
   new body; editing a shared `models.py` produces the same single all-view reload and both
   routes serve the new version (never one old and one new); editing an editable package or
   template **outside** the working directory that the app loaded triggers that same
   reload; editing `./Blastfile` or a `preload` module re-reads and reloads the topology and
   re-runs the preload cycle without dropping `:8080`; an invalid intermediate `Blastfile`
   save leaves the server serving and prints the `Blastfile` error; `--no-reload` returns
   the old body and logs no `dev.reload`.
3. On a PHP fixture: editing an included `.php` reloads all views together and the next
   request returns the new body.
4. Under Docker's default seccomp profile, `blastit dev` prints exactly one
   `dev.reload.unavailable` and answers `200` for the whole run; it never crashes and
   never opens a second kind of watch.
5. `scripts/check-no-inotify.sh` reports zero `inotify_*` dynamic symbols in
   `target/debug/blastit` and in the wheel's `_blastit*.so`, zero `notify`/`inotify`
   crates in the dependency tree, and zero `inotify_init`/`inotify_add_watch`/
   `inotify_rm_watch` occurrences outside its documented allowlist.
6. `blastit check` and a production-mode boot create no fanotify group
   (`strace -f -e trace=fanotify_init,fanotify_mark` recorded by `MB.23.6`), and
   `crates/blastit-sys/src/seccomp.rs` is unchanged.
7. The docs are shipped: `docs/local.md`, `docs/reference/cli.md`,
   `docs/reference/log-events.md`, `docs/troubleshooting.md`, with `dx` and
   `ai-ergonomics` critics passed and the `--no-reload` fence executed live by
   `make docs-test`.
8. `docs/testing/AUDIT-MB.23.md` exists, the `dev_reload` policy module clears its 75 %
   mutation floor, and every open gap is filed as a follow-up node.

# Notes

- **Order:** `23.1` docs → `23.2` design/ADR → `23.3` + `23.4` (disjoint paths, parallel)
  → `23.5` supervisor wiring → `23.6` audit. Nothing in `crates/` starts before `23.2` is
  accepted, and nothing in `docs/` starts before `23.1` is.
- **`target/` discipline:** `MB.21.5.3` owns `target/` until it closes. No task here runs
  `cargo` or `make check` before `MB.21` is `done`; the epic is blocked, not "mostly
  ready". `MB.23.3` and `MB.23.4` never build the workspace at the same time as each other.
- **Parallel safety:** `23.3` (`blastit-sys` + `scripts/` + `Makefile`) and `23.4`
  (`blastit-view` + `blastit-php-sapi` + `stage1.rs`/`php_view.rs`) share no file. `23.5`
  owns the frame/scheduler/supervisor/CLI/events surfaces and lands after both.
- **Language rules for every page, brief and comment in this epic:** no `inotify` word on
  a public page; never mention the retired manifest filename; PHP is simply another
  backend that reloads, and no second transport is ever named; no kernel internals on a
  user page (`docs/local.md`, `reference/`, `troubleshooting.md`).
- **Seams are for tests, not for production paths.** The watcher and the clock are injected so the
  reload policy is tested deterministically with zero sleeps; there is exactly one real
  watcher and exactly one production path.
