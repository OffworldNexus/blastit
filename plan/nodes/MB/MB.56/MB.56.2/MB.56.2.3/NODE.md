---
id: MB.56.2.3
title: "Consolidate blastit-manifest §7 suite (15k -> <=1.5k LOC) incl. every docs/README Blastfile fence"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.56.2.2]
paths: ["crates/blastit-manifest/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-20T17:15"
updated: "2026-11-20T17:15"
---

# Goal
Collapse `crates/blastit-manifest`'s **15,017 test LOC across 30+ files** into **1–2 clean files totalling ≤ 1,500 LOC** — the `TESTING_PLAN.md` §7 "`Blastfile` language & surgical editing" suite.

1. Delete the fragmented suites (`critical_manifest_011.rs` retired TOML, `mutation_gaps.rs`, `edit_branches.rs`, the grant/route/trait/formatting/lexer splits, and every inline `#[cfg(test)]` in `src/**`).
2. Write `crates/blastit-manifest/tests/blastfile_language.rs` covering, against the real parser/formatter:
   - grammar parsing of the documented `Blastfile` surface (`app`/`entrypoint`/`postgres`/`view`/`static`/`trait`, `use`, grants, `body`, `immutable`, `regex '…'`, `"/*"`);
   - clear **line:column** diagnostics for malformed input;
   - route precedence (`exact`, prefix, `regex`) and trait composition;
   - comment-preserving, idempotent formatting (`fmt` twice = no change) and lossless grant insertion;
   - **every `Blastfile` fence in `README.md` and `docs/**` parses and survives `fmt --check`** (the docs are the contract: a broken example must fail this suite).
3. Allowed+denied pairs where the contract has a refusal; no vacuous asserts; no fixture duplication.

# Acceptance
- `make fast-check` green, zero warnings; `crates/blastit-manifest/tests/` ≤ 1,500 LOC in ≤ 2 files.
