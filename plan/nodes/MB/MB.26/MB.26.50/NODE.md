---
id: MB.26.50
title: "Cover codeAction's inverted range and the escaped double-quoted interior end to end"
kind: task
status: proposed
owner: implementer
priority: 2
depends_on: []
paths: [crates/blastit/src/lsp/mod.rs, crates/blastit/tests/critical_lsp.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Two `codeAction` branches of §7A have no test: the **inverted range** (`start > end` → `[]`)
and the **escaped `"…"` interior end to end** (§7A.4's re-encoding through the handler; only
`encode_interior`'s `\`/`"` arms are unit-tested, and its newline/tab arms are untested).

# Context

- `AUDIT-MB.34` §2 rows "an inverted range" and "the edit re-encodes the `"…"` spelling".
- The handler is `Server::code_action` in `crates/blastit/src/lsp/mod.rs`; the unit test is
  `the_interior_is_re_encoded_for_its_spelling`.
- §7A.4's example: `query "SELECT \"o\".id FROM t WHERE email = $1"` →
  `SELECT \"o\".id FROM t WHERE email = $?`.

# Acceptance

```console
$ cargo test -p blastit --lib lsp
$ cargo test -p blastit --test critical_lsp
```

- A `codeAction` whose `start` is after its `end` on the same tracked document returns `[]`
  (and not `-32602`).
- The escaped-interior document returns exactly one action: `newText` is
  `SELECT \"o\".id FROM t WHERE email = $?`, the range is the whole interior, and the applied
  text re-checks clean through `blastfile::check`.
- Unit tests cover `encode_interior` for a literal newline and a tab (each escaped as `\n` /
  `\t`), keeping the `\` first order.

# Notes

- Adding to the protected file is allowed; the two new critical assertions are the point.

# Provenance

Re-filed from `MB.34.7.4` (`AUDIT-MB.34` G4) at the `v0.2.0` gate: `codeAction` inverted-range (`start > end`) and escaped double-quoted interior e2e test.
