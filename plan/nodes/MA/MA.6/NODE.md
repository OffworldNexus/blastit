---
id: MA.6
title: "Leaf 13: backport PR #60/#61 perf work into 0.1.1"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-http/**"
  - "crates/blastit-scheduler/**"
  - "crates/blastit-view/**"
  - "crates/blastit-frame/**"
  - "crates/blastit-events/**"
  - crates/blastit/src/logs.rs
  - crates/blastit/src/roles.rs
  - ARCHITECTURE.md
  - CHANGELOG.md
  - docs/knowledge/LESSONS.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T09:39"
updated: "2026-11-13T10:04"
---

# Goal

Backport the already-reviewed performance work from the two open PR branches (`origin/perf-hot-path`, 13 commits; the production-code part of `origin/feature/benchmark`) onto `develop` so it ships in **0.1.1**, then prove the release is unchanged in behaviour.

The owner asked for this explicitly so the PRs can be closed instead of merged after the release. This is a **backport**, not new design: the changes already exist and were benchmarked upstream. Do not redesign them; adapt only where `develop` diverged.

# Context

- Both branches are based on `2c05139` (the `0.1.0` release); `develop` is ~51 commits ahead.
- `git merge-tree --write-tree develop origin/perf-hot-path` reports **2 conflicted files only**:
  - `crates/blastit-http/src/conn.rs`
  - `crates/blastit-scheduler/src/scheduler.rs`
- `git merge-tree` for `origin/feature/benchmark` is **clean**; its only production-code commit is the Nagle change that also appears in the perf branch.
- Perf branch touches: `blastit-http` (response.rs, server.rs, flowcontrol tests), `blastit-scheduler` (fair.rs, scheduler.rs, task.rs, new `tests/head_allocs.rs`), `blastit-view` (environ.rs, lib.rs, request.rs, new `tests/environ.rs`, concurrency tests), `blastit/src/logs.rs`, `blastit/src/roles.rs`, plus `ARCHITECTURE.md` and docs.
- Scope discipline: production code + its tests only. The `benchmark/` harness from `feature/benchmark` (ansible, compose, grid.py, harness.py, oha scripts, ~2k lines of dev tooling) is **out of scope for 0.1.1** — record it under Follow-ups.

# Acceptance

1. The perf work is on `develop` as a reviewed backport: `git log --oneline develop | head -20` shows the backport commit(s), and `git diff origin/perf-hot-path develop -- crates/` shows no unexplained loss of the upstream perf changes (only `develop`-side evolution).
2. Both conflicted files are resolved deliberately, not by taking one side wholesale: `crates/blastit-http/src/conn.rs` and `crates/blastit-scheduler/src/scheduler.rs` each keep `develop`'s newer semantics (0.1.1 manifest/learn/check wiring) **and** the perf change. Paste the resolution reasoning in RESULT.md.
3. The upstream performance tests are present and pass:
   - `cargo test -p blastit-scheduler --test head_allocs`
   - `cargo test -p blastit-view --test environ`
   - `cargo test -p blastit-http --test flowcontrol`
   - `cargo test -p blastit-view --test concurrency`
4. `make check` exit 0 (guard, critical-guard, fmt, clippy -D warnings, doc -D warnings, workspace tests, 200 Rust critical + 50 Python critical, docs-unit, docs-check, docs-test, plan-validate) — paste the exit code and the summary tails.
5. `make e2e` exit 0 with 8x `PASS: 0 failure(s)` — required because HTTP, scheduler and view process behaviour changed.
6. No protected critical test is weakened: `make critical-guard BASE=<pre-backport-sha>` reports no unauthorised modification.
7. `ARCHITECTURE.md` reflects the new hot-path/concurrency model if the upstream branch changed it; `CHANGELOG.md` gets one honest line about the backported performance work under `## 0.1.1`.

# Notes

- Work on `develop` directly (local commits only). Never push or tag; the release PR is opened by the owner/conductor afterwards.
- If a perf change is genuinely incompatible with 0.1.1 semantics (e.g. fairness/flow-control interacting with the new ingress check path), **drop that single commit** and record exactly what was dropped and why under Follow-ups. Do not force it.
- Prefer one merge commit or a small ordered series over 13 cherry-picks if that keeps the conflict resolution readable.
- Keep the diff honest: no opportunistic refactors, no new dependencies.
