---
id: MB.55
title: "Enforce TESTING_PLAN.md human gate, E2E/oracle review & mandatory uncovered-code coverage audit across .opencode/ and AGENTS.md"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [AGENTS.md, Makefile, "tools/**", ".opencode/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T15:49"
updated: "2026-11-20T15:57"
---

# Goal
Update the local agentic setup (`AGENTS.md`, `.opencode/agents/*.md`, `.opencode/skills/*/SKILL.md`) and add a fast `make coverage` (`tools/coverage_report.py`) helper so the autonomous harness strictly enforces `TESTING_PLAN.md`:

1. **Human Validation Gate on `TESTING_PLAN.md` for New Features:**
   - Update `AGENTS.md`, `.opencode/agents/{conductor,strategist,architect,planner,lead,writer}.md`, and `.opencode/skills/{dx-first,plan-system,testing}/SKILL.md`:
     * Whenever a new user-facing feature, runtime, proxy, or workflow is introduced, the first step (alongside user-facing DX docs) is proposing the pure user-story update to `TESTING_PLAN.md` (zero file/crate/code references!) and **blocking on human validation** (`uv run --no-project tools/plan.py --agent <role> human add "Validate TESTING_PLAN.md update for <feature>" --node <id>` + `set <id> blocked`, with `conductor` presenting it via `question`).
     * No tests or production code for a new feature may proceed until the human validates the `TESTING_PLAN.md` update, and no coding slice may silently alter `TESTING_PLAN.md`.

2. **Tests Must Follow `TESTING_PLAN.md` (E2E Priority, Zero Pointless Unit Tests, Zero Broken Oracles):**
   - Update `AGENTS.md` (also update `8.3, 8.4, 8.5` -> `8.3, 8.4, 8.5, 8.6` and `Repository Map` in `AGENTS.md`!), `.opencode/agents/{implementer,expert,planner,lead}.md`, and `.opencode/skills/testing/SKILL.md`:
     * Every test must map directly to a user story in `TESTING_PLAN.md` and run **end-to-end** against the real binary by default.
     * Unit tests are forbidden unless testing the focused combinatorial language/protocol/editor surfaces in `TESTING_PLAN.md` §7 (`Blastfile` syntax/formatting, SQL grant matching, HTTP framing, LSP, Zed) where per-case E2E process boots are impractical.
     * Zero meta-tests, zero YAML/bash/prose linters, zero redundant micro-tests.

3. **Reviewer Mandate (`review-arch`, `review-ux`, `lead`) — Reject Pointless Tests / Broken Oracles & Run Code Coverage to Audit Uncovered Code Utility:**
   - Add `tools/coverage_report.py` + `Makefile` target `coverage` (`make coverage TESTS="-p blastit --test ..." FILES="crates/blastit/src/..."`, using rustup's `llvm-profdata` + `llvm-cov` in `$(rustc --print sysroot)/lib/rustlib/<host>/bin/`, `CARGO_TARGET_DIR=target/cov`, `BLASTIT_COVERAGE=1`, and `uvx --offline coverage` or `trace` for Python when `python/blastit/` is touched) that runs the specified E2E/test target(s) and prints a concise summary + exact uncovered line ranges for `FILES`!
   - Update `.opencode/agents/review-arch.md` (and `review-ux.md` / `lead.md` / `.opencode/skills/testing/SKILL.md`):
     * **`TESTING_PLAN.md` & Oracle Audit (`VERDICT: FAIL` on violation):** Reject any test that does not serve a story in `TESTING_PLAN.md`, any pointless unit test duplicating E2E coverage, and any **broken or vacuous oracle** (e.g., tests that pass when skipped/unexercised, assert exit code or log substrings without checking actual HTTP/SQL/sandbox state, or test mock/helper artifacts instead of real behaviour).
     * **Mandatory Code Coverage Utility Check:** On every code review touching `crates/` or `python/blastit/`, `review-arch` (as the single Cargo runner during review) runs `make coverage TESTS="..." FILES="..."` on the changed source files against the user-story tests and inspects **every uncovered branch/function**:
       - If uncovered code is dead, speculative, redundant, or serves no story in `TESTING_PLAN.md` -> **`VERDICT: FAIL` (`[MAJOR]`): demand DELETING or simplifying the uncovered code** (NEVER ask for filler unit tests to cover unneeded code!).
       - If uncovered code implements a real user story or fail-closed boundary from `TESTING_PLAN.md` that is untested -> **`VERDICT: FAIL` (`[MAJOR]`): demand extending the E2E user-story test** to exercise it!

# Acceptance
- `make coverage TESTS="-p blastit-lsp --tests" FILES="crates/blastit-lsp/src/lib.rs"` runs cleanly and prints coverage % + uncovered line ranges in <15s without touching `target/debug`
- All `.opencode/` agent prompts, `testing/SKILL.md`, `dx-first/SKILL.md`, and `AGENTS.md` are consistent, concise, and verified with `uv run --no-project tools/plan.py validate` and `uv run --no-project tools/guard.py`
