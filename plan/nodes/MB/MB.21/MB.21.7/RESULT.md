# MB.21.7 — the harness reads the moved PHP page

## Summary

`MB.20` moved `docs/frameworks/php.md` → `docs/frameworks/php/index.md`; the harness kept bind-mounting the old path, so Docker created it as a root-owned directory — breaking the PHP leg and `make doc-tone` at once. Every host-side reference now points at `index.md` (container path `/opt/e2e/php.md` unchanged), and the page contract follows the rewritten page: its two `$ ` commands are `blastit dev --learn` and `blastit fmt --check`, so `command_kind` learns `fmt-check` and `replay` runs `blastit fmt --check` on the fixture (exit 0, fixture bytes unchanged).

## Evidence

- After `rm -rf e2e/__pycache__`, `grep -rn 'frameworks/php\.md' e2e/ --include='*.py' --include='*.yaml' --include='*.md' --include='Dockerfile' --include='Blastfile'` → no output, `exit=1`.
- `ls -d docs/frameworks/php*` → `docs/frameworks/php` only; `e2e/compose.yaml:133` mounts `../docs/frameworks/php/index.md:/opt/e2e/php.md:ro`.
- One clean `make e2e` on an idle host → `E2E_EXIT=0`, **311 `PASS`/`ok`, 0 `FAIL`** (`/tmp/opencode/mb21-e2e-clean.log`), tail: `ok and the restarted boot serves it (200)`, `ok still 0 decoy opens (0)`.
- `make doc-tone` → ok. `make docs-check` → `No issues found` (`zensical build --strict`).

## Files

`git show --stat 33661e7`, e2e side:

```
e2e/compose.yaml           |  2 +-
e2e/php/Dockerfile         |  2 +-
e2e/php/README.md          | 13 +++----
e2e/php/fixture/Blastfile  |  2 +-
e2e/php2shell.py           | 52 +++++++++++++++++++++++-----
e2e/run.py                 |  2 +-
```

## Follow-ups

- One `make e2e` per checkout: overlapping runs share fixed container names and poison both logs — `pgrep -af 'make e2e|e2e/run.py'` first.
- The `P6 E10` 502 was a product race, not this node's defect; filed as `MB.26.8`.
- The page of record is now `docs/frameworks/php/index.md`: any further restructure must move the harness mount with it, or Docker recreates the old path root-owned.
