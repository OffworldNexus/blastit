---
id: MB.26.52
title: "Gate the CI refusal contract: check --json exits 2 with empty stdout"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: [python/tests/test_check.py, crates/blastit/tests/critical_static_cli.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-19T00:36"
---

# Goal

`docs/ci.md`'s new paragraph is a CI contract: "A `query` that does not parse is not a finding
… `check` exits `2`, writes the refusal to stderr — with the `line:col` before the sentence —
and prints nothing on stdout: `--json` included, and an empty stdout is a refusal, never a
clean audit." It is proved today only by the hand-recorded `docs/testing/E2E-MB.34.md` §2.

Add the gated test: a refused `query` through `blastit check --json`.

# Context

- `python/tests/test_check.py` already asserts the exit codes and the empty-stdout rule for a
  *failing pytest suite* (`test_a_failing_suite_is_exit_three_with_no_document`) but not for a
  refused manifest.
- `crates/blastit/tests/critical_static_cli.rs` is the existing CLI critical surface; either
  home is acceptable, the Python one is cheaper.

# Acceptance

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_check.py -q
$ cargo test -p blastit --test critical_static_cli
```

A project whose `Blastfile` holds `query 'SELCT id FROM t'` run through
`blastit check --json`: exit `2`, zero bytes on stdout, exactly one stderr line whose prefix is
the fixture's `line:col` and whose text is the page's `Syntax` sentence; plain `check` exits `2`
with the same line. The assertion fails if the refusal is turned into a JSON finding.

# Notes

- Do not add a new `kind` or `fix.action`; that is the paragraph's point.

# Provenance

Re-filed from `MB.34.7.6` (`AUDIT-MB.34` G6) at the `v0.2.0` gate: automated CLI test for `blastit check --json` exiting `2` with empty stdout on a broken `query` (verified live in `E2E-MB.34.md`).
