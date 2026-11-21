---
id: MB.34.7.7
title: "Run the tree-sitter corpus expectations inside make zed-check"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: [scripts/check_zed_extension.py, scripts/test_check_zed_extension.py]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:46"
updated: "2026-11-18T20:36"
---

# Goal

Three new corpus files encode §9.2a's trees (`query-sql.txt` 22 cases, `query-hash.txt` 4,
`query-spelling.txt` 4; 30 of the 34 expectations name `sql_string`'s children). No gate runs
`tree-sitter test`: `make zed-check`'s loop only checks that the 23 in-repo `Blastfile`s parse
with no `ERROR`/`MISSING`, and skips (with a note) when the pin is absent. A regression that
keeps `sql_string` but changes which children it holds passes every gate.

# Context

- `scripts/check_zed_extension.py` already locates the pinned generator (`tree_sitter()`), has
  a `--require-parse` mode and a parse cache helper.
- `scripts/test_check_zed_extension.py` builds faithful temp trees and requires the exact
  `FAIL` set — the model for the new self-test.

# Acceptance

```console
$ make zed-check
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
```

`make zed-check` runs `tree-sitter test` for `editors/tree-sitter-blastfile` when the pinned
generator is present and fails on any corpus mismatch; `--require-parse` also requires the
corpus run; a self-test doctors one expectation in a copied corpus file and requires the exact
`FAIL` line; the skip path still prints a note (never a silent `ok`) on a host without the pin.

# Notes

- Keep the guard stdlib-only and offline; `tree-sitter test` is the existing binary, not a new
  dependency.
