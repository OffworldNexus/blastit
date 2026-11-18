---
id: MB.26.55
title: "Protect the published query squiggle: the LSP range must equal the CLI's line:col"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: [crates/blastit/tests/critical_lsp.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-19T00:36"
---

# Goal

The epic's headline editor promise — "one squiggle whose range starts at the `SELCT`, not at
`(0,0)`" — has no gated test. `AUDIT-MB.34` §1.3/§2 proves the *model* side
(`critical_query_sites.rs`) and the *existing* LSP error path, but no test publishes a
`TextDocument/publishDiagnostics` for `ManifestError::QueryTemplate` or `LiteralQuery` and
asserts its range. Today only the hand-recorded `docs/testing/E2E-MB.34.md` transcript does.

Add one protected case in `crates/blastit/tests/critical_lsp.rs`.

# Context

- The four-defect fixture from `docs/testing/E2E-MB.34.md` §1 is the oracle: `SELCT` typo on
  line 5, multi-line missing `FROM` on line 6, inline literal in the `"…"` spelling on line 8,
  redacted `…` on line 9, in one `view`.
- CLI prints 1-based `line:col`; the LSP `character` is 0-based UTF-16. The audited values are
  `(4,15)–(4,20)`, `(5,15)–(5,21)`, `(7,53)–(7,54)`, `(8,53)–(8,54)`.
- The message must be `ManifestError::message()` (no `line:col` prefix) and must never quote the
  statement.
- Adding to a protected file is always allowed; do not touch the ten pre-existing cases.

# Acceptance

```console
$ cargo test -p blastit --test critical_lsp
$ uv run --no-project tools/critical_guard.py --base <commit-before> --node <this node>
```

The new test asserts, for all four defects: `range.start`/`range.end` equal the values above,
`message` equals the un-prefixed sentence, the CLI text and the LSP range agree on line **and**
column, and no diagnostic starts at `(0,0)`. It fails if `error_diagnostic` ignores
`error.position()`.

# Notes

- Use the existing `Session` harness in the file; no new dependency, no new script.
- `Severity` is 1 for all four; `source` is `blastit`.

# Provenance

Re-filed from `MB.34.7.9` (`AUDIT-MB.34` G1) at the `v0.2.0` gate: automated `critical_lsp.rs` assertion of published `QueryTemplate`/`LiteralQuery` squiggle ranges vs CLI `line:col` (verified live in `E2E-MB.34.md`).
