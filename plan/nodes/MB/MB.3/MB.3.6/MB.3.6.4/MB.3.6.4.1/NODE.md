---
id: MB.3.6.4.1
title: "Two-shape http_obs reader, executor_ran accounting and the learn static proposal - design MB.3.6c part 1"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.3.6.3.3]
paths: [crates/blastit/src/audit_v1.rs, crates/blastit/src/dev_learn.rs, crates/blastit/src/check.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-18T20:36"
---

# Goal

Make the observation path honest for a route with no Python: the two-shape
`http_obs.jsonl` reader, `CheckObsV1::new_with_upstream`, the combined-set guards,
`executor_requests_per_route` (`executor_ran`), the `413` `missing_ingress` finding and the
`--learn` static proposal (design leaf `MB.3.6c`, part 1: the pure/audit half).

# Context

- Design of record: this brief's §4.10.1 (one file, two line shapes, one write path),
  §4.10.2 (`--learn` proposals) and §4.10.3 (`check` reporting), over the embedded worker's
  observations (`ADR-0010` §1); the retired design that first wrote those shapes is deleted
  with `ADR-0010`.
- Files (the whole leaf, 3): `crates/blastit/src/{audit_v1,dev_learn,check}.rs`.
- What lands:
  - The two-shape reader: a legacy `HttpObsLine` **without** `kind` still parses, unchanged;
    the new `Observation` lines (`wire_request`/`param`/`static_file`/`output`) parse into
    the same `CheckObsV1` via `CheckObsV1::new_with_upstream`.
  - Combined-set empty guards in `unexercised_routes`/`unrequested_static_roots` (an empty
    combined set never reports "everything unexercised").
  - `executor_requests_per_route` and the `executor_ran` accounting: `true` for every
    `exchange` answer, `false` only for a pre-executor `plan` head `413`, so `missing_ingress`
    fires without counting the un-run front controller in `routes.exercised`.
  - The `413` → `missing_ingress` finding and the `--learn` static proposal
    (`[route.static] root = "public"`).
  - Nothing PHP-specific on the observation path: `Param` carries no value, and a
    `?token=`/header/cookie sentinel never reaches `http_obs.jsonl`.
- `CheckObsV1` keeps `Debug, Clone, PartialEq, Eq` and **no** serde derive; the derive
  matrix in §8 is authoritative for `Observation`/`ObservationKind`/`ObsTarget`/
  `ParamDecision`/`OutputStream` (defined in `MB.9.1.1`).
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit --locked
$ grep -q 'new_with_upstream' crates/blastit/src/audit_v1.rs && echo "one reader, two line shapes"
$ grep -q 'executor_requests_per_route' crates/blastit/src/audit_v1.rs && echo "executor_ran accounting lives here"
$ make check
```

# Notes

- `cli.rs`, `check::serve_and_run`, the 3-way branch order and `critical_php_learn.rs`
  are `MB.3.6.4.2`'s; this leaf is the reader and the pure audit logic only.
- `crates/blastit/src/check.rs` is shared with `.2`; the two leaves are serialized by
  `depends_on`, so land the reader side and leave the CLI half untouched.
- `executor_ran` is `#[serde(default = "default_true")]` on the `WireRequest` variant
  (`MB.9.1.1`), so an old line without the field still reads as `true`.
