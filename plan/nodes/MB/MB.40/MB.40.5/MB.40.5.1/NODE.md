---
id: MB.40.5.1
title: "dev banner, learn badge, structured reload event, aligned columns, phase lines & clean stderr refusals"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit/src/**"
  - "crates/blastit-events/src/**"
  - crates/blastit/tests/critical_dev_and_fail_closed.rs
  - crates/blastit/tests/critical_static_cli.rs
  - crates/blastit/tests/critical_dev_reload.rs
  - "python/**"
  - e2e/dx_replay.py
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-19T04:27"
updated: "2026-11-19T04:54"
---

# Goal
Make `blastit dev` tell the truth at a glance: a clickable `http://host:port` banner (bracketed for IPv6), a `[LEARN MODE]` badge, the declared view table, a structured `reload` event instead of a free-text line, aligned event columns, coloured HTTP status on `request` lines, phase lines before every blocking wait, and clean human refusals on stderr — with the three protected banner/reload assertions converged to the new, charter-mandated shape.

# Context
`MB.40.5` group 3, split out because three protected tests pin the *old* banner/reload format that the Developer-Experience charter (`AGENTS.md` §4: "Print clickable `http://host:port` … `--learn` badge, route table, phase lines before waits, fixed-width aligned event columns") explicitly replaces. Those tests are not defects; they encode the pre-charter shape and must be re-pinned to the new shape **without losing what they prove**.

# Exact Targets (`file:line` pre-diagnosed in `/tmp/opencode/audit-ux.md`)
- `crates/blastit/src/stage0/dev.rs` / `crates/blastit/src/cli/run.rs` (the dev boot that prints the banner): render `http://127.0.0.1:<port>` (clickable) and `http://[::1]:<port>` for IPv6, a `[LEARN MODE]` badge when `--learn` is active, and one line per declared view (`view  METHODS pattern`); fix `1 route`/`1 directory` singular/plural.
- `crates/blastit/src/logs.rs` + `crates/blastit-events/src/pretty.rs`: emit a structured `reload` event instead of `reload: on (1 directories)` (`supervise.rs`), widen `NAME_COLUMN` so columns stay aligned at the longest event name, and colour HTTP status codes on `request` lines (2xx green, 3xx cyan, 4xx yellow, 5xx red) only when `Colour` says colour is on.
- Phase lines: emit a concise line before the blocking `dbproxy` readiness wait and before the stage-1 app import, so a slow startup never looks hung.
- Production pre-boot refusals: when `stderr` is a TTY (and `BLASTIT_LOG_FORMAT` is not `json`), render clean human lines on stderr instead of nine raw JSON `app.output` lines on stdout (`audit-ux.md` §A10.3).
- `crates/blastit/src/check.rs:721`: stop dumping raw JSON `pg.refused` events onto pytest's interactive progress line during `check`.

# Authorization (protected critical tests — exactly these three)
1. `crates/blastit/tests/critical_dev_and_fail_closed.rs:1224-1238` — the `banner.len() == 3` and exact banner-line assertions may be re-pinned to the new banner (host line with a clickable URL, learn badge when `--learn`, view table). Preserve the intent: the banner is *informational, printed once, before serving, and never an event*.
2. `crates/blastit/tests/critical_static_cli.rs:290-298` — the same three exact strings (`http · scheduler · dbproxy · 0 routes`) may be re-pinned to the new banner shape, preserving the same intent and still asserting the component list is complete (`http`, `scheduler`, `dbproxy`) and the route count is right.
3. `crates/blastit/tests/critical_dev_reload.rs:1619-1652` — `reload: on (N directories)` may be re-pinned to the structured `reload` event, preserving the intent: exactly one reload report per reload, it appears once (no duplicate from the writer and the emitter), it is aligned with the other event columns, and it never leaks as a raw `app.output` line on stdout.

Every other assertion in those three files must stay untouched: no test renamed, deleted, `#[ignore]`d, no count changed, no other line edited. `uv run --no-project tools/critical_guard.py --base 7caf56a --node MB.40.5.1` must pass.

# Acceptance
- `make fast-check`
- `make test-critical`
- `uv run --no-project tools/critical_guard.py --base 7caf56a --node MB.40.5.1`
- Evidence in RESULT.md: captured real output (pty and piped) for the banner, a reload, an aligned event table, an IPv6 banner, `--learn`, and a production refusal on stderr; plus proof the three protected files still carry the same test names and counts.

# Notes
Single Cargo writer. Colour comes from the existing typed `logs::Colour` — never re-probe TTY/env here. Files <500 prod LOC.
