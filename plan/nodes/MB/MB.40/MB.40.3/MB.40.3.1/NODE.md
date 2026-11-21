---
id: MB.40.3.1
title: "ObsVerdict -> sqlpolicy LearnVerdict (wire serde adapter)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-sqlpolicy/src/learn.rs"
  - "crates/blastit-dbproxy/src/learn.rs"
  - "crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs"
  - crates/blastit/src/check.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/audit_v1.rs
  - Cargo.toml
  - Cargo.lock
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T01:45"
updated: "2026-11-19T02:03"
---

# Goal
Delete MB.40.3's remaining `ObsVerdict` wire fork: the dbproxy's local verdict enum becomes `blastit_sqlpolicy::LearnVerdict` behind exactly one serde adapter, with the `WireSqlObs.verdict` JSON contract byte-identical, plus the one protected critical-test assertion update authorized below.

(The `RequestIo` -> `ChanIo` seam removal originally planned beside this item moved to `MB.40.3.4`, because it is pinned by the ADR-0016 exemption in `crates/blastit/tests/critical_artifacts.rs` and needs its own authorization.)

# Exact Targets (`file:line` pre-diagnosed — quoted from MB.40.3/RESULT.md §Follow-ups 1)
**Item 1 — `ObsVerdict` → `LearnVerdict`:**
- `crates/blastit-dbproxy/src/learn.rs:104-175` owns `ObsVerdict` (tag `kind`, `rename_all = "snake_case"`, `query{normalized}` / `refused{redacted,reason:String}`) plus `is_query`/`normalized`/`redacted`/`reason` and `impl From<LearnVerdict>`. RESULT.md: "`LearnVerdict`/`RefuseReason` carry no `serde`, while `WireSqlObs.verdict` is the JSON contract (`{"kind":"query"|"refused",…}`) read by `check`/`audit` and Python, so the slice must add a `#[serde(with = …)]` adapter in `blastit-dbproxy` (plus a `RefuseReason::Reported(String)`-style variant so a refused wire record round-trips its one-line reason exactly)".
- `crates/blastit-sqlpolicy/src/learn.rs:84` (`pub enum RefuseReason`, currently `Unparseable`/`MultipleStatements`/`InlineLiteral`/`RedactedPlaceholder`/`DangerousFunction`/`CopyProgram`) and `:174-224` (`LearnVerdict`, `reason() -> Option<&RefuseReason>`, non-exhaustive).
- Consumers to switch: `crates/blastit/src/dev_learn.rs:373`, `crates/blastit/src/audit_v1.rs:745,1531,1557`, `crates/blastit/src/check.rs:1247` (the `blastit-dbproxy` copies named in the original plan do not exist — the consumers are in the `blastit` crate).
- Wire contract must stay byte-identical for `WireSqlObs` (`--json` output of `blastit check`/audit, and the Python readers): same tag `kind`, same field names `normalized`/`redacted`/`reason`, `reason` still a one-line redacted string. Add a round-trip serde test proving `{"kind":"refused","redacted":"…","reason":"…"}` serialises and deserialises through the adapter, and that a `Reported(String)` reason round-trips exactly.

# Authorization (protected critical tests)
Edits to `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` are authorized as an assertion-preserving adaptation of `:782-789` only: `verdict.reason().contains("literal")` becomes the `Option<&RefuseReason>` shape (e.g. `matches!(verdict.reason(), Some(RefuseReason::InlineLiteral))`), which must still assert the same refusal; this is never a weakening, an added `#[ignore]`, or a deletion, and no other line of that file may change.

# Acceptance
- `make fast-check`
- `cargo test -p blastit-sqlpolicy --locked`
- `cargo test -p blastit-dbproxy --locked` (includes the protected `critical_learn_and_redaction`)
- `cargo test -p blastit --locked --lib`
- `uv run --no-project tools/critical_guard.py --base e6b1291 --node MB.40.3.1`
- Evidence in RESULT.md: `grep -rn "ObsVerdict" crates/` returns nothing; one quoted `--json` learn record before/after the change is identical.

# Notes
Single Cargo writer: run the gates above serially, never in parallel with another slice. No new dependency unless the serde adapter needs one; if it does, `Cargo.toml` + `Cargo.lock` move in this same commit.
