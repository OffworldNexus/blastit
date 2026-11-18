---
id: MB.34.12
title: "Cover the codeAction inverted range and the escaped double-quoted interior end to end (AUDIT-MB.34 G4)"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: [crates/blastit/src/lsp/mod.rs, crates/blastit/tests/critical_lsp.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:46"
updated: "2026-11-18T05:46"
---

# Goal

Cover the `codeAction` edge branches end to end: an inverted range, and the escaped `"…"` interior
through the handler (not just `encode_interior`).

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit --lib lsp
$ cargo test -p blastit --test critical_lsp
```

A `codeAction` whose `start` is after its `end` (same uri, same line) returns `[]` and not
`-32602`; `query "SELECT \"o\".id FROM t WHERE email = 'a@b.com'"` returns exactly one action whose
`newText` is `SELECT \"o\".id FROM t WHERE email = $?`, whose applied text re-checks clean, and
whose `range` is the whole interior; unit tests cover `encode_interior` for a literal newline and a
tab.

# Notes

- §7A.2's inverted-range clause and §7A.4's re-encoding are the two untested §7A rules.
