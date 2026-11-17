---
id: MB.26
title: "Post-0.2.0 Blastfile coverage hardening: the audit's remaining survivors and the PHP view-retire race"
kind: epic
status: ready
priority: 2
depends_on: []
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:14"
updated: "2026-11-17T06:14"
---

# Goal

Land the Blastfile work that `MB.21.5.3`'s mutation audit filed but deliberately kept out of the
`0.2.0` release, plus the PHP view-retire race `MB.21.7` measured during the gate. Nothing here is a
fail-closed defect in the shipped `0.2.0`: the refusals are correct, the parser never reads the
environment, and no shipped input reaches the untested editor branches. What remains is oracle
strength, message precision, one vacuous uid-0 test, and one real concurrency bug.

# Context

- **Why this epic exists (not inside `MB.21`).** `MB.21.5.3`'s `cargo-mutants` campaign measured
  **92.0 % kill** (827 accounted mutants of 828, 688 of 748 viable; `docs/testing/AUDIT-MB.21.md`
  §2.5) and split the 30 testable survivors two ways: three pre-gate leaves — `MB.21.5.5`,
  `MB.21.5.7`, `MB.21.5.11`, all `done` — and coverage-only refinements that do not block a release:
  `MB.21.5.6`, `MB.21.5.8`, `MB.21.5.9`, `MB.21.5.10`, `MB.21.5.12`, `MB.21.5.13`, `MB.21.5.14`.
  The epic-lead ruling is recorded in `MB.21`'s `# Notes` and `MB.21.5.3`'s `RESULT.md`; the seven
  were excluded by id from `MB.21.5`'s acceptance item 1 and would keep the `0.2.0` release epic
  open, so they are re-homed here instead of being weakened or deleted.
- **`MB.21.8` is not coverage.** It is a **known product race**, filed while doing `MB.21.7`: a
  request queued behind an in-flight one is later dispatched to a view that already answered and
  retired, and the scheduler answers `502 {"outcome":"view_died",…}` instead of respawning — measured
  as **1 × `200` and 7 × `502`** on eight concurrent GETs to one PHP view (one-slot view). Same class
  as `MB.3.45` (there 1-in-20 and fixed); here it reproduces at will, so it leads this epic.
- Precedent evidence and traces: `docs/testing/AUDIT-MB.21.md` §2.4, §3, §4, §5.3, §5.5;
  `docs/design/blastfile-dsl.md` §4.5, §6.7, §8.3, §13.1, §14.2, §14.3, §14.5, §20; lessons
  `MB.3.13.1` (stale single-slot bookkeeping frames) and `MB.1.1` (`cargo-mutants` outcome files).

# Acceptance

```console
$ uv run --no-project tools/plan.py tree --under MB.26
$ cargo test -p blastit-manifest
$ make check
$ ls docs/testing/AUDIT-MB.21.md
```

1. Every child is `done` with a `RESULT.md` carrying its own command + output tails; no child is
   `superseded`-by-silence or quietly dropped — a child that turns out unnecessary is closed with the
   reason and, where relevant, a note back into `docs/testing/AUDIT-MB.21.md`.
2. Each mutation-coverage child reports the **measured** before/after survivor count from
   `scripts/mutants-run.sh` with its `MUTANTS_TMP` path; no protected assertion is removed to make a
   mutant die — every one of these nodes is additive or authorised in its own `paths`.
3. `MB.21.8` proves the race with a regression test that fails at the pre-fix commit and passes after
   it, and `make check` stays green.
4. `make check` green at the end of the epic, `cargo test -p blastit-manifest` green, and
   `tools/critical_guard.py` clean for every protected file the epic touched.

# Notes

- **Consume before extending.** The audit's survivor list is the backlog; do not start a fresh
  full-crate campaign (it cost 4 h and was lost twice on a loaded host). Scope each campaign to the
  module and test targets the child names.
- `MB.21` (the `0.2.0` release epic) and `MB.21.5` are closed once these ids moved here; this epic is
  the single place the remaining work lives, so do not re-file anything under `MB.21`.
- Two children are already known to be partly ruled out by their sources: `MB.21.5.13` must not test
  `print.rs::write_leading_comments` (dead code, three equivalent mutants — note it for deletion) and
  `MB.21.5.14` names five equivalent mutants that must stay unkilled.
