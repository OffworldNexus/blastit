---
id: MB.21.5.11
title: "Pin the named-store grant guard (coverage; a regression would accept a typo'd grant)"
kind: task
status: active
owner: implementer
priority: 4
depends_on: []
paths:
  - "crates/blastit-manifest/src/blastfile/parser.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_parser.rs"
  - "crates/blastit-manifest/tests/blastfile_grants.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T01:13"
updated: "2026-11-17T04:54"
---

# Goal

**Coverage only (post-0.2.0).** Pin the named-store grant guard at `parser.rs:1270`:
`TokenKind::Word if self.peek_word() == Some("unrestricted")`. The guard is currently unpinned — mutating
it to `true` survives the whole suite — and such a regression would let `postgres db bogus` parse as an
empty grant instead of being refused. **The shipped parser already refuses it**
(`parser.rs:1267-1274`, `_ => Err(unexpected("unrestricted or {"))`), so this is an oracle gap, not a
product defect: no reviewed build accepts the typo.

# Context

- `MB.21.5.3`'s campaign (survivor `parser.rs:1270:36`): with the guard erased, `postgres db bogus`
  parses; `lower::grant` walks the words, ignores the one that is neither `unrestricted` nor the first
  name, and produces an **empty** `PgGrant` — so a typo the operator meant as a grant boots as a dead
  grant instead of the §6.7 refusal `expected `unrestricted` or `{`, found `bogus``.
- Design §6.7 (the refusal table) and R4's "the parser is the grammar": every token that is not
  `unrestricted` or `{` after a store name is a refusal at its own `line:col`.
- Not a security escalation and not a shipped defect: the refusal exists today. What is missing is a
  test that would notice if the guard were loosened, which is why this node is coverage and its
  priority is 4.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_blastfile_parser
$ cargo test -p blastit-manifest --test blastfile_grants
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb2159 MUTANTS_JOBS=3 \
  scripts/mutants-run.sh -p blastit-manifest -f crates/blastit-manifest/src/blastfile/parser.rs
```

1. A test asserts `postgres db bogus` (and the same with `select`, a keyword) is refused with the
   parser's sentence at the word's position, not accepted and not a lowering error.
2. `parser.rs:1270:36`'s mutant is caught; the file's other survivors are unchanged (see the audit).
3. The accepted twins stay green: `postgres db unrestricted`, `postgres db { … }`, `postgres
   unrestricted`, `postgres $URL`.

# Notes

- Add the row to `parser.rs::every_refusal_row` (unit, pub(crate) parser) **and** to
  `critical_blastfile_parser.rs::the_refusal_matrix` so the sentence is pinned through the public
  entry point. Both files are in `paths`; the protected one is additive (allowed).
