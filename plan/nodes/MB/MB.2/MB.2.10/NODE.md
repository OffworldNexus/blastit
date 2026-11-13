---
id: MB.2.10
title: Sync the static design with the shipped API surface and status rules
kind: task
status: done
owner: designer
priority: 2
depends_on: [MB.2.5.1]
paths: ["docs/design/static-and-spa.md", "docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md"]
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-13T22:13"
updated: "2026-11-13T23:01"
---

# Goal

Bring `docs/design/static-and-spa.md` and `ADR-0007` back in line with what the implemented leaves actually shipped, so the remaining leaves (MB.2.4.2, MB.2.5.2, MB.2.6, MB.2.7) cannot drift from a stale contract.

# Context

Four reviews of the implementation leaves produced doc-sync findings that no node owns, because `MB.2.2` closed before the code existed:

1. `crates/blastit-scheduler/src/static_asset.rs` exposes a fifth `#[doc(hidden)] pub` seam, `head_segments` (the head-step entry point MB.2.5.2 must call), which design §4.2's "only the functions below are `pub`" list does not contain.
2. `crates/blastit-scheduler/src/rewrite.rs` exposes `pub fn declares_body(headers: &[(Vec<u8>, Vec<u8>)]) -> bool`. The behaviour is exactly §4.4's rule (`Content-Length > 0` or any `Transfer-Encoding`), but §4.2 lists only `decide`.
3. §4.4's table does not say that a refusal is terminal for every method: `Refused` → `400`, `Hidden`/`SpaLost` → `404` even on `POST`, and only a *servable* static path gets `405`. MB.2.5.1 pinned that reading in tests; the design must state it or MB.2.5.2 can drift.
4. `critical_rewrite.rs` carries the oracles of **both** MB.2.5.1 and MB.2.5.2, so the §8 line budgets must record the raised ceiling instead of inviting a future leaf to trim the suite.

# Acceptance

```console
# 1. Every name the code exposes is in the design's API section, and every doc(hidden) seam is justified.
$ grep -n 'head_segments\|declares_body' docs/design/static-and-spa.md
$ grep -rn 'pub fn \|pub const ' crates/blastit-scheduler/src/{static_asset.rs,rewrite.rs} | grep -v 'pub(crate)'

# 2. The refusal-terminality rule is stated where §4.4's status table lives.
$ grep -n 'terminal\|405' docs/design/static-and-spa.md | head

# 3. §8 records that critical_rewrite.rs serves two leaves (both budgets), and ADR-0007 does not
#    contradict any of the above.
$ grep -n 'critical_rewrite' docs/design/static-and-spa.md

# 4. Docs gates.
$ make docs-check
$ make docs-unit
```

# Notes

- Documentation only: no Rust, no test edits, no new behaviour. If a discrepancy turns out to be a *code* defect rather than a doc gap, stop and report it — do not "fix" it by weakening the design.
- Keep §4.2 a table/list of the real surface; a reader must be able to diff it against the crate.
- Same lane rules as every docs node: lean prose, no kernel lecture beyond what §5 already carries, and every line-number cross-reference you touch must point at text that exists.
