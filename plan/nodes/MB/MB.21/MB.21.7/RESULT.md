# Result MB.21.7

## Summary

MB.20's restructure moved `docs/frameworks/php.md` to `docs/frameworks/php/index.md`; the e2e harness
kept bind-mounting the old path, so Docker created it as a root-owned **directory**, which broke the
PHP leg (`IsADirectoryError`) *and* `make doc-tone` at once. Every host-side reference now points at
`index.md` (container path `/opt/e2e/php.md` unchanged), and the page **contract** follows the
rewritten page: exactly two `$ ` commands, so `command_kind` learns `fmt-check` and `replay` executes
`blastit fmt --check` on the fixture (exit 0, bytes unchanged); the stale `uv tool install`/preset
assertions became "the page's two commands" and "the binary comes from the release tarball, not the
wheel".

## Evidence

| Acceptance item | Command | Tail |
|---|---|---|
| Old path gone from the harness | `grep -rn 'frameworks/php\.md' e2e/ --include='*.py' --include='*.yaml' --include='*.md' --include='Dockerfile' --include='Blastfile'` (after `rm -rf e2e/__pycache__`) | no output, exit 1 |
| Old path is not a file | `ls -d docs/frameworks/php*` | `docs/frameworks/php` — the directory only |
| One clean `make e2e` on an idle host | `pgrep -af 'make e2e\|e2e/run.py'` (only its own `bash -c` matched) then `make e2e` → `E2E_EXIT=0` | `grep -cE '^PASS: \|^ok'` = **311**, `FAIL` = **0**; tail: `ok   and the restarted boot serves it (200)` / `ok   still 0 decoy opens (0)` |
| — page's two commands replayed | in the log (`/tmp/opencode/mb21-e2e-clean.log:559-561`) | `PASS: the page's command is replayed or explained: blastit fmt --check` / `PASS: blastit fmt --check accepts the page's fixture (0: '')` / `PASS: and --check changed nothing (it names a file, never rewrites one)` |
| `make doc-tone` | `make doc-tone` → `DOC_TONE_EXIT=0` | `doc-tone: ok — 30 user-facing page(s), no banned phrase` |
| `make docs-check` | `make docs-check` → `DOCS_CHECK_EXIT=0` | `uvx --offline zensical build --strict` … `No issues found` |

## Files

`e2e/compose.yaml`, `e2e/php/Dockerfile`, `e2e/php/README.md`, `e2e/php/fixture/Blastfile`,
`e2e/php2shell.py`, `e2e/run.py` (commit `33661e7`).

## Follow-ups

- **One `make e2e` per checkout:** overlapping runs share fixed container names and poison both logs
  (this node's first attempts failed that way). Run `pgrep -af 'make e2e|e2e/run.py'` first.
- The probe `P6 E10` 502 was *not* this node's defect: it is a product race (8 concurrent
  `/api/__inspect` → 1×200, 7×502 `{"outcome":"view_died"}`), filed as `MB.26.8`.
- `docs/frameworks/php/index.md` is now the page of record, so any further restructure must move the
  harness mount with it.
