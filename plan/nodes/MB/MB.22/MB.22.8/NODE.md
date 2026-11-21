---
id: MB.22.8
title: "Make the blastit lsp page's exit codes and uv form true (1 also for an unframeable stream, 2 for a bad argument, --no-sync for the editor)"
kind: task
status: superseded
owner: "doc-writer"
priority: 2
depends_on: []
paths: [docs/reference/cli.md, docs/local.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T07:14"
updated: "2026-11-17T07:22"
---

## Goal

Correct the two `blastit lsp` claims in `docs/reference/cli.md` that the shipped code does not
make, and the vague `uv run` sentence in `docs/local.md` §Editors.

## Context

`AUDIT-MB.22.md` §1.3 nit 7 and `docs/design/blastfile-lsp-zed.md` §12.8's "a follow-up docs node
(MB.22.1 is closed)" row. Measured on `d08d04c`:

- `docs/reference/cli.md` §`blastit lsp` says "It exits `0` after the client's `shutdown`, and `1`
  only if the client disappears without one". The code also exits `1` for a stream it cannot frame
  (a junk header, a header block over 8 192 bytes, a body over `MAX_FRAME_BODY_BYTES`) and `2` for a
  bad argument (`blastit lsp --bogus`, `blastit lsp serve`); `critical_lsp.rs::exit_codes` and
  `lsp_subcommand_is_reachable` pin all of them.
- The page offers `uv run blastit lsp`; the extension deliberately runs
  `uv run --frozen --no-sync --quiet blastit lsp` (§8.3), because a bare `uv run` syncs the project
  and executes the worktree's build backend.
- `docs/local.md` says the extension starts `blastit lsp` "from `PATH` or through `uv run`" — say the
  exact argv once.

## Acceptance

```console
$ grep -n "only if the client disappears" docs/reference/cli.md   # no match
$ grep -n -- "--no-sync" docs/reference/cli.md docs/local.md      # both pages name it
$ make docs-check
$ make docs-unit
```

- Three rows added or corrected: `1` also covers a stream the server cannot frame, `2` is a bad
  argument, and the editor's form is `uv run --frozen --no-sync --quiet blastit lsp`.
- No claim the code does not make; every command in the page still runs.
