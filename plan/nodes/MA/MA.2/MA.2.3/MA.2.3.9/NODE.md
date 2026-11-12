---
id: MA.2.3.9
title: "Pin check --learn refusal exit 1 and wrote=false end to end"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MA.2.3.6]
paths: [python/tests/critical/test_check_contract.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T20:50"
updated: "2026-11-12T21:52"
---

# Goal

Pin R29's end-to-end exit code: `blastit check --learn --json` that observed a refused statement
exits `1`, prints `"wrote": false`, and leaves `blastit.toml` byte-identical.

# Context

`docs/testing/AUDIT-MA.2.3.md` finding F5, proved by mutation #18: deleting
`plan.refused.is_empty()` from `crates/blastit/src/check.rs:940` keeps every test green.

```rust
let clean = plan.requests > 0 && plan.refused.is_empty() && failure.is_none();
```

The engine pins `LearnPlan::would_write` (`critical_check_engine.rs:867`) and the contract pins
"writes nothing when the suite fails", but nothing pins the exit code path for a refusal. §1.1:
`1` = "`--learn` refused a statement / observed `0` requests", `"wrote": false`.

# Acceptance

```console
$ PYTHONPATH=target/python-critical:python uv run --no-project --with pytest --offline \
    pytest python/tests/critical/test_check_contract.py -q
$ make test-critical
```

- New protected test in `test_check_contract.py`: a mini-runner whose suite sends an inline-literal
  statement (e.g. `SELECT 1 WHERE 's3cr3t' = 's3cr3t'`) through the real dbproxy learn policy →
  exit `1`, one JSON document with `"wrote": false`, no `[[route]]`/`queries` line added, and the
  refusal reason on stderr. A positive twin in the same test proves a parameterised statement in
  the same shape writes and exits `0`.
- The test must fail when `plan.refused.is_empty()` is deleted from `check.rs` (verify once by hand
  in a scratch copy; do not commit the mutation).
- The refused statement's literal never appears in the document or on stderr (R24).

# Notes

- Depends on the CI-job repair task: both touch `test_check_contract.py`.
- Uses the existing `STUB_PYTEST` + `MockPgServer` harness; no new dependency.
