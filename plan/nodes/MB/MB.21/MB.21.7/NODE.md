---
id: MB.21.7
title: Repoint the e2e harness at the moved PHP page and replay its two commands
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:08"
updated: "2026-11-17T06:13"
---

# Goal

`MB.20`'s restructure moved `docs/frameworks/php.md` to `docs/frameworks/php/index.md`. The e2e
harness still bind-mounted and read the **old** path, so Docker created `docs/frameworks/php.md` as a
root-owned **directory** at the mount point, which broke both the PHP leg of `make e2e` and
`make doc-tone`. Repoint every host-side reference at the real file while keeping the **container**
path `/opt/e2e/php.md` unchanged, and update the page *contract* the rewritten page now implies:

- the page has exactly **two** `$ ` commands — `blastit dev --learn` and `blastit fmt --check` — so
  `command_kind` gains `fmt-check`, and `replay` runs `blastit fmt --check` on the fixture (exit 0,
  fixture bytes unchanged);
- the stale `uv tool install` / preset assertions become "the page's two commands" and "the binary
  comes from the release tarball, not the wheel".

# Context

- Files carrying the old path: `e2e/compose.yaml`, `e2e/run.py`, `e2e/php2shell.py`,
  `e2e/php/fixture/Blastfile`, `e2e/php/Dockerfile`, `e2e/php/README.md`.
- The repoint edits and the `fmt-check` contract are **already present in the working tree,
  uncommitted** (written 2026-11-17 05:53–05:54, before this node existed): `compose.yaml:133` mounts
  `../docs/frameworks/php/index.md:/opt/e2e/php.md:ro`, and `php2shell.py:157-163` maps
  `"blastit fmt --check"` → `fmt-check` with `php2shell.py:1210` asserting
  `kinds == ["dev-learn", "fmt-check"]`. Verify every one of them by reading the file; do not
  re-apply, and do not "improve" the contract away from two commands.
- Stale bytecode hides this class of bug: delete `e2e/__pycache__` before grepping, or a `.pyc`
  (or, worse, a missing hit) lies to you.
- Read `docs/frameworks/php/index.md` before touching the probe list: the page is the source of
  truth, `e2e/php/probes.md` and the `php2shell` matrix must not promise anything the page dropped.

# Acceptance

- `grep -rn 'frameworks/php\.md' e2e/ --include='*.py' --include='*.yaml' --include='*.md' --include='Dockerfile' --include='Blastfile'` → no output.
- `ls -d docs/frameworks/php*` → only `docs/frameworks/php` (a directory, not a file).
- One **clean single-invocation** `make e2e` on an idle host → the PHP leg executes the page's two
  commands verbatim and the suite reports `263+ PASS`, `0 FAIL`.
- `make doc-tone` → passes.
- `make docs-check` → passes.

# Notes

- Never overlap two `make e2e` runs in one checkout: container names are fixed and a duplicate run
  poisons both logs. Check `pgrep -af 'make e2e|e2e/run.py'` first and wait for an idle host.
- Keep the container path `/opt/e2e/php.md` — scripts, docs and the `--page` flag all name it; only
  the host side moves.
- The page's recording goes in `RESULT.md`: the exact `make e2e` command, its PASS/FAIL tally, and
  the tail of `blastit fmt --check`'s verdict inside the PHP container.
