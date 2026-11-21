---
id: MB.22.9
title: "Prove blastit lsp exits 1 when the process cannot drop its file capabilities (S10's seventh session, rootless e2e)"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: [e2e]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T07:14"
updated: "2026-11-17T07:22"
---

## Goal

Give `docs/design/blastfile-lsp-zed.md` §11 S10's seventh session an owner and a test: a process
that carries a capability and cannot `caps::hold_none()` exits `1` with the one fixed sentence.

## Context

`AUDIT-MB.22.md` §1.2 (S10 row) and gap 3. `crates/blastit/src/cli.rs::run_lsp` returns `FAILURE`
(1) and writes `blastit lsp: internal error` when `blastit_sys::caps::hold_none()` fails, and
`critical_lsp.rs::the_lsp_arm_drops_capabilities` only greps the *order* of the call in `cli.rs`.
`grep -rn "lsp" e2e/` finds no harness for it (the hits are `urlsplit`), and the unprivileged
critical tier cannot make `hold_none()` fail, so the design's "rootless e2e only" row has no test
anywhere.

## Acceptance

```console
$ make e2e   # or the rootless scenario that carries the new case
```

- One rootless scenario runs `blastit lsp` in a process holding `CAP_SETUID` (the same shape the
  capability e2e already uses for the production boot) and asserts exit `1`, an empty stdout, and
  exactly `blastit lsp: internal error` on stderr.
- The unprivileged path stays covered by `critical_lsp.rs` (no skip, no `#[ignore]`).
