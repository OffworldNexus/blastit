---
id: MB.34.10
title: "Protect the QuerySite Debug redaction of the statement it carries (AUDIT-MB.34 G2)"
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

Move the `QuerySite` `Debug` redaction rule (design §13.4.5 / S11: never log a statement's value)
into the protected tier.

`tests/validate.rs::a_query_site_never_renders_the_statement_it_carries` is the only oracle for a
secret-leak rule.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest
$ make critical-guard NODE=<id>
```

A protected case builds `query "SELECT 's3cr3t' FROM t"`, asserts `query_sites()[0].raw` holds the
value, and that `format!("{:?}", site)`, `format!("{:?}", grant.query_sites())` and
`format!("{:?}", grant)` contain no `s3cr3t` while they do contain `SELECT … FROM t` (redacted).

# Notes

- The grant-level assertion is the one that matters: `PgGrant`'s derived `Debug` is the hole
  `MB.38` closes, and this case must stay green across that change.
