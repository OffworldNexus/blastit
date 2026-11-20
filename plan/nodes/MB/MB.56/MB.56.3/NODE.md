---
id: MB.56.3
title: "Consolidate crates/blastit/tests/ (30+ files -> 4 crisp user-story E2E suites)"
kind: epic
status: active
priority: 1
depends_on: [MB.56.2]
paths: ["crates/blastit/tests/**", Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T16:02"
updated: "2026-11-20T18:45"
---

# Goal
Consolidate the ~30 fragmented `crates/blastit/tests/*.rs` files (`~28k` LOC) into **4 crisp, high-signal end-to-end suites (`~4,000–5,500` LOC total)** driving the real `blastit` binary and mapping 1-to-1 to `TESTING_PLAN.md` §3–§6:

1. **`crates/blastit/tests/e2e_cli.rs` (`TESTING_PLAN.md` §3 CLI & Handoff):**
   - `--version`, `help`, hidden internal `--stage`/`--role` flags, unknown `--preset` exit `2`, `NO_COLOR`/`TERM=dumb`.
   - Safe local CLI handoff (`dev`, `check`, `fmt`, `lsp` delegating to `.venv/bin/blastit`, `vendor/bin/blastit`, `node_modules/.bin/blastit`; skipping unsafe ambient `$VIRTUAL_ENV`; refusing unsafe `BLASTIT_BIN` or wrong-uid binaries; never handing off on production `blastit`).

2. **`crates/blastit/tests/e2e_dev.rs` (`TESTING_PLAN.md` §2 & §4 Local Development):**
   - Python WSGI multi-view app (3.13 & 3.14, `trait` grants, parameterized `$1` table/column read/write grants, ungranted SQL/column rejection, zero credentials in worker env).
   - Embedded PHP multi-view front controller + static files (PHP `exit()`/fatal recovery without dropping worker pool, PostgreSQL grants/denials, `php2shell` CGI/INI injection immunity).
   - Standalone Static & SPA mounts (`immutable`, `ETag`/`304`, `Range`, SPA `fallback`, strict confinement blocking `..`, symlinks, dotfiles, and `.php` source leaks, booting with zero Python/PHP).
   - Live reload on `.py`, `.php`, and `Blastfile` edits (syntax-error resilience keeping last good config serving; clean `SIGINT`/`SIGTERM` shutdown).
   - Single-pass `blastit dev --learn` & `--learn --dry-run` (comment preservation, scanner probe `/.env`/`/.git`/`..` rejection, raw SQL literal refusal) + loopback safety (`--host 0.0.0.0` refused without `BLASTIT_DEV_EXPOSE=1`).

3. **`crates/blastit/tests/e2e_check_fmt.rs` (`TESTING_PLAN.md` §5 CI/CD & Formatting):**
   - `blastit check` & `blastit check --json` clean pass (exit `0` on full WSGI + Postgres suite, DB-free suite, and static-only `Blastfile`).
   - Actionable human + `--json` parity (exit `1`) on missing grants, dead grants (`sql`, `proxy`, `view <name> postgres`), unrouted requests, unexercised views, and missing `body` policies.
   - `blastit check --learn` bootstrapping new views without widening existing views.
   - `blastit fmt` idempotent comment-preserving formatting & `blastit fmt --check` unified diff on unformatted files.

4. **`crates/blastit/tests/e2e_prod_refusals.rs` (`TESTING_PLAN.md` §6 Fail-Closed Startup):**
   - Production `blastit` failing closed before serving on: missing OS isolation privileges (unprivileged caller), group/world-writable binaries or parent directories, symlink/FIFO `Blastfile`, tampered entry/shim descriptors, unknown presets, and `body raw` without `unvalidated`.

Delete all superseded `crates/blastit/tests/*.rs` files (`smoke.rs`, `critical_*.rs`, `dx_*.rs`, `common_harness_parity.rs`, etc.) once their user-story coverage lives cleanly in these 4 files!

# Acceptance
- `make fast-check` green, `crates/blastit/tests/` holds only the 4 consolidated E2E suites (+ shared `common/` helper if needed, `<= 5,500` LOC total)
