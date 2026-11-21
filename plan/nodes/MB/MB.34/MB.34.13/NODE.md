---
id: MB.34.13
title: "Protect the file-wide parse cap enforcement at the 4097th query (AUDIT-MB.34 G5)"
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

Protect the enforcement of the file-wide parse cap (`MAX_TEMPLATE_PARSES`, 4 096), a DoS bound.

The constant is protected; its enforcement (the 4 097th distinct entry refused at its own `at`)
lives only in the unprotected `tests/validate.rs`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest
$ make critical-guard NODE=<id>
```

A protected case writes 4 097 distinct `query` lines, asserts exactly one
`ManifestError::QueryTemplate { at: Position::at(4101, 16), kind: TooManyQueries }`, that the first
4 096 are not refused, and that the message is the page's sentence with no `line:col` prefix.

# Notes

- A 16 MiB file with tens of thousands of `query` lines is the attack this bounds; the number is
  the design's, not a guess.
