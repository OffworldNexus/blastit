# Result MB.56.3.3 — `e2e_check_fmt.rs` (§5 check/fmt/learn) and the `audit_v1` retirement

**STATUS: done**

`crates/blastit/tests/e2e_check_fmt.rs` is the one §5 suite: 24 tests against the real
binary, the real dbproxy role and a real PostgreSQL v3 peer (`common/pg_upstream.rs`).
Its suite is `common/check_suite.rs`, a scriptable `pytest` shim (the host has no
pytest in the audited interpreter): the fixture chooses the HTTP observations, the
control targets, the statements, the argv and the exit shape; the gate, the audit,
the learn editor, the exit table and the formatter are the product's. The six suites
it absorbs are deleted, `e2e_dev`'s duplicate `--learn --dry-run` leg moved here, and
the temporary `audit_v1` alias plus `check::learn::plan_learn_json` are gone.

Files: `crates/blastit/tests/e2e_check_fmt.rs` (new, 24 tests; 1,700 lines after
`rustfmt`), `crates/blastit/tests/common/check_suite.rs` (new, 255, the suite shim
and its `argv`/per-store support), `crates/blastit/tests/common/check_cli.rs` (new,
the CLI/dev harness extracted so the suite file stays at the §5 cases), six deleted
suites, `e2e_dev.rs` (−49), `src/lib.rs` (−4), `src/check/mod.rs` (−1 export),
`src/check/learn.rs` (−22), and the docs named under “Review fixes”.

## Old file → new test map

### `critical_check_engine.rs` (1,327 LOC) → mostly §5's human/JSON + learn legs

| Old test | New owner | Note |
|---|---|---|
| `infer_routes_reproduces_all_thirteen_rows` | `a_dry_run_learn_…`, `learn_writes…` | end-to-end route inference (2 numeric paths → 2 views) only; the 13-row unit table is dropped (see below) |
| `the_r27_golden_proposes_one_orders_route_with_both_exact_queries` | `a_dry_run_learn_…` | 2 routes / 3 queries proposed, absolute path, byte-identical file; the exact `orders` name/pattern is not re-asserted |
| `the_r27_golden_is_a_clean_audit_after_the_edit` | `a_clean_wsgi_and_postgres_suite_exits_zero` | a granted parameterized statement through dbproxy audits clean (`1 request, 1 statement, 1/1 view, clean`) |
| `every_add_query_reparses_under_parse_blastfile` | `human_and_json_report_the_same_findings_and_fixes`, `learn_writes…` | `fix.to` is `view shop postgres`; the learned grant is re-read by `check` and a second learn sees it granted |
| `a_redacted_literal_and_a_savepoint_are_observe_never_add_query` | — | dropped (see below) |
| `a_lying_query_verdict_carrying_a_redaction_is_observe_never_add_query` | — | dropped (forged verdict unreachable through the real dbproxy) |
| `connection_setup_sql_names_the_connection_trait_grant` | `human_and_json_…` | `SELECT set_config('TimeZone', $1, false)` over the real peer → `add_group` / `trait connection` in both forms |
| `ordinary_statements_keep_the_per_view_add_query` | `human_and_json_…` | `DELETE` → `add_query` / `view shop postgres`; a `SELECT 1` twin is not scripted |
| `only_connection_setup_names_a_trait_and_never_a_refusal` | `human_and_json_…` | `trait shared` dead grant; the `set_config`/`DELETE` pair is one run; redaction/tx refusals still dropped |
| `every_finding_kind_and_its_interaction_counts` | `human_and_json_…` | 9 findings / 7 kinds (missing, dead, unexercised, missing/mismatch/raw ingress, unrouted), exact human block + exact JSON array; the full 9-kind 11-problem table is not reproduced |
| `an_empty_suite_is_no_observations_and_suppresses_unexercised_routes` | `an_unobserved_run_names_no_observations_and_proxy_unused` | empty HTTP: `no_observations`, `exercised == 0`, no `unexercised_route` |
| `requests_without_any_statement_are_no_observations` | `an_unobserved_run_names_no_observations_and_proxy_unused` | a routed request and no SQL: `no_observations` + `proxy_unused` |
| `a_clean_run_is_one_line_and_ok` | `a_clean_wsgi_…` | byte-exact one-line human form |
| `the_body_finding_names_the_view_body_node_and_never_unvalidated` | `human_and_json_…` | `body raw unvalidated` is the fix; the three body-node locators are dropped |
| `ingress_findings_have_positive_twins` | `a_clean_wsgi_…`, `a_database_free_…` | a bodyless clean run; explicit `body json`/`body raw unvalidated` twins dropped |
| `an_unrouted_statement_never_yields_a_route_less_missing_grant` | — | dropped; the unrouted **HTTP** request is asserted in parity |
| `an_inline_literal_or_zero_requests_never_writes` | `a_dry_run_learn_…` | dry-run writes nothing |
| `zero_requests_never_write_even_with_sql_to_learn` | `a_failed_suite_…`, `a_dry_run_learn_…` | a non-zero pytest exit and a dry-run both write nothing |
| `transaction_control_is_a_hint_never_a_silent_flag` | — | dropped (see below) |
| `schema_version_must_be_one_inside_the_engine_too` | `a_malformed_observation_is_a_start_failure` | the CLI's malformed-observation exit 2; the in-type version refusal is dropped |

### `critical_blastfile_fmt.rs` (279) and `fmt_command.rs` (296) → `fmt_*`

| Old test | New owner |
|---|---|
| `check_writes_nothing_at_all` / `check_reports_a_non_canonical_file_and_exits_one` | `fmt_check_prints_a_unified_diff_and_writes_nothing` (read-only dir, unified diff, inode/mtime, entries) |
| `check_on_a_canonical_file_touches_nothing` / `check_on_a_canonical_file_exits_zero` | same (canonical → `0`, both streams empty) |
| `in_place_rewrite_is_atomic_and_keeps_the_mode` / `a_non_canonical_file_is_rewritten_and_exits_zero` | `fmt_rewrites_in_place_preserving_comments_and_is_idempotent` (comments kept, mode `0640`, no temp file, second run untouched) |
| `a_crlf_canonical_file_shows_the_carriage_returns_it_would_drop` | `fmt_check_…` (CRLF hunk keeps `\r`) |
| `a_missing_final_newline_still_shows_a_diff` | `fmt_check_…` (`\ No newline at end of file`) |
| `a_parse_error_writes_nothing` / `a_broken_file_exits_two_with_a_position` | `fmt_refusals_name_the_file_and_write_nothing` (`./Blastfile:2:10`, file untouched) |
| `a_missing_file_exits_two` | `fmt_refusals…` (exact §15.1 sentence) |
| `a_set_but_empty_manifest_variable_is_refused_at_every_entry_point` (fmt half) | `fmt_refusals…` |
| `a_second_path_is_refused` / `an_unknown_argument_is_refused` | `fmt_refusals…` |
| `blastit_manifest_names_the_file` | `manifest_discovery_is_one_file_and_never_walks_up` |
| `a_path_argument_names_the_file` | `fmt_rewrites_in_place_preserving_comments_and_is_idempotent` (`fmt tmp/other.Blastfile`, `./Blastfile` unchanged, `--check` on the copy exits `0`) |
| `help_is_printed_and_exits_zero` | dropped (`e2e_cli::help_lists…` owns the command line) |
| `a_write_that_fails_leaves_the_original_intact` | dropped (uid-0 silent-skip case; the read-only `--check` leg remains) |
| `a_file_it_would_grow_past_the_cap_is_refused` | dropped (same cap pinned in `crates/blastit-manifest/tests/blastfile_format.rs:132`) |

### `critical_learn_executor_blocks.rs` (288) → learn legs

| Old test | New owner |
|---|---|
| `a_learned_query_keeps_the_mount_and_the_entrypoint_on_disk` | `learn_writes_one_grant_without_widening_a_view` — the grant lands in the existing view, the existing grant survives, the neighbouring view is byte-identical, no duplicate view; the static-mount/entrypoint shape is not reproduced |
| `a_learn_run_that_saw_nothing_writes_nothing` | `a_dry_run_learn_…` and the second `check --learn` in `learn_writes…` |

### `audit_group_hint.rs` (166) → parity

`human_and_json_report_the_same_findings_and_fixes` scripts
`SELECT set_config('TimeZone', $1, false)` through the real dbproxy/PG peer and
asserts the human `fix: add it to trait connection` **and**
`{"action":"add_group","to":"trait connection"}`; the same run keeps `DELETE` on
`{"action":"add_query","to":"view shop postgres"}` and a dead trait grant on
`trait shared`. The `SAVEPOINT`/redacted-`observe` twin is still dropped (a real
dbproxy run cannot forge a verdict).

### `blastfile_discovery.rs` (458) → `manifest_discovery_is_one_file_and_never_walks_up`

| Old test | New coverage |
|---|---|
| `blastit_manifest_wins_over_the_default` | named `Other` audits clean, default exits 2; `fmt --check` on the named file exits 0 |
| `the_default_is_dot_slash_blastfile_only` | default is read; a second file is never the default (the "other file untouched" assertion is dropped) |
| `nothing_is_read_above_the_working_directory` | child dir: `check`, `check --json`, `fmt` all print the one missing sentence; the `dev` leg is dropped (`e2e_dev` owns dev boot refusals) |
| `a_set_but_empty_manifest_variable_is_refused_at_every_entry_point` | check + `--json` + `fmt`; `dev` leg dropped |
| `a_body_that_is_not_a_blastfile_is_an_ordinary_syntax_error` | check + `--json` + `fmt`, positioned `1:1`, no detection/TOML/migration sentence; `dev` leg dropped |
| `a_manifest_path_holding_a_newline_prints_one_escaped_line` | check leg, one line, `./a\nb`; `fmt`/`dev` legs dropped |
| `a_fifo_at_the_manifest_path_cannot_block_dev_or_check` | bounded `check` + `fmt` (one line, "not a regular file"); `dev` leg dropped |

## MUST-COVER from `LOG.md`

| Item | Where |
|---|---|
| PHP preflight: `check` validates the `libphp`/shim gate and refuses with the documented sentence | `the_php_preflight_validates_the_libphp_gate` — `BLASTIT_LIBPHP` = a text file ⇒ exit 2, empty stdout, `route "app": entrypoint php "public/index.php": … not an ELF shared object`, `BLASTIT_LIBPHP`, never the pytest sentence |
| No drifting host-PHP probe (was `critical_dev_reload.rs:2473-2545`) | `e2e_dev` calls `tests/common/php_host::host_php()` (`PROBES` includes 8.6); the local copy is gone |
| readable root under owned `0700` accepted on check **and** dev | `a_readable_root_under_an_owned_0700_ancestor_is_accepted` (check `--json` 0 + real `dev` serving `GET /` 200) |
| unlistable root refused | `an_unlistable_root_and_a_root_that_is_a_file_are_refused` (check 2, dev 1, `permission denied`, empty stdout) |
| root that is a file refused | same (`not a directory`) |
| `BLASTIT_APP` on an all-static manifest refused | `an_app_on_an_all_static_manifest_is_refused` (dev 1 + check 2; names the variable, never its value) |
| all-static `check --learn` refused with the sentence | `an_all_static_check_refuses_learn_and_names_a_missing_root` (`nothing to learn: the file answers every request from static mounts`) |
| unrequested static mount is a warning, not a finding | `an_unrequested_static_mount_is_a_warning_not_a_finding` (exit 0, `ok:true`, `findings:[]`, stderr contains `static "/site"`/`root "public"`/`was never requested`; the twin replaces the mount with a `view` and gets `unexercised_route`) |
| missing mount root named with its prefix before output | `an_all_static_check_refuses_learn_and_names_a_missing_root` (`Blastfile`, `mount "/" (index 0)`, `static root "public"`, empty stdout) |
| mixed tree without an app still names `BLASTIT_APP` | `check_without_an_app_runs_the_suite_and_dev_names_blastit_app` (the `dev` half) |
| `check` without an app still runs the suite | `check_without_an_app_runs_the_suite_and_dev_names_blastit_app` (the `check` half reaches "no interpreter to run pytest on", not `BLASTIT_APP`) |
| PHP app with no suite: exit 2 without pytest; a missing root named before the suite | `the_php_preflight_validates_the_libphp_gate` — a marker shim proves pytest is never invoked; with a usable host libphp the `static root "nowhere"` refusal precedes it |

## Node acceptance (1–11)

1. Clean WSGI+Postgres / DB-free / static-only → three `…_exits_zero` tests; exit `2`/`1` are in the parity, start-failure and static tests.
2. Human ⇄ `--json` parity → `human_and_json_report_the_same_findings_and_fixes` (exact human block and exact `findings` array with `missing_grant` ×2 — one `add_query`/`view shop postgres`, one `add_group`/`trait connection` — two `dead_grant`s (`view` and `trait shared`, `unsafe:true`), `unexercised_route`, `missing_ingress`, `ingress_mismatch`, `raw_ingress`, `unrouted_request`; header `4 requests, 3 statements, 3/5 views, 2 proxies`; counts in `a_clean_…` (`1 request, 1 statement, 1/1 view`)).
3. Exit table → `0` in the clean tests, `1` in parity, `2` in start/static/PHP/Django refusals, `3` for pytest `1/2/3/4/5` and a signal (`a_failed_suite_is_exit_three_for_every_pytest_failure_shape`); stdout empty under `--json` in the start/suite failures.
4. `--learn --dry-run` → `a_dry_run_learn_prints_the_proposal_and_writes_nothing` (exact `learn: 2 routes, 3 queries proposed (dry-run) → /abs/Blastfile`, byte-identical manifest, no ` added `, empty scratch `TMPDIR`; the `--json` twin asserts `wrote:false`, `proposed` present and no `findings`).
5. `--learn` JSON/build → `learn_writes_one_grant_without_widening_a_view` (`proposed.routes`, never `route`; `wrote:true`; `learn: 0 routes, 1 query added → /abs` on stderr; grant added to `shop`, `house` byte-identical, idempotent second run; a failing suite exits `3` and leaves the file byte-identical with no ` added ` line).
6. Gate discovery → `the_installed_bundle_carries_the_gate_and_the_venv_wins` (cargo tree: caller `PYTHONPATH` only; `make install`-shaped copy: caller + `<exe>/../python`; project `.venv` marked "venv", PATH candidate never runs).
7. Django handshake → `the_django_handshake_is_private_and_removed` (`0600`, `test_app`, real URL, swapped local socket URL, fresh path each run, removed after a passing **and** a failing suite), `django_start_failures_name_the_missing_store`.
10. `fmt` → `fmt_rewrites_in_place_preserving_comments_and_is_idempotent` (including `fmt PATH` and `--check PATH`), `fmt_check_prints_a_unified_diff_and_writes_nothing`, `fmt_refusals_name_the_file_and_write_nothing`.
8. Nine static cases → the table above.
9. PHP preflight → above.
10. `fmt` → `fmt_rewrites_in_place_preserving_comments_and_is_idempotent`, `fmt_check_prints_a_unified_diff_and_writes_nothing`, `fmt_refusals_name_the_file_and_write_nothing`.
11. Deletions and shims → six suites deleted; `e2e_dev`'s duplicate dry-run leg removed; `#[doc(hidden)] pub use check as audit_v1` (4 lines) and `plan_learn_json` + re-export (22 lines) removed; `rg 'audit_v1|plan_learn_json' crates/` is empty.

## Deliberate drops (with reasons)

- **`observe` for a redacted literal / transaction control**, and **a lying `query` verdict** (`critical_check_engine`): a real dbproxy run cannot forge a verdict or guarantee a refused literal; these were engine-level unit oracles. The SQL-policy half is §7's own suite.
- **The 13 `infer_routes` rows**: a pure-function table; the suite holds the end-to-end half (2 numeric paths → 2 routes) and the learn insertion.
- **The full 9-kind / 11-problem interaction table**: the new suite asserts seven user-facing finding classes plus a clean run, and the `no_observations`/`proxy_unused` pair separately; the interaction-count oracle is not reproduced.
- **Body-node locators** (`view <v> body`, `view <v> POST body`): the fix text is asserted; the three node spellings are not.
- **`fmt` write-failure (exit 2, original intact)**: needs a non-root read-only-dir write; the case historically skipped under uid 0. The read-only `--check` leg remains.
- **`fmt` 16 MiB cap**: already pinned at the formatter level by `crates/blastit-manifest/tests/blastfile_format.rs:132`; the 9 MiB CLI fixture costs ~3 s.
- **`fmt --help`**: `e2e_cli` owns the command line; the positional `PATH` and the default/`BLASTIT_MANIFEST` discovery are kept.
- **Discovery `dev` legs** (missing manifest, empty variable, foreign body, newline path, FIFO): `e2e_dev` drives the dev boot; `check`/`fmt` keep the sentences. The FIFO is still bounded.
- **The literal `blastit app` production shape**: an unprivileged production boot refuses on capabilities before reading the manifest; `BLASTIT_APP` is asserted through `dev` and `check`, the two reachable entry points.

## Review fixes (two rounds)

Round 1: `trait connection` is now asserted end-to-end in parity; `no_observations`/`proxy_unused`
have a data-driven test (both exit **`1`**, not `0` — `no_observations` is an error in
`findings.rs:216`; the review's `exit 0` would grade a different product); the Django
handshake is proven removed after a failing suite; the dry run never says `added` and
leaves an isolated `TMPDIR` empty; root skips the unlistable half loudly and still runs
the file half; `dev` uses `free_port()` with three bind retries and an early-exit check.

Round 2: parity also carries `raw_ingress` (`body raw` with no `unvalidated`) and
`ingress_mismatch` (`body json` sent `application/x-www-form-urlencoded`) with their
human lines and JSON `fix`, plus a second store and a statement on it so the header
prints `, 2 proxies`; the docs stop describing the retired “0.2.x gap”
(`docs/reference/cli.md` `--learn`, `docs/ci.md` bootstrap, `docs/reference/blastfile.md`)
and `README.md` says an all-static clean run prints nothing; `fmt PATH` is covered;
`check -- -k products` records an argv ending in `-k products`; `--learn` after a
failing suite is exit `3` with byte-identical bytes and no ` added `; `--learn --dry-run
--json` is `wrote:false`/`proposed`/no `findings`; the unrequested-mount hint is matched
by fact and its `unexercised_route` twin runs; pytest `2`/`3` joined the exit-table
rows; the PHP leg proves pytest never ran and a missing static root is named first.

## Evidence

```
$ rg -n 'audit_v1|plan_learn_json' crates/
rg: none
```

```
$ cargo check -p blastit --all-targets
    Checking blastit v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.04s
```

```
$ cargo test -p blastit --test e2e_check_fmt
running 24 tests
...
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
```

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.41s
DOCS_CHECK_EXIT=0
```

```
$ make fast-check
guard: OK (4 staged/untracked file(s) checked)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.43s
... running tests/e2e_check_fmt.rs ...
running 24 tests
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.67s
...
All checks passed!            (ruff)
plan OK                      (tools/plan.py validate)
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
FAST_CHECK_EXIT=0
```

No compiler, manifest, clippy or docs warning was emitted. The suite is formatted
with `cargo fmt --all`; `e2e_check_fmt.rs` is 1,700 lines after it.
