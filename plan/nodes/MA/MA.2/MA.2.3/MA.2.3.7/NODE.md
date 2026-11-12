---
id: MA.2.3.7
title: "Re-pin test_pytest_plugin.py against Django 6.1 setup statements and the egress warning"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [python/tests/test_pytest_plugin.py, python/tests/project/blastit.toml]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T20:50"
updated: "2026-11-12T21:19"
---

# Goal

Make `python/tests/test_pytest_plugin.py` green under the dependencies the `pytest-plugin` CI job
installs (unpinned `django` = 6.1.3 at audit time, `pytest-django` 4.14, PostgreSQL 18), without
weakening the plugin's guarantees.

# Context

`docs/testing/AUDIT-MA.2.3.md` finding F3. Two independent expectations no longer hold:

1. `assert len(baseline.setup_statements()) == 2` (`:349`). Django 6.1 sends `set_config('TimeZone',
   …)` only when the server's reported `TimeZone` differs from `settings.TIME_ZONE`
   (`django/db/backends/postgresql/base.py::_configure_timezone`), so the baseline observes only
   `SET application_name = 'blastit-test'`. Cascade: `test_the_report_counts_what_the_manifest_does_
   not_cover`, `test_the_terminal_summary_is_short_and_actionable`, `test_strict_fails_the_session`,
   `test_the_report_is_enough_to_fill_the_manifest_in`,
   `test_more_than_one_proxy_needs_a_mapping`, `test_xdist_workers_are_merged`,
   `test_xdist_reports_are_not_merged_twice`.
2. `report["manifest"]["warnings"] == []`: since `80638f1` (MA.2.2.1) the fixture's
   `python/tests/project/blastit.toml:116` (`egress = true`, no address/port restriction) emits
   `route "note-cache": egress = true opens any outbound address and port`.

# Acceptance

```console
$ DATABASE_URL=postgresql://… PYTHONPATH=<installed wheel> \
    uv run --no-project --with pytest --with psycopg --with django --with pytest-django \
    --with pytest-xdist --offline pytest python/tests/test_pytest_plugin.py -q
# 39 passed, 0 failed
```

- The connection-setup expectations are driven by what the fixture actually makes the server send
  (pin the statement the connection receiver executes, or restrict the assertion to the statements
  present); do not delete the report-to-stdout assertions.
- The manifest-warning expectation pins the real warning (or the fixture restricts `egress` with an
  address/port, keeping the "opens any address" route that documents the warning). The manifest
  warning itself must not be removed.
- `test_live_server.py` and `test_check.py` in the same job stay green.

# Notes

- The warning is documented R11 behaviour from MA.2.2.1; the fixture is the part that drifted.
- Verify against the versions the job installs, not the local offline cache, if they differ.
