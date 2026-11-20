# MB.56.3.2.2 — `e2e_dev.rs` part 2: embedded PHP, per-view grants, `php2shell`, pinning

## What changed

`crates/blastit/tests/e2e_dev.rs` grew the **PHP half** of `TESTING_PLAN.md` §2
(plus §6's tampered-entry story) and absorbed the three suites it replaces. The
consolidation plan allows four `e2e_*.rs` files at **≤ 3,000 LOC** each; this file
is **2 769 LOC / 16 tests** and stays a single suite (no fifth file).

| File | Before | After |
|---|---|---|
| `crates/blastit/tests/e2e_dev.rs` | 1 504 LOC, 8 tests (WSGI only) | **2 769 LOC, 16 tests** (8 WSGI + 7 PHP + 1 probe-drift) |
| `crates/blastit/tests/common/php_host.rs` | — | 215 LOC (new: the host probe) |
| `crates/blastit/tests/common/views.rs` | 126 LOC | 168 LOC (`request_with` + doc) |
| `crates/blastit/tests/common/libphp_probes.rs` | 251 LOC | 127 LOC (its own `#[cfg(test)]` self-checks removed) |
| `crates/blastit/tests/critical_php_embed.rs` | 4 480 | **deleted** |
| `crates/blastit/tests/critical_php_backend.rs` | 1 159 | **deleted** |
| `crates/blastit/tests/critical_parallel_views.rs` | 1 033 | **deleted** |

No production logic changed. No `#[ignore]`, no mock server, no `StubPolicy`: every
PHP leg is a real `blastit dev` boot, a real `libphp` + shim, a real `view.chan` and
(where grants are involved) a real PostgreSQL v3 peer (`tests/common/pg_upstream.rs`)
dialled through the route's own dbproxy socket. The concurrency calibration
(`views::{WIDTH, CONTROL_FLOOR, CONTROL_MARGIN, burst}`) is reused, not duplicated.

The host pair is resolved by `tests/common/php_host.rs`, an independent probe (raw
`std` file access + the `ADR-0010` §4.1 names), and its `PROBES` table is compared
with `blastit-view-php/src/discovery.rs` at compile time, so the probe can never
walk a host the boot walks differently.

## New tests (PHP half)

| Test | `TESTING_PLAN.md` story |
|---|---|
| `a_multi_view_php_front_controller_serves_assets_and_only_its_view_grants` | §2 "Laravel and Symfony (PHP) with per-view PostgreSQL rights and static files" |
| `a_php_split_store_renders_the_laravel_slots` | §2 same story, `docs/frameworks/php/laravel.md`'s split `DB_*` store |
| `a_php_exit_and_a_fatal_recover_without_dropping_the_pool` | §2 "recover cleanly when an individual request calls `exit()` or hits a fatal" |
| `a_php_request_cannot_become_interpreter_arguments_or_a_proxy` | §2 "immune by construction to CGI/FastCGI argument-injection (`php2shell`)" + the `form` body reaching the app |
| `a_php_entry_rewritten_in_place_is_refused_until_a_restart` | §6 "tampered application entry files" |
| `a_php_pool_serves_four_requests_at_once` | §2 concurrency + §4 diagnostics (same control as the WSGI leg) |
| `a_php_view_process_cap_bounds_the_pool` | §6 sizing: `BLASTIT_MAX_VIEW_PROCESSES` bounds one view |
| `the_php_probe_table_matches_the_boot` | fixture-oracle honesty: the probe's discovery order is the boot's |

What each proves, concretely:

- **Grants.** `public GET "/read", "/read/*"` holds `select products(id, name)`;
  `admin POST "/admin/write"` holds the same select plus `insert products(id, name)`.
  The public view's `SELECT … FROM orders`, its `… cost FROM products`, an
  `INSERT INTO orders` and an `UPDATE products` are each a `500` with the proxy's
  `42501` in the body (a proxy→app passthrough oracle, not a documented sentence)
  and an `sql.denied` event naming `public db parse` with the statement normalised
  to `$?` (never `= 7`, never `'leak'`). The upstream peer records `FROM products`
  and the admin view's insert, and **no** statement mentioning `orders`, `cost` or
  `UPDATE` — the public view contributed none of its own, without any literal
  count that would pin the test's own request order. The compromised view keeps
  serving its own grant, the sibling is untouched, and the rendered
  `DATABASE_URL` in the worker carries neither the real password nor the proxy's
  real user.
- **Split store.** `postgres { host $DB_HOST … }` renders `DB_HOST=/proc/self/cwd`,
  the proxy's port, `DB_DATABASE=shop` and no operator credential; one granted
  statement runs over the socket those slots name.
- **Static.** `static "/" { root "public" }` answers `public/assets/app.css` from
  disk without forking a worker (`spawned_pids` stays empty), and
  `/index.php`, `/index.phtml`, `/index.phar` are all `404`.
- **Faults.** `BLASTIT_MAX_WORKERS=1` + `prefork` warms the pool's one slot;
  `/crash-exit` (`exit(0)`) and `/crash-fatal` (`E_USER_ERROR`) are each a `5xx`
  request event with `outcome: "bailed"` (never `200`, never `view_died`), the
  crashed pid leaves `/proc`, the next request is a `200` carrying the pinned
  page, and the live worker set is exactly one **fresh** pid — the pool kept its
  slot. Prints the `retired pid … respawned pid …` line CI greps.
- **`php2shell`.** The CVE-2024-4577 `%AD` payload, a literal
  `-d+auto_prepend_file` query string, and `Proxy:`/`Proxy-Authorization` headers
  all answer `200` with `"argv":[]`, `"proxy":""` and empty
  `auto_prepend_file`/`auto_append_file`; both `-d` vectors name a
  `public/prelude.php` that **exists** and prints `PREPENDED-RAN`, so "it never
  ran" is a witness that can actually fire. `allow_url_include` is not consulted
  (PHP 8.0 removed it, so it is falsy whatever a payload does). A form body
  carrying the same payload reaches `$_POST` **as data** and changes no setting.
- **Pin.** A same-size, in-place rewrite (same inode, same `st_size`, `st_ctime`
  crossed) makes every request a `5xx` that never contains the decoy marker, and
  the worker logs `changed since this boot recorded it` naming the entry plus
  `restart blastit to run the file as it is now`; restoring the page in place and
  restarting serves it again.
- **Concurrency.** Four concurrent 200 ms requests finish `CONTROL_MARGIN` under a
  serialized control measured in the same function, on four warm workers, with no
  `process.spawned` inside the window; and the same burst under
  `BLASTIT_MAX_VIEW_PROCESSES=2` runs on exactly two processes.
- **Evidence.** The grant leg prints `served: HTTP/1.1 200 OK (libphp …)` and the
  fault leg the `retired pid … respawned pid …` pair, which is what
  `make rs-test-php-image`, `php-sapi.yml` and `e2e/php-sapi/run-two-package.sh`
  grep.

## Why the grant legs speak the wire instead of `pdo_pgsql`

The review asked for PDO. It cannot run in this repository's environment, measured
rather than assumed:

```
$ php -r 'var_dump(extension_loaded("pdo_pgsql"));'
bool(false)
$ ls /etc/php/8.4/mods-available/ | grep -i pgsql     # (no match: no pgsql.ini)
```

and the leg images install `phpX.Y-dev`, `…-embed` and `…-curl` but **no**
`phpX.Y-pgsql` (`e2e/php-sapi/Dockerfile`'s `ARG PHP_CURL=php8.4-curl`), so a PDO
branch would be dead code on every host and every CI leg. The fixture says so in
its own doc comment instead of claiming Laravel shape: it is a PSR-style router
over a real PostgreSQL v3 client that keeps the shape the grants are learned in
(`$1` binds, never inlined literals), and the Laravel/Symfony `$_SERVER` shape is
`e2e/php2shell.py`'s. The split `DB_*` store *is* proved at runtime (above); a PDO
leg becomes possible the day an image ships `phpXY-pgsql` — recorded under
Follow-ups.

## Review fixes outside the test file

- `Makefile`: `PHP_SUITE_ARGS` is now
  `--test e2e_dev --no-default-features --features php,static` (one executable, no
  CPython shim needed); `rs-test-php`/`rs-test-php-image` describe that, the
  image's executable-count assertion is `-ne 1`, and both the root and the
  unprivileged docker runs must print the `served:` + crash-recovery lines and
  must contain **no** `^SKIP ` line.
- `.github/workflows/php-sapi.yml`: the step is `make rs-test-php-image (e2e_dev
  PHP legs on the real pair)`, the summary row names `e2e_dev`, and the guard is
  now `guard: the PHP legs must serve, not skip` — it fails the job on `^SKIP ` *
  and * still requires `served: HTTP/1.1 200 OK` and `retired pid … respawned pid`.
  That image remains the only boot-level witness of the docs' 8.3–8.6 statement.
- `e2e/php-sapi/Dockerfile` + `Dockerfile.two-package`: compile
  `e2e_dev --no-default-features --features php,static`, record **one** executable.
- `e2e/php-sapi/run-two-package.sh`: fails on a `SKIP` line instead of the deleted
  suite's refusal sentence; keeps the `served:` and resolved-library assertions.
- `scripts/platform_matrix.py`: the `suites` contract and every `SHIM_REACHES`
  caption name the PHP legs of `e2e_dev` instead of the deleted suites (the
  served measurements stand; the deleted pass counts are gone).
- `e2e/php/probes.md`: the `Owner file::fn` column is labelled what it is — a
  **design-era** test id — and the R8 paragraph states that the authoritative
  witness of every row is its `e2e/php2shell.py` probe (`critical_fastcgi_lockdown`
  was deleted with the FastCGI param channel).
- `docs/troubleshooting.md`: a row quoting the entry-pin refusal verbatim
  (`changed since this boot recorded it` … `restart blastit to run the file as it is now`).

## Old file → new test map

`critical_php_backend.rs`

| Old test | Disposition |
|---|---|
| `a_php_route_is_served_or_the_boot_refuses_naming_the_package` | **served half absorbed** into the grant/static leg (asset from disk, miss → front controller, `.php`/`.phtml`/`.phar` `404`); the package-refusal half dropped (§6) |
| `a_symlinked_blastit_libphp_is_resolved_before_it_is_served` | dropped — discovery/provenance, `blastit-view-php` discovery + `e2e_prod_refusals` |
| `a_blastit_libphp_that_is_not_a_library_refuses_the_boot` | dropped — refusal sentence (§6) |
| `check_reaches_the_same_discovery_gate` | dropped — `blastit check` exit `2` (§5, `e2e_check_fmt`) |
| `a_static_only_manifest_is_not_refused_by_this_gate` | dropped — static-only boot (`.3` slice) |
| `the_probe_table_matches_stage0` | **absorbed** as `the_php_probe_table_matches_the_boot` |

`critical_php_embed.rs`

| Old test | Disposition |
|---|---|
| `a_non_library_blastit_libphp_refuses_before_anything_loads` | dropped — refusal sentence |
| `a_zts_libphp_refuses_before_anything_loads` | dropped — ZTS refusal + canary (needs `cc` stub ELFs) |
| `an_unsupported_php_minor_refuses_the_boot_naming_the_supported_set` | dropped — minor support sentence |
| `a_shim_for_another_minor_refuses_the_boot_before_the_listener` | dropped — stage-1 probe refusal + canary |
| `a_probe_that_hangs_is_killed_and_refuses_the_boot` | dropped — `BLASTIT_PROBE_TIMEOUT_MS` (§6) |
| `an_egress_route_lets_the_probe_open_a_socket` | dropped — egress filter shape during the probe (§6) |
| `a_libphp_replaced_between_the_stages_refuses_the_boot` | dropped — stage-0→stage-1 inode pin of `libphp` (§6) |
| `a_shim_replaced_between_the_stages_refuses_the_boot` | dropped — the same pin for the shim (§6) |
| `the_worker_cannot_be_built_without_the_entry_pin` | **dropped** — a source-text assertion; the charter bans source-grep guards and the behavioural pin leg is the rule |
| `no_libphp_is_mapped_above_the_view_worker` | dropped — `/proc` maps containment (§6) |
| `an_exit_is_contained_and_the_next_request_is_served` | **absorbed** into `a_php_exit_and_a_fatal_recover_without_dropping_the_pool` |
| `a_fatal_is_contained_and_the_next_request_is_served` | **absorbed** into the same leg |
| `a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time` | dropped — charge accounting (§5 `1 request`), not §2 fault isolation |
| `a_blocking_script_is_charged_cpu_time_not_wall_time` | dropped — `cpu_ns` accounting (§5) |
| `a_queued_request_is_never_handed_to_a_retiring_view` | dropped — queue lifecycle / fair queuing (§6, scheduler) |
| `a_form_body_reaches_the_front_controller` | **absorbed** (body → `$_POST` as data) into the `php2shell` leg; the oversized-body `413` half dropped (§6 ingress cap, `critical_raw_ingress`) |
| `a_decoy_inside_the_manifest_dir_is_refused_and_a_restart_serves` | dropped — rename/inode half of the pin (the node names the in-place rewrite) |
| `an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file` | **absorbed** as `a_php_entry_rewritten_in_place_is_refused_until_a_restart` |
| `a_world_writable_manifest_root_refuses_a_production_boot` | dropped — production provenance (§6, `host::Provenance`) |
| `a_world_writable_shim_directory_refuses_a_production_boot` | dropped — same |
| `a_runpath_into_a_writable_directory_refuses_a_production_boot` | dropped — same (`DT_RPATH`/`DT_RUNPATH`) |
| `the_deployment_gate_is_invoked_behind_its_sentence` | dropped — the gate's call site (§6) |
| `the_same_tree_owned_by_init_is_accepted` | dropped — provenance accepted twin (§6) |
| `a_dev_boot_of_a_world_writable_tree_still_serves` | dropped — the `Production`-scoped twin of the same rule (§6) |
| `a_dev_boot_reloads_a_php_save_in_place_by_rename_and_through_an_include` | **not absorbed here** — live reload + loaded-file reporting is `MB.56.3.2.3` (§4) |

`critical_parallel_views.rs`

| Old test | Disposition |
|---|---|
| `serialized_then_parallel_php` | **absorbed** as `a_php_pool_serves_four_requests_at_once` |
| `the_cap_holds_and_the_pool_shrinks` | **absorbed** as `a_php_view_process_cap_bounds_the_pool` (cap + pid set); the idle-retirement half dropped (§6 scheduler) |
| `the_probe_table_matches_stage0` | **absorbed** (one drift test now, not three) |

Dropped cases are refusals/accounting/queue-lifecycle stories (§5, §6) whose owners
are `e2e_prod_refusals` (`MB.56.3.4`), `e2e_check_fmt` (`MB.56.3.3`), the
scheduler/dbproxy crate boundaries and `.3`'s §4 reload slice — no §2 story lost.

## Evidence

```
$ cargo test -p blastit --test e2e_dev
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

served: HTTP/1.1 200 OK (libphp /usr/lib/libphp8.4.so) — the mount answered the asset …
php faults: retired pid 597720 respawned pid 597726 after a contained bailout
php2shell: query string, headers and a form body stayed data over the real SAPI
php entry pin: served, refused in place with the documented sentence, and served again after a restart
php: serialized control 816ms (BLASTIT_MAX_WORKERS=1), parallel 202ms over 4 warm process(es)
php cap: BLASTIT_MAX_VIEW_PROCESSES=2 served four concurrent requests over [597884, 597889] in 417ms
php split store: DB_HOST=/proc/self/cwd, DB_DATABASE=shop, a scoped credential, and one granted statement through the rendered slots

$ bash -n e2e/php-sapi/run-two-package.sh && bash -n <the other touched scripts>
```

Host: `libphp8.4.so` 8.4.28 + `target/php-sapi/libblastit_php84-x86_64-gnu.so`, so
every PHP leg ran its **served** branch (no `SKIP`). On a host without the pair each
leg prints one `SKIP …` line naming what to install and returns — never a panic, and
never an assertion of a refusal §2 does not own. CI's `php-sapi` image is the served
witness there and fails the job on that line.

`make fast-check` and `make docs-check`: see the tails below.

## Follow-ups

- **`crates/blastit/src/**` doc comments** still name the deleted suites
  (`host.rs:57`, `stage0/gates.rs:261,435`, `stage1/spawn.rs:382`) and so do
  `blastit-view-php/src/{pinning,discovery}.rs` and `docs/knowledge/LESSONS.md`;
  these are prose, not gates, and the nodes that own those paths
  (`MB.56.3.4`, `MB.56.5`) should re-point them.
- A **PDO leg** becomes possible once a leg image ships `phpXY-pgsql`: add the
  package to `e2e/php-sapi/Dockerfile` (and `platform_matrix.py`'s `PHP_BUILDS`),
  then branch the fixture's client on `extension_loaded('pdo_pgsql')` so the wire
  client stays the fallback.
- The **`PR_SET_DUMPABLE(0)` maps witness** (`no_libphp_is_mapped_above_the_view_worker`)
  and the **no-`php-fpm`-surface listener witness** have no replacement here: both
  are §6 containment/startup stories owned by `MB.56.3.4`.
- Comment-only references to `critical_php_*` inside `e2e_dev.rs` describe the
  absorption itself and are intentional.
