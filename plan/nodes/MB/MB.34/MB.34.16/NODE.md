---
id: MB.34.16
title: "Protect the position_at past-the-end fallback so the surviving < mutant dies (AUDIT-MB.34 G8)"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_query_sites.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:46"
updated: "2026-11-18T05:46"
---

# Goal

Move `validate.rs::a_site_points_at_the_origin_when_the_offset_is_past_the_text` into the protected
tier.

It is the only oracle for `QuerySite::position_at`'s past-the-end fallback (`if walked < offset {
return self.at }`), and the surviving mutant `model.rs:245:19 replace < with >` is killed only by
that standard-tier case: a past-the-end offset would answer `(5,22)` instead of the token's own
origin `(5,16)`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest
$ make critical-guard NODE=<id>
```

A protected case with `raw = "SELECT"` asserts `position_at(7) == at(5,16)` (the token's own origin,
never an invented column) and that the mutant `walked < offset` → `>` is caught by the protected
tier alone.

# Notes

- The audit measured 20/21 killable (95.2 %) against the protected tier; this closes the one miss
  by placement, not by rewriting code.
