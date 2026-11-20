---
id: MB.56.3.2.3
title: "e2e_dev.rs part 3: static/SPA, live reload, learn, shutdown; absorb smoke"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.56.3.2.2]
paths: ["crates/blastit/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T18:54"
updated: "2026-11-20T20:28"
---

# Goal
Extend `crates/blastit/tests/e2e_dev.rs` with the **static/SPA, live reload, `--learn` and shutdown** half of `TESTING_PLAN.md` §2 & §4, and delete the suites it absorbs.

1. Standalone Static & SPA mounts: `immutable` fingerprinted assets, `ETag`/`304`, `Range`, SPA history `fallback`, and strict confinement blocking `..`, symlinks, dotfiles (except the `.well-known` exemption) and `.php` source leaks; boots with **neither Python nor PHP** installed.
2. Live reload on `.py`, `.php` and `Blastfile` edits, with syntax-error resilience: the last valid configuration keeps serving, and the reload is visible without dropping an in-flight request.
3. Clean `SIGINT`/`SIGTERM` shutdown: exit `0`, port closed, no surviving descendants, dev root removed.
4. `blastit dev --learn` and `--learn --dry-run`: records missing views and parameterized `$?` grants, preserves comments, refuses vulnerability-scanner probes (`/.env`, `/.git`, `..`), never records a raw SQL literal, prints the exact `learn:` lines; loopback safety (`--host 0.0.0.0` refused unless `BLASTIT_DEV_EXPOSE=1`, with the warning when it is).
5. Absorb and delete: `critical_static_cli.rs`, `critical_dev_reload.rs`, `dev_signal_reap.rs`, `critical_raw_ingress.rs`, and the serving half of `smoke.rs`. Re-prove both static backends (`openat2` and `Walk` through the documented seam).
6. Final: whole file ≤ 2,000 LOC, `crates/blastit/tests/` keeps only the four `e2e_*.rs` suites (+ `MB.56.3.3`/`.4` still to come).

# Acceptance
- `make fast-check` green, zero warnings; `cargo test -p blastit --test e2e_dev` green; every absorbed file deleted.
