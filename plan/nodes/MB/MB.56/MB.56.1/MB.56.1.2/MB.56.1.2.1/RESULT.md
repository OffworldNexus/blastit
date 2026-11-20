# MB.56.1.2.1 — inline live `check`/`dev --learn` grant diff, delete `blastit-audit`

## What moved

`crates/blastit-audit`'s **live** modules now live under `crates/blastit/src/check/`
as one cohesive tree, and the crate is gone from the workspace:

| file | prod LOC | origin |
|---|---|---|
| `check/mod.rs` | 418 | `crates/blastit/src/check.rs` (entry, `run`/`execute`, constants, suites) |
| `check/dbproxy.rs` | 378 | `check.rs` (listeners, `Dbproxy`, `Ready`, `ObsReader`, `EventDrain`) |
| `check/django.rs` | 196 | `check.rs` (one-shot `pytest-django` handshake) |
| `check/pytest.rs` | 279 | `check.rs` (interpreter discovery, preflights, spawn) |
| `check/report.rs` | 174 | `check.rs` (`emit_report`, `learn`, `learn_line`, `resolve_targets`) |
| `check/session.rs` | 132 | `check.rs` (`CheckRoot`, descriptor helpers) |
| `check/evaluate.rs` | 337 | `blastit-audit/src/evaluate.rs` |
| `check/findings.rs` | 322 | `blastit-audit/src/findings.rs` |
| `check/learn.rs` | 314 | `blastit-audit/src/learn.rs` |
| `check/human.rs` | 310 | `blastit-audit/src/human.rs` (minus `probe_ingress_json`) |
| `check/infer_routes.rs` | 227 | `blastit-audit/src/infer_routes.rs` |
| `check/ledger.rs` | 188 | `blastit-audit/src/ledger.rs` (minus egress/table-grant document) |
| `check/dead.rs` | 167 | `blastit-audit/src/dead.rs` |
| `check/diff.rs` | 140 | `blastit-audit/src/diff.rs` (minus `resolve_proxy`, `proxy_names`, `TcProbe`) |
| `check/obs.rs` | 139 | `blastit-audit/src/obs.rs` |
| `check/ingress.rs` | 98 | `blastit-audit/src/ingress.rs` |
| `check/patterns.rs` | 64 | `blastit-audit/src/patterns.rs` |
| `check/static_roots.rs` | 63 | `blastit-audit/src/static_roots.rs` |
| `check/error.rs` | 41 | `blastit-audit/src/error.rs` (minus `AuditError`) |

Every prod file is `< 500` LOC; `check/` is 3,987 prod LOC (4,870 with the
`check.rs` suites carried over unchanged, which still compile against
`check::`).

## What was deleted (not moved)

- The retired `fold`/`pytest_plugin`/`--internal-json` bridge: `pytest.rs`
  (`audit`), `assemble.rs`, `sections.rs`, `observe.rs`, `model.rs`,
  `analyze.rs` (`analyze`/`classify`/`covering`), `normalize.rs`
  (`has_quoted_literal`/`redact_literals` re-exports),
  `human::probe_ingress_json`, `diff::{resolve_proxy, proxy_names, TcProbe}`,
  `error::AuditError`.
- The bridge-only ledger state: `EgressTrace`, `UnroutedTrace`,
  `table_grants_json`, `mark_transaction_control`/`pair_mut`, the per-pair
  table-grant fields, `RouteLedger::suite_requests`/`egress`.
- The crate's own suites: `src/tests.rs`, `src/tests/**`, `src/check_tests.rs`,
  `tests/grant_diff_parity.rs`, plus `crates/blastit-audit/Cargo.toml`.
- `crates/blastit/Cargo.toml`'s `blastit-audit` dependency and the
  `Cargo.lock` entry (`cargo metadata` regenerated it).

`crates/blastit/src/lib.rs` now publishes `pub mod check;` and
`pub use check::infer_routes;`.

## Evidence

```
$ cargo check --workspace --all-targets --features python
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.56s

$ cargo clippy -p blastit --all-targets --features python -- -D warnings
(no output)

$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit --no-deps --features python
(no output)

$ cargo test -p blastit --lib --features python check::
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 266 filtered out

$ cargo test -p blastit --features python --test critical_check_engine \
      --test critical_learn_executor_blocks --test audit_group_hint --test critical_static_cli
test result: ok. 4 passed  (audit_group_hint)
test result: ok. 20 passed (critical_check_engine)
test result: ok. 2 passed  (critical_learn_executor_blocks)
test result: ok. 12 passed (critical_static_cli)

$ rg 'blastit_audit|blastit-audit' crates/ Cargo.toml Cargo.lock
(none)
```

Final gate: `make fast-check` (run after this file, green, zero warnings).

## Follow-ups

- `crates/blastit/src/lib.rs` keeps `#[doc(hidden)] pub use check as audit_v1;`
  (and `check::learn::plan_learn_json`) because `crates/blastit/tests/`
  (`audit_group_hint.rs`, `critical_check_engine.rs`, `critical_static_cli.rs`)
  still name `blastit::audit_v1` and `audit_v1::plan_learn_json`, and no slice
  here owns those paths — `MB.56.3` (`crates/blastit/tests/**`) should rewire
  them to `blastit::check` and delete the alias and `plan_learn_json` with it.
- `check::obs::{parse_http_obs, parse_sql_obs}` and `check_audit` survive as the
  JSONL schema entry points; `MB.56.5` (coverage sweep) can confirm whether any
  of them is still reachable.

## Consolidated review fix

1. **Dead `check_audit` deleted** from `check/evaluate.rs` (with the
   `_blastit.check_audit`/Python-NDJSON doc that claimed two retired owners) and
   from `check/mod.rs`'s `evaluate` re-export. `blastit check --json` is
   `report::emit_report` → `evaluate` → `Report::to_json` only.
2. **One `--learn --json` renderer**: `report::learn` now prints
   `plan.json(options.dry_run || !wrote)` instead of its own inline `json!`
   document, so `LearnPlan::json` (the schema `critical_check_engine` pins) is
   the only renderer. The `|| !wrote` term keeps the document byte-identical on
   the failed-write path, where the renderer's derived `wrote` would otherwise
   say `true`; `would_write` is true exactly when a successful write happened,
   so every branch reproduces the previous `wrote` value. `plan_learn_json` and
   `LearnPlan::json` stay for `MB.56.3`.
3. **API narrowed**: `Occurrence`, `ProxyLedger`, `RouteLedger`, `Ledger`,
   `GrantCheck`, `grant_of`, `grant_locator`, `trait_locator` are no longer
   re-exported (`check`'s submodules name them by their own paths). Narrowing
   exposed four genuinely dead items, now deleted: `GrantCheck::{granted,
   transaction_control}` (fields and accessors), `RouteLedger::proxy`,
   `Decision::normalized`. Public `check` is now exactly `Finding`, `Fix`,
   `Report`, `RouteCounts`, `ProxyCounts`, `Severity`, `CheckObsV1`,
   `HttpObsLine`, `SCHEMA_VERSION`, `ObsError`, `parse_manifest`, `evaluate`,
   `human`, `plural`, `plan_learn` (+`plan_learn_json` for `MB.56.3`),
   `LearnPlan`, `parse_{http,sql}_obs`, `pattern_is_writable`,
   `unrequested_static_root(s)`, `infer_routes`, `run` and the three
   `BLASTIT_CHECK_*` constants.

```
$ cargo clippy --workspace --all-targets --features python -- -D warnings
(no output)
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit --no-deps --features python
(no output)
$ cargo test -p blastit --lib --features python check::
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 266 filtered out
$ cargo test -p blastit --features python --test critical_check_engine \
      --test critical_learn_executor_blocks --test audit_group_hint --test critical_static_cli
test result: ok. 4 passed  (audit_group_hint)
test result: ok. 20 passed (critical_check_engine)
test result: ok. 2 passed  (critical_learn_executor_blocks)
test result: ok. 12 passed (critical_static_cli)
```

Prod LOC after the fix: `mod.rs` 416, `evaluate.rs` 320, `ledger.rs` 176,
`report.rs` 170, `diff.rs` 109 (every `check/` file `< 500`).

Final gate: `make fast-check` (run after this section).
