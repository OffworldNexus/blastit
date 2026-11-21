# RESULT — MB.56.3.4

`crates/blastit/tests/e2e_prod_refusals.rs` (2,153 LOC, 19 tests) holds
`TESTING_PLAN.md` §6 end to end through the real binary. Absorbed suites are
deleted; the one production gap the epic named (a symlinked `Blastfile`) is
closed in `crates/blastit/src/`.

## Production change (symlink `Blastfile`, §6)

- `crates/blastit/src/config/manifest_io.rs`: new `ManifestReadError::Symlinked`,
  its sentence (`<path>: the Blastfile is a symbolic link, and a production boot
  refuses one … replace the link with the file itself (cp --remove-destination …)`),
  and `read_production_manifest()` — `symlink_metadata()` first, then the one
  reader. `manifest_wiring` gained the matching (unreachable-from-dev) arm.
- `crates/blastit/src/stage0/mod.rs` (production boot) and
  `crates/blastit/src/stage1/boot.rs` (re-read, gated `dev.is_none()`) use it.
  FIFO/device/dir refusals are unchanged (`blastfile::read`'s metadata), and
  `blastit dev` keeps `read_manifest`, so a developer's symlinked `Blastfile`
  still boots — pinned by the dev twin test. `docs/reference/blastfile.md` says
  nothing about a symlink manifest, so the split is the production path's.

## Old file → new test map

| Deleted / trimmed | Now |
|---|---|
| `critical_dev_and_fail_closed::unprivileged_production_has_no_single_uid_fallback` | `production_never_serves_without_isolation_privileges` (also the preset ordering: unknown → 2, known → stage-0 refusal → 1) |
| `critical_dev_and_fail_closed::dev_boots_five_unprivileged_roles_and_strips_secrets` (§6 half) | `the_worker_environment_drops_blastit_and_keeps_a_present_default` |
| `no_python_build::a_wsgi_manifest_is_refused_by_the_capability_gate` | `a_wsgi_manifest_is_refused_by_the_capability_gate` (`cfg(not(feature="python"))`) |
| `no_python_build::a_manifest_asking_for_the_retired_external_php_pool_is_refused_by_name` | **dropped** — the retired `fastcgi` keyword removal is a `blastit-manifest` grammar case owned by `MB.56.2.3`, not §6 |
| `no_python_build::a_php_only_manifest_is_refused_by_the_php_discovery_gate` | `a_php_only_manifest_is_refused_by_the_php_discovery_gate` (`cfg(all(not(python), php))`) |
| `critical_raw_ingress::production_stage_one_refuses_raw_ingress_without_unvalidated` + pretty twin | `production_refuses_a_raw_route_without_the_acknowledgement` (both formats) |
| `critical_raw_ingress::production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it` | `a_manifest_edited_after_stage_zero_approved_it_is_still_refused` |
| `smoke::a_writable_working_directory_is_refused` | `a_tree_another_uid_can_write_is_refused_before_serving` (0777 + 1777) |
| `smoke::a_writable_directory_only_on_pythonpath_is_refused` | same test (0777 entry + 0755 twin) |
| `smoke::a_writable_directory_above_the_working_directory_is_refused` | same test (ancestor) |
| `smoke::a_directory_a_view_can_read_through_its_own_group_is_not_a_blocker` | same test (accepted `root:<view-uid> 0750` twin) |
| `critical_php_embed` refusal legs (deleted by `MB.56.3.2.2`) | `a_libphp_that_is_not_a_shared_object_names_the_package`, `a_zts_libphp_is_refused_from_dynsym_without_a_dlopen`, `an_unsupported_php_minor_names_the_supported_set`, `a_relative_blastit_libphp_is_refused` |
| `critical_php_embed::a_shim_for_another_minor_refuses_the_boot_before_the_listener` | `a_shim_for_another_minor_refuses_before_the_listener` |
| `critical_php_embed::a_probe_that_hangs_is_killed_and_refuses_the_boot` (denied twin) | `a_probe_that_hangs_is_killed_by_its_timeout_and_reaped` |
| `critical_php_embed::a_libphp_replaced_between_the_stages_refuses_the_boot` + shim twin | `a_libphp_or_shim_replaced_between_the_stages_is_refused` (one loop, both objects) |
| `critical_php_embed::no_libphp_is_mapped_above_the_view_worker` | `no_libphp_is_mapped_above_the_view_worker` |
| LOG: “no PHP server beside it — no `php-fpm`/FastCGI socket” | `no_php_server_stands_between_blastit_and_the_library` (served route, tree `comm`, runtime-root sockets) |
| — (new §6 gap) | `a_blastfile_that_is_a_fifo_is_refused_without_reading_it`, `production_refuses_a_symlinked_blastfile`, `dev_still_serves_a_symlinked_blastfile`, `one_boot_reports_every_environment_refusal_by_name_only`, `an_unusable_cpython_is_refused_with_its_documented_sentences` |

`critical_config_and_presets.rs` was **read and left in place**: its remaining
legs are preset/config precedence (`E1`, `R18`), not startup integrity, and
`MB.56.3.5` owns collapsing it with the CLI suite. `MB.56.3.2` had already
deleted `critical_raw_ingress.rs`; its two §6 legs came here.

## LOG MUST-COVER matrix

| LOG item | Test | Result |
|---|---|---|
| libphp not a shared object / package sentence | `a_libphp_that_is_not_a_shared_object_names_the_package` | covered (route+entry+package+`BLASTIT_LIBPHP`, no fallback word) |
| ZTS refusal, `.dynsym`, zero stage-0 `dlopen` canary | `a_zts_libphp_is_refused_from_dynsym_without_a_dlopen` | covered (canary empty) |
| unsupported minor naming 8.3–8.6 | `an_unsupported_php_minor_names_the_supported_set` | covered (8.2 stub, verbatim sentence) |
| shim built for another minor refused before the listener | `a_shim_for_another_minor_refuses_before_the_listener` | covered (`803` vs `8.4.27`, no `ready`, no listener, load only below the fork) |
| relative `BLASTIT_LIBPHP` refused | `a_relative_blastit_libphp_is_refused` | covered (`must be an absolute path to a libphp.so`) |
| probe hang killed by `BLASTIT_PROBE_TIMEOUT_MS` + containment | `a_probe_that_hangs_is_killed_by_its_timeout_and_reaped` | covered (2 s, `pid≠init`, `ppid=init`, `pgid=pid`, uid/euid = route's, `inet=1`, gone from `/proc`) |
| libphp/shim replaced between stages (inode pin) | `a_libphp_or_shim_replaced_between_the_stages_is_refused` | covered (rendezvous, both objects, sentence + fix) |
| `no_libphp_is_mapped_above_the_view_worker` (`/proc` maps) | same name | covered (init clean; above-view roles EACCES or clean) |
| “no second listener / no php-fpm” | `no_php_server_stands_between_blastit_and_the_library` | covered |
| manifest edited after stage-0 approval | `a_manifest_edited_after_stage_zero_approved_it_is_still_refused` | covered |
| production: no privileges / writable tree / symlink+FIFO / raw / preset | see map above | covered (writable legs are root-gated and skip loudly here) |
| env refusals by name, never value | `one_boot_reports_every_environment_refusal_by_name_only` | covered (4 errors, one boot; sentinel absent) |
| `BLASTIT_*` stripped, injected default never overrides | `the_worker_environment_drops_blastit_and_keeps_a_present_default` | covered (`PYTHONDONTWRITEBYTECODE=0` survives) |
| WSGI host refusals verbatim | `an_unusable_cpython_is_refused_with_its_documented_sentences` | covered (3.12, 3.14t, missing `libpython` with 4 remedies) |

## Dropped cases (with reasons)

- **`target_session_attrs=any`**: this tree **accepts and forwards** it
  (`blastit-env` allowlists the parameter; no value check exists anywhere), so
  there is no boot refusal to assert. Adding one is a production-behaviour
  change outside this slice.
- **Python `--py-lib-fd`/`--py-shim-fd` tampering**: not reachable from an
  integration test without a full stage-0 configuration document (and a real
  libpython pair); the PHP inode pin covers the “entry/shim descriptor tampered”
  story end to end.
- **PHP probe containment `egress = true` twin**: LOG asks for the denied
  containment only; the twin asserts the filter honours a route's `egress`,
  which is `e2e_dev`'s grant/egress surface, not a refusal.
- **World-writable shim dir / RUNPATH deployment-gate legs** (deleted with
  `critical_php_embed`): they need root **and** a staged rootless tree; the same
  `host::Provenance` gate is exercised by this suite's root-only writable-tree
  leg (`is writable by another uid`, `chmod go-w`, sticky twin). Recorded here
  rather than asserted a second time.
- **`critical_config_and_presets` preset/seccomp legs**: left in that file (see
  above); `blastit --preset nope` is asserted here only as the *ordering* proof
  that nothing reaches stage 0.

## Evidence

```
$ cargo test -p blastit --locked --features python --test e2e_prod_refusals
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make fast-check
…
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
FAST_CHECK_EXIT=0
```

`make fast-check` exit **0**, zero warnings (`rs-lint` clippy clean after
reaping the child unconditionally in `run_bounded`/`run_spawned`).

## Follow-ups (not in this node's paths)

1. `Makefile:203` still runs `cargo test -p blastit --test no_python_build`
   (deleted) — must become `--test e2e_prod_refusals` (`MB.56.3.5`).
2. `.github/workflows/ci.yml` root-test exclusion list still names
   `critical_dev_and_fail_closed` with `EXCLUDED_COUNT = 2`; the step fails when
   a listed suite disappears. Drop that entry (this suite's unprivileged leg
   skips loudly as root) (`MB.56.3.5`).
3. `make rs-no-default` (`clippy --no-default-features --all-targets
   -D warnings`) is already red for `e2e_dev.rs`; this suite shares that
   dead-code shape under zero features (`MB.56.3.5`'s Makefile sweep).
4. File size: 2,153 LOC — the ten PHP legs and the two stub C sources are the
   reason; `MB.56.3.5` owns the global ≤5,500 budget.

## ARCH REVIEW FIX

- **Symlinked-`Blastfile` refusal reverted** (it refused the canonical
  ConfigMap/Secret mount and `BLASTIT_MANIFEST=/proc/self/fd/N`, and a regular
  file is swappable just the same): `Symlinked`, `read_production_manifest`, the
  wiring arm, the stage-0/stage-1 call sites and both symlink tests are gone;
  FIFO/device/dir refusals are unchanged. `docs/reference/blastfile.md` now
  states that stage 0 approves the file and stage 1 re-reads and re-verifies it
  (regular file or link alike), so a swap between the stages is refused.
- **`make check` owners fixed**: `Makefile:203` runs `--test e2e_prod_refusals
  -- --nocapture` (the two `cfg(not(feature="python"))` legs run again, and skips
  are visible); `.github/workflows/ci.yml`'s root-job exclusion list is now empty
  (`EXCLUDED_COUNT = 0`) and its comments no longer name deleted suites.
- **Restored §6/§5 legs in `e2e_prod_refusals.rs`**: `only_a_granted_route_gets_a_pg_socket`,
  `an_empty_grant_gets_a_denying_listener`, `an_egress_false_view_cannot_open_inet`
  (so the egress oracle is Rust-tier again, not Docker-only), `json_events_never_carry_a_port_key`,
  and `a_dev_boot_on_an_occupied_port_names_the_bind_failure` (the real binary,
  replacing the deleted harness-retry leg).
- **Oracles strengthened**: `no_php_server_stands_between_blastit_and_the_library`
  now matches the tree's socket **inodes** against `/proc/<pid>/net/{tcp,unix}`
  (listener must be exactly the http port; no named php/fastcgi socket); the
  non-dumpable-role halves skip loudly (printed) and are complete in the CI root
  job. Production refusals assert `assert_port_closed`/`assert_never_served`.
- **Loud skips**: every skip uses `println!` (visible under `--nocapture`, which
  the Makefile/CI legs now pass), never `eprintln!`.

### `--learn` attribution legs (closed)

- `learn_attributes_queries_per_route_across_concurrent_live_views` and
  `first_declared_route_keeps_all_queries_when_unrouted_requests_exist` are
  restored in `crates/blastit/tests/e2e_dev.rs` over its existing `--learn`
  harness (`learn_dev`, `Project`, `PgUpstream`): a dedicated
  `LEARN_ATTRIBUTION_APP` runs one fixed statement per path so the learned
  grants are read back from `effective_grants()` — route `a` keeps `SELECT 111`
  and `SELECT 333` while `b`'s view is live and never receives `b`'s
  `SELECT 222`, and with unrouted requests the inferred routes are exactly
  `["a", "b", "extra", "zzz"]` with the fallback's `SELECT 1` kept off both
  declared routes. Nothing is left over from this node.
