# RESULT: MA.2.3.4 (Leaf 11) — check audit engine, route inference and learn plan

STATUS: done

## What landed

- `crates/blastit/src/audit_v1.rs` (new)
  - `CheckObsV1` + `HttpObsLine`, `schema_version == 1` enforced by `new` and
    again by `evaluate`; `parse_http_obs`/`parse_sql_obs`/`from_jsonl` return
    `ObsError::Malformed { file, line, reason }` (the caller owns the abort).
  - `evaluate` over the closed nine-kind `Finding` enum, the typed `Report`
    (`schema_version`, `ok`, `requests`, `statements`, `routes`, `proxies`,
    `findings`), `Report::to_json`.
  - `add_query` only for `ObsVerdict::Query` without `…`; redacted literals and
    transaction control (`SAVEPOINT`/`BEGIN`/`COMMIT`/`ROLLBACK`/`RELEASE`) are
    `observe`; unrouted SQL counts but never yields `missing_grant`.
  - `dead_grant` for inline and group grants via
    `Manifest::is_query_covered_by_tables_or_groups`; `unexercised_route`
    suppressed when `obs.http` is empty; `statements == sum(proxies[*])`;
    `ok == no error-severity finding`.
  - Byte-exact `human(&Report)` for the §1.3 clean line, header, 9-row finding
    lines, indented missing statements and footer.
  - `plan_learn`/`plan_learn_json` (`ObsTarget::Unrouted` -> inferred route
    queries, dedup against effective grants + coverage helper, routes + exact
    queries only, tx-control hint collected and printed by `emit_hints`, refusals
    block the write), `probe_ingress_json`.
- `crates/blastit/src/infer_routes.rs` (new): all 13 fixture rows, first
  **static** segment (numeric/UUID segments are placeholders), both-end trim,
  `-2`/`-3` disambiguation, insertion above both `.*` spellings.
- `crates/blastit-py/src/check.rs` (new) + `lib.rs`: `_blastit.check_audit`,
  `_blastit.plan_learn`, `_blastit.probe_ingress` (thin wrappers over
  `blastit::audit_v1`); existing `main`/`normalize`/`audit`/`analyze`/
  `covering`/`classify` unchanged. `audit.rs` now consults the shared
  `is_query_covered_by_tables_or_groups` for a route's own grants (group
  statements keep the documented per-route/`unused_global` accounting, so the
  existing suite stays green).
- `crates/blastit/tests/critical_check_engine.rs` (new, 13 tests) and
  `crates/blastit-py/tests/critical_check_schema.rs` (new, 6 tests).

## Acceptance evidence

```console
$ cargo test -p blastit --test critical_check_engine --locked
running 13 tests
test an_unrouted_statement_never_yields_a_route_less_missing_grant ... ok
test an_inline_literal_or_zero_requests_never_writes ... ok
test a_redacted_literal_and_a_savepoint_are_observe_never_add_query ... ok
test infer_routes_reproduces_all_thirteen_rows ... ok
test transaction_control_is_a_hint_never_a_silent_flag ... ok
test schema_version_must_be_one_inside_the_engine_too ... ok
test ingress_findings_have_positive_twins ... ok
test the_r27_golden_proposes_one_orders_route_with_both_exact_queries ... ok
test every_add_query_reparses_under_parse_and_validate ... ok
test the_r27_golden_is_a_clean_audit_after_the_edit ... ok
test a_clean_run_is_one_line_and_ok ... ok
test an_empty_suite_is_no_observations_and_suppresses_unexercised_routes ... ok
test every_finding_kind_and_its_interaction_counts ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```console
$ cargo test -p blastit-py --test critical_check_schema --locked
running 6 tests
test the_bridge_probe_renders_the_body_sample_the_gate_sends ... ok
test a_corrupt_http_obs_line_names_the_file_and_the_line ... ok
test python_http_obs_lines_conform_to_the_engine ... ok
test check_obs_v1_accepts_one_and_rejects_every_other_schema_version ... ok
test statements_is_the_sum_of_the_declared_proxy_counts ... ok
test the_bridge_plan_learn_matches_the_engine_proposal ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

```console
$ make check
guard: OK (5 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 10 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check                 # clean
cargo clippy … -D warnings                # clean
cargo doc … -D warnings                   # clean
cargo test --workspace --all-targets      # all green
cargo test -p blastit --test critical_dev_and_fail_closed  # 11 passed
cargo build --locked -p blastit-py -p blastit-dbproxy      # ok
python -m unittest discover -s python/tests/critical
Ran 28 tests in 1.190s
OK
uvx --offline ruff check python tools      All checks passed!
uvx --offline ruff format --check python tools  40 files already formatted
uvx --offline zensical build --strict     No issues found
uv run --no-project tools/plan.py validate  plan OK
```

## Negative twins covered

- Redacted `…` and `SAVEPOINT s1` denials (`fix.action == "observe"`, no table)
  beside a grantable `DELETE … $?` (`add_query`) — and a wrongly-`Query`-tagged
  `…` still never reaches a `queries` list.
- `ObsTarget::Unrouted` SQL increments `statements`/`denied` but emits no
  `missing_grant`; unseen `POST /orders/42/` proposes `orders`.
- Inline literal and zero-request runs render `"wrote": false`; `--dry-run` too.
- `raw_ingress` only without `allow_raw`; `missing_ingress`/`ingress_mismatch`
  have allowed twins.
- Empty suite: `no_observations` + `proxy_unused`, no `unexercised_route`.

## Follow-ups

- Non-obvious Rust/Cargo fact for future nodes (kept here because `docs/**`
  is outside this node's `paths`): a `cdylib`-only package (`blastit-py`)
  produces no rlib, so `crates/<pkg>/tests/*.rs` cannot link the library;
  those tests can still use the package's ordinary `[dependencies]` and must
  exercise the exact function the pyo3 wrapper calls.
- `crates/blastit/tests/critical_dev_and_fail_closed.rs::only_a_granted_route_gets_a_pg_socket`
  failed once under the full parallel `rs-test` run (dev exited `1`), then passed
  alone and on the next `make check`. Looks like a pre-existing timing/port
  race in the Leaf-10 dev harness, not this node (`audit_v1` is not on that
  path); worth a `--test-threads` or socket-wait hardening in MA.2.3.3's suite.
- `check.rs` (Leaf 12) should call `audit_v1::check_audit` / `plan_learn_json`
  and map `ObsError::Malformed` to exit `2` naming the file; the engine already
  names file + line.
