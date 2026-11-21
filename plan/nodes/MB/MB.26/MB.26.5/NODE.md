---
id: MB.26.5
title: "Close the four message-only refusal-matrix survivors (lexer name_run, three parser guards)"
kind: task
status: superseded
owner: implementer
priority: 4
depends_on: []
paths:
  - "crates/blastit-manifest/src/blastfile/lexer.rs"
  - "crates/blastit-manifest/src/blastfile/parser.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_parser.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:14"
updated: "2026-11-19T00:36"
---

Moved from `MB.21.5.12` (`MB.21`) to `MB.26.5` at the 0.2.0 release gate; scope, `paths`, owner and acceptance are unchanged. Ids quoted inside the text below keep their pre-move numbering — use this node's id in commands.

# Goal

Close the four message-only survivors of `MB.21.5.3`'s campaign, each killed by one more refusal row:

| Mutant | Today (mutated) | Should say |
|---|---|---|
| `lexer.rs:418:45` `==`→`!=` in `name_run` | `$lower_x` reported as `lower` | the whole run, `$lower_x` |
| `parser.rs:173:29` `&&`→`||` in `starts_method` | `GET, post "/x"` → "a list has no trailing comma" | "`post` is not a method" |
| `parser.rs:178:56` `&&`→`||` in `starts_pattern` | `view v GET foo "/x"` → "expected a pattern, found `foo`" | "`view \"v\"` lists methods but no pattern" |
| `parser.rs:661:32` guard `== Some("regex")`→`true` | `view v { GET foo }` → "`regex` must be followed by a raw string" | "expected a pattern, found `foo`" |

# Context

- `docs/testing/AUDIT-MB.21.md` §3 lists each survivor with its trace; all four are one-line test
  additions, none is an equivalent mutant.
- The sentences are UX: a developer who writes `GET, post` must be told the method is lower-case, not
  that a comma is dangling — the second sentence sends them to the wrong fix (`dx-first`).
- `lexer.rs::env_names` already asserts `$lower`; the `_` case is the missing twin.

# Acceptance

```console
$ cargo test -p blastit-manifest --test blastfile_errors
$ cargo test -p blastit-manifest
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb2160 MUTANTS_JOBS=3 \
  scripts/mutants-run.sh -p blastit-manifest -f crates/blastit-manifest/src/blastfile/lexer.rs \
  -f crates/blastit-manifest/src/blastfile/parser.rs
```

1. `lexer.rs::env_names` gains `$lower_x` → "`$lower_x` is not an environment variable name: …".
2. `parser.rs::every_refusal_row` gains the three rows with their exact sentences and `line:col`,
   and `critical_blastfile_parser.rs::the_refusal_matrix` mirrors at least `GET, post "/x"`.
3. All four mutants are caught; no new timeout appears.

# Notes

- Message-only: no behaviour changes, so no protected assertion is weakened; the parser matrix only
  grows.
