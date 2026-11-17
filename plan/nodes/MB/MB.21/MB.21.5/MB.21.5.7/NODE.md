---
id: MB.21.5.7
title: "Land the design's >= 20-document round-trip corpus floor and the hoisted-comment owner case"
kind: task
status: active
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-manifest/tests/critical_blastfile_print.rs"
  - "crates/blastit-manifest/tests/common/mod.rs"
  - "crates/blastit-manifest/src/blastfile/print.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T23:03"
updated: "2026-11-17T04:58"
---

# Goal

Land the design's round-trip corpus floor: §20 says `MB.21.5.2` re-runs P1–P3 over the migrated
corpus and asserts **≥ 20 files** ("15 fixtures + the documented documents"), so the final gate cannot
pass vacuously. `MB.21.5.2` did not touch `critical_blastfile_print.rs`: `common::CORPUS_SIZE` is
still 15 and four suites assert exactly 15.

# Context

- `docs/design/blastfile-dsl.md` §20 "The round-trip corpus" and §21's `MB.21.5.2` row.
- `docs_fences.rs` parses and validates every documented document but never runs P1 (lossless), P2
  (fixed point) or P3 (meaning) over them.
- `MB.21.5.3`'s audit also found that `print.rs::p2b_comment_owners_survive` never exercises a comment
  hoisted out of an unwrapped list (§1.3 nit 7), though the text-level oracle covers the shape.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_blastfile_print
$ cargo test -p blastit-manifest --test blastfile_fmt
$ make docs-test
```

1. `critical_blastfile_print.rs` runs P1/P2/P2b/P3 over **≥ 20** documents: the 15 fixtures plus the
   ten `text title="Blastfile"` documents read from `README.md`/`docs/**` (parse them with the one
   reader; skip nothing silently — a page that fails to yield a document fails the count).
2. The floor is an assertion (`assert!(corpus.len() >= 20, …)`), not a comment, and the existing
   `CORPUS_SIZE` contract keeps its exact-15 meaning for the fixture-only suites.
3. `print.rs::p2b_comment_owners_survive` gains one document whose comment is written inside a list the
   layout unwraps, and asserts the hoisted comment's owning node is the statement's node.
4. `blastfile_fmt`'s fixture loop still asserts 15 (it is the fixture set, not the corpus).

# Notes

- Additive to a protected file, allowed; `paths` names it.
- Read the pages through a small helper in `tests/common/mod.rs` so the protected suite does not grow a
  Markdown parser; `docs_fences.rs` already has the fence scanner and can be pointed at (copy the two
  functions rather than depending on a test binary).

- **Pre-gate (epic lead's ruling).** This is the design's §20 round-trip corpus floor — the epic's own
  acceptance cites it, and without it the final gate can pass vacuously. The epic lead requires it
  before the 0.2.0 gate; it is not a post-0.2.0 follow-up.
