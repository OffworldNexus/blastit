---
id: MA.2.3.6
title: "Repair the check-contract suite and psycopg guard so the pytest-plugin CI job is green"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [python/tests/critical/test_check_contract.py, python/tests/test_plugin_helpers.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T20:50"
updated: "2026-11-12T21:19"
---

# Goal

Make the `pytest-plugin` CI job (`pytest python/tests`, `.github/workflows/ci.yml:222`) green for
the two deterministic failures the MA.2.3 gate audit found, without weakening any assertion.

# Context

`docs/testing/AUDIT-MA.2.3.md` findings F1 and F2.

**F1 — `python/tests/critical/test_check_contract.py` fails 7/10 whenever `pytest-django` is
importable** (i.e. in that CI job, which installs it). `run_check()` builds the child env from
`os.environ` + `PYTHONPATH=<project>/stub:<wheel>`; the stub dir shadows `pytest`.
`Project.install_django_stub()` writes a `pytest_django.py` stub only for the Django twins, so the
seven non-Django tests let `_check_pytest._refuse_unhandshaken_django()` import the **real**
`pytest_django`, whose `@pytest.fixture` decorators run against the stub `pytest` module →
`AttributeError: module 'pytest' has no attribute 'fixture'` → `blastit check` exits 3 where 0/1/2
is expected. `make test-critical` is green only because that interpreter has no `pytest_django`.

**F2 — `python/tests/test_plugin_helpers.py:61` raises `NameError`.** `d291078` (MA.2.3.1)
replaced `pytest.raises(Exception)` with `pytest.raises(psycopg.ProgrammingError)` while the
module-level guard at `:25` is a bare `pytest.importorskip("psycopg")`, which does not bind the
name.

# Acceptance

```console
$ PYTHONPATH=target/python-critical:python uv run --no-project --with pytest --with psycopg \
    --with django --with pytest-django --offline pytest \
    python/tests/critical/test_check_contract.py python/tests/test_plugin_helpers.py -q
# all pass (21 + 3 contract, 53 plugin helpers)
$ make test-critical
# still green, 49 python tests + every crates/*/tests/critical_*.rs
```

- Every `Project` in `test_check_contract.py` gets the empty `pytest_django.py` stub, including the
  non-Django twins; the dedicated "pytest-django is configured but no handshake was written" twin
  still proves `ControlError` → exit 3, empty stdout, zero upstream statements.
- `test_plugin_helpers.py` binds `psycopg` from `importorskip` (or imports it after the skip) and
  drops the wrong `# noqa: F821 - importorskip binds it` comment.
- No assertion is deleted, relaxed, or skipped to get green.

# Notes

- Both files are protected/owned by MA.2.3 (`python/tests/**`); adding to `critical_guard.py`'s
  protected set is unchanged (the contract file is already protected).
- Do not "fix" this by removing `pytest_django` from the CI job: the real-PostgreSQL Django twins in
  `python/tests/test_check.py` depend on it.
