---
id: MB.21.3.5
title: "Surgical comment-preserving Blastfile editor for --learn (replaces the TOML patcher)"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.21.3.4]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/blastfile/write.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit_executor.rs"
  - "crates/blastit-manifest/tests/critical_learn_executor_blocks.rs"
  - "crates/blastit-manifest/tests/edit_branches.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:16"
---

# Goal

Replace the TOML text-patcher behind `--learn` with a **surgical CST editor** on `Blastfile`: add a
missing statement grant, a trait and its `use`, a body directive or a new view, while every other byte
of the file — comments, ordering, quoting, blank lines — stays exactly as the developer wrote it.

# Context

- Design: `docs/design/blastfile-dsl.md` §14.5 (the edit API, P4) and `MB.21.3.8`'s refusals. Today's
  behaviour is `crates/blastit-manifest/src/edit.rs` (2049 lines): `--learn` adds `queries`, `groups` and new routes,
  and every write is guarded by a postcondition (`verify`) that re-parses and compares the whole
  expected model. That guard is **not optional** — it is what stops a patch from silently dropping an
  executor block (`MB.3.21`).
- The editor must offer the same actions on the new syntax: `add_query` (append one `query '…'` inside
  the named store's grant block, creating the block if absent), `add_trait` + `use` (for
  connection-setup statements — the `add_group`/`add_trait` fix action), `add_view` (insert above the
  method-unrestricted catch-all `view`), and `add_body` (attach `body json|form` to a method endpoint).
  `raw` is never written (`ADR-0011`).
- Postcondition: re-parse the patched text and compare it to `expected` (the original model plus the
  intended change) exactly as today, including `static_policy` and `php`; a mismatch is
  `EditError::Verification` and **no write happens**. Writes stay atomic and keep file permissions.
- Protected suites that must be rewritten to Blastfile fixtures (they encode the user-visible learn
  contract and are listed in `paths` deliberately): `crates/blastit-manifest/tests/critical_manifest_edit.rs`,
  `crates/blastit-manifest/tests/critical_manifest_edit_executor.rs`,
  `crates/blastit-manifest/tests/edit_branches.rs`. The assertions stay the same or stronger; only the
  document format changes.
- Keep `Manifest::from_toml_str` and the old patcher alive until the sweep: `MB.21.4.5` deletes them.
  Put the new editor beside it behind the small internal trait the design names.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked --test critical_manifest_edit
$ cargo test -p blastit-manifest --locked --test critical_manifest_edit_executor
$ cargo test -p blastit-manifest --locked --test edit_branches
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.3.5
$ make check
```

1. Each action round-trips: applying it to a fixture changes exactly the intended node and leaves every
   comment, blank line and quote style byte-identical (asserted on the full text, not a substring).
2. A postcondition mismatch writes nothing and returns `EditError::Verification`; a test proves the
   file is unchanged and that a route with `static`/`php` blocks is reproduced rather than truncated.
3. Applying the same action twice is a no-op (no duplicate `query`/`use`/`view`).
4. `critical_guard.py` passes for the three rewritten suites; no assertion was weakened, and
   `RESULT.md` lists each edit to them with the reason (format change only).
5. `make check` exit 0.

# Notes

- Port `edit_branches.rs`'s branch coverage rather than deleting cases: the TOML patcher's bracket and
  comment branches have Blastfile twins (a comment inside the grant block, a trailing comma, a
  missing final newline, CRLF).
- Do not touch `crates/blastit/src/dev_learn.rs` here; wiring `--learn` to the new editor is
  `MB.21.4.3`.
