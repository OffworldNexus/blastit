---
id: MB.56.3.2
title: "Create e2e_dev.rs (§2 & §4 local development: WSGI, PHP, static, reload, learn)"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.3.1]
paths: ["crates/blastit/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T18:45"
updated: "2026-11-20T18:45"
---

# Goal
Create `crates/blastit/tests/e2e_dev.rs` (`TESTING_PLAN.md` §2 & §4 — local development) and delete the suites it absorbs. This is the biggest suite: target ≲ 2,000 LOC.

1. Fold in: a Python WSGI multi-view app (3.13 & 3.14, `trait` grants, parameterized `$1` table/column read/write grants, ungranted SQL/column rejection, zero credentials in the worker environment).
2. Embedded PHP multi-view front controller + static mounts: PostgreSQL grants/denials, `exit()`/fatal recovery without dropping the pool, `php2shell` CGI/INI injection immunity, entry pinning.
3. Standalone Static & SPA: `immutable`, `ETag`/`304`, `Range`, SPA `fallback`, confinement blocking `..`, symlinks, dotfiles, `.php` source leaks; boots with zero Python/PHP.
4. Live reload on `.py`, `.php` and `Blastfile` edits: syntax-error resilience (last good config keeps serving), clean `SIGINT`/`SIGTERM` shutdown with no dropped in-flight request and no surviving descendants.
5. `blastit dev --learn` and `--learn --dry-run`: comment preservation, scanner-probe refusal (`/.env`, `/.git`, `..`), raw SQL literal refusal (record `$?`), the exact `learn:` lines; loopback safety (`--host 0.0.0.0` refused without `BLASTIT_DEV_EXPOSE=1`).
6. Absorb and delete: `critical_dev_reload.rs`, `critical_php_embed.rs`, `critical_php_backend.rs`, `critical_parallel_views.rs`, `critical_python_shim.rs`, `critical_raw_ingress.rs`, `critical_static_cli.rs`, `dev_signal_reap.rs`, `python_activation_gate.rs`, `embed_venv_sys_path.rs`, and the serving half of `smoke.rs`.
7. Re-cover the cases `MB.56.2.1`/`MB.56.2.2` inventoried in their `RESULT.md` (grant isolation, fair queueing, CPU accounting, `DONE`/fatal outcomes, static matrix, both `openat2`+`Walk` backends via `StaticRoot::open_with`).

# Acceptance
- `make fast-check` green, zero warnings; `cargo test -p blastit --test e2e_dev` green; every absorbed file deleted.
