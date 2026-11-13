---
id: MA.11
title: "Leaf 18: remove the request-line ordering race in smoke pretty-format"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/tests/smoke.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T12:42"
updated: "2026-11-13T12:57"
---

# Goal

Remove the ordering race in `pretty_renders_a_forwarded_event_as_the_human_line` so the release CI stops flaking, without weakening what it asserts.

# Context

`smoke (root)` on PR #62 failed on `crates/blastit/tests/smoke.rs:1628`:

```text
thread 'pretty_renders_a_forwarded_event_as_the_human_line' panicked at crates/blastit/tests/smoke.rs:1628:28
no request line in: <captured stdout>
test result: FAILED. 30 passed; 1 failed
```

The same binary passed **31/31** as root in the previous run on a commit whose `crates/` content is identical (`dc553ad` → `0f926da` touched only `.github/workflows/ci.yml` and `plan/`). So this is a race in the test, not a product regression.

Mechanism: the test calls `server.wait_for_event("sql.denied")`, then reads `server.stdout_text()` once and expects the ` request ` line for `GET /query`. `wait_for_event` polls up to `EXIT_TIMEOUT`, but the request line is emitted *after* the denied event depending on process scheduling — under load the single snapshot can miss it.

# Acceptance

1. The test waits for its own precondition instead of assuming ordering: e.g. a bounded poll (same 20 ms cadence and `EXIT_TIMEOUT` budget as `wait_for_event`) until a line matching ` request ` + `GET /query` exists, then asserts the same properties on that line. On timeout it must print the full captured log (as it does today) — a flake must still be diagnosable.
2. Every existing assertion stays: route reaches the line (`→ data`), status `200`, duration ends in `ms`, no `method=` field dump, not raw JSON, and the same "written once" property the test's comment claims. Do not relax or delete any of them.
3. Run the test repeatedly to show the race is gone, not merely less likely: at least 20 consecutive runs as root with the same env the CI job uses (`BLASTIT_TEST_PG_URL` set, a PostgreSQL reachable) — paste the pass count. Run as the unprivileged user too if the test's `needs_root` guard makes that meaningful.
4. `make check` exit 0; `uv run --no-project tools/plan.py validate` ok; `make critical-guard BASE=0f926da` clean (smoke.rs is not a protected `critical_*` test, but prove it).
5. State what the next CI run must show: `smoke` as root `test result: ok. 31 passed`, and `smoke (root)` job success.

# Notes

- Do not raise timeouts to hide the race; the wait is bounded and the assertion is what must change.
- Do not touch `critical_*` tests, the manifest/check logic, or the workflow (MA.9/MA.10 own it).
- If you find the ordering is *guaranteed* by the product and the test's assumption is therefore correct, prove it from the event encoder's flush/ordering path instead of papering over it — but then the flake needs a product fix, so say so and stop for a decision.
- Do not push; the conductor pushes and watches CI.
