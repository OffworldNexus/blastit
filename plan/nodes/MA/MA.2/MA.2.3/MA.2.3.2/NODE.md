---
id: MA.2.3.2
title: "Leaf 9: CLI grammar, CWD manifest discovery, presets and seccomp gate"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MA.2.2.2, MA.2.2.3, MA.2.2.4]
paths:
  - "crates/blastit-events/src/**"
  - "crates/blastit-events/tests/**"
  - crates/blastit/src/lib.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/config.rs
  - crates/blastit/tests/critical_config_and_presets.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:12"
---

# Goal

Implement the `0.1.1` command surface in `crates/blastit`: `Command::{Serve, Dev, Check}` argv parsing with exact exit codes and stdout purity, CWD-only `./blastit.toml` discovery, `app`/`preload` manifest fallback with empty-variable precedence, `pretty` default for `dev`/`check`, presets as overridable defaults, and the `BLASTIT_SECCOMP=0` preset abort — while events carry `privilege` and a `preload`-only `build` field.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §1.1, §4.2 C/E.1, §7.2 row `R1..R3, R6, R9..R10, R18, R23`, §8 Leaf 9; `ADR-0001`, `ADR-0002`, `ADR-0005`; requirements `R1`, `R2`, `R3`, `R6`, `R9`, `R10`, `R18`, `R22` (event field), `R23`.
- `cli::main(host, argv, py)` dispatch order is normative: `dev` first, then `check`/`test` (split on `--`, reject forwarded `--blastit-manifest`/`--blastit-live-server` with exit `2`), else the unchanged stage-0/stage-1/`--role=dbproxy` loop. Top-level `--help` lists `blastit`, `blastit dev`, `blastit check`, and `PRESETS`. No `--version`.
- `Lifecycle::build` is `Some(env!("CARGO_PKG_VERSION"))` **only** on the `preload` `Boot` event; zero log event carries a port. `BLASTIT_LOG_FORMAT` defaults `json` for `blastit`, `pretty` for `dev`/`check`.
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit --test critical_config_and_presets --locked
$ cargo test -p blastit-events --locked
$ make check
```

- `crates/blastit/tests/critical_config_and_presets.rs` is **new**: `./blastit.toml` loads with `BLASTIT_MANIFEST` unset (and no upward search), both presets expose their 4 values with `preset_overridden` when `BLASTIT_*`/`PORT` (including `""`) override `--preset`, `build == CARGO_PKG_VERSION` on `preload` `boot` only.
- Negative twins via a table-driven `cli_matrix`: `dev --manifest`, `dev --preset`, `dev --port 70000`, `dev --host 999.9.9.9`, `check --manifest`, `check --preset`, `check --dry-run` (without `--learn`), `test --preset`, `blastit --stage=1 --preset x`, `check -- --blastit-manifest x` each assert the exact exit code (`1` vs `2`), `stderr` text and `stdout == b""`; `BLASTIT_SECCOMP=0` with a preset exits `1`.
- Env-mutating tests hold one global `Mutex` (or the suite runs `--test-threads=1`).

# Notes

- ~380 changed lines. Do not implement the supervisor (`MA.2.3.3`) or `check.rs` (`MA.2.3.5`); parse and dispatch only.
- `blastit-events` round-trip tests must keep `json.rs`/`pretty.rs` in sync for `build` and `privilege`.
