---
id: MB.3.6.4
title: "check::serve_and_run, the 3-way check branch order and PHP learn attribution - design MB.3.6c"
kind: epic
status: ready
priority: 3
depends_on: [MB.3.6.3]
paths:
  - crates/blastit/src/cli.rs
  - crates/blastit/src/check.rs
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/tests/critical_fastcgi_learn.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T03:37"
updated: "2026-11-14T10:50"
---

# Goal

Make `blastit check` and `blastit dev --learn` honest for a route that has no Python: the
observation reader understands both line shapes, `check` picks the right branch of the
three-way gate, `blastit check -- <suite>` boots the documented `dev` server and runs a real
suite against it, and `--learn` attributes PHP routes, params and grants like every other
backend.

| Plan id | Lands | Depends |
|---|---|---|
| `MB.3.6.4.1` | two-shape `http_obs.jsonl` reader, `CheckObsV1::new_with_upstream`, combined-set guards, `executor_ran` accounting, `413` `missing_ingress`, `--learn` static proposal | `MB.3.6.3.3` |
| `MB.3.6.4.2` | 3-way branch order, `check::{DEV_ROOT, SQL_OBS_FD, POOL_PARENT_FD}`, `check::serve_and_run`, protected learn suite | `MB.3.6.4.1` |

# Context

- Design of record: `docs/design/php-fastcgi.md` §1.3, §4.10.1 (one file, two line shapes,
  one write path), §4.10.2 (`--learn` proposals), §4.10.3 (`check` reporting and
  `serve_and_run`) and §8's two rows; §8.2's `MB.3.6.4` row.
- **`.2` owns the handoff.** `<CheckRoot>/dev` via `BLASTIT_CHECK_DEV_ROOT`;
  `check::POOL_PARENT_FD` (`BLASTIT_CHECK_POOL_PARENT_FD`) threads the
  `Vec<(usize, OwnedFd)>` from `check_fastcgi_sockets` with `clear_cloexec` **immediately
  before** spawning the child `dev`; `blastit check -- <suite>` maps a non-zero suite exit
  to `3`; `executor_ran` drives the exercise count (`false` only for a pre-executor `plan`
  head `413`).
- The **three-way branch order** is (1) `is_all_static()` → `0`; (2)
  `config.app.is_none() && can_boot_without_app()` → external-executor mode (empty `--`
  exits `2` naming the first FastCGI route); (3) everything else → the existing pytest path
  with `--` forwarded **verbatim**. `docs/ci.md` must keep working verbatim.
- Neither leaf edits a `Cargo.toml` or lists `Cargo.lock`; both edit
  `crates/blastit/src/check.rs` and are serialized by `depends_on`.
- `critical_fastcgi_learn.rs` is a **new** protected file; `critical_static_cli.rs` (MIXED
  `"no interpreter to run pytest on"`) and `critical_static_manifest.rs` stay unmodified.

# Acceptance

- [ ] Both children `done` with `RESULT.md` evidence, and the epic gate confirms:
```console
$ cargo test -p blastit --test critical_fastcgi_learn --locked
$ cargo test -p blastit --locked
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
$ grep -q 'fn serve_and_run' crates/blastit/src/check.rs && echo "external-executor mode exists"
$ grep -q 'can_boot_without_app' crates/blastit/src/check.rs && echo "branch order gated on the manifest"
$ make check
```
- [ ] The page's JSON is asserted whole (`requests:3, exercised:0, unexercised_route`), one
  `/users/42` gives `requests:1, exercised:1`, and a `413` emits `missing_ingress` while
  keeping `exercised: 0`.

# Notes

- **Protected tests: add, never weaken.** A half-wired `check` that reports PHP routes as
  unexercised-but-green is worse than a scoped gap: land the branch order +
  `serve_and_run` + exit `3` first and record any residue in `RESULT.md`.
- Never write `sudo` into a documented command; `dev` must boot unprivileged.
- Do not import `smoke.rs` from a new suite: duplicate the `children()`/`fds()` helpers
  locally (precedent `critical_dev_and_fail_closed.rs:482`).
