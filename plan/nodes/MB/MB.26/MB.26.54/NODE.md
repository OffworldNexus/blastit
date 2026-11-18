---
id: MB.26.54
title: "Protect position_at's past-the-end fallback (the critical tier's one surviving mutant)"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_query_sites.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

The protected tier misses exactly one mutant of the new position logic
(`AUDIT-MB.34` §3.2): `model.rs:245:19 replace < with >` in `QuerySite::position_at` — the
past-the-end fallback `if walked < offset { return self.at }`. §13.4.5's rule ("a mapping that
cannot be computed is a refusal at the token, not `0:0`") is proved only by the unprotected
`validate.rs::a_site_points_at_the_origin_when_the_offset_is_past_the_text`.

# Context

- `QuerySite`'s fields are public, so the state is constructible in a test.
- `position_at(offset)` returns `Position::NONE` when `at.is_none()`, and `self.at` when
  `offset` exceeds `raw`'s character count.
- Do not change the code; the mutant is equivalent in production (no document can produce
  `offset > len`). Only the tier moves.

# Acceptance

```console
$ cargo test -p blastit-manifest
$ uv run --no-project tools/critical_guard.py --base <commit-before> --node <this node>
```

A protected case asserts `position_at(0) == at`, `position_at(6) == at(5,22)` for
`at(5,16)`/`raw = "SELECT"`, `position_at(7) == at(5,16)`, `position_at(u32::MAX) == at(5,16)`,
and `NONE` for every offset of a `Position::NONE` site. Re-running the audit's campaign
(`--re 'position_at|query_sites|with_query_sites|check_queries|escape_indices|unsited'`,
protected test selection) reports 0 missed, 21/21 killable.

# Notes

- The campaign command and its output shape are in `docs/testing/AUDIT-MB.34.md` §3.2.

# Provenance

Re-filed from `MB.34.7.8` (`AUDIT-MB.34` G8) at the `v0.2.0` gate: promote `QuerySite::position_at` past-the-end fallback test from `tests/validate.rs` to `critical_query_sites.rs`.
