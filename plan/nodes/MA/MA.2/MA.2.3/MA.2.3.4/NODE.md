---
id: MA.2.3.4
title: "Leaf 11: check audit engine, route inference and learn plan"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MA.2.3.3]
paths:
  - crates/blastit/src/lib.rs
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/src/infer_routes.rs
  - crates/blastit/tests/critical_check_engine.rs
  - "crates/blastit-py/src/**"
  - "crates/blastit-py/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:12"
---

# Goal

Build the framework-agnostic audit engine: `CheckObsV1` (`schema_version == 1`), route inference, the nine-kind `audit_v1::evaluate` with a guaranteed-bootable `add_query` contract, `plan_learn` for unrouted observations, and the `_blastit.{check_audit, plan_learn, probe_ingress}` Python bridge — shared by `blastit check` and `blastit-py`.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §1.3, §4.2 E.2/E.3/E.4, §7.2 row `R27..R28, R32..R38`, §8 Leaf 11; `ADR-0003`, `ADR-0004`; requirements `R27`, `R28`, `R32`, `R33`, `R34`, `R35`, `R36`, `R37`, `R38`.
- `infer_routes` must reproduce all 13 fixture rows exactly (name sanitizer trims **both** leading and trailing non-`[a-z0-9]`, `/` -> `root`, digits -> `[0-9]+`, UUIDs -> `[^/]+`, `-2`/`-3` disambiguation, insertion above both catch-all spellings).
- `missing_grant` emits `add_query` **only** for grantable parameterized SQL; redacted `…` and transaction-control denials (savepoints normalized to `"SAVEPOINT"`) emit `{"action":"observe"}`. `dead_grant` covers route and group grants via `Manifest::is_query_covered_by_tables_or_groups`; `unexercised_route` is suppressed when `obs.http.is_empty()`.
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit --test critical_check_engine --locked
$ cargo test -p blastit-py --test critical_check_schema --locked
$ make check
```

- `crates/blastit/tests/critical_check_engine.rs` is **new**: all 13 `infer_routes` rows; the `R27` golden (unrouted `POST /orders/42/` running 2 queries -> `[[route]] name = "orders"` with those exact `queries`); every emitted `add_query` re-parses under `Manifest::parse_and_validate`; the exact §1.3 human lines byte-for-byte; all nine finding kinds plus their interaction counts.
- `crates/blastit-py/tests/critical_check_schema.rs` is **new**: `CheckObsV1::new` rejects `schema_version != 1`; Python `HttpObsLine` -> `_blastit.check_audit` conformance; `statements == sum(proxies[*].statements)`; `ok == no error-severity finding`.
- Negative twins: redacted/`SAVEPOINT` denials never emit `add_query`; `--learn` with an inline literal or 0 requests sets `"wrote": false` (behaviour owned by `MA.2.3.5`, asserted here at the planner level); no route-less `missing_grant` for `ObsTarget::Unrouted`.

# Notes

- ~400 changed lines. Do not create `crates/blastit/src/check.rs` (Leaf `MA.2.3.5`); do not touch `python/blastit/fold.py` or `Makefile`.
- The human formatter table in §1.3 is normative; assert spaces and commas, not just substrings.
