---
id: MA.8
title: "Leaf 15: root-cause the two red Django check tests on the release CI"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - python/tests/test_check.py
  - "crates/blastit/**"
  - "crates/blastit-scheduler/**"
  - "crates/blastit-http/**"
  - "crates/blastit-view/**"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-13T11:21"
updated: "2026-11-13T11:48"
---

# Goal

Make the two remaining red `blastit check` tests in the release CI pass for the right reason, and prove whether the 0.1.1 hot-path backport changed any observed request/route semantics.

# Context

Release PR #62 (branch `release/0.1.1`). After MA.7 fixed the collection errors, the `pytest plugin (CPython 3.14, PostgreSQL)` job now runs the real suite and fails **2 of 368** (run 43407063410, job 131823878809):

```text
FAILED python/tests/test_check.py::test_pytest_django_db_suite_has_zero_teardown_warnings_and_drops_test_db
FAILED python/tests/test_check.py::test_pytest_django_atomic_view_still_enforces_transaction_control
E       assert 1 == 0        # completed.returncode / granted.returncode
python/tests/test_check.py:393, :555
```

Both run `python -m blastit check --json -- -W error::pytest ...` and expect exit `0`. Exit `1` means `check` reported **findings**, so pytest's own output ("1 passed") is not the problem — the audit engine is flagging something.

The tests are in `python/tests/test_check.py` (not `python/tests/critical/`), and the failing assertions print only `stderr`, so the findings JSON is not in the CI log — you must reproduce it.

**Prime suspect:** MA.6 backported `4493699` "Scheduler: forward the request head without decoding it" (flow-key equality was proven for fair queuing, but the *observation/audit* path may read the head differently), plus the in-place response-head encoding and pooled view threads. A raw-vs-decoded head difference in what `check --learn`/`check` records could surface as a spurious route/grant finding — or hide a real one. Do not assume; get the JSON.

Second suspect: the Django test-database teardown path (`drops_test_db`) interacting with pooled view threads/thread reuse — a leftover connection would leave the DB alive and produce a teardown warning.

# Acceptance

1. Reproduce locally under the CI's conditions (venv with `pytest pytest-django django psycopg[binary]`, a PostgreSQL reachable at `DATABASE_URL`, the built wheel) and paste the **full findings JSON** for both tests, naming each finding.
2. Root-cause each finding: is it a real behaviour change from the MA.6 backport (`crates/blastit-scheduler`, `blastit-http`, `blastit-view`), a stale fixture expectation, or an environment difference? State the evidence (files, commits, or the code path). No guessing.
3. Fix at the right layer: if the backport changed observable semantics, fix the code and explain why the change is correct and still matches the 0.1.1 ingress/audit contract. If the tests encode a stale assumption, update the test with a comment explaining what changed. Never weaken an assertion just to get green.
4. `make check` exit 0 and the two tests pass locally with the same command shape the CI job uses (`python -m blastit check --json -- -W error::pytest ...`).
5. If a guard is affected, `make critical-guard BASE=<pre-change-sha>` stays clean.
6. Confirm whether the `smoke (root)` job (MA.7's sudo-rs fix) went green in the same run and report it.

# Notes

- The audit findings are user-facing DX (the `--json` report is what an AI agent consumes); a spurious finding in 0.1.1 would be worse than a red build, so treat this as DX-critical, not test plumbing.
- Do not push; the conductor pushes and watches CI.
- If the root cause turns out to be an unrelated pre-existing flake, prove it (same failure on `develop`'s base commit) and open a follow-up node instead of hiding it.
