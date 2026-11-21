# MB.56.3.2.3 — `e2e_dev.rs` part 3: static/SPA, live reload, `--learn`, shutdown

**Size: 3,100 LOC against the amended 3,100 cap** (`wc -l` below), 24 tests.
Every fixture-heavy block moved into `crates/blastit/tests/common/` rather than
any assertion being dropped: `project.rs` (the temporary root), `proc.rs` (the
`/proc` probes), `static_assets.rs` (the SPA/mount fixtures, the two-backend
oracle), `reload.rs` (the two-view reload project), `check_learn.rs` (the
`check --learn --dry-run` fixture). `e2e_dev.rs` holds the tests and the harness.

## Deleted suites (absorbed, nothing `#[ignore]`d)

`critical_static_cli.rs` (778), `critical_dev_reload.rs` (2,905),
`dev_signal_reap.rs` (545), `critical_raw_ingress.rs` (692) and
`common/libphp_probes.rs` (126, its only caller was a source-parity meta-test).
`critical_dev_reload.rs`'s private host-PHP probe copy (`host_php`,
`php_libphp`, `php_minor`, `php_shim`, walking only `[85,84,83]`) went with it;
every PHP leg reads `common/php_host::host_php()`.

## What the 24 tests prove (story → test)

| `TESTING_PLAN.md` story | Test |
|---|---|
| §2 Django/Flask WSGI across 3.13 + 3.14, trait grants, `$1` read/write, per-view refusal, zero credentials | `a_multi_view_wsgi_project_runs_only_its_trait_grants_on_both_minors` |
| §2 interpreter refusals verbatim | `an_unusable_interpreter_is_refused_with_its_documented_sentence` |
| §2 missing shared `libpython` with all four remedies (loud `SKIP` when both minors ship it) | `a_prefix_without_a_shared_libpython_is_told_every_remedy` |
| §2/§4 threaded concurrency against a serialized control | `a_threaded_wsgi_route_serves_four_requests_at_once` |
| §2 PSR front controller: mount, `.php` source refused, per-view grants, four denials, no credential | `a_multi_view_php_front_controller_serves_assets_and_only_its_view_grants` |
| §2 fault isolation (`exit()`, fatal) | `a_php_exit_and_a_fatal_recover_without_dropping_the_pool` |
| §6 `BLASTIT_MAX_VIEW_PROCESSES` really caps one view (2 pids, two waves, no third fork) | `the_view_process_cap_bounds_one_views_pool` |
| §2 `php2shell`/CGI argument injection | `a_php_request_cannot_become_interpreter_arguments_or_a_proxy` |
| §6 tampered entry file, refused until a restart | `a_php_entry_rewritten_in_place_is_refused_until_a_restart` |
| §2 standalone SPA: `immutable "/assets/*"` (crosses `/`) year-caches `assets/app.9f3c.js` **and** `assets/chunks/vendor.abc.js`, while `report.txt` stays `no-cache`; `ETag`/`304`; `HEAD` = the `GET` head with no body on the asset **and** the fallback; the `index` answer is `no-cache`; `.well-known/security.txt` and `.well-known/acme-challenge/<token>` are served and `.well-known/.env` is `404`; `..` → `400`, dotfile/symlink/`.php` → `404` with **every planted canary absent** (`postgres://leak`, `well-known-leak`, `source-leak`, `/etc/hostname`); no Python or PHP on `PATH` | `a_standalone_spa_serves_immutable_assets_and_confines_every_refusal` |
| §2/§6 both documented backends: same status/kind/cache/`ETag`/length/bytes, `GET` **and** `HEAD`, on 11 rows, with the refusal **kind** asserted (`refused` vs `hidden` vs `spa-fallback`) so `Missing` can never pass for `Hidden` | `both_static_backends_serve_the_same_bytes_and_the_same_refusals` (+ `common::static_assets::assert_parity`) |
| §2 prefix mount strips its prefix; the longest prefix wins | `a_prefix_mount_serves_under_its_prefix_and_the_longest_one_wins` |
| §2 a missing static root stops the boot before a listener binds (`Blastfile`, `mount "/" (index 0)`, `static root "public"`, `no such file or directory`, empty stdout, exit 1) | `a_missing_static_root_stops_the_boot_before_it_binds` |
| §4 banner: one row per view and per mount, and a non-loopback boot names the operator's uid + `BLASTIT_DEV_EXPOSE` | `the_banner_names_every_view_and_mount`, `a_non_loopback_host_needs_the_exposure_acknowledgement` |
| §2 a mount's `fallback` answers `GET`/`HEAD` only: another method is `405` with `Allow: GET, HEAD` (`ADR-0007`) | `the_banner_names_every_view_and_mount` |
| §4 live reload: the interpreter's own `SyntaxError` sentence for a broken preloaded module, the parser's `line:col` for a broken `Blastfile`, last valid topology serving, one reload for the good save, `routes` = **both** view names, `files[0]` absolute, `reason` `blastfile` then `loaded` for a module save, an in-flight request answered `200`, and a `__pycache__` write never reloading | `a_broken_save_keeps_serving_and_a_good_one_reloads_without_dropping_a_request` |
| §4 `--no-reload`: nothing is watched, a save and a cache write change nothing | `a_no_reload_boot_watches_nothing_and_a_cache_write_never_reloads` |
| §4 a `.php` save reloads the route | `a_php_save_reloads_and_the_next_request_serves_the_new_source` |
| §4 `dev --learn`: proposed view + `{id:int}`, `$?` grant (never `= 7`), comment preserved, `/​.env`, `/​.git/config`, `../..` never proposed, exact `learn: 1 route, 1 query added → <abs>` | `a_learn_pass_records_views_and_parameterized_grants_and_ignores_probes` |
| §4 a literal is refused instead of written (exit 1, `learn: refused`, the literal never written) | `learn_refuses_a_literal_and_never_writes_it` |
| §5/§4 `check --learn --dry-run`: exact `learn: 2 routes, 3 queries proposed (dry-run) → <abs>`, the proposal printed, and the `Blastfile` **byte-identical** afterwards (the suite is a shim interpreter that writes real HTTP observations and pushes one `push_unrouted` target through the proxy's control channel, so the counts are the product's) | `a_learn_dry_run_prints_the_proposed_line_and_writes_nothing` (+ `common::check_learn`) |
| §4/`R20` clean shutdown: `SIGTERM` **and** `SIGINT`, exit 0, port closed, no surviving descendant by `(pid, starttime)`, `--dev-root` removed | `sigterm_and_sigint_reap_the_tree_close_the_port_and_remove_the_root` |
| §6 `body raw` without `unvalidated` refused on the production path in both log formats; `dev` serves it byte for byte with the warning | `a_raw_route_without_the_acknowledgement_is_refused_on_the_production_path`, `dev_serves_a_raw_route_byte_for_byte_and_still_warns` |

## Dropped / changed, with the reason

- **`Range` is not asserted at all, either way.** `TESTING_PLAN.md` §2 says
  "byte-range requests" while `StaticResponse::respond` answers the full `200`
  and documents that `Range` is not a parameter. That conflict is a **human
  item** (`uv run --no-project tools/plan.py --agent implementer human add
  "decide: byte-range support or amend TESTING_PLAN §2 static/SPA"`), not a
  test's to settle.
- **`reason` is `loaded` for a module save, not `preload`.** The product's
  `ReloadReason` vocabulary is `loaded` / `blastfile` / …; `preload` names the
  *source* of a report, not the reason word. The suite pins the product's word
  and asserts the two-view name set separately.
- **`HEAD`** is now covered on both the asset and the fallback, through the
  parity helper's method parameter (so `openat2` and `Walk` both answer it).
- **The static `405`/`Allow: GET, HEAD`** is asserted where the docs promise it —
  a **view** pattern that does not declare `POST` — because a path only a
  mount's `fallback` answers is `404` for another method (no view matched).
- **`smoke.rs`'s serving half is kept** (root-only production `Server::start`,
  `needs_root`): an unprivileged suite cannot reach that boot, so `e2e_dev`'s
  dev-mode legs cannot re-prove it. See `## Handed off`.

## Handed off (with reasons)

- `MB.56.4` (production fair-queueing E2E): **PHP concurrency** —
  `critical_parallel_views`' PHP half (four 200 ms requests over four warm
  workers against a serialized control). The pool's fan-out is already witnessed
  here twice (the fault leg's one slot; the cap leg's two-process/two-wave
  bound), and the §6 "slow views cannot starve fast ones" promise is that
  suite's.
- `MB.56.4`: the four-worker PHP wall-clock leg above; the split `DB_*` store
  runtime witness (`docs/frameworks/php/laravel.md`) — store lowering is
  §5/config (`critical_config_and_presets`), the rendered-endpoint promise is
  the grant leg's URL form.
- `MB.56.3`: `smoke.rs`'s root-only production serving legs (routing,
  environment, proxy, crash, parallel, privilege, granted query, stdio,
  credential, idle retire) — fold into a production E2E suite, never delete.
- `MB.56.3.3` (already logged): the **9 static-`check` cases** from
  `critical_static_cli.rs` (all-static `check` audits and exits 0 with the v1
  document; a missing root exits 2; `check --learn` on a static file refuses;
  a mount the suite never requested is a warning, not a finding; `BLASTIT_APP`
  on an all-static file is refused by name).
- `MB.56.3.4` (already logged): the **tampered-manifest TOCTOU** case
  (`production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it`).
- `MB.56.3`: the reload cases that remain unproved here: `.git/`, `node_modules/`
  and log files outside the project are ignored; a store added or removed is one
  warning naming the restart; `--learn` reloads views but skips the manifest
  cycle; an `EPERM` watcher degrades to one `dev.reload.unavailable`; a queue
  overflow reloads every view once; the debounce windows.
- `MB.56.5`: `dev_reload.rs`'s `#[cfg(any(test, debug_assertions))]` doubles
  (`TestClock`, `FakeWatcher`, `report_pair`, `send_batch`) lost their last
  caller with `critical_dev_reload.rs`.

## Documentation

- `docs/local.md`: the reload-window caveat (a request landing in the instant a
  reload swaps the image may wait for the next cycle; nothing in flight is
  dropped).
- `docs/reference/cli.md`: the `dev --learn` row now records that a statement
  inlining a literal is refused instead of written, with `learn: refused` and
  exit `1`.

## Evidence

```
$ cargo test -p blastit --test e2e_dev --locked
running 24 tests
… test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.91s

$ wc -l crates/blastit/tests/e2e_dev.rs
3100 crates/blastit/tests/e2e_dev.rs

$ cargo clippy -p blastit --all-targets --locked --features python -- -D warnings
Finished `dev` profile

$ make fast-check      # green
$ make docs-check      # green
```

Loud skips, never panics: `SKIP PHP …` (no NTS `libphp` + shim), `SKIP … live
reload …` (`dev.reload.unavailable`: Docker's overlayfs answers `EPERM`), `SKIP
a_prefix_without_a_shared_libpython…` (both minors installed).
