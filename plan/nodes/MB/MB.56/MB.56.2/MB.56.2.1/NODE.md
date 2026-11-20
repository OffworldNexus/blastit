---
id: MB.56.2.1
title: "Delete non-§7 micro-tests in scheduler, dbproxy, view-* and crates/blastit/src"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-scheduler/**"
  - "crates/blastit-dbproxy/**"
  - "crates/blastit-view/**"
  - "crates/blastit-view-python/**"
  - "crates/blastit-view-static/**"
  - "crates/blastit-view-php/**"
  - "crates/blastit/src/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T17:15"
updated: "2026-11-20T17:30"
---

# Goal
Delete every test that does not protect a `TESTING_PLAN.md` §7 surface, for the **heaviest** crates: `blastit-scheduler` (12,244 test LOC), `blastit-dbproxy` (3,296), `blastit-view-python` (3,115), `blastit-view-static` (3,388), `blastit-view-php` (260), `blastit-view` (inline only), plus the inline `#[cfg(test)]` modules in `crates/blastit/src/**`.

1. Remove `crates/blastit-scheduler/tests/**`, `crates/blastit-dbproxy/tests/**`, `crates/blastit-view-{python,static,php}/tests/**` and every inline `#[cfg(test)]` module in those crates and in `crates/blastit/src/**` (25 occurrences today).
2. **Before deleting**, read each suite and record in `RESULT.md` every unique **user-visible** security or behaviour case it covered (per-view grant isolation, fair queueing, `exit()`/fatal recovery, static confinement `..`/symlink/dotfile/`.php`, live reload, PHP pinning, `php2shell` immunity, …) so `MB.56.3` can prove 100% of them end-to-end in `crates/blastit/tests/`. A case with no §-story home is simply dropped — say so.
3. Keep **zero** unit tests here: any remaining `#[cfg(test)]` in these crates must be deleted, not `#[ignore]`d.
4. Do not touch production behaviour; deleting a test must never change `src/**` logic. If a test only compiles because production code is `pub(crate)` for it, leave the visibility alone (later slices prune dead code by coverage).

# Acceptance
- `make fast-check` green, zero warnings; `rg 'cfg\(test\)' crates/blastit-scheduler crates/blastit-dbproxy crates/blastit-view* crates/blastit/src` returns nothing.
- `RESULT.md` carries the user-visible-case inventory that `MB.56.3` must re-cover.
