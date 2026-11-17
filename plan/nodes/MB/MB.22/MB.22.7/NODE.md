---
id: MB.22.7
title: "Gate grammar.js against the committed tree-sitter parser (regenerate + diff), because tree-sitter test never reads grammar.js"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths:
  - "editors/tree-sitter-blastfile/grammar.js"
  - "editors/tree-sitter-blastfile/src"
  - scripts/check_zed_extension.py
  - scripts/test_check_zed_extension.py
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T07:14"
updated: "2026-11-17T07:22"
---

## Goal

Make a `grammar.js` edit that was never regenerated a red gate, and prove it with a negative self-test.

## Context

`AUDIT-MB.22.md` §2 regression 5 (measured 2026-11-17, base `d08d04c`): deleting
`_terminator` from `source_file` in `editors/tree-sitter-blastfile/grammar.js` left
`tree-sitter test` **green, 15/15**. The command compiles the committed `src/parser.c`
(even with `--rebuild`), so the corpus is an oracle for the committed parser only. After
`tree-sitter generate` the same edit turned 15/15 red and exited 1. Nothing in `make check`
runs `generate`, so a grammar edit that is never regenerated ships a parser the corpus and
the design (§9.2) do not describe; `scripts/check_zed_extension.py` checks `grammar.js`'s
name and the absence of externals, not that `src/` is its output.

## Acceptance

```console
$ tree-sitter generate && git diff --exit-code -- editors/tree-sitter-blastfile/src
$ # doctoring grammar.js (e.g. remove `_terminator` from `source_file`) must make the gate red
```

- A target (inside `make zed-check`, or a sibling `grammar-fresh` that `make check` runs)
  regenerates into a scratch copy and fails on any diff, using the pinned 0.25.10 binary when
  it is present and skipping loudly (never silently) when it is not.
- One negative self-test doctors `grammar.js` and requires the gate to name it.
- `make check` stays green on the unmodified tree.
