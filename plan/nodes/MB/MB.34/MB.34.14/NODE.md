---
id: MB.34.14
title: "Make check --json's refusal contract a test: exit 2, empty stdout, one line:col on stderr (AUDIT-MB.34 G6)"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths: [python/tests/test_check.py]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T05:46"
updated: "2026-11-18T05:46"
---

# Goal

Make `docs/ci.md`'s refusal contract a test: `blastit check --json` on a refused `query` exits `2`,
writes zero bytes to stdout and one `line:col` sentence to stderr.

A CI script that reads empty stdout as "no findings" would silently pass a manifest PostgreSQL
cannot parse.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_check.py -q
```

A `Blastfile` holding `query 'SELCT id FROM t'` run through `blastit check --json` exits `2`, writes
zero bytes to stdout, and writes exactly one stderr line whose prefix is the fixture's `line:col`
and whose text is the page's `Syntax` sentence; plain `check` exits `2` with the same line; the
assertion fails if the refusal is turned into a JSON finding.

# Notes

- Today this is proved only by the recorded transcript in `docs/testing/E2E-MB.34.md` §3.
