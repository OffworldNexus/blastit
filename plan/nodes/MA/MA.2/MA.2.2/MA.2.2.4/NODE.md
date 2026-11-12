---
id: MA.2.2.4
title: "Leaf 7: dbproxy learn, dynamic-check and literal redaction"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MA.2.2.1]
paths: ["crates/blastit-dbproxy/src/**", "crates/blastit-dbproxy/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:12"
---

# Goal

Turn `blastit-dbproxy` into the wire observation source of truth for `0.1.1`: redact every SQL literal before any denial or observation leaves the proxy, add the `Enforce`/`Learn`/`DynamicCheck` listener modes with typed `ObsTarget`/`WireSqlObs`, the JSON-line `check_ctl_fd` control channel, and one-shot introspection/session-setup replay.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §4.2 D.3, §5 (`S1`), §7.2 row `R26 (db), R31`, §8 Leaf 7; `ADR-0003`, `ADR-0004`; requirements `R24`, `R26`, `R31`.
- `ObsTarget::{Routed { route }, Unrouted { method, path }}` is serde-tagged `scope`/`snake_case`; `WireSqlObs.sql` is always `redact_literals(normalized)`; `ListenerSpec.route == "*"` is accepted only when `mode != Enforce`; `RoutePolicySpec` derives solely from `Manifest::effective_grants`.
- `DynamicCheck` and `Learn` always return `QueryDecision::Allow` and never emit `sql.denied` on stdout; `Enforce` still returns `42501` and rejects a root listener unless `allow_root_listener`.
- Session setup (`SET`, `SHOW`, `KNOWN_STATIC_SETUP_SQL`, read-only `pg_catalog.version()`/`current_schema()`/`to_regtype($?)`) is recorded per session (max 8) while `active_route_stack` is empty and replayed once on the first routed statement; fixture/migration SQL under an empty stack is un-audited.
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit-dbproxy --test critical_learn_and_redaction --locked
$ cargo test -p blastit-dbproxy --locked
$ make check
```

- `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs` is **new**: empty-stack fixture `INSERT` produces zero `obs_fd` lines while `SET TIME ZONE 'UTC'` and `select pg_catalog.version()` are recorded and replayed once on first `push_routed`; `DynamicCheck` allows an ungranted statement; `set_db` switches the database.
- Negative twins: `Enforce` returns `42501`; `'s3cr3t'` appears in **no** byte of the `SqlDenied` event or `obs_fd` stream; a `uid == 0` listener without `allow_root_listener` is rejected; malformed/oversized `ctl` lines time out after 5 s instead of hanging.
- Uses `MockPgServer` on `127.0.0.1:0`; no root, no Docker.

# Notes

- ~420 changed lines in `crates/blastit-dbproxy/**`; parallel with `MA.2.2.3`.
- Do not add `python/**` or `crates/blastit/src/**` changes; the orchestrator is Leaf `MA.2.3.5`.
