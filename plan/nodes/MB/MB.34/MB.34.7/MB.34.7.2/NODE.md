---
id: MB.34.7.2
title: "Protect the QuerySite Debug redaction (a query value must never reach {:?})"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_query_sites.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:45"
updated: "2026-11-18T20:36"
---

# Goal

`QuerySite`'s hand-written `Debug` redacts `raw` (§13.4.5, security S11: a `query` may hold a
value and `PgGrant` derives `Debug`). That leak rule is proved only in the unprotected
`crates/blastit-manifest/tests/validate.rs::a_query_site_never_renders_the_statement_it_carries`.

Move/duplicate it into the protected suite so a regression cannot ship.

# Context

- `AUDIT-MB.34` §2 and §4: a redaction rule belongs in the critical tier; the audit's own rule
  is "a position/redaction rule is critical, a formatting helper is not".
- `critical_query_sites.rs` already builds grants from source and has `PkgGrant`/`Manifest`
  imports.

# Acceptance

```console
$ cargo test -p blastit-manifest
$ uv run --no-project tools/critical_guard.py --base <commit-before> --node <this node>
```

A protected case builds `query "SELECT 's3cr3t' FROM t"` and asserts `query_sites()[0].raw`
contains `s3cr3t`, while `format!("{:?}", site)`, `format!("{:?}", grant.query_sites())` and
`format!("{:?}", grant)` all contain neither `s3cr3t` nor the surrounding statement, and do
contain `SELECT … FROM t`.

# Notes

- Deleting the now-duplicated `validate.rs` case is optional; keeping both is cheap.
- Do not change `QuerySite`'s fields or `Debug` output.
