---
id: MB.1.6
title: "Name missing ./blastit.toml first, add group-grant hint, allocate nothing for policy-less bodies"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: []
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/audit_v1.rs
  - "crates/blastit-scheduler/src/scheduler.rs"
  - crates/blastit/tests/critical_config_and_presets.rs
  - crates/blastit/tests/audit_group_hint.rs
  - "crates/blastit-scheduler/tests/head_allocs.rs"
  - docs/troubleshooting.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T14:44"
---

# Goal

Fix the three cheap-but-real DX defects from the `MA` gate: name the missing `./blastit.toml` before `BLASTIT_APP` at both entry points, teach `blastit check` to suggest a group-scoped grant for connection-setup SQL, and stop `Scheduler::plan_request_body` from allocating header `String`s on routes with no ingress policy — then reconcile `docs/troubleshooting.md`.

# Context

- **(a) Missing manifest** (`crates/blastit/src/stage0.rs:444` production, `:666` `dev`): `config.app.is_none()` is checked before `std::fs::read_to_string(&config.manifest)`, so an empty directory reports `BLASTIT_APP: the WSGI callable ... is required` instead of the missing `./blastit.toml` (`MA/RESULT.md` §3, item 1). Check manifest existence/readability first, then `app`. Exit code/stderr-only contract unchanged (startup errors leave stdout empty).
- **(b) Group-grant hint** (`crates/blastit/src/audit_v1.rs`, the `MissingGrant` `fix` construction around line 634): for connection-setup SQL, per-route `add_query` is the wrong advice — the statement fires on whichever route opens the connection first (Django's `SELECT set_config('TimeZone', $?, false)`, `MA.8`, `LESSONS.md` last bullet). Use `blastit_sqlpolicy::is_introspection_setup_sql` (already a dependency) to classify; emit a new `add_group` action pointing at `[group.connection.db].queries`. **Every other statement keeps `add_query`** — the Python contract asserts `{"action": "add_query", "to": "[route.db].queries"}` for `SELECT 1` (`python/tests/critical/test_check_contract.py:989`, `python/tests/test_check.py:224`).
- **(c) Hot path** (`crates/blastit-scheduler/src/scheduler.rs::plan_request_body`, line ~1224): `method = String::from_utf8_lossy(...)` and the two header `String`s are built before `policy` is consulted. Return `Ok(BodyPlan::AllowEmptyOnly)` as soon as `policy.is_none()`.
- Protected files: `crates/blastit/tests/critical_config_and_presets.rs` is listed in `paths` **only** to add one new test (never weaken/delete an existing one). `crates/blastit/tests/critical_check_engine.rs` is **not** in `paths` and must stay green untouched. `docs/troubleshooting.md` is the only doc file this node may edit.

# Acceptance

```console
# 1. Missing manifest is named first, at both entry points (empty dir, no BLASTIT_* set).
$ CARGO_NET_OFFLINE=true cargo build --locked -p blastit
$ tmp=$(mktemp -d) && ( cd "$tmp" && env -u BLASTIT_APP -u BLASTIT_MANIFEST "$OLDPWD/target/debug/blastit" dev ) >out.txt 2>err.txt
$ grep -q 'blastit.toml' err.txt && ! grep -q 'BLASTIT_APP' err.txt && test ! -s out.txt && echo "dev names the manifest"
$ tmp=$(mktemp -d) && ( cd "$tmp" && env -u BLASTIT_APP -u BLASTIT_MANIFEST "$OLDPWD/target/debug/blastit" ) >out.txt 2>err.txt
$ grep -q 'blastit.toml' err.txt && ! grep -q 'BLASTIT_APP' err.txt && echo "production names the manifest"

# 2. Pinned in the protected suite (added test only; the existing 11 stay).
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_config_and_presets --locked

# 3. Group hint, with its per-route twin.
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test audit_group_hint --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit --test critical_check_engine --locked   # untouched, still green

# 4. Zero header allocations on a policy-less route.
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test head_allocs --locked

# 5. Docs + full gate.
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- `docs/troubleshooting.md`: the `blastit.toml: no such file` row must now be the first symptom for an empty directory, and the `BLASTIT_APP ... is required` row must say it means the manifest exists but declares no `app`. Add a row under "`blastit check` fails" for a connection-setup statement, pointing at `[group.<name>.db]` and `groups = ["..."]`.
- `add_group` is a new member of the closed `fix.action` set: keep the JSON shape (`{"action":"add_group","to":"[group.connection.db].queries"}`), do not rename `to`, do not add fields. The hint must never fire for transaction-control or redacted statements (both already `observe`).
- `head_allocs.rs` already has a counting global allocator: extend it, do not add a second allocator (one global allocator per test binary).
- Keep the three changes independent in the diff so a reviewer can read each one alone. Report the allocation number before/after in `RESULT.md`.
