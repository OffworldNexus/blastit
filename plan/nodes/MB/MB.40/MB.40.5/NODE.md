---
id: MB.40.5
title: "CLI visual & UX craft overhaul: --version, clean --help, NO_COLOR/IsTerminal, http:// banner, actionable check & fmt diffs"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.40.4]
paths: ["crates/blastit/**", "crates/blastit-events/**", "python/**", Cargo.toml, Cargo.lock]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-19T00:16"
updated: "2026-11-19T00:16"
---

# Goal
Overhaul Blastit's CLI visual craft, `--help`/`--version`, `NO_COLOR`/per-stream TTY handling, `blastit dev` banner/phases/log alignment, stderr TTY boot refusals, human `blastit check` actionable fix output, and `fmt --check` / `--learn --dry-run` diffs.

# Exact Targets (`file:line` pre-diagnosed in `/tmp/opencode/audit-ux.md`)
1. **`--help`, `--version`, `help`, `--preset` validation & exit codes (`crates/blastit/src/cli.rs`, `crates/blastit/src/config/`, `python/blastit/cli.py`, `crates/blastit/tests/critical_config_and_presets.rs:556-583`, `crates/blastit/tests/smoke.rs`, `crates/blastit/tests/dx_invariance.rs`):**
   - Support `blastit --version` / `-V` (`blastit 0.2.0`) and `blastit help [dev|check|fmt|lsp]`. Update `critical_config_and_presets.rs:556-583` (and any smoke/invariance test) to assert `--version` works!
   - Hide internal re-exec flags (`--stage=1`, `--role=dbproxy`, `--*-fd=N`) from user-facing `--help` (still parsed when invoked internally by stage0!). Add clean ANSI bold/cyan section headers & command names in `--help` when stdout is a color-capable TTY.
   - Reject unknown `--preset <name>` (and `BLASTIT_PRESET=<name>`) immediately with exit `2` naming valid presets (`digitalocean-paas`, `compose-traefik`) instead of silently ignoring it (`audit-ux.md` §A10.4).
   - Unify CLI unknown-flag / usage errors across `blastit`, `dev`, `check`, `fmt`, `lsp` to exit `2` with clear messages (`unknown subcommand 'x' — run 'blastit --help'`, never `` `x` is not `--key=value` ``), and ensure Python wheel CLI delegates/matches binary `--help` identically!
2. **Per-stream `std::io::IsTerminal` + `NO_COLOR` & `TERM=dumb` (`crates/blastit/src/logs.rs:118-123`, `crates/blastit-events/src/pretty.rs`):**
   - Replace `unsafe { libc::isatty(...) }` with safe `std::io::IsTerminal` evaluated **separately** for `stdout` and `stderr`, disabling ANSI codes whenever `NO_COLOR` is set (non-empty) or `TERM == "dumb"`. Unify DEBUG palette (`dim` or `cyan` consistently).
3. **`blastit dev` banner, phase feedback, log alignment & TTY prod refusals (`crates/blastit/src/{dev_learn,stage0,stage1,supervise,logs}.rs`, `crates/blastit-events/src/pretty.rs`):**
   - Banner (`dev_learn.rs:69-83`): render clickable `http://127.0.0.1:8080` (and bracketed `http://[::1]:8080` for IPv6), show `[LEARN MODE]` badge when `--learn` is active, list declared views (`view_name  METHODS pattern`), and fix `1 route` / `1 directory` singular/plural!
   - Emit structured `reload` event instead of raw unaligned `reload: on (1 directories)` (`supervise.rs:2202-2208`), widen `NAME_COLUMN` in `pretty.rs` so event columns stay aligned, color HTTP status codes on `request` lines (`2xx` green, `3xx` cyan, `4xx` yellow, `5xx` red when color on), and emit concise phase lines before blocking `dbproxy` readiness / stage-1 app import!
   - When production `blastit` fails pre-boot (e.g. missing capabilities on a TTY), render clean human-readable error lines to **`stderr`** when `stderr` is a TTY (unless `BLASTIT_LOG_FORMAT=json` is explicitly set), killing the 9 raw JSON `app.output` lines on `stdout` (`audit-ux.md` §A10.3)!
4. **`blastit check`, `--learn --dry-run` & `fmt --check` actionable output (`crates/blastit/src/{audit_v1,check,cli}.rs`):**
   - Stop `check.rs:721` from dumping raw JSON `pg.refused` events onto pytest's interactive TTY progress line (route dbproxy events through pretty/captured sink during `check`).
   - Upgrade human `blastit check` (`audit_v1.rs:1189-1307`) to render every actionable field `--json` already computes: dead-grant `sql`, `proxy` name, observed MIME types, and exact `fix.to` (`view catalog postgres`), using `Blastfile` nouns (`view`/`trait`) and proper singular/plural (`1 request`, `1 statement`, `1 problem`)!
   - Make `blastit fmt --check` print a unified diff (`--- <path>` / `+++ <path>`) when non-canonical, and make `blastit check --learn --dry-run` print the proposed `Blastfile` block(s)!

# Acceptance
- `make fast-check`
- `make test-critical`
