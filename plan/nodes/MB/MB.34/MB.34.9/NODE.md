---
id: MB.34.9
title: "Protect the published QueryTemplate/LiteralQuery squiggle range against the CLI's line:col (AUDIT-MB.34 G1)"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: [crates/blastit/tests/critical_lsp.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:46"
updated: "2026-11-18T05:46"
---

# Goal

Protect the epic's headline promise: the published `QueryTemplate`/`LiteralQuery` squiggle is the
CLI's own `line:col`.

Today that is proved only by the hand-recorded `docs/testing/E2E-MB.34.md`; the model side is
protected, the LSP `publishDiagnostics` range is not, so a regression that keeps the finding and
shifts the range passes every gate.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit --test critical_lsp
$ make critical-guard NODE=<id>
```

One protected case opens the four-defect fixture (`SELCT` typo, multi-line missing `FROM`, an
inline literal in the `"…"` spelling, a redacted `…`) and asserts each `publishDiagnostics`
`range.start`/`range.end` equals the CLI `line:col` mapped to 0-based UTF-16 — `(4,15)-(4,20)`,
`(5,15)-(5,21)`, `(7,53)-(7,54)`, `(8,53)-(8,54)` — that the message equals
`ManifestError::message()` with no `line:col` prefix, and that no diagnostic sits at `(0,0)`.

# Notes

- The four ranges are in `docs/testing/E2E-MB.34.md` §3, already reproduced by two reviewers.
- Deleting a case is a failed leaf; this is an addition only.
