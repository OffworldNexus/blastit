---
id: MB.26.42
title: "Apply the parallel-views documentation deltas: ARCHITECTURE §2.4/§2.6/§8/§8.2, settings.md and php.rs"
kind: task
status: proposed
owner: "doc-writer"
priority: 2
depends_on: []
paths: ["crates/blastit-view-php/src/worker.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Ship the documentation deltas `docs/design/parallel-views.md` §4.10 owes now that the per-route
view pool is in the tree (`MB.24.3`), so no page still describes one process per route. The pages
a developer reads must be true verbatim, with no new flag, key or step.

# Context

- Design §4.10 lists every sentence: `ARCHITECTURE.md` §2.4 *Admission* (the budget bounds requests,
  a pooled route forks per request), §2.6, §8 *Sizing* and §8.2 (the derived cap counts live
  **processes**, the in-flight fork included), `docs/reference/settings.md:68-69`
  (`BLASTIT_VIEW_MAX_WORKERS` sizes one process and is never a per-route lever;
  `BLASTIT_MAX_WORKERS` is the route's process limit when its provider serves one request per
  process), and `crates/blastit-view/src/php.rs`'s module comment (the scheduler forks
  `1..=N` such processes — N is the concurrent-request count, never a manifest directive).
- Do not name a `workers` key: `ADR-0012` §7 deleted it. One PHP transport only.
- `ADR-0010` §1 item 2 was already reconciled by `MB.24.4`; keep the two documents consistent.

# Acceptance

```console
$ uv run --no-project tools/plan.py validate
$ make docs-check
$ make docs-unit doc-tone
$ uv run --no-project tools/plan.py tree --under MB.28
```

- every item of design §4.10 is applied or explicitly redirected with the reason;
- `grep -rn "one view per route\|1..=workers" ARCHITECTURE.md docs/ crates/blastit-view/src/php.rs`
  finds nothing that contradicts the shipped pool;
- `make check` green apart from the known `docs/reference/cli.md` fence `MB.23.5` owns.

# Notes

- A designer reviews the four `ARCHITECTURE.md` sections; a `dx` critic reads the settings table.
- Lean prose: one short paragraph per section, no design-doc wholesale copying.

# Provenance

Re-filed from `MB.28` at the `v0.2.0` gate: `ARCHITECTURE.md` (§2.4/§2.6/§8/§8.2) and `docs/reference/settings.md` parallel-views wording landed in the `0.2.0` gate sync; only the module doc comment in `crates/blastit-view-php/src/worker.rs` (moved from `crates/blastit-view/src/php.rs` in `MB.36.5`) remains.
