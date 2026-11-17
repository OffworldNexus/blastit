---
id: MB.26.6
title: "Close the printer and pattern-name survivors (six mutants, four corpus cases)"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-manifest/src/blastfile/print.rs"
  - "crates/blastit-manifest/src/blastfile/pattern.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_print.rs"
  - "crates/blastit-manifest/tests/blastfile_patterns.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:14"
updated: "2026-11-17T06:14"
---

Moved from `MB.21.5.13` (`MB.21`) to `MB.26.6` at the 0.2.0 release gate; scope, `paths`, owner and acceptance are unchanged. Ids quoted inside the text below keep their pre-move numbering — use this node's id in commands.

# Goal

Kill the printer/pattern survivors `MB.21.5.3`'s campaign measured; each is one more case, and none is
an equivalent mutant.

| Mutant | Why it survived | Case that kills it |
|---|---|---|
| `pattern.rs:307:73` `\|\|`→`&&` in `is_parameter_name` | no pattern names a parameter with `_` | `fragment("/x/{a_b}") == "/x/[^/]+"`, it matches `/x/7`, and `{a-B}` is still refused |
| `print.rs:315:5` `shares_line`→`true` | no document has a comment as the **first** element inside a block on its own line | `view v "/x" {\n # note\n body json\n}` keeps `# note` on its own line (and is a fixed point) |
| `print.rs:409:49`, `418:25`, `419:25` (paren counter) | no document wraps a column list with a newline **immediately after `(`** | `select products(\n id,\n name\n)` formats to `select products(id, name)` and is a fixed point |
| `print.rs:366:13` (Token arm of `Line::comment_trails`) | no grant clause carries a trailing comment; `body json # c` ends in a *node*, so the Node arm answers | `postgres {\n transactions # note\n}` keeps `# note` on the `transactions` line |

# Context

- `docs/testing/AUDIT-MB.21.md` §3 has the trace for each; the design rows are §8.3 (parameter names),
  §14.2 (canonical layout), §14.3 (P2/P2b).
- The paren counter exists precisely for a wrapped `table_grant` (§14.2's "lists … never wrapped"), and
  the only current case wraps after a comma, which the `Comma` condition already handles — so the
  counter is unpinned.
- `print.rs::write_leading_comments` is **dead** (its comments are re-emitted by `inline_text`'s
  hoisting); its three mutants are equivalent. Do not add a test for it; note it for deletion.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_blastfile_print
$ cargo test -p blastit-manifest --test blastfile_patterns
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb2161 MUTANTS_JOBS=3 \
  scripts/mutants-run.sh -p blastit-manifest -f crates/blastit-manifest/src/blastfile/print.rs \
  -f crates/blastit-manifest/src/blastfile/pattern.rs
```

1. The six mutants above are caught (four table rows; the paren row is three mutants).
2. Each new case asserts the canonical text **and** the fixed point, like the existing ones.
3. No existing case is weakened; `critical_blastfile_print.rs` only grows.

# Notes

- Add the pattern case to `pattern.rs`'s unit tests (`is_parameter_name` is private) and mirror the
  accepted/refused pair in `blastfile_patterns.rs` if the public surface allows it.
