# AUDIT MB.3.13 (closure) — did the seven gaps close, and what did closing them open?

- **Date:** 2026-11-15
- **Node / gate:** `plan/nodes/MB/MB.3/MB.3.13/NODE.md` (epic, `ready`, now
  depending on `.37`, `.39`, `.40`, `.41`); this is the closure pass over
  `docs/testing/AUDIT-MB.3.13.md` (the first audit).
- **Base / tip:** base `7d4df45` (the first audit's tip); tip `1523e50`
  (`MB.3.41`). In between: `.38` `3c2afab`, `.39` `e8763ff`, `.40` `9b77ad2`,
  `.42` `c56b1e1`, `.43` `304c5b1`, `.44` `d1a016b`, `.37` `4749882`, `.41`
  `1523e50`. Range delta: **+7 682 / −213**, of which **+1 580** is
  `critical_php_embed.rs` (8 → 19 tests), **+884** `host.rs`, **+794**
  `stage0.rs`, **+407** `scheduler.rs`, **+130** `view/src/php.rs`.
- **Scope (read, never edited):** `crates/blastit/src/{php_view,stage0,host,supervise}.rs`,
  `crates/blastit/tests/{critical_php_embed,critical_php_backend}.rs`,
  `crates/blastit-view/src/{php,runtime}.rs`,
  `crates/blastit-php-sapi/{src/**,tests/critical_shim_contract.rs}`,
  `crates/blastit-frame/src/**`, `crates/blastit-scheduler/src/scheduler.rs`
  (`charge`, `view_done`/`view_end`, `completed_outcome`, `answered_a_bailout`),
  `crates/blastit-events/src/**`, and the CI wiring that decides whether any
  real-pair leg runs (`php-sapi.yml`, `Makefile`'s `rs-test-php*`, `ci.yml`,
  `e2e/php_sapi_probe.py`).
- **Requirements:** `MB.3.13` `# Acceptance` (4 bullets); `ADR-0010` §1.1–§1.3,
  §2, §3, §4.1–§4.2; `docs/design/backend-runtimes.md` `R6`, `R7`, `R11`, `R12`,
  `R18`, `R19`, `B2-R5` and its oracle map; skills `testing` §1–§4,
  `security-invariants` §1–§3/§5, `dx-first`.

## Verdict

**All seven gaps CLOSED; no regression. Two new gaps (one MAJOR, one MINOR) and
five residuals.** The MAJOR one is new *in kind*: `MB.3.41` moved the bailout's
label and charge onto a frame (`DONE`) whose producer and consumer are exercised
end-to-end **only** on a host with a real `libphp`, and left the scheduler's own
in-process harness unable to say `5xx` — so the rule the epic's third
acceptance bullet rests on is invisible to `make check` while three unit tests
of its pure helpers are green. That is `MB.3.46`. The MINOR one is a stale
claim inside the protected suite's own doc (`MB.3.47`). The residuals are the
production `run()` scope literal (untestable unprivileged, named with its line), the
`st_dev` half of the inode pin no fixture can move, and the recorded `Boot::launch` port flake.

## Inventory & classification (everything added or changed since the first audit)

11 new protected tests, 15 new standard tests, 5 rewritten; **0 fluff,
0 broken-oracle, 0 misplaced**.

| Suite (tier) | Tests | Added | Class | Note |
|---|---|---|---|---|
| `crates/blastit/tests/critical_php_embed.rs` (critical, protected) | 19 | +11 | **critical** | 19/19 green here in 1.59 s. Every new leg has an accepted twin or an ordering assertion; none is `#[ignore]`d, none skips |
| `crates/blastit/tests/critical_php_backend.rs` (critical, protected) | 5 | 0 | **critical** | untouched in this range and still the only end-to-end "serves or refuses-by-name" leg |
| `crates/blastit-php-sapi/tests/critical_shim_contract.rs` (critical, protected) | 15 | 0 | **critical** | unchanged; still the ABI/INI/httpoxy oracle |
| `crates/blastit-scheduler/src/scheduler.rs` (standard) | 3 | +3 | **useful, insufficient** | `a_completed_response_is_a_bailed_5xx_or_an_answer`, `only_a_bailed_5xx_a_view_answered_is_a_crash`, `a_bailed_500_pays_its_wall_and_a_scheduler_502_or_504_does_not` test *pure helpers*; the call sites they justify are untested (`N1`) |
| `crates/blastit-view/src/php.rs` (standard, feature `php`) | 17 | +1 | **useful** | `a_contained_bailout_is_its_own_done_outcome` is the shim-result → `DONE` mapping; the transport already proves `DONE` carries `end.outcome`, and the end-to-end label is protected in the embed suite |
| `crates/blastit-events/src/{json,lib}.rs` (standard) | 2 changed | +1 | **useful** | `a_bailed_request_decodes_with_its_own_outcome` (encode+decode) plus the `as_str` distinctness asserts; the JSON table row is a byte-exact line |
| `crates/blastit-frame/src/{frame,lib}.rs` (standard) | 1 changed | 0 | **useful** | `DoneOutcome::Bailed` in `every_frame()` and `from_code(3)`; `from_code(9)` still refuses, so the set stays closed |
| `crates/blastit/src/host.rs` (standard) | 24 | +5 | **useful** | `dynamic_search_paths_reads_both_tags_and_both_classes`, `a_search_entry_is_resolved_only_when_the_boot_can`, `a_writable_search_path_is_refused_and_a_protected_one_is_not`, `a_writable_tree_is_refused_and_a_protected_one_is_not`, `a_capable_object_is_refused` (capability predicate injected, so it runs unprivileged). Each has its accepted twin |
| `crates/blastit/src/stage0.rs` (standard) | 18 in `php_boot_gate` | +5 | **useful** | the pair/tree refusals at unit level, the `Deployment` seam's two arms (`MB.3.44`), and the `dev`-scope twin. Unit twins of protected legs, not substitutes |
| `crates/blastit/src/php_view.rs` (standard) | 5 (4 + 1 re-exec'd) | 0 | **useful** | `MB.3.42` exchanged the fixture acquisition for `rename()` and added the "the fixture did not replace the file" panic; assertions unchanged and equally strong |
| root-only halves in `host.rs` / `*::placement` | 2 | 0 | **useful, silent skip** | `has_file_caps_sees_one_that_is_there` and `a_real_capability_on_the_shared_interpreter_is_refused_under_every_name` return early when not root (`security.capability` needs `CAP_SETFCAP`). `ci.yml`'s root phase runs them; the unprivileged rule is carried by the injected-predicate twins above. Pre-existing, documented in `MB.3.43` |
| `crates/blastit-php-sapi/tests/php_native.rs`, `crates/blastit-view/tests/php_native.rs` | 2 + 1 | 0 | **useful** | `#[ignore]`d harnesses for the real `dlopen`; correct tier, unchanged |

Assertions that assert nothing, called out on purpose:

- `assert_no_serving_surface`'s `TcpStream::connect` (`critical_php_embed.rs`)
  is **vacuous**: the child has already exited, so the port is closed either
  way. The real witness for "no listener" is the absence of `ready` (emitted
  after `bind_tcp`). Documented; no action (the flake it can produce is `R4`).
- `a_dev_boot_of_a_world_writable_tree_still_serves` accepts any
  `HTTP/1.1 …` status (`first_answer`). That is the leg's contract (served vs
  refused) and it asserts `ready` first, so it is not vacuous.

## Requirement → protected-suite map (against the current suites)

| Requirement | Protected oracle (today) | State |
|---|---|---|
| `ADR-0010` §1.1 — `200` through `view.chan`, `cpu_ns` on the completion path | `critical_php_backend::a_php_route_is_served_or_the_boot_refuses_naming_the_package` (served branch) | **proved; CI-witnessed** (php-sapi `rs-test-php-image`) |
| §1.2 — `[route.php]` without `BLASTIT_APP`; static+php forwards misses | same leg (asset / front controller / `.php` 404) + `route.rs` unit test | **proved; CI-witnessed** |
| §1.1 / `R11` — `libphp` never in `init`/`http`/`scheduler`/`dbproxy` | `no_libphp_is_mapped_above_the_view_worker` (init content, above-view content or `EACCES`, `PR_SET_DUMPABLE(0)` asserted) + the shim-minor canary pair | **proved; CI-witnessed** |
| §1.3 / `R6` — discovery, NTS statically, ZTS refusal, no `dlopen` before the `execve` | `a_non_library…`, `a_zts_libphp_refuses_before_anything_loads`, `stage0::php_boot_gate` (18) | **proved everywhere** |
| §1.3 / `R7` — `(st_dev, st_ino)` per object, `O_NOFOLLOW` re-open | `a_libphp_replaced…` **and** `a_shim_replaced_between_the_stages_refuses_the_boot` (G1) | **proved everywhere** |
| §1.3 — probe child before `bind_tcp`, own pgid, route uid, `PDEATHSIG`, timeout kill + reap | `a_shim_for_another_minor…`, `a_probe_that_hangs_is_killed_and_refuses_the_boot` + `assert_probe_containment` | **proved everywhere** |
| §3 — the filter takes the route's `Egress`; INI and filter cannot disagree | `an_egress_route_lets_the_probe_open_a_socket` (G2) **and** its denied twin; INI half in `critical_shim_contract::allow_url_fopen_follows_the_declared_egress_both_ways` | **proved everywhere** |
| §4.1 — the refusal sentences (missing / ZTS-or-mismatch / inode / timeout) | every refusal leg via `assert_package_sentence` + `assert_no_fallback_recipe` | **proved everywhere** |
| §2 / `R12` — `$_SERVER` in C, `Proxy:` never `HTTP_PROXY`, no `PHP_VALUE`, INI `ZEND_INI_SYSTEM`, `allow_url_fopen` by egress | `critical_shim_contract` (4 tests) + the `php.rs` seam test | **proved at the primitive** (the `view.chan`→`$_SERVER` seam stays unprotected by design: one composition line) |
| `R12` — a request body reaches PHP; the worker's cap bounds it | `a_form_body_reaches_the_front_controller` (G4) | **proved; CI-witnessed** |
| Epic bullet 3 — `exit()`/fatal contained, slot respawned, next request served | `an_exit_…`, `a_fatal_…` (`containment()`) | **proved; CI-witnessed** |
| Epic bullet 3 — "charged at least its wall time" | `a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time` + the `cost_ns >= wall_ns` assertion in all three | **proved; CI-witnessed** — *fast-tier witness missing (`MB.3.46`)* |
| §1.1 / `R19` — `cpu_ns` is process CPU, not duration | `a_blocking_script_is_charged_cpu_time_not_wall_time` (G3) + the sleep leg | **proved; CI-witnessed** |
| Epic bullet 4 / `R11` — no `libphp` symbol above the worker | `no_libphp_is_mapped_above_the_view_worker` | **proved; CI-witnessed** |
| `R7`/`B2-R5` — plain/`!file caps`/no writable `DT_RPATH`/`DT_RUNPATH`/non-writable `manifest_dir`/`entry`/`static.root` | `the_deployment_gate_is_invoked_behind_its_sentence` + `a_world_writable_{manifest_root,shim_directory}…` + `a_runpath_into_a_writable_directory…` + `the_same_tree_owned_by_init_is_accepted` + `a_capable_object_is_refused` | **proved everywhere** (G6) |
| `R7` — the rule is `Production`-only, `dev` still serves a 0777 checkout | `a_dev_boot_of_a_world_writable_tree_still_serves` (end-to-end) + `a_development_boot_does_not_apply_the_production_rule` | **proved everywhere** |
| §8.1 vocabulary — a contained bailout is `bailed`, an app's own `5xx` is `ok` | `containment()`'s `outcome == "bailed"` on the boot's own JSON line | **proved; CI-witnessed** — *fast-tier witness missing (`MB.3.46`)* |
| `R6` — `check` exits `2`, empty stdout, same sentence | `check_reaches_the_same_discovery_gate` | **proved everywhere** |
| `R18` — `RESP_END_COST` 9/17 bytes, once per request | shim contract + view unit tests + `answered_a_bailout` never touching the frame | **proved; frame size unchanged** |

## G1–G7 verdicts

| Gap | Verdict | Oracle (exact name) | Still passes if the rule is deleted? |
|---|---|---|---|
| **G1** shim inode pin | **CLOSED** | `critical_php_embed::a_shim_replaced_between_the_stages_refuses_the_boot` | **No.** A removed/loosened pin makes every attempt *serve*; the bounded retry then panics saying "a **removed inode pin** produces exactly these same served attempts". The *miscompare* mutation (shim vs the library's pair) is also red here, via the probe twins which require the timeout's own sentence — `assert_package_sentence` would see the inode sentence instead. Acceptance also pinned by `a_php_slot_gets_one_binding_and_a_wsgi_slot_none` (unprotected) |
| **G2** `egress = true` twin | **CLOSED** | `an_egress_route_lets_the_probe_open_a_socket` (denied twin: `a_probe_that_hangs_is_killed_and_refuses_the_boot`) | **No.** A hardcoded `egress: false` (the exact `unwrap_or(false)` at `php_view.rs:314`) answers `inet == 1`; the leg asserts `0`. Runs on a **PHP-free** host, so `make check` sees it |
| **G3** cpu-vs-wall | **CLOSED** | `a_blocking_script_is_charged_cpu_time_not_wall_time` (200 ms `usleep`, `cpu < wall/4`) + `a_sleeping_bailout…` (`cpu <= wall/20`) | **No.** Reporting the request's duration as `cpu_ns` puts `cpu ≈ wall` and misses by the whole block; both legs also assert the block is in `duration_ms` |
| **G4** POST body | **CLOSED** | `a_form_body_reaches_the_front_controller` | **No.** A `RequestV1` without its body answers `x=absent`; the leg asserts `x=1`. The denied half (8 KiB past `BLASTIT_MAX_BODY_BYTES` → `4xx`/`5xx`, never a truncated `200`) and "the slot still serves" are asserted too. The *scheduler's* `BodyPlan` refuses first, as the test says |
| **G5** crashed-request charge | **CLOSED** | `a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time`, plus `cost_ns >= wall_ns` in `an_exit_…`/`a_fatal_…` | **No.** The 60 ms sleep makes any cpu-based charge miss by orders of magnitude, and `outcome == "bailed"` pins the label. `MB.3.41` narrowed the rule to *bailed* `5xx` (an app's own `500` keeps the clamp), the honest reading of `R19` — but its wiring has no fast-tier witness (N1) |
| **G6** production provenance | **CLOSED** | `the_deployment_gate_is_invoked_behind_its_sentence` (the *call site*, via `host::deployment_boot_check` → the same `php_boot_in`) + the three decision legs + the accepted twin + the `dev`-scope leg | **No.** Deleting the `deployment_refusal` call, the tree claims, the search-path walk, the file-cap predicate or the `Production`-only scope each reddens a named leg. Residual: the literal that *chooses* `Deployment` at `stage0.rs:1852` is untested (R1) |
| **G7** CI witness | **CLOSED** | `.github/workflows/php-sapi.yml` → `make rs-test-php-image` (both protected suites on the image's real pair) + `guard: the pair legs must serve, not refuse` | **No.** If the pair legs take the refusal branch the guard fails on the suites' own marker; if the served evidence disappears (`served: HTTP/1.1 200 OK`, `retired pid … respawned pid`) it fails too; a missing image fails the target instead of skipping. Triggers are `push`/`pull_request` on **`develop`** only — a push to `release/0.2.0` does not run it; the gate is the PR |

## The new wire code (`DoneOutcome::Bailed`)

What a test would catch, and what nothing catches today:

| Question | Answer |
|---|---|
| unknown/out-of-set code | **Caught at the codec**: `DoneOutcome::from_code(9)` → `BadEnum{field:"outcome"}` and `from_code(3)` → `Bailed` in `frame::tests::enum_fields_reject_undefined_values`; `every_frame()` round-trips `Bailed` (`blastit-frame`, 71 tests green). **Not caught end-to-end**: no test sends a `DONE` with byte `4` down a `view.chan` and asserts the scheduler fails closed instead of reusing the slot. Folded into `MB.3.46` leg 3 |
| a `bailed` frame whose status is `2xx` | **Caught in the decision**: `completed_outcome(true, 200, true) == Ok` and `answered_a_bailout(true, 200, true) == false` (an `exit()` after a completed `200` is an answer). **Not caught in the wiring** — no fast-tier test drives it (`MB.3.46` leg 1's twin) |
| completed-`5xx` (`ok`) vs bailout | **Caught in the decision** (the three helper tests) and **end-to-end on a PHP host** (`containment()` asserts `bailed`). The `MB.3.41` rule that an app's own `500` is `ok` has **no** end-to-end leg: every PHP fixture bails. **Fast tier: no** |
| is the event's `bailed` reachable from `critical_php_embed`? | **Yes.** `containment()` reads `boot.request_event("/crash")["outcome"]`, i.e. a line the boot wrote through `events::json` and the test decoded — the only end-to-end assertion of the JSON vocabulary |
| `DONE`-less completed `5xx` | Documented degradation: `view_end` defers the label and the charge to `DONE`; if the frame never arrives, `release` (via `view_gone`/stuck retire) charges the clamp and the event keeps the default `Outcome::Ok`. `MB.3.45` item 3 owns narrowing or fixing that promise |

Exact hole: **the producer (`view::serve_forever` writing `end.outcome`), the
scheduler's two call sites (`view_done`'s `request.bailed = bailed`, `view_end`'s
`deferred`) and therefore the label+charge pair are witnessed only by
`critical_php_embed` on a real pair.** Reverting any of them leaves `make check`
green — the helpers still pass. Filed as **`MB.3.46`**; the harness change is
small (`tests/common/mod.rs` can already send any status via `resp_head`).

## New gaps

| Id | Severity | Gap | Owner |
|---|---|---|---|
| **N1** | **MAJOR** | The `bailed` label + wall charge have no unprivileged witness: `crates/blastit-scheduler/tests/common/mod.rs`'s `answer()`/`answer_with_cost()` only ever send `200` + `DoneOutcome::Completed`. `MB.3.38` (follow-up 3) and `MB.3.41` (follow-up 3) both recorded it; **no node owned it**. Three mutations survive `make check` (see the wire table). | **`MB.3.46`** |
| **N2** | **MINOR** | `critical_php_embed.rs:48–58` ("# CI coverage … `php-sapi.yml` … never this suite … the containment leg has no CI witness") is **false** since `MB.3.39`. `MB.3.39` left it as a follow-up ("protected, not in this node's paths"); `MB.3.37` and `MB.3.43` both owned the file afterwards and did not fix it. A gate reading it would either re-add the job or discount a green `php-sapi` run. | **`MB.3.47`** |
| **D1** | **MINOR** (owned) | `docs/reference/log-events.md:39–40` lists the closed `outcome` set without `bailed` (and glosses `cost_ns` as fair-queuing only, which the bailout rule now qualifies). A public page. | already **`MB.3.45`** item 1 — not duplicated |
| **R1** | residual | `stage0.rs:1852` passes `PhpShimScope::Deployment`; the seam pins `php_boot_in`, not that literal. An unprivileged test cannot reach `run()`. | no task: `e2e/rootless` is the only place it could be proved |
| **R2** | residual | `supervise.rs:559`'s `egress.get(index).unwrap_or(false)` for a **PHP** slot is pinned only for the WSGI case (`critical_dev_and_fail_closed::an_egress_false_view_cannot_open_inet`); the PHP vector alignment is pinned at the probe (`php_view.rs:314`) by G2's twin | no task (same builder, both ends tested once) |
| **R3** | residual | `has_file_caps_sees_one_that_is_there` and `a_real_capability_on_the_shared_interpreter_is_refused_under_every_name` return early when not root. `ci.yml`'s root phase runs them; the unprivileged rule has injected-predicate twins | no task (`MB.3.43`'s documented split) |
| **R4** | residual | `Boot::launch`'s free-port hand-off (bind, drop, let the child bind) can lose the port to a foreign listener; a "must refuse" leg then reads "something accepted" as "served" (~1 run in 12 under load). Noise-red, never a false green | recorded in `MB.3.40` follow-up 4; named for `MB.3.47`'s owner |
| **R5** | residual | `php_view::open_pinned` compares `st_dev` **and** `st_ino`; every fixture changes only the inode (a `rename()` in the same directory), so deleting the `st_dev` comparison is a survivor in both legs. A bind mount is what would move `st_dev`. | no task (campaign note for `blastit-php-sapi`/`blastit` at the milestone gate) |

## Mutation

**Not run this pass** (dispatch: no campaign; it compiles the workspace per
mutant). `cargo-mutants 27.1.0` is installed and `make mutants-rust` is the gate
tool. The milestone gate should campaign **two** crates, in this order:

```console
# 1. the policy/validation crate by the testing §4 definition: ini.rs renders the
#    ZEND_INI_SYSTEM lockdown, request.rs maps CGI names (httpoxy, reserved
#    $_SERVER), abi.rs is the frozen C layout. Its oracle is one protected,
#    table-driven file.
$ make mutants-rust PKG=blastit-php-sapi

#    File-scoped, same wrapper (disk scratch + cgroup memory cap + the Makefile's
#    RUST_ENV), because `make mutants-rust` forwards no extra arguments:
$ TMPDIR="$(make -s print-mutants-tmp)" \
  MUTANTS_JOBS=2 MUTANTS_TIMEOUT=90 scripts/mutants-run.sh \
  -p blastit-php-sapi --file src/ini.rs --file src/request.rs --file src/abi.rs

# 2. the PHP runtime's shim→frame mapping (`ended`, the body/limit paths), which
#    `make check` does compile (feature unification) but only a real pair serves.
$ TMPDIR="$(make -s print-mutants-tmp)" \
  MUTANTS_JOBS=2 MUTANTS_TIMEOUT=90 scripts/mutants-run.sh \
  -p blastit-view --features php --file src/php.rs
```

Target: **≥ 75 %** killed on `ini.rs`/`request.rs`; on `php.rs`, judge the
survivors by whether `MB.3.46`'s new leg would kill them (that is the point of
the task). Neither campaign needs `--test`: the unit tests live in the lib
target, and a `--test` filter would skip them.

Manual branch inspection (the places a campaign would start), with the
"would a test fail?" answer — **run**, not read, wherever it could be:

| Branch | A test fails? |
|---|---|
| `php_view::open_pinned` shim comparison deleted | **yes** — `a_shim_replaced_between_the_stages_refuses_the_boot` (protected, `make check`) |
| shim compared against the *library's* pair (the `.13.3` bug) | **yes** — `a_php_slot_gets_one_binding_and_a_wsgi_slot_none` (unit) and the probe twins (protected: they need the timeout's own sentence) |
| `probe_bindings`'s `unwrap_or(false)` | **yes** — `an_egress_route_lets_the_probe_open_a_socket` (protected, `make check`) |
| `supervise.rs:559`'s `unwrap_or(false)`, PHP slot | **no** for PHP; **yes** for WSGI (`an_egress_false_view_cannot_open_inet`) — `R2` |
| `deployment_refusal`'s call removed from `php_boot_in` | **yes** — `the_deployment_gate_is_invoked_behind_its_sentence` |
| `run()`'s `Deployment` literal → `Development` | **no** — `R1` |
| `php_trees` drops the static-root/entry claim | **yes** — `a_world_writable_manifest_root…`, `the_same_tree_owned_by_init_is_accepted` |
| `dynamic_search_paths` returns `vec![]` on an unreadable object | **yes** — `dynamic_search_paths_reads_both_tags_and_both_classes` ("no ELF magic") and `a_runpath_into_a_writable_directory…` |
| `resolve_search_entry("")` → `Some("")` | **yes** — `a_search_entry_is_resolved_only_when_the_boot_can` |
| `charge_ns`'s `failed` branch removed | **yes** — `a_bailed_500_pays_its_wall_and_a_scheduler_502_or_504_does_not` (unit) |
| `answered_a_bailout`'s `head_sent` half dropped | **yes** — `only_a_bailed_5xx_a_view_answered_is_a_crash` (`(false, 502/503/504, false)` arms) |
| `view_done`'s `request.bailed = bailed` deleted | **no** in `make check`; yes on a real pair — **N1** |
| `view_end`'s `deferred` removed | **no** in `make check`; yes on a real pair — **N1** |
| `serve_forever` writes `Completed` for `end.outcome` | **no** in `make check`; yes on a real pair — **N1** |
| the `st_dev` half of `open_pinned` dropped (compare `st_ino` only) | **no** — both swap fixtures `rename()` within one directory, so only `st_ino` moves; a same-inode, different-device pair needs a bind mount. Both legs (`libphp`, shim) keep this survivor — `R5` |
| `ended`'s `bailed_out` branch deleted | **yes** — `a_contained_bailout_is_its_own_done_outcome` (unit, deletable in the same commit) |
| `Outcome::Bailed.as_str()` → `"ok"`; `parse_outcome("bailed")` → `None` | **yes** — `a_bailed_request_decodes_with_its_own_outcome`, `reason_enums…` |
| `DoneOutcome::from_code(3)` → `Completed` | **yes** — frame unit tests |

## Tasks filed

| Node | Title | Tier |
|---|---|---|
| `MB.3.46` | The bailed charge and label need an unprivileged witness: scheduler `DONE` legs (`critical_charge`) | critical test, fast tier |
| `MB.3.47` | `critical_php_embed`'s CI-coverage note is false since `MB.3.39` (protected file, doc-only) | protected test doc |

`MB.3.45` (filed by `lead` while this audit ran) already owns the public
`log-events.md` row, so it is **not** duplicated here. `MB.3.46` and `MB.3.47`
touch disjoint files (`scheduler/tests/**` vs the protected embed file) and both
depend only on done nodes, so they can run in parallel; `MB.3.46` must not touch
`src/scheduler.rs` or `view/src/php.rs` while `MB.3.45` holds them.

## Evidence

```console
$ cargo test -p blastit --locked --test critical_php_embed
running 19 tests ... test result: ok. 19 passed; 0 failed; 0 ignored (1.59 s)
$ cargo test -p blastit --locked --test critical_php_backend
running 5 tests ... test result: ok. 5 passed; 0 failed; 0 ignored (0.27 s)
$ cargo test -p blastit-view --locked --features php
one target 30 passed; doc-tests 1 passed  (php::tests = 17 via --lib php::tests)
$ cargo test -p blastit-scheduler --locked
lib 50 passed; admission 11, critical_ingress_enforcement 9, critical_rewrite 32,
critical_static_assets 14, critical_static_entry 6, critical_static_resolve 18,
ctl_channel 9, fairness 17, flow 6, head_allocs 2, isolation 13, lifecycle 18,
memory 14, routing 14, doctest 1 — all ok, 0 failed
$ cargo test -p blastit-frame --locked && cargo test -p blastit-events --locked
71 passed + doctest; events lib/bin green
$ cargo test -p blastit-php-sapi --locked
critical_shim_contract 15 passed; php_native 2 ignored (needs the host pair)
$ cargo test -p blastit --locked --lib php_view::tests
4 passed, 1 ignored (re-exec'd as a single-threaded child)
$ cargo test -p blastit --locked --lib stage0::php_boot_gate   # 18 passed
$ cargo test -p blastit --locked --lib host::tests             # 24 passed
```

`make check` was started and **reached the docs phase**, then stopped by this
audit because `docs-unit` builds a wheel and this pass must not:

```console
$ make check
guard OK · critical-guard OK · rs-fmt OK · rs-lint OK · rs-doc OK ·
rs-test + test-critical: 2 503 `test … ok` lines, 0 failed, up to and including
critical_php_embed 19/19, critical_php_backend 5/5, critical_shim_contract 15/15,
python critical 50/50 · py-lint: `ruff check` "All checks passed!" (the run then moved on)
→ stopped at docs-unit (wheel build, out of this audit's licence)
```

## Gate recommendation

**Clear the MB.3.13 epic gate: all seven gaps are closed with tests that go red
when the rule is deleted, the protected tier is in the right place, and
`make check` plus the `php-sapi` job together cover every acceptance bullet —
two of them only in the Docker job, which is now the PR-into-`develop` witness
G7 asked for.** One condition should ride along rather than block: `MB.3.46`
before the *milestone* gate, because `MB.3.41` moved a `R19` accounting rule onto
a frame whose producer and consumer only a real `libphp` host exercises, and
resource accounting is exactly the class we must not leave to one slow job; and
`MB.3.47` (doc-only, protected file) so the suite's own account of its CI
coverage stops contradicting the workflow. Neither is a regression of the seven
gaps; both are the cost of closing them.
