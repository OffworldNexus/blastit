# Result MB.3.13

## Summary

Every `[route.php]` route is served by a forked `SeccompRole::EmbeddedView` worker per
route slot (`max_inflight == 1`) over `view.chan`, driving the host NTS `libphp` + the
matching shim through one transport; no `php-fpm` process, no FastCGI, no fallback
(`MB.3.13.1`, `MB.3.13.3`).

Nothing that could make that unsafe reaches a listener: discovery, the static NTS check,
per-object `(st_dev, st_ino)` pins, a probe child that loads the pair, the minor/ABI
match, `workers` vs the view budget and the `Production` provenance rule all run at boot,
each refusal naming the route, its `entry`, the package and the fix (`MB.3.13.2`,
`MB.3.36`, `MB.3.40`).

CPU is exact and unforgeable (`CLOCK_PROCESS_CPUTIME_ID` in the shim), rides the same
`RESP_END_COST` path WSGI uses, and a contained bailout is billed its wall time and
labelled `bailed`, not `ok` (`MB.3.38`, `MB.3.45`, `MB.3.46`).

For the developer: one documented page shape serves as written and one message names the
package to install (`MB.3.13.3` §1); the epic adds no CLI flag and no `blastit.toml` key.

The ADR's OPcache open question is settled by what shipped: the **post-fork
`php_module_startup`** path — one startup per worker, no shared writable OPcache SHM
(`MB.3.13.1` Readings, `ADR-0010` §1/§4.3).

## Acceptance

**Bullet 1 — a one-line `[route.php] entry` fixture serves, and `cpu_ns` reaches the
`request` event.** `critical_php_backend::a_php_route_is_served_or_the_boot_refuses_naming_the_package`
(served branch prints `served: HTTP/1.1 200 OK  cpu_ns=… wall_ns=…`) plus
`critical_php_embed::a_blocking_script_is_charged_cpu_time_not_wall_time`. Evidence:
`MB.3.13.3` §1 (event JSON `cpu_ns: 20376019` for `/php-probe`, `cpu_ns: null` for the
static answer) and §2 (the served leg's own `cpu_ns`/`wall_ns` print); `MB.3.37` mutation
(c). Reddens when the route is
accepted but never forked (no `process.spawned`, no `200`), when the PHP request event says
`cpu_ns: null`, or when the shim reports duration as CPU (`.37`'s `cpu_ns=200502401` against
`wall=215034319`). CI witness: `php-sapi.yml` → `make rs-test-php-image`; on any other host
this leg takes the refusal branch.

**Bullet 2 — the boot refusals, protected, with no listener.**
`critical_php_embed::{a_non_library_blastit_libphp_refuses_before_anything_loads,
a_zts_libphp_refuses_before_anything_loads, a_shim_for_another_minor_refuses_the_boot_before_the_listener}`
and `critical_php_backend::a_blastit_libphp_that_is_not_a_library_refuses_the_boot`; the
"only when a `[route.php]` exists" half by
`critical_php_backend::a_static_only_manifest_is_not_refused_by_this_gate` and the unit
twin `stage0::php_boot_gate::a_host_with_no_libphp_refuses_only_when_a_php_route_exists`.
Each asserts exit `1`, empty stdout, `ADR-0010` §4.1's sentence (`assert_package_sentence`),
no fallback recipe, and **no `ready`** — the audit records that
`assert_no_serving_surface`'s `connect` is vacuous and the absent `ready` (emitted after
`bind_tcp`) is the witness. Evidence: `MB.3.13.4` §1, `MB.3.36` §3–4, `MB.3.13.2`
"the exact strings". Reddens when a refusal serves, the sentence loses the route/`entry`/
package, a `ready` line appears, or a static-only manifest is refused. On a **PHP-free
host `make check` runs these same suites and this is the branch they take** — executed, not
skipped (`MB.3.13.4` §5: `unshare -m -r` + `mount --bind /dev/null`, whole suite green); the
served twin is only ever `php-sapi.yml`.

**Bullet 3 — `exit()`/fatal kills neither worker nor slot, and the crash pays its wall.**
`critical_php_embed::{an_exit_is_contained_and_the_next_request_is_served,
a_fatal_is_contained_and_the_next_request_is_served,
a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time}`; the same rule at the
fast tier in `critical_charge::{a_bailed_5xx_pays_its_wall_where_the_same_frames_returned_pay_the_clamp,
a_deferred_5xx_is_charged_to_its_last_byte_not_to_its_done,
a_view_that_answers_then_retires_is_billed_for_its_answer_not_a_death}`. Each asserts
`5xx` (never `200`), `cost_ns >= wall_ns`, `outcome != "ok"`, a respawn whose `ppid` is
init with exactly one live worker, then a `200` on the next request. Evidence: `MB.3.38`
(`cost_ns == wall_ns == 18 013 008` after; `cost 3218461, wall 18003078` when the charge is
reverted), `MB.3.46` mutations 1–2, `MB.3.45` item 7 (the label/charge race). Reddens when
the charge reverts, the label falls back to `ok`, or no respawn appears. CI witness:
`php-sapi.yml` only — the label and the bill are produced and consumed on the real pair.

**Bullet 4 — no `libphp` symbol above the worker.**
`critical_php_embed::no_libphp_is_mapped_above_the_view_worker` reads init's
`/proc/<pid>/maps` and content-asserts every other child of init as root (`EACCES` when
unprivileged), and `a_shim_for_another_minor_refuses_the_boot_before_the_listener`'s
constructor canaries (`(pid, ppid)` with `ppid` = the boot) place both loads below the
fork. Static twin: `readelf -d target/debug/blastit | grep -c libphp` → `0` (`MB.3.13.1`).
Evidence: `MB.3.13.4` §1/§6, `MB.3.36` §4. Reddens when any `dlopen` moves above the fork
(init's maps gain the library, or a canary's `ppid` is not the boot). `ci.yml`'s root phase
runs the content half; the serving leg is again `php-sapi.yml`.

`make check`'s prerequisite list deliberately excludes the pair legs (`MB.3.39`), and
`ci.yml` is untouched.

## Evidence

Verified by the lead in this session; **this writer ran no build or test**:

| Command | Outcome |
|---|---|
| `cargo test -p blastit --locked` | green |
| `make check` | exit 0 |
| `make e2e` | exit 0 (Docker + rootless) |
| `make rs-test-php-image` | 5 + 19 green (`critical_php_backend` + `critical_php_embed`) |
| closure audit | all seven gaps CLOSED; 26 added tests classified (11 protected, 15 standard; 5 rewritten), 0 fluff (`docs/testing/AUDIT-MB.3.13-CLOSURE.md`) |

Carried by the nodes' own RESULTs rather than re-run here: the suite counts (`MB.3.48`
Evidence — 5 and 19; `MB.3.45` — `make rs-test-php-image` `5 + 19`, exit 0), the teeth
(`MB.3.37` four mutation controls, `MB.3.38`'s charge revert, `MB.3.40`'s M1–M4, `MB.3.46`'s
three scheduler mutations), and the flake work (`MB.3.48`: 0 red in 1200 `critical_php_backend`
runs; `MB.3.45`: 1 in 20 before the `Ctl::Exited` ordering fix, 0 in 40 after).

## Files

Per-file change tables are in the child RESULTs (`MB.3.13.1` "What landed", `MB.3.13.3`
10-file table, `MB.3.36` §2, `MB.3.40` "What was implemented"); do not re-list them here.
The surface this epic's `paths` cover (`crates/blastit-view/src/**`, `crates/blastit/src/**`,
`crates/blastit/tests/**`) plus the files the audit had to read to judge it:

- `crates/blastit/src/`: `php_view.rs`, `stage0.rs`, `stage1.rs`, `supervise.rs`, `config.rs`,
  `check.rs`, `host.rs`
- `crates/blastit-view/src/`: `php.rs`, `flow.rs` (`Credit::try_take`)
- `crates/blastit-php-sapi/src/loader.rs`; `crates/blastit-frame/src/frame.rs`;
  `crates/blastit-events/src/{json,lib}.rs`; `crates/blastit-scheduler/src/{scheduler,route}.rs`
- tests: `crates/blastit/tests/{critical_php_embed,critical_php_backend,no_python_build}.rs`,
  `crates/blastit-scheduler/tests/critical_charge.rs`
- CI and operator surface: `.github/workflows/php-sapi.yml`, `Makefile` (`rs-test-php*`),
  `docs/reference/{settings,log-events}.md`

Some of those were siblings' `paths` (`.38`/`.45` scheduler, `.37`/`.46`/`.48` tests, `.39`
Makefile/workflow); the epic's own three globs are the ones above.

## Follow-ups

1. **`MB.3.49`** (active) — `MB.3.45`'s review MINORs: `can_dispatch`'s guard has no leg, a
   `DEBUG` print in a protected test, four stale claims. Does not block: none is a rule this
   epic asserts, and the charge rule is pinned by `critical_charge`'s six legs.
2. **`MB.3.39`'s CI trigger** — `php-sapi.yml` fires on `push`/`pull_request` on **`develop`**
   only, so a push to `release/0.2.0` does not run it. The PR into `develop` is the witness;
   until it opens, bullets 1, 3 and 4's served halves are host-proven only.
3. **`MB.3.40` follow-up 3 — the `.ini`/`extension_dir`/`extension=`/`zend_extension=`
   pre-scan** (`ADR-0010` §3, `R7`) is unimplemented. Does not block: it is defence in depth
   behind the seccomp wall, and both egress directions are proved by
   `an_egress_route_lets_the_probe_open_a_socket` and its denied twin.
4. **`Boot::launch`'s free-port flake** (`MB.3.40` follow-up 4) — **closed** by `MB.3.48`:
   both suites now partition on the boot's own `ready`, not on "the port accepts"; 0 red in
   1200 runs. Recorded so the gate does not re-open it.
5. **Deferred to `MB.9`, not a pass** — the `MB.9.3` hand-off: the `Upstream`/`ViewUpstream`
   retrofit and the shared conformance suite. `NODE.md` Context: "This is the node that makes
   `MB.9.3`'s `ViewUpstream` able to serve `Executor::Php` without any backend-kind branch in
   dispatch." The worker is what `MB.9.3` plugs into; no test in this epic can carry that
   retrofit, and `MB.9` must land before `MB.4`/Node. (Numbering note: `# Acceptance` bullet 2
   is the boot refusals and is proved above; the deferral is this `MB.9.3` hand-off.)
6. Residuals with **no task**, recorded by the audit: `stage0.rs:1852`'s `Deployment` literal
   is unreachable by an unprivileged test (`R1`; `e2e/rootless` is the only place), and the
   `st_dev` half of `open_pinned` needs a bind mount no fixture can perform (`R5`). Neither is
   an acceptance leg; both are mutation survivors, not behaviour gaps.
