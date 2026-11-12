---
id: MA.2.3.11
title: "Fix the live-server suite: assert R24-redacted denial and pin the dev memory watermark"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [python/tests/test_live_server.py, python/blastit/pytest_plugin.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T21:20"
updated: "2026-11-12T21:52"
---

# Goal

Make the standard Python live-server tier truthful and host-independent: `test_live_server.py` must assert the R24-redacted SQL form (never a raw literal that can never appear), and the plugin's live-server environment must pin the memory watermark so a loaded host does not turn every route into a 10 s `503 queue_timeout`.

# Context

- Found at the MA.2.3 epic gate by the MA.2.3.7 reviewer (see `docs/testing/AUDIT-MA.2.3.md`, `plan/nodes/MA/MA.2/MA.2.3/MA.2.3.7/RESULT.md`).
- `python/tests/test_live_server.py:191,259` asserts `"SELECT 'denied'"` is inside `missing_sql("live-denied")`, but the live report is built from `sql.denied`, which R24 redacts to `SELECT …` (observed `['SELECT 1', 'SELECT …']`). The test can never pass in any mode.
- `python/blastit/pytest_plugin.py:1657-1664` (`_live_server_environment`) does not pin `BLASTIT_MEM_HIGH_WATERMARK`; `Scheduler::make_room` refuses the view spawn above the 85 % default (this host sits at 89 %), so every route answers `503 queue_timeout` after 10 s. `crates/blastit/tests/critical_dev_and_fail_closed.rs` already pins `100` for the same reason.
- Skills `python-standards`, `testing`, `dx-first`.

# Acceptance

```console
$ DATABASE_URL=... uv run --no-project --with pytest --with psycopg --offline pytest python/tests/test_live_server.py -q
# all green, no skip/xfail
$ uv run --no-project tools/plan.py validate
```

- `test_live_server.py` asserts the redacted `SELECT …` denial **and** that the raw literal is absent from the report, keeping R24 intact (both the denied and the allowed case asserted).
- `_live_server_environment` pins `BLASTIT_MEM_HIGH_WATERMARK=100` (with a one-line comment naming `make_room`), and the suite passes on a host above the 85 % default.
- No assertion is deleted or skipped; a test that would still pass with the literal-redaction rule removed must be rejected.

# Notes

- Do not touch Rust, docs, or `python/tests/project/`.
- If the 85 % default is itself a DX trap worth documenting or changing, record it under Follow-ups (a `MA.3` docs item) instead of changing scheduler policy here.
