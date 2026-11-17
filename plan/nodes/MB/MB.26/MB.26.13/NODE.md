---
id: MB.26.13
title: "Give the five security-bearing zed-guard properties a negative twin (no capabilities, no process, argv, no scanner, no externals)"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: [scripts/test_check_zed_extension.py]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T07:22"
updated: "2026-11-17T07:22"
---

## Goal

Make a *broken* guard check red, not only a deleted one, for the properties that carry S1 of
`docs/design/blastfile-lsp-zed.md` §11.1.

## Context

Moved from `MB.22.11` at the `0.2.0` release gate: post-0.2.0 work, re-homed from the `MB.22` epic.

`AUDIT-MB.22.md` §1.3 nit 5 and §2 regression 7. `scripts/test_check_zed_extension.py` has 19 cases
for the guard's 42 properties. `test_the_repository_passes` pins the exact 42 names, so deleting a
`guard.require(...)` is caught (measured: removing `indents: capture` reddens
`test_the_repository_passes` and `test_missing_indent_is_red`), but a check whose *condition* is
inverted or weakened still passes. The five that matter are the ones a registry reviewer relies on:
`lib: no capabilities`, `lib: no process`, `lib: argv`, `grammar: no scanner`, `grammar: no externals`
(`[capabilities]` turns a zero-config install into a permission prompt; a scanner is C in the
editor's grammar wasm; a wrong argv runs the worktree's build backend).

## Acceptance

```console
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
$ make zed-check
```

- Five new cases, each doctoring one property in the temp tree and requiring `FAIL <property>`:
  a `[capabilities]` table under `editors/zed`, a `Command::new` in `src/lib.rs`, a swapped argv,
  a `src/scanner.c`, an `externals:` key in `grammar.js`.
- The existing 19 cases and the 42-name pin stay green; no guard property is renamed.
