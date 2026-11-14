# Testability review — `docs/design/backend-runtimes.md` (MB.12), round 2

**Lens:** `testability`
**Reviewed:** `docs/design/backend-runtimes.md`, `docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md`, `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md`, `docs/design/backend-runtimes.resolve-owner.md`, `docs/VISION.md`; repo files cited by them: `Makefile`, `crates/blastit-sys/src/{lib,testutil,seccomp}.rs`, `crates/blastit/tests/{smoke,dev_signal_reap}.rs`, `crates/blastit-scheduler/tests/fairness.rs`, `crates/blastit-dbproxy/src/learn.rs`, `crates/blastit-manifest/tests/{critical_php_manifest,docs_fences}.rs`, `crates/blastit-php-sapi/{src,tests,c}`, `crates/blastit-py/src/audit.rs`, `python/tests/runnable_docs/inventory.toml`, `e2e/rootless/run.py`, `e2e/php-sapi/`, `docs/research/php-embed-sapi.md`.
**Verdict:** **FAIL** — 0 `BLOCKER`, 6 `MAJOR`, 15 `MINOR`.

The Round-1 resolutions are mostly real: the `ProcSource` seam, the exact `fairness.rs:414,461,509` replacement, the 4×3 residue matrix, the `docs` tier row, and the `(M-1)`–`(M-6)` table are now in the design, and the stub-ELF split is the right shape for a PHP-free `make check`. Six defects remain, all in *oracle wiring*: one cited helper is unreachable from the test that must use it, two requirements are assigned to files whose owning leaf does not implement them, one "machine-checked" gate has no checker, one stub cannot be written because the detection mechanism it feeds is unspecified, and one enum variant is contradicted by the doc's own tables.

| Prompt check | Result |
|---|---|
| 1. Host-independent `cc` stub ELFs (`critical_php_embed.rs`) vs real `libphp` (`make php-sapi-e2e` / `MB.3.18`) | ⚠️ Split is right; ZTS/ABI stub contract unspecified (M5), 5 s timeout budget (m13) |
| 2a. `R4` budget check in `critical_provider_parity.rs` | ❌ Oracle file's owning leaf (`MB.3.23.2`) is scoped to subreaper/residue (M2) |
| 2b. Cross-UID `EACCES` in `e2e/rootless/run.py` | ❌ Probe absent; no leaf schedules it (M3) |
| 2c. `ProcSource` seam + replace `fairness.rs:414,461,509` | ✅ exact lines; seam specified |
| 2d. `ObsTarget::Fixture` in `learn.rs` | ❌ Contradicted by §4.6/§5.2 (`Unrouted`); producer unspecified (M6) |
| 2e. 4×3 `(pid, starttime)` residue matrix | ✅ dimensions specified; grandchild case needs a `View` worker (m10) |
| 2f. `docs` tier row (`MB.3.20` `STALE` removal) | ✅ `docs_fences.rs` + `inventory.toml` + `make docs-check` |
| 2g. Machine-checked `platform-matrix.json` + `(M-1)`–`(M-6)` | ❌ No checker; `(M-1)`/`(M-2)` oracles vague (M4, m4, m5) |

---

## MAJOR

### M1 — `blastit_sys::testutil::child_ok` cannot be reached from the integration test that `R14` assigns it to

**Quotes.** §7.1 `R14` (line 312): “`crates/blastit-sys/tests/critical_embedded_seccomp.rs` (`MB.3.23.1`, using `blastit_sys::testutil::child_ok` per case)”. `crates/blastit-sys/src/lib.rs:141–142`: “`#[cfg(test)]` / `mod testutil;`”. `crates/blastit-sys/src/testutil.rs:100`: `pub fn child_ok`.

**Failure mode.** An integration test compiles the library **without** `cfg(test)`, and `mod testutil` is private, so `blastit_sys::testutil::child_ok` does not exist in that compilation. `install()` returns `NotSingleThreadedSeccomp` on a cargo test thread (`seccomp.rs:114–130`), so the test cannot simply call `install()` inline. The cheap implementer either gets a compile error and re-implements the fork/exit-code dance (which the doc says to reuse), or moves the test into `seccomp.rs` (contradicting §7.1's path and `MB.3.23.1`'s `tests/**` ownership). Round-1 `m8` suggested this citation without checking the `cfg(test)` boundary.

**Suggestion.** Make the module reachable: `#[cfg(any(test, feature = "testutil"))] pub mod testutil;` plus a `testutil` feature enabled for the critical target, **or** move `child_ok`/`in_child` into `crates/blastit-sys/tests/common/mod.rs` and cite that path. State which one in §7.1 so `MB.3.23.1` does not guess.

### M2 — `R4`'s budget oracle lives in a file whose owning leaf is scoped to subreaper/residue, so the budget check is unowned

**Quotes.** §7.1 `R1`/`R4`/`R5`/`R15` (line 307): “**R4 budget check (`M1-T1`):** `BLASTIT_VIEW_MAX_WORKERS=2` rejects `workers = 3` (naming `2` and `3`) & `ceiling = 1 < allocation = 2`”. §8.2 row 10 (line 364): “`MB.3.23.1`, `MB.3.23.2` … `.23.2`: subreaper + `WNOWAIT` + 4×3 residue (`cargo test -p blastit --test critical_provider_parity --locked`)”. `crates/blastit-manifest/tests/critical_php_manifest.rs:219–245` only tests the grammar bound `1..=64`, and its own comment says the budget is “a boot check with a `Config`, not a manifest rule”.

**Failure mode.** The only leaf that creates `critical_provider_parity.rs` is scoped to the subreaper/residue work; a cheap implementer following §8.2 row 10 will not add R4. A mutant that deletes the Stage-0 `workers > view_worker_cap()` and `provider_ceiling < route_allocation` comparisons survives the whole critical tier. The doc also does not say how the env-dependent case is isolated (`std::env::set_var` is process-global across parallel tests).

**Suggestion.** Add R4 to `MB.3.23.2`'s deliverable text and acceptance, **or** move the R4 case to `critical_config_and_presets.rs` (already covered by `MB.3.15`'s `tests/**`) and name that file in §7.1. Specify the isolation: spawn a child `blastit` with `BLASTIT_VIEW_MAX_WORKERS=2` and assert the exit code/message, not an in-process env mutation.

### M3 — The cross-UID `EACCES` oracle is cited to `e2e/rootless/run.py`, which has no such probe, and no leaf schedules adding it

**Quotes.** §7.1 `R3`/`R16` (line 309): “**e2e:** `e2e/rootless/run.py` (`M2-T1`) … `e2e/rootless`: `setresuid` route B `connect()` to route A's `0700` socket dir -> `EACCES`”. §7.2 `(M-3)` (line 326): “`e2e/rootless/run.py` + `php-sapi-e2e` socket probe”. `e2e/rootless/run.py` contains no `setresuid`, `connect(`, `views/`, or `EACCES`; it checks capabilities, `os.setuid(0)` refusal, and boot logs. The only cross-UID `EACCES` oracle on disk is `crates/blastit/tests/smoke.rs:1790`, a standard-tier test that prints “skipped: needs root” (`smoke.rs:9,61`).

**Failure mode.** R16's DAC claim and `(M-3)` are unverified in the e2e tier; the doc cites a driver that does not contain the probe, and §8.2 row 12's `MB.3.7`/`MB.3.18` deliverable says only “`make e2e`”. A cheap implementer will either not add it or add an early-return that `critical-skip-check` does not catch (it is not a `critical_*` file).

**Suggestion.** Specify the probe concretely — e.g. `docker compose exec -u <route_b_uid> sh -c '… connect to /run/blastit/views/<route_a>/.s.PGSQL.5432 …'` asserting `EACCES`, plus the owner-UID positive case — and add it to `MB.3.7`/`MB.3.18`'s deliverable in §8.2. Alternatively, cite `smoke.rs` as the standard-tier oracle and make the e2e probe the privileged confirmation.

### M4 — “Machine-checked” `scripts/platform-matrix.json` has no executable checker

**Quotes.** §7.2 (line 320): “`scripts/platform-matrix.json` records `{minor, arch, libc, status: "proven" | "dropped", reason, sha256}`; a release check fails if any packaged `libblastit_php*.so` lacks `status == "proven"` or if a `"dropped"` entry has an empty `reason`”. Repo grep: zero references to `platform-matrix` in `Makefile`, `scripts/`, `python/tests/`, `crates/`, or CI; `scripts/` has no such file.

**Failure mode.** “A release check fails” is prose. Nothing prevents a shim from shipping with no `proven` record, or a `dropped` entry with an empty `reason`; the `(M-1)`–`(M-6)` table names per-gate commands but not the provenance checker. The drop-not-degrade rule in `ADR-0010` §4 item 4 therefore cannot gate a release.

**Suggestion.** Name the checker (`scripts/check-platform-matrix.py` or `python/tests/critical/test_platform_matrix.py`), its command (`make platform-matrix-check`), and its owning leaf (`MB.3.18`), and add it to §8.2 row 12's deliverable. State the packaged-shim discovery rule (`target/php-sapi/*.so` or the release manifest) and the exact failure message.

### M5 — ZTS/ABI detection is unspecified under the “zero `dlopen` in Stage 0” rule, so `libphp_stub_zts.so` cannot be written

**Quotes.** `R6` (line 49): “filesystem `stat`/`open` only in Stage 0 — zero `dlopen` before Stage-1 `execve`; missing, unreadable, ZTS, or shim-unmatched runtimes exit `1`”. §4.7 item 1 (line 249): “the host `libphp`'s probed `(major, minor, NTS)`”. `R8` (line 51): “Never `fork` a `libphp` probe from secret-bearing Stage 0”. Research `docs/research/php-embed-sapi.md:54`: “At Stage-0 preflight, a short-lived probe child `dlopen`s the host's `libphp.so` (`RTLD_LOCAL | RTLD_LAZY`), calls … `php_version(void)` … verifies NTS (`dlsym("executor_globals") != NULL` && `dlsym("tsrm_startup") == NULL`)”. §7.1 `R6`/`R7`/`R8` (line 310): “`libphp_stub_{nts,zts,runpath,hang}.so`”.

**Failure mode.** The design forbids the only documented ZTS detector (a Stage-0 `dlopen` probe) and does not specify the static replacement. A test author cannot decide what `libphp_stub_zts.so` must contain (a `tsrm_startup` export? an ELF note? a filename suffix?) or what Stage 0 reads. A cheap implementer either `dlopen`s in Stage 0 (breaking the secret barrier) or cannot detect ZTS at all, and the “ZTS refusal” assertion becomes vacuous. The doc also never says whether the Stage-1 `--role=php-probe` loads the shim; the current `php-sapi-probe` does (`crates/blastit-php-sapi/src/probe/main.rs`), and the real shim's undefined Zend symbols would then need a stub shim too.

**Suggestion.** Pin the static detector (e.g. parse `.dynsym` for `executor_globals` present / `tsrm_startup` absent) and state the stub's exported symbols; or move ZTS detection into the Stage-1 probe and change `R6`/P3 to say so. Update `php-embed-sapi.md:54` to match the chosen mechanism, and state explicitly whether the probe loads the shim (and, if it does, that `critical_php_embed.rs` builds a stub shim as well).

### M6 — `ObsTarget::Fixture` is contradicted by §4.6/§5.2 (`ObsTarget::Unrouted`), and the fixture-tagging producer is unspecified

**Quotes.** `R22` (line 65): “`crates/blastit-dbproxy/src/learn.rs` adds `ObsTarget::Fixture` (distinct from HTTP `Unrouted { method, path }`)”. §7.1 `R20`/`R21`/`R22` (line 314): “`<suite>` SQL on `dev/fixture` tagged `ObsTarget::Fixture` (never `route[0]`)”. §4.6 (line 235): “`check -- <suite>` (`serve_and_run` + `dev/fixture` `Unrouted` socket)”. §5.2 (line 283): “`<CheckRoot>/dev/fixture` (`ObsTarget::Unrouted`)”. `crates/blastit-dbproxy/src/learn.rs:84–97` has only `Routed`/`Unrouted`; `:439–447` has `ControlCommand::{PushRouted, PushUnrouted, Pop, SetDb}`.

**Failure mode.** The test oracle names two different variants. A cheap implementer following §4.6/§5.2 reuses `Unrouted`, so fixture SQL is indistinguishable from an unrouted HTTP request and R22's “never pollutes Route 0” assertion cannot be written. The doc also never says how dbproxy learns a connection is the fixture (`PushFixture`? a listener-path→target map? a `SetDb`-style command?), so `critical_php_learn.rs` cannot drive the producer.

**Suggestion.** Change §4.6/§5.2 to `ObsTarget::Fixture`, and specify the producer (e.g. `ControlCommand::PushFixture` sent by `check::serve_and_run` before the suite starts, or a per-listener target configured at bind time) plus the observable (`learn.jsonl` `"scope":"fixture"` and the `check --json` field). Add both cases: fixture SQL under `fixture` and never under `route[0]`; `dev/fixture` `ENOENT` in `Production`/standalone `dev`.

---

## MINOR

- **m1 — `make rs-test-conformance` does not exist.** §7.1 `R24`/`R25` (line 316) and §8.2 row 11 (line 365) cite it; `Makefile` has no target. `MB.9.4` is listed as the owner, but the doc should say “`MB.9.4` adds `make rs-test-conformance`” and give the fallback (`cargo test -p blastit-backend --test conformance`).
- **m2 — `R21`'s JSON keys are wrong.** §7.1 `R20`/`R21`/`R22` says “`requests: 3, exercised: 0` + `unexercised_route`”; `crates/blastit-py/src/audit.rs:892,905` emits `unexercised_routes` and `routes_exercised`. Use the exact keys so the test asserts the real document.
- **m3 — `R2`/`R9`'s `readelf` oracle is not wired.** §7.1 `R2`/`R9`/`R10` says `readelf -d target/.../blastit | grep -c 'libpython\|libphp' == 0`; `make rs-no-python` only checks `cargo tree -e normal | grep -c pyo3`. Add the `readelf` assertion to the target (or name it as a manual acceptance command).
- **m4 — `(M-1)` “heap scan” is not a command.** §7.2 (line 324) says “`/proc/<worker_pid>/environ` + heap scan”. Specify the observable (a canary in `Prepared::app_env` absent from `/proc/<pid>/environ` and from a PHP `getenv`/`$_ENV` dump) and the command.
- **m5 — `(M-2)` “identical … to `glibc`” needs a baseline.** The Alpine container has no glibc run. Pin the exact `widest` count and sorted `DENIED` list in the test (the current `seccomp.rs:894` asserts `widest == 54`; the doc says “`<= 96`”).
- **m6 — `R13`'s `/dev/shm` oracle has no named command.** §7.1 `R11`/`R12`/`R13` says “`/proc/<pid>/maps` no `/dev/shm` & zero `libphp` in `init`/`scheduler`/`dbproxy`”; name the e2e command (`make php-sapi-e2e`) and the assertion.
- **m7 — `R10`'s “zero `match` on `kind()`” has no static oracle.** Add a source-level test (grep/AST) or a conformance assertion; otherwise a `match` can be reintroduced silently.
- **m8 — `R24`'s “zero `case.id` branching” has no oracle.** Same: a source-level check or a conformance case that would fail if `case.id` were matched.
- **m9 — `R6`'s “zero `dlopen` before Stage-1 `execve`” has no canary.** A stub `libphp` constructor that writes a canary file would prove Stage 0 never loaded it; specify it.
- **m10 — 4×3 residue's grandchild case needs a `View` worker.** PHP `EmbeddedView` denies `fork` (`R14`), so “forked grandchildren via subreaper” must use a WSGI `View` worker; state that in the matrix.
- **m11 — `R3`'s “`workers = 1..=64` accepted” is parse-only.** `critical_php_manifest.rs` parses; the boot cap is R4. Say “parses” to avoid a test that boots `workers = 64` under a smaller `view_worker_cap()`.
- **m12 — `e2e/php2shell.py` is not in `MB.3.7`'s deliverable.** `inventory.toml:240–270` already references it; §8.2 row 12 says only “`make e2e`”. Name the file and its assertions (`/users/42` body, asset cache header, hostile query string, `--json` unexercised route).
- **m13 — the 5 s probe-timeout test consumes most of the `< 60 s` critical budget.** §7.1 `R6`/`R7`/`R8` and the testing skill's `< 60 s` target. Consider a shorter configurable timeout (`BLASTIT_PROBE_TIMEOUT_MS=500`) for the test, or move the timeout case to e2e.
- **m14 — `R14`'s “exact updated `widest` count” is still `<= 96`.** The existing test asserts `widest == 54` (`seccomp.rs:894`); pin the new exact number and update the line reference (`seccomp.rs:883` is stale).
- **m15 — `R20`'s `"runtimes"` key does not exist yet.** The current `boot` event (`crates/blastit-events/src/lib.rs:914`) has no `runtimes` field; state that `MB.3.13.3` adds it and name the schema test.

---

## Verified Round-1 resolutions (no action)

- **`ProcSource` seam (`M3`) — resolved.** §4.5 (line 224) defines `pub trait ProcSource` with `read_threads`/`read_pgrp_schedstat`/`read_stat_ticks`, injected into `Scheduler` (`OsProcSource` in prod/dev, deterministic fake in `critical_fair_cpu.rs`). §7.1 `R17`/`R18`/`R19` (line 313) asserts the exact arithmetic, the `hidepid=2` `PermissionDenied` → `Ceiling` path, and one real forked `view.chan` smoke test.
- **`fairness.rs:414,461,509` replacement (`M4`) — resolved.** The three line numbers are exactly `an_under_reporting_view_is_charged_at_least_until_its_first_frame` (414), `an_under_reporting_view_does_not_buy_its_client_priority` (461), and `a_report_larger_than_the_observed_time_is_clamped` (509). §4.5 (line 224) says `MB.9.3` removes the superseded first-frame floor and replaces those assertions with `ProcSource`-backed floor tests, so the old and new models cannot both pass.
- **4×3 residue matrix (`M9`) — resolved.** §7.1 `R1`/`R4`/`R5`/`R15` (line 307) specifies (normal exit, Stage-0 boot refusal, SIGTERM/SIGINT, worker crash) × (0 descendant survivors identified by `(pid, starttime)` incl. forked grandchildren, `<runtime>` dir `ENOENT`, sockets `ENOENT`), reusing `crates/blastit/tests/dev_signal_reap.rs`'s identity-based tracker (`dev_signal_reap.rs:22–29,140–165`).
- **`docs` tier row (`M8`) — resolved.** §7.1 `R20`/`R21`/`R22` (line 314) names `docs_fences.rs` (`MB.3.20`) + `inventory.toml` + `make docs-check`; `docs_fences.rs:54–55` holds the `STALE` entry `("docs/frameworks/php.md", "fastcgi: unknown key", "MB.3.20")`, and §8.2 row 2 assigns the removal to `MB.3.20`.
- **`R23` deferral (`M7`) — resolved.** §7.1 `R23` (line 315) explicitly defers the runtime half to `MB.4`/`MB.5` and names `critical_http_upstream_smuggling.rs`, `critical_node_worker.rs`, `critical_jvm_worker.rs`; `critical_always_strip.rs` covers the `NODE_*`/`JAVA_*`/`CLASSPATH` strip now.
- **The PHP grammar test is real.** `crates/blastit-manifest/tests/critical_php_manifest.rs` already asserts `[route.fastcgi]` is an unknown key that names itself, `listen` is not a PHP key, `workers` is `1..=64`, and the accepted twin parses in the same test.
- **The shim contract test is real.** `crates/blastit-php-sapi/tests/critical_shim_contract.rs` proves `PHP_VALUE`/`PHP_ADMIN_VALUE`/`REDIRECT_STATUS`/`HTTP_PROXY` are unrepresentable (`ServerVar::new` returns `Err(Reserved)`), with the C shim re-checking them.
- **The docs-snippet machinery is real.** `docs_fences.rs` parses every complete `blastit.toml` fence, and `python/tests/runnable_docs/inventory.toml` records per-fence reasons; the docs-tier gap is wiring, not a new mechanism.
