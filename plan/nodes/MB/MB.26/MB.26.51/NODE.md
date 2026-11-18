---
id: MB.26.51
title: "Protect the 4096-parse cap: TooManyQueries at its own site"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_query_sites.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-19T00:36"
---

# Goal

`MAX_TEMPLATE_PARSES` (4 096) is a DoS bound: a 16 MiB file of tiny `query` lines must not cost
one `pg_query` parse per entry on every publish. The constant is asserted in the protected
suite, but the **enforcement** — the 4 097th distinct entry refused as `TooManyQueries` at its
own `at`, and the first 4 096 not refused — lives only in
`crates/blastit-manifest/tests/validate.rs`.

Protect it.

# Context

- `AUDIT-MB.34` §2 ("one parse per distinct entry") and §4 (misplaced tier).
- `validate.rs::a_file_past_the_parse_cap_is_refused_at_the_entry_past_it` is the existing
  oracle: 4 097 `query 'SELECT <n>'` lines, one error at `Position::at(4101, 16)`.

# Acceptance

```console
$ cargo test -p blastit-manifest
$ uv run --no-project tools/critical_guard.py --base <commit-before> --node <this node>
```

A protected case in `critical_query_sites.rs` writes 4 097 distinct entries and asserts exactly
one `ManifestError::QueryTemplate { at: Position::at(4101, 16), kind: TemplateKind::TooManyQueries }`,
that the first 4 096 entries produce no `TooManyQueries`, and that the rendered message is the
page's sentence with no `line:col` prefix.

# Notes

- Build the fixture with `format!`/`push_str`; do not add a fixture file (16 MiB class documents
  are already covered elsewhere).

# Provenance

Re-filed from `MB.34.7.5` (`AUDIT-MB.34` G5) at the `v0.2.0` gate: promote existing 4096-parse cap (`TooManyQueries`) test from `tests/validate.rs` to `critical_query_sites.rs`.
