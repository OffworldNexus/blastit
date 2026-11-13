---
id: MA.2.3.10
title: "Pin three unprotected audit_v1 rules: add_query with ellipsis, no_observations zero statements, would_write zero requests"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [crates/blastit/tests/critical_check_engine.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T20:50"
updated: "2026-11-12T22:05"
---

# Goal

Close the three `audit_v1` rules that survive deleting the guard they claim to protect: `add_query`
carrying `…`, `no_observations` on a suite with requests but no statements, and `would_write` with
zero requests but unrouted SQL.

# Context

`docs/testing/AUDIT-MA.2.3.md` findings F6/F7/F8, proved by mutations #2, #6 and #9 (all survived
the whole crate and the Python contract).

1. `audit_v1.rs:661-673`: the `add_query` branch is guarded by `!normalized.contains('…')`; only a
   `Refused` verdict is ever fed to it. A lying `Query` verdict with a redaction must stay `observe`
   (R33: "every `add_query` is bootable"). The `plan_learn` twin exists at
   `critical_check_engine.rs:867`; the engine has none.
2. `audit_v1.rs:695`: `exercised == 0 || (!proxies.is_empty() && statements == 0)`. The empty-suite
   test only covers the first clause; `ingress_findings_have_positive_twins` makes requests with no
   SQL but asserts only the absence of ingress kinds. §4.2 E.3 names the second clause (R35).
3. `audit_v1.rs:1171`: `would_write`'s `self.requests > 0` is masked by the existing zero-request
   test, which also has nothing to write. A run with 0 HTTP requests but an unrouted SQL statement
   (the gate can observe SQL without a request; `plan_learn` takes paths from SQL too) must render
   `"wrote": false`.

# Acceptance

```console
$ cargo test -p blastit --test critical_check_engine --locked
$ cargo test -p blastit --lib --locked
$ make test-critical
```

- `critical_check_engine.rs` gains three cases (allow + deny twin each):
  (a) a routed statement with `verdict.kind == "query"` whose `normalized` contains `…` yields
  `missing_grant` with `fix.action == "observe"` and no `to`; the grantable twin still yields
  `add_query`;
  (b) a manifest declaring one proxy, a suite with ≥ 1 routed request and zero statements →
  `no_observations` exactly once; the twin with one statement has none;
  (c) `plan_learn` over an observation set with `requests == 0` and an unrouted `SELECT 1` →
  `would_write(false) == false` and `json(false)["wrote"] == false`; the twin with one HTTP line
  writes.
- Each new case fails with its mutation applied (spot-check once in a scratch copy).

# Notes

- `critical_check_engine.rs` is protected; adding tests is allowed by `critical_guard.py`.
- Keep the file under one screen per case; reuse `run()`, `kinds()`, `grantable()`.
