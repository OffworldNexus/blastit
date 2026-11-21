# MB.56.3.5 — finish the E2E consolidation

## What landed

- `crates/blastit/tests/` is now **exactly four `e2e_*.rs` suites plus `common/`**:
  no `smoke.rs`, no `critical_config_and_presets.rs`, no `critical_lsp.rs`, no
  orphaned `common/mod.rs`, no `dx-*` fixture.
- `tests/common/deployment.rs` (new) is the one child-process helper for a
  **deployment** boot (root, real uid isolation, `ready` event, HTTP, `/proc`
  probes); the §6 serving legs that only exist under root use it.
- `e2e/dx_replay.py` deleted (it read the deleted `dx-mask.txt` /
  `dx-mask-host.txt` / `dx-parity.txt`); nothing in `Makefile`, `.github/`,
  `scripts/` or `e2e/` referenced it. `docs/design/view-crates-architecture.md`
  R14 (§3 statement **and** the §7 oracle row) now names the four suites.
- Dead test legs fixed: `cargo clippy -p blastit --no-default-features
  --all-targets -- -D warnings` is green (`e2e_dev` is an empty suite under zero
  features; `e2e_prod_refusals`' dev harness and its four constants are
  `#[cfg(feature = "python")]`/`php`). `Makefile`/`.github/workflows/*` were
  already retargeted to `e2e_{cli,dev,prod_refusals}` by MB.56.3.4 — verified,
  plus the stale `tests/smoke.rs` and "smoke tests" commentary in `ci.yml` and
  the deleted-suite names in `e2e/rootless/run.py`,
  `e2e/php-sapi/Dockerfile.two-package`.
- **No production-code change.** No `#[ignore]`. `TESTING_PLAN.md` untouched.

## Per-file LOC, before → after

`crates/blastit/tests/` started this epic at **26,643 LOC across 29 files**
(MB.56.3.1's cap decision). At this node's base (MB.56.3.4) it was 16,282 LOC
across 7 suites + 10 `common/` files.

| File | Before (base) | After | Delta |
|---|---:|---:|---:|
| `e2e_cli.rs` | 1,378 | 1,378 | 0 |
| `e2e_check_fmt.rs` | 1,700 | 1,700 | 0 |
| `e2e_dev.rs` | 3,308 | 3,310 | +2 |
| `e2e_prod_refusals.rs` | 2,580 | 2,988 | **+408** (absorbed legs) |
| `smoke.rs` | 2,577 | **deleted** | −2,577 |
| `critical_lsp.rs` | 2,477 | **deleted** | −2,477 |
| `critical_config_and_presets.rs` | 503 | **deleted** | −503 |
| `common/deployment.rs` | — | 385 | **+385** |
| `common/check_cli.rs` | 213 | 213 | 0 |
| `common/check_learn.rs` | 154 | 154 | 0 |
| `common/check_suite.rs` | 255 | 255 | 0 |
| `common/pg_upstream.rs` | 346 | 346 | 0 |
| `common/php_host.rs` | 218 | 218 | 0 |
| `common/static_assets.rs` | 260 | 260 | 0 |
| `common/views.rs` | 156 | 156 | 0 |
| `common/proc.rs` | 71 | 71 | 0 |
| `common/project.rs` | 49 | 49 | 0 |
| `common/reload.rs` | 30 | 30 | 0 |
| `common/mod.rs` | 7 | **deleted** (orphan: nothing did `mod common`) | −7 |
| **total** | **16,282** | **11,513** | **−4,769** |

`crates/blastit-lsp/tests/e2e_lsp.rs`: 600 → 977 (+377, the absorbed §7.4/§7.5
framing and hygiene legs; 11 → 19 tests).

### Why 5,500 and 500-LOC `common/` were not reachable

- The node's own required `common/` owners are `pg_upstream` 346 + `php_host`
  218 + `static_assets` 260 + `views` 156 + the three `check_*` 622 = **1,602
  LOC** before a single spawn helper: 3× the 500 target. Moving them elsewhere
  would either hide the same bytes in a suite or duplicate them (MB.40.3.3's
  single-copy rule). Landed: the duplicate child-process helpers (e2e_cli's
  `Cli`, `check_cli`'s runner, `critical_config_and_presets`' `run_cli_env`) now
  have **one** deployment/spawn owner in `common/deployment.rs`; the per-suite
  `Boot`/`Dev` remain because each asserts a different boot (dev, production
  refusal, deployment).
- The four suites' 9,376 LOC are §2–§6 stories with two CPython minors, four PHP
  minors, three static backends, live reload, `--learn`, `check`/`fmt` and every
  fail-closed refusal. Cutting to 5,500 means deleting stories from
  `TESTING_PLAN.md`, which this node may not touch and which the LOG's cap
  decision already forbade. Ratio: **26,643 → 11,513, a 2.3× cut**, with the two
  riskiest suites (claim+proof) unchanged in test count (21 + 25 + 24 + 29 = 99
  tests, plus 19 in `blastit-lsp`).

## Every dropped case and its successor

### `smoke.rs` — 22 tests, all deleted

| Case | Successor / reason |
|---|---|
| `routing_reaches_two_routes_and_each_route_has_its_own_process_and_uid` | **absorbed**: `e2e_prod_refusals::a_deployment_runs_each_route_in_its_own_process_and_uid` (uids `base+1000/+1001`, distinct pids, stable pid, live component per uid). The 404 / `/_blastit/health` half is covered by `e2e_dev::a_prefix_mount_serves_under_its_prefix_and_the_longest_one_wins` + the banner leg. |
| `the_app_environment_has_no_blastit_variables_and_home_is_its_scratch_directory` | **absorbed** in the same leg (HOME/TMPDIR/CWD = `/run/blastit/views/<uid>`, `gid == uid`, `blastit.route`, `REMOTE_ADDR`/`REMOTE_PORT`, zero `BLASTIT_*`). |
| `a_declared_proxy_boots_dbproxy_and_replaces_the_credential_with_a_local_endpoint` | `e2e_prod_refusals::only_a_granted_route_gets_a_pg_socket` (local endpoint, no credential) + `the_worker_environment_drops_blastit_and_keeps_a_present_default`. The `0600`/view-uid **socket mode** is not re-asserted → follow-up. |
| `a_view_that_crashes_gives_502_and_the_next_request_respawns_it` | `e2e_dev::a_php_exit_and_a_fatal_recover_without_dropping_the_pool` (crash → recover, same pool) + `blastit-scheduler`'s respawn suites. The WSGI **502 → respawn** leg is lost → follow-up. |
| `concurrent_requests_to_one_route_run_in_parallel_threads` | `e2e_dev::a_threaded_wsgi_route_serves_four_requests_at_once`. |
| `the_boot_events_say_init_runs_as_root` | **absorbed** (boot `privilege: "root"`, `preset: null`, no `port` in the deployment leg). The "other process(es) run as uid 0" sibling warning is host-shaped and dropped. |
| `a_non_root_init_without_its_capabilities_names_them` | `e2e_prod_refusals::production_never_serves_without_isolation_privileges` (same six capability names, `setcap`, `blastit dev`, `boot` as first line). |
| `a_root_boot_without_cap_setpcap_runs_and_says_the_bounding_set_is_kept` | No successor: needs `setpriv --bounding-set -setpcap`. → follow-up (root-only). |
| `a_rootless_standalone_install_runs_dbproxy_from_the_capability_free_copy` | No successor: the `BLASTIT_EXEC` capability-free-copy leg. → follow-up (§3/§6 rootless install). |
| `stdout_is_one_event_per_line_and_covers_the_lifecycle` | **absorbed**: `a_deployment_writes_json_events_unless_the_format_says_pretty` (`ts` 24 chars, `level`, `component`, ≤2048 bytes, lifecycle reaches `ready`). |
| `sigterm_drains_and_exits_zero_within_the_graceful_timeout` | `e2e_dev::sigterm_and_sigint_reap_the_tree_close_the_port_and_remove_the_root` (exit 0, port closed, tree reaped, ephemeral root removed). |
| `pretty_renders_a_forwarded_event_as_the_human_line` | **absorbed** in the pretty half of the same leg (human request line, `→ <route>`, ≤2 JSON stage-0 first lines). The `sql.denied` pretty line is not re-asserted (needs a live PostgreSQL) → follow-up. |
| `a_granted_query_is_forwarded_and_an_ungranted_one_is_denied` | `e2e_prod_refusals::only_a_granted_route_gets_a_pg_socket` / `an_empty_grant_gets_a_denying_listener`; SQLSTATE `42501` is pinned in `blastit-dbproxy` + `blastit-sqlpolicy`. The cross-uid `EACCES` on another view's socket is lost → follow-up. |
| `a_route_keeps_serving_while_its_view_is_retired` | Not re-asserted (36-request retire-under-load burst) → follow-up. |
| `a_view_holds_only_its_stdio_and_its_channel` | Not re-asserted (view fd-table containment) → follow-up. |
| `the_real_credential_is_in_no_environment` | `e2e_prod_refusals::the_worker_environment_drops_blastit_and_keeps_a_present_default` (rewritten `DATABASE_URL`, no secret). |
| `a_component_that_dies_takes_init_with_it` | Not re-asserted (timing-sensitive supervisor leg) → follow-up. |
| `an_idle_view_is_retired_and_the_next_request_respawns_it` | Not re-asserted → follow-up. |
| `a_proxy_with_no_grant_gets_no_dbproxy_and_no_credential` | `e2e_prod_refusals::an_empty_grant_gets_a_denying_listener`. |
| `a_preset_whose_assumption_does_not_hold…` / `a_failing_boot_still_says_the_preset_assumption…` | Partial: preset reporting/`preset_overridden` absorbed; the "contradicted assumption" warnings are not → follow-up. |
| `a_boot_s_first_lines_are_in_the_format_its_settings_name` / `a_boot_with_no_readable_configuration_writes_its_first_lines_as_json` | Partial: the JSON/pretty default is absorbed; the unreadable-configuration boot line is not → follow-up. |
| `a_preset_is_accepted_and_reported_at_boot` | **absorbed**: `a_deployment_preset_is_reported_and_decides_which_forwarded_header_is_trusted` (`preset`, 4 `preset_settings`, `preset_overridden`). |
| `the_command_line_preset_wins_and_says_so` | Partial: the flag winning is covered by the same leg; the flag-vs-`BLASTIT_PRESET` conflict sentence is not → follow-up. |
| `a_client_ip_header_is_only_believed_when_it_is_configured_to_be` | **absorbed**: the trusted header is believed, the other one is ignored, for **both** presets. |

### `critical_config_and_presets.rs` — 10 tests, all deleted

| Case | Successor / reason |
|---|---|
| `cwd_manifest_is_discovered_with_no_upward_search` | duplicate of `e2e_check_fmt::manifest_discovery_is_one_file_and_never_walks_up`. |
| `a_set_but_empty_variable_wins_over_the_manifest` | **absorbed** as process legs: `e2e_prod_refusals::an_empty_directory_names_the_missing_manifest_before_the_app` (empty/missing manifest vs `BLASTIT_APP`) and `dev_ignores_an_exported_preset_and_an_unknown_one`. The `BLASTIT_PRELOAD=""` half → follow-up. |
| `both_presets_supply_their_four_values` | **absorbed** (`preset_settings` has 4 entries for both presets). |
| `a_set_variable_overrides_a_preset_even_when_empty` | Partial: `preset_overridden` contains `BLASTIT_LOG_FORMAT`; the empty-value override trio → follow-up. |
| `log_format_defaults_to_json_for_serve_and_pretty_for_dev_and_check` | **absorbed**: serve writes JSON by default, `BLASTIT_LOG_FORMAT=pretty` renders a forwarded event humanly; `dev`'s human default is asserted throughout `e2e_dev`. |
| `dev_and_check_ignore_an_exported_preset` | **absorbed**: `dev_ignores_an_exported_preset_and_an_unknown_one` (known preset + `BLASTIT_SECCOMP=0`, and an unknown preset, both still serve). |
| `seccomp_zero_aborts_with_a_preset_and_warns_without_one` | **absorbed** (abort half): `an_environment_driven_abort_is_a_real_exit_code_and_names_its_cause`. The "warns without a preset" half → follow-up. |
| `a_boot_event_round_trips_the_build_and_never_a_port` | `e2e_prod_refusals::json_events_never_carry_a_port_key` + the JSON leg (no `port` on a boot). `build` round-trip → follow-up. |
| `an_environment_driven_abort_is_a_real_exit_code_and_names_its_cause` | **absorbed**: `a_port_that_is_not_a_port_is_a_refusal_that_names_the_value` (exit 1, stdout empty, value named) + the seccomp leg. |
| `an_empty_directory_names_the_missing_manifest_before_the_app` | **absorbed** under the same name. |

### `critical_lsp.rs` — deleted; the §7 owner is `crates/blastit-lsp/tests/e2e_lsp.rs`

- **Absorbed there** (11 → 19 tests): declared length = UTF-8 **bytes**; id echo
  verbatim + notification silence + `"id": null` → `-32600` + `$/…` request →
  `-32601`; the whole error table (`-32002`, double `initialize`, wrong
  `jsonrpc`, unknown method, bad params, after `shutdown`); malformed bodies
  (`-32700`/`-32600`, loop survives); an oversized declared body refused before
  reading; the 8 192-byte header cap with its `8 193` refusal; a FIFO `uri`
  never opened; a document parsed and never executed (no child, no statement
  text, empty stderr); 2 000 fuzz-shaped bodies leaving exactly one well-formed
  reply each.
- **Already covered** in that sheet: capability document, diagnostics
  (severity/source/UTF-16/parse/quoted-literal), `didOpen`/`didChange`/
  `didClose`, hover (`$?`, `...`, directives, `$ENV`), formatting
  (empty/edit/`null`), the quick fix in both spellings + `only` + applied,
  argv/exit statuses, poisoned environment.
- **Dropped as source-text meta-tests** (policy forbids source greps):
  `the_lsp_arm_drops_capabilities`, `the_lsp_module_is_a_text_tool`,
  `no_position_of_a_validation_error_is_invented`'s `error.rs` grep (its session
  half is covered by the diagnostics legs).
- **Not re-asserted** (recorded, not silently lost): diagnostics cap,
  open-document cap/eviction, incremental-change refusal, CRLF normalisation,
  hover inverse UTF-16, definition declined, code-action param validation,
  `drain_stdout` positive control, and `blastit lsp --help`'s
  `usage: blastit lsp` first line (`e2e_cli` lists the subcommand).

## Follow-ups (out-of-scope coverage, no code change here)

1. Root-only legs with no successor: `BLASTIT_EXEC` capability-free dbproxy copy,
   `setpriv`-dropped `cap_setpcap` bounding set, per-route socket `0600`/view-uid
   mode, cross-uid `EACCES` on another view's PostgreSQL socket, WSGI 502 →
   respawn, view fd-table containment, idle retirement, component-death
   supervision, preset-assumption warnings, unreadable-configuration boot line,
   `BLASTIT_PRELOAD=""`, `build` round-trip on the boot event, empty-value
   preset overrides, flag-vs-`BLASTIT_PRESET` conflict sentence, pretty
   `sql.denied` line.
2. APIs those legs were the only test of (`run_to_exit`, `setpriv` shape) are now
   unpinned; a future node that re-introduces a root serving harness should own
   them.
3. `docs/design/ma-lean-dx.md` (§DX) and `docs/testing/AUDIT-MB.36.md` still
   quote the retired `critical_config_and_presets.rs` / `dx_replay.py` as live
   artifacts; both are historical design/audit records outside this node's
   `paths`, so they were left as written.
4. The four suites still carry per-suite `Boot`/`Dev`/`Scratch` helpers that a
   later slice could fold into `common/`; folding them now would touch three
   suites' fixture semantics for no coverage change.

## Evidence

```console
$ ls crates/blastit/tests
common  e2e_check_fmt.rs  e2e_cli.rs  e2e_dev.rs  e2e_prod_refusals.rs

$ cargo clippy -p blastit --all-targets --locked -- -D warnings
(no output)

$ cargo clippy -p blastit --locked --no-default-features --all-targets -- -D warnings
(no output)

$ cargo clippy -p blastit-lsp --all-targets --locked -- -D warnings
(no output)

$ cargo test -p blastit-lsp --locked --test e2e_lsp
test result: ok. 19 passed; 0 failed; 0 ignored

$ cargo test -p blastit --locked --features python --test e2e_prod_refusals
test result: ok. 29 passed; 0 failed; 0 ignored
# 3 deployment legs print `skipped: a deployment boot needs root` on this uid

$ cargo test -p blastit --locked --features python --test e2e_cli
test result: ok. 21 passed; 0 failed; 0 ignored

$ cargo test -p blastit --locked --features python,static --test e2e_check_fmt
test result: ok. 24 passed; 0 failed; 0 ignored

$ cargo test -p blastit --locked --features python,php,static --test e2e_dev
test result: ok. 25 passed; 0 failed; 0 ignored
```

`make fast-check` / `make zed-check` tails are appended below by the final gate
run.

```console
$ cargo fmt --all -- --check
fmt=0
$ make fast-check
fast=0        # 59 `test result: ok` lines, 332 tests passed, zero failed/ignored
              # (guard, rs-fmt, rs-lint, rs-doc, rs-test, py-lint, plan-validate,
              #  universal-smoke: e2e_cli 21 passed, install-test all cases passed)
$ make zed-check
zed=0         # zed 11 passed
```
