---
id: MB.34.15
title: "Run the tree-sitter corpus expectations in the zed gate (AUDIT-MB.34 G7)"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: [scripts/check_zed_extension.py, scripts/test_check_zed_extension.py, Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:46"
updated: "2026-11-18T05:46"
---

# Goal

Run the tree-sitter corpus expectations (`tree-sitter test`) in a gate, so a regression that keeps
`sql_string` but changes which children it holds fails.

`make zed-check`'s loop only proves the fixtures parse without `ERROR`/`MISSING`; the three new
corpus files encode §9.2a's trees and nothing runs them.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make zed-check
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
```

`make zed-check` runs `tree-sitter test` for `editors/tree-sitter-blastfile` when the pinned
generator is present and fails on a mismatch; `--require-parse` also requires the corpus run; a
self-test doctors one expectation in a copied corpus file and requires the exact `FAIL` line; a
host without the pin still prints a `note`, never a silent `ok`.

# Notes

- The CI wiring (`Makefile`, `.github/workflows/ci.yml` installing the pin) belongs to `MB.35.6`;
  until it lands this guard can only run where the generator is installed.
