---
id: MA.3.13
title: "Run the docs-gate unit suites in make check and pin the harness checks in-repo"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [Makefile, python/tests/runnable_docs/test_harness.py, python/tests/runnable_docs/harness.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T02:52"
updated: "2026-11-13T03:10"
---

# Goal

Close the last two gate holes the MA.3 audits left open: `make check` never runs `python/tests/test_runnable_docs.py` (so a developer who breaks the fence scanner still gets a local green), and the harness's own static-check logic has no in-repo regression tests (the 13 red probes live only in `/tmp`).

# Context

- Found by the MA.3.9 and MA.3.10 reviewers and `docs/testing/AUDIT-MA.3.md`: the new unit suite is only collected by the `pytest python/tests` CI job, not by `make check`; `assert_flag_tables()` and `_require_commands()` are unpinned in-repo; the doctl flag tables are absent from the audit set, so an invented `doctl` subcommand/flag can still pass.
- Skills `python-standards`, `testing`.

# Acceptance

```console
$ make docs-test
$ make check
$ grep -n 'test_runnable_docs\|test_harness' Makefile
```

- `make check` runs the runnable-docs unit suites (pinned `--with pytest==9.2.0`, offline) — either through `docs-test` or a `py-test` target — so reverting `fences.scan_page` or `assert_flag_tables()` turns a local gate red; prove by reverting one guard in a /tmp copy.
- `python/tests/runnable_docs/test_harness.py` (new) pins allowed/denied pairs for: flag-table drift (an invented flag is refused), a nonexistent `-f/--file/--spec/--manifest` path, the zero-command rule, and wheel/provenance mismatch.
- The doctl tables join the flag audit (`--audit-flags` reports `not installed here: doctl` on this host) and the docstring/RESULT claims match what is audited.
- No skip/xfail masking; `make docs-test` stays under ~5 minutes.

# Notes

- Paths are the Makefile, the new test file, and `python/tests/runnable_docs/harness.py` (flag tables only). Do not edit docs prose or crates/.
