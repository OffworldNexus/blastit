# AUDIT MB.3.13 — `PhpEmbedRuntime`: boot gate, probe child, worker, containment

- **Date:** 2026-11-15
- **Node / gate:** `plan/nodes/MB/MB.3/MB.3.13/NODE.md` (epic, `ready`); children
  `.13.1`–`.13.4` plus the two siblings this epic cannot ship without, `.35`
  (shim name) and `.36` (probe child). Child `RESULT.md` files read at
  `plan/nodes/MB/MB.3/MB.3.13/{MB.3.13.1..4}/RESULT.md` and
  `plan/nodes/MB/MB.3/MB.3.36/RESULT.md`.
- **Scope (this audit reads it; it edits nothing):**
  `crates/blastit/src/{stage0.rs,stage1.rs,php_view.rs,supervise.rs,check.rs,config.rs}`,
  `crates/blastit/tests/{critical_php_embed.rs,critical_php_backend.rs,no_python_build.rs}`,
  `crates/blastit-scheduler/src/route.rs`,
  `crates/blastit-php-sapi/{src/loader.rs,tests/critical_shim_contract.rs,tests/php_native.rs}`,
  `crates/blastit-view/{src/php.rs,tests/php_native.rs}`, and the CI wiring that
  decides whether any of it runs with a real `libphp`.
- **Base / tip:** base `2ad39c3` (parent of `.13.1` `341bf72`); tip `7d4df45`
  (`.13.4`), with `.13.2` `71eed2f`, `.35` `34d0bb7`, `.13.3` `72e6d1f`, `.36`
  `271b0b7`, `.13.1` `341bf72` in between (17 commits in the range, several of them
  the `.28`–`.33` siblings). Epic-attributable delta: **+9 220 / −103** lines, of
  which **+2 799** are the two protected suites and **+6 398** are `src/` under
  `crates/blastit`, `blastit-view/src/php.rs` and `blastit-php-sapi/src/loader.rs`.
- **Requirements:** `docs/decisions/ADR-0010-*.md` §1.1–§1.3, §2, §4.1–§4.2;
  `docs/design/backend-runtimes.md` `R6`, `R7`, `R11`, `R12`, `R18`, `R19`,
  §7.2's `R6`/`R7`/`R8` and `R11`/`R12`/`R13` rows (line 318–319, the oracle map),
  and the epic's own `# Acceptance` (four bullets); skills `testing` §1–§4,
  `security-invariants` §1–§3, §5, `dx-first` (the runnable `php.md` page).
- **Not audited:** `MB.3.18`'s platform matrix and container images;
  `e2e/php2shell.py`, `e2e/rootless` (`MB.3.7`/`MB.3.8`); `MB.3.6.4`'s `check`
  stage-1 `Reexec` half; `MB.9.3`'s `ViewUpstream`; the scheduler's own
  `critical_*` suites (only `route.rs` is in scope); the `.28`–`.33` PROVISIONING
  guard suites beyond the one `route.rs` rule `.13.3` changed.

## Verdict

**PASS on tier and on teeth; NOT yet pass on the epic's own bar. 7 real gaps, 4
tasks filed (`.37`–`.40`), no BLOCKER.** The protected tier is in the right place
and would fail a permissive or inverted implementation everywhere it looks — but
it does not look at four rules it is supposed to own (the shim's inode pin, the
`egress = true` half, CPU-vs-wall, the request body), it encodes a **weaker** rule
than the epic's acceptance for the crashed request's charge, and **two of the epic's
four acceptance bullets — 1 (the served fixture with `cpu_ns`) and 3 (containment
and respawn) — have no CI witness at all, and neither does bullet 4's unprivileged
half**, because every CI runner is PHP-free.

- **Tier:** correct. The new oracle is two protected files
  (`critical_php_embed.rs` 8 tests, `critical_php_backend.rs` 5 tests) plus
  `critical_shim_contract.rs` (15) for the C-side INI/httpoxy invariants. Nothing
  in scope is `#[ignore]`d in a protected file, nothing skips, and the suite's two
  outcomes are keyed on an **independent** `host_php()` probe rather than on the
  boot's own exit status — the single most important structural decision in this
  epic, and it is right (`.13.3` attempt 2 MAJOR 2).
- **Teeth:** every refusal has its accepted twin (non-library vs ZTS vs shim-minor
  vs inode vs timeout vs no-host; static-only boot; `workers == cap`; the in-cap
  body and header burst). No test in scope is fluff, and none has a broken oracle.
  Three assertions are **misplaced** (a §3/R7 and a CVE-class rule living in
  unprotected `src` unit tests) — see §"Tier & teeth".
- **Bar:** `# Acceptance` bullet 3 ("the crashed request is charged at least its
  wall time") is **not met** by the implementation and **not asserted** by the
  suite; the node's own evidence shows `cost_ns = 3.2 ms` against `wall = 18.3 ms`.
  `MB.3.13.4` records the deviation in a follow-up, but the assertion stayed weak,
  so a regression cannot be told from today's behaviour. `.38`.
- **CI:** `make check` runs on a PHP-free host, and `php-sapi.yml` never runs
  either protected suite. The containment legs, the real served leg and the maps
  leg's unprivileged half are proved only on a developer host. `.39`.
- **Mutation:** not run this session (see §"Mutation"). `blastit-php-sapi` and
  `blastit/src/php_view.rs` are the two modules I would spend a campaign on, and
  the exact command is recorded for the milestone gate.

## Method

1. Read the epic `NODE.md`, all five `RESULT.md` (including the three review rounds
   recorded inside `.13.1`/`.13.3`/`.13.4`), `ADR-0010` §1–§4, the
   `backend-runtimes.md` rows `R6`/`R7`/`R11`/`R12`/`R18`/`R19` and its oracle map
   (lines 318–319), and every line of the two protected suites.
2. Read the production code those suites claim to cover: `stage1.rs:330–405` (the
   call order), `php_view.rs` (`boot_bindings`, `open_pinned`, `probe_bindings`,
   `probe_child`, `probe_route`), `stage0.rs::php_boot_gate`,
   `supervise.rs::fork_view`/`record_view`, `blastit-view/src/php.rs`
   (`Prepared`, `PhpSink`, `serve_forever`, `read_body`) and
   `blastit-php-sapi/src/loader.rs`.
3. Walked the epic's four `# Acceptance` bullets and the `R6`/`R7`/`R8` oracle-map
   row one by one against the suites, asking for each: **would this assertion still
   pass if the rule were deleted or inverted?**
4. Checked the CI wiring that decides whether any real-pair leg ever runs:
   `Makefile` (`rs-test-critical`, `test-critical`), `.github/workflows/ci.yml`
   (PHP-free `make check`, the root run), `.github/workflows/php-sapi.yml` (the one
   job with a real pair), `e2e/php_sapi_probe.py` (what Docker covers instead).
5. Counted tests per file rather than trusting the `RESULT.md` counts, and diffed
   `2ad39c3..HEAD` per file to separate the epic's lines from the siblings'.
6. Read `cargo-mutants`' availability (`27.1.0` is installed) and decided against a
   campaign in this pass, with the reason and the command recorded below.

## Inventory & classification

| Path | Tests | Tier | Class | Note |
|---|---|---|---|---|
| `crates/blastit/tests/critical_php_embed.rs` | 8 | critical (protected) | **critical** | the epic's refusal matrix (non-library, ZTS, shim minor, inode swap, probe timeout), the maps leg, the two containment legs. 1 927 lines, 0.7 s on a PHP host / 0.6 s on a PHP-free one; `cc` is a hard requirement |
| `crates/blastit/tests/critical_php_backend.rs` | 5 | critical (protected) | **critical** | the served page (asset / front controller / `.php` 404), `cpu_ns <= wall_ns`, the symlink resolution, `check`'s gate, the static-only twin |
| `crates/blastit-php-sapi/tests/critical_shim_contract.rs` | 15 (+136/−10 this epic) | critical (protected) | **critical** | `_Static_assert` layout, reserved `$_SERVER` names, httpoxy drop, INI lockdown both ways, `allow_url_fopen` by `egress`, shim self-containment, the toolchain diagnostics (`.29`'s spawn-vs-not-found fix keeps this file stable) |
| `crates/blastit/src/stage0.rs::php_boot_gate` | 13 | standard | **useful** | the unit-level oracle for discovery: candidate list, `.dynsym` NTS/ZTS, version from the name, shim tuple, `workers` cap, symlink resolution, "reads the ELF and never loads it" |
| `crates/blastit/src/config.rs::php_settings` | 3 | standard | **useful** | the document round-trip that keeps stage 0's `(dev, ino)` pair for **both** objects visible to stage 1 |
| `crates/blastit/src/php_view.rs::tests` | 4 | standard | **misplaced** (2 of 4) | `a_replaced_libphp_or_shim_is_refused_by_name` is the **only** oracle for the shim half of the `(st_dev, st_ino)` pin — a §3/`R7` boundary in an unprotected `src` module (G1). The other two are real plumbing tests (child sequence, missing record) |
| `crates/blastit-view/src/php.rs::tests` | 16 | standard (feature `php`) | **useful**, 1 **misplaced** | `the_php_environment_never_carries_http_proxy` is the CVE-class seam (`Prepared::new`) — its primitive is protected in `critical_shim_contract.rs`, the seam is not (G-notes, folded into `.37`'s context, no separate task). The transport tests (window, cancel, caps, `max_requests`) are genuine |
| `crates/blastit-php-sapi/src/loader.rs::tests` | 5 | standard | **useful** | typed refusals (missing / text / NUL / wrong ABI / wrong minor) with the fix named |
| `crates/blastit/tests/no_python_build.rs` | 3 | standard | **useful** | the interpreter-free build refuses a `[route.php]` manifest at the discovery gate, naming the package (`.13.3` rewrote the fixture off the deleted interim sentence) |
| `crates/blastit-scheduler/src/route.rs::tests` | 1 new (+1 renamed) | standard | **useful** | `a_static_and_php_page_forwards_its_misses_to_the_front_controller` pins all three directions of `.13.3`'s `static_forward` fix. Its only **protected** twin is host-gated (G7) |
| `crates/blastit-php-sapi/tests/php_native.rs` | 2 | e2e-ish, `#[ignore]` + env-gated | **useful** | the only `SharedObject::Fd` load through `/proc/self/fd/<n>`; runs only when `BLASTIT_LIBPHP`/`BLASTIT_PHP_SHIM` are set |
| `crates/blastit-view/tests/php_native.rs` | 1 | same | **useful** | two real requests through `view.chan`, `max_requests = 2`, the clean exit the supervisor respawns on |
| `crates/blastit/src/supervise.rs` | 0 new | — | — | `spawn_now` → `fork_view` + `record_view` is a 156/−33 refactor with **no** new unit test; the PHP SPAWNED/exit path is covered by `critical_php_embed`'s respawn assertions, the WSGI half by the existing suites |
| `e2e/php_sapi_probe.py` (+ `e2e/php-sapi/`) | 1 harness, 3 probe runs | e2e (Docker, CI) | **useful** | shim-level `exit()`/fatal containment, `cpu_ns <= wall_ns`, reserved names, `allow_url_fopen`. It never boots `blastit`, so it cannot substitute for the containment legs |
| `docs/frameworks/php.md` → `docs_fences.rs` | 1 fence | docs | **useful** | the documented manifest parses and validates; the page's served shape is the protected fixture, minus `[route.ingress]` (G4) |

**0 fluff. 0 broken-oracle. 3 misplaced assertions** (one §3/`R7` pin, one CVE
seam, one routing rule whose protected twin is host-gated). Nothing here should be
deleted; the two `php_native.rs` harnesses are worth keeping exactly as they are —
host-gated is the honest tier for "loads the host's real `libphp`".

## Tier & teeth (anti-fluff, `testing` §3)

The column that matters is the middle one: **what still passes if the rule is
deleted or inverted**.

| Rule | Assertion that survives a permissive/inverted implementation | What gives it teeth |
|---|---|---|
| Stage 0 decides from `.dynsym`, never `dlopen` (`R6`/`R11`) | the non-library leg (a text file has no constructor, so it proves nothing about loading) — and the suite says so | the **ZTS** leg: the file *is* loadable, carries an `__attribute__((constructor))` canary, and stage 0 must still refuse it with an empty canary file |
| The load happens only below the fork (`ADR-0010` §1.1) | `no_libphp_is_mapped_above_the_view_worker` alone: a worker that never loads PHP would also leave init's maps clean | the shim-minor leg's canary pair `(pid, ppid)`: both stubs were mapped by a process whose parent is the boot, and not by the boot. Plus the served leg's body, which only the real `libphp` can produce |
| No listener before the gate (epic bullet 2, `R6`) | **`assert_no_serving_surface`'s `TcpStream::connect` check (`critical_php_embed.rs:1092`) is vacuous** — the child has already exited, so the port is closed either way. `critical_php_backend` has no port check at all (text: banner + `view` line) | the `ready` event: `stage1.rs` emits it in `serve`/`serve_without_interpreter`, i.e. after `bind_tcp` (`:399`, `:617`, `:690`), so "no `ready`" is the real witness for "never bound". Correct today, but the vacuous check should not be mistaken for a second witness |
| Boot order gate → bindings → probe → listener (`ADR-0010` §3) | the same (vacuous-or-absent) port check | `stage1.rs:359/370/399` is the order, and each refusal leg asserts its own sentence plus no `ready`/`spawned`/`request`: a gate moved after `bind_tcp` would emit `ready` before refusing |
| `(st_dev, st_ino)` pin for `libphp` (`R7`) | — | the swap leg: 4 bounded attempts, a served attempt is only a *lost race*, and 4 served attempts panic with "a removed inode pin produces the same four served attempts". Red if the pin is deleted |
| `(st_dev, st_ino)` pin for the **shim** (`R7`) | **nothing protected** | only `php_view::tests::a_replaced_libphp_or_shim_is_refused_by_name` (G1) |
| `exit 1` / empty stdout (§1.1, `R6`) | — | stage-0 legs assert `stdout == ""` exactly; stage-1 legs (probe, inode) assert "no `ready` and nothing listening" instead, and the module doc says why (stage 1 has already printed its boot document). `check`'s leg asserts exit `2` + empty stdout |
| The refusal names the package and the override, never a fallback (`ADR-0010` §1, DX) | a sentence naming a fallback would still contain the package | `assert_no_fallback_recipe` bans `php-fpm`/`fastcgi`/`route.fastcgi`/`wsgi`/`meanwhile`, applied to every refusal — including the ones that only mention `libphp` legitimately |
| `cpu_ns` is exact process CPU (`ADR-0010` §1.1, `R19`) | **`cpu_ns <= wall_ns` + `cpu_ns > 0` pass for `cpu_ns = duration`**; every fixture is a busy loop, so nothing separates CPU from wall (G3) | `RESP_END_COST`'s presence (a worker that died without reporting makes the event carry `cpu_ns: null` and the leg panics), and the scheduler clamp in `fairness.rs` |
| Crashed request charged ≥ its wall (epic bullet 3, `R19`) | **the current assertion `cost_ns > 0 && cost_ns >= cpu_ns` is the weaker rule** and passes at 3.2 ms against an 18.3 ms wall (G5) | nothing. `.38` tightens it to the epic's sentence |
| `exit()`/fatal kill neither worker nor slot (epic bullet 3) | a `5xx` could be a `view_died` `502` | `cpu_ns` must be present (so the shim completed and reported), `process.exited status 0`, the respawn's `PPid == init`, the crashed pid gone, exactly one live worker of the route |
| `Egress` reaches the filter (`ADR-0010` §3, epic's Egress doctrine) | **`egress = false` → `EPERM` passes for a hardcoded `false`**; the INI's `allow_url_fopen` is proved both ways but in a different crate (G2) | `probe_bindings`'s `egress.get(slot).copied().unwrap_or(false)` is exactly the silent fallback the twin would catch |
| `check` reaches the same gate (`R6`) | a `check` that refused for another reason would still exit `2` | the sentence assertions + empty stdout + "no pytest ran" (the fixture has no suite, so the gate must come first) |

**Fixture hygiene.** `stubs()` is a `Mutex<Option<Stubs>>` that rebuilds when the
cached root is gone (`.13.4` attempt 3's MAJOR: `OnceLock` + `--test-threads=1`
was deterministically red); canaries are `-D`-baked absolute paths because
`blastit-env` strips every `BLASTIT_*` across the stage-1 `execve`; each case owns
a `blastit-php-embed-<pid>-<name>` scratch tree and the last guard removes the stub
root. Both were verified by reading, not by re-running the suite.

## Requirement → protected-suite map

| Requirement | Protected oracle | State |
|---|---|---|
| `ADR-0010` §1.1 — one backend, `200` through `view.chan`, `cpu_ns` on the completion path | `critical_php_backend::a_php_route_is_served_or_the_boot_refuses_naming_the_package` (served branch, host-gated) | **proved, CI-unwitnessed** (G7) |
| §1.2 — `[route.php]` serves with no `BLASTIT_APP`; `[route.static]` + php page forwards misses | same leg (asset / front controller / `.php` 404) + `route.rs`'s unit test | **proved on a PHP host only** (G7) |
| §1.1 / `R11` — `libphp` never in `init`/`http`/`scheduler`/`dbproxy` | `no_libphp_is_mapped_above_the_view_worker` (init content, every above-view child content or `EACCES`) + the shim-minor canary pair | proved; **unprivileged half is host-only** (G7) |
| §1.3 / `R6` — discovery, NTS statically, ZTS refusal, no `dlopen` before the `execve` | `a_non_library…`, `a_zts_libphp_refuses_before_anything_loads`, `stage0::php_boot_gate` (13) | **proved** |
| §1.3 / `R7` — `(st_dev, st_ino)` per object, stage 1 re-opens `O_NOFOLLOW` | `a_libphp_replaced_between_the_stages_refuses_the_boot` (**library only**) | **half proved** (G1) |
| §1.3 — probe child before `bind_tcp`, own pgid, route uid, `EmbeddedView`, `PDEATHSIG`, timeout kill + reap | `a_shim_for_another_minor…`, `a_probe_that_hangs_is_killed_and_refuses_the_boot` (shape file: `pid`/`ppid`/`pgid`/`uid`/`euid`/`inet`, `/proc` gone) | **proved** (no-PHP host, deterministic) |
| §3 — the filter takes the route's `Egress`; INI and filter cannot disagree | probe child's `inet == EPERM` only (`egress = false`) | **half proved** (G2) |
| §4.1 — the two refusal sentences (missing / ZTS-or-mismatch), naming route, `entry`, tuple, shim, package, `BLASTIT_LIBPHP` | every refusal leg (`assert_package_sentence`) + `.13.2`'s unit tests for the tuple shape | **proved** |
| §2 / `R12` — `$_SERVER` populated in C, `Proxy:` never `HTTP_PROXY`, no `PHP_VALUE` route, INI `ZEND_INI_SYSTEM` set, `allow_url_fopen` by egress | `critical_shim_contract` (4 of its 15 tests) + the `php.rs` seam test (unprotected) | **proved at the primitive**; the `view.chan` → `$_SERVER` seam has no protected oracle (no separate task — one composition line, and the primitive is protected) |
| Epic bullet 3 — `exit()`/fatal contained, slot respawned, next request served | `an_exit_…`, `a_fatal_…` | **proved, CI-unwitnessed** (G7) |
| Epic bullet 3 — "charged at least its wall time" | `cost_ns > 0 && cost_ns >= cpu_ns` | **NOT met** (G5) |
| Epic bullet 4 / `R11` — no `libphp` symbol above the worker | `no_libphp_is_mapped_above_the_view_worker` | **proved** |
| `R7`/`B2-R5` — `plain_is_protected` + `!has_file_caps` + zero writable `DT_RPATH`/`DT_RUNPATH` + non-writable `manifest_dir`/`entry`/`static.root` | **none** — the code does not do it (`host::plain_is_protected` is wired for the plain executable only, `host.rs:273`) | **unimplemented and unprotected** (G6) |
| `R12`/`R6` — a request body reaches PHP; the worker's `max_body_bytes` is the cap | `php.rs` unit tests only | **unprotected** (G4) |
| `R6` — `check` exits `2`, empty stdout, same sentence | `check_reaches_the_same_discovery_gate` | **proved** (the `--json` variant is inherited from `critical_static_cli`'s generic leg) |
| `R18` — `RESP_END_COST` 9/17 bytes, once per request, ignored `wall_ns` | `php.rs` unit tests + scheduler's own suites | out of this epic's protected scope; unchanged |

## GAPS

Severity is "what a regression costs", not "how hard the fix is".

### G1 — the shim's `(st_dev, st_ino)` pin has no protected witness — **MAJOR** → `MB.3.37`

`php_view::open_pinned` compares **both** re-opened objects against stage 0's
record, and `.13.3`'s "What the first run found" is the bug this exists for: the
record held the library's pair and the shim was compared against it, so every boot
refused. The protected suite swaps only `libphp`
(`critical_php_embed.rs:1493`, `Swap { from: spare, to: libphp }`), while the shim
half is asserted only by
`crates/blastit/src/php_view.rs:1034` — a `#[cfg(test)]` module in a file the
implementer owns. Delete the shim comparison (or compare it against
`config.php.dev/ino`) and `make check` stays green: the swap leg still refuses on
the library, and the unit test can be deleted in the same commit with no
`critical_guard` finding.

### G2 — `egress = true` has no protected oracle — **MAJOR (DX; fails closed)** → `MB.3.37`

`probe_bindings` and the view fork both read the route's `egress` from a
per-slot vector with `unwrap_or(false)` (`php_view.rs:314`). The one protected
witness is the hang stub's `inet == 1` (EPERM), which a hardcoded `false`
satisfies. Meanwhile `critical_shim_contract::allow_url_fopen_follows_the_declared_egress_both_ways`
proves the INI renders `1` for an egress route — so a regression here makes the
INI and the filter *disagree*, which is the exact invariant the epic's Egress
doctrine names, and silently breaks every PHP route that legitimately needs the
network.

### G3 — `cpu_ns` and wall time are indistinguishable — **MAJOR** → `MB.3.37`

Every fixture is a busy loop (`INDEX_PHP`, `EXIT_PHP`, `FATAL_PHP`), so
`cpu_ns ≈ wall_ns` by construction and all three assertions
(`cpu_ns <= wall_ns`, `cpu_ns > 0 || wall < 1 ms`, `cost_ns >= cpu_ns`) hold for an
implementation that reports the request's duration as `cpu_ns`. The epic's claim is
"exact and unforgeable by PHP userland": a block (`usleep`) is what separates the
clocks, and nothing blocks today.

### G4 — no protected test that a request body reaches PHP — **MINOR/MAJOR** → `MB.3.37`

The documented manifest carries `[route.ingress] body = "form"`; `php2shell` is a
request-data class; and the transport's whole-body buffer, `max_body_bytes`, and
`RequestV1`'s body pointer exist precisely for it. Protected coverage: zero POSTs.
The native harness (`--ignored`) proves `body_bytes` once, on a host, never in CI.
A body-less `RequestV1` would serve every `POST` as an empty `$_POST` and every
test in scope would stay green.

### G5 — the crashed request is charged **less** than its wall time — **MAJOR** → `MB.3.38`

Epic bullet 3: "the crashed request is charged at least its wall time". `R19`:
"crashes/`504` charge `ceiling.max(kernel_floor_ns)`". `.13.4`'s own transcript:

```console
exit(): status 500  cpu_ns=1543666  wall_ns=18336040  cost_ns=3221782  retired pid 2853758  respawned pid 2853759
fatal:  status 500  cpu_ns=777317   wall_ns=19330351  cost_ns=3996608  retired pid 2853693  respawned pid 2853697
```

3.2 ms charged for an 18.3 ms request. Cause (recorded by `.13.4`'s follow-up 3,
not fixed): a contained bailout reports `RESP_END_COST`, so the scheduler charges
`min(report, ceiling).max(floor)` like any completed request instead of the crash
rule. The suite asserts `cost_ns > 0 && cost_ns >= cpu_ns`, i.e. it encodes the
weaker rule, so the deviation is invisible to `make check`. Either the charge
changes or the epic's sentence does — silently keeping the weaker assertion is the
one option that is not allowed.

### G6 — the production provenance half of `R7` is neither implemented nor tested — **MAJOR** → `MB.3.40`

`ADR-0010` §3 and design `R7`/`B2-R5` require `plain_is_protected(path, init_uid)` +
`!has_file_caps(path)` for `libphp`, the shim and the pre-scanned `.ini` targets,
zero writable `DT_RPATH`/`DT_RUNPATH`, and `manifest_dir`/`entry`/`static.root` not
writable by a route UID/GID — and design line 318 makes them **critical-tier**
requirements of the `R6`/`R7`/`R8` row (`/tmp/evil.so`, writable `extension_dir`,
`opcache.preload`). The primitives exist (`host.rs:431`, `:315`) and are wired for
the plain executable only (`host.rs:273`); stage 0's PHP gate checks the ELF symbols
and the tuple and nothing else. `MB.3.13.3`'s follow-up 4 says so; no plan node owns
it; no test can fail. A route UID that can write the shim's directory owns every
`[route.php]` worker on the host.

### G7 — the real-pair legs never run in CI — **MAJOR (process)** → `MB.3.39`

`make check` is PHP-free by design, and `php-sapi.yml`'s only job with
`php8.4-dev` + `libphp8.4-embed` runs `make php-sapi-e2e` (Docker, shim-level
probe) — never `critical_php_embed` or `critical_php_backend`. So on every CI run:
the served page, `cpu_ns` on the event, both containment legs + respawn, the maps
leg's unprivileged half, and the symlink-served branch are **unverified**. The
suites' refusal branches are the only thing CI has ever seen. `e2e/php_sapi_probe.py`
cannot cover it: it loads the shim in a probe process and never boots Blastit.

## Mutation (not run this session — and why)

`cargo-mutants 27.1.0` is installed here, and `make mutants-rust PKG=<crate>` is the
gate tool. I did **not** run it, for the reason the dispatch allows: the epic's
mutable surface is ~3 000 lines of `unsafe`/FFI-adjacent loader, a forking
supervisor glue module, and ELF parsing, and a campaign on `blastit` (the crate that
owns both suites) compiles the whole workspace per mutant — well past this audit's
budget, with no equivalent to the fast `blastit-sqlpolicy` campaigns.

**The crate I judge essential for the milestone gate: `blastit-php-sapi`** (with
`blastit-view`'s `php` feature as the second). Reason: `src/ini.rs` renders the
`ZEND_INI_SYSTEM` lockdown and `src/request.rs` maps CGI names (httpoxy, reserved
`$_SERVER` keys) — policy/validation modules by the `testing` §4 definition — and
their only oracle is one protected file whose assertions are mostly
`assert_eq!(render(...), expected)` on a table. Mutating a single directive to
`""` or flipping an `allow_url_fopen` arm is exactly the class a table test can
miss. Suggested, scoped, at the milestone gate:

```console
$ make mutants-rust PKG=blastit-php-sapi
$ # file-scoped if the full crate is too slow:
$ cargo mutants --file src/ini.rs --file src/request.rs --test critical_shim_contract
```

Manual branch inspection done instead (the same places a campaign would start):

| Branch | Would a test fail? |
|---|---|
| `open_pinned`'s shim comparison | **no** (G1) — the unit test only |
| `probe_bindings`'s `unwrap_or(false)` | **no** (G2) |
| `Loaded::load`'s `startup_attempted` leak-on-failure | **no** — only `.13.1`'s native harness (host-gated) |
| `boot_bindings`'s `entry.strip_prefix(document_root)` refusal | **no** — no test for an `entry` outside the root at boot (the manifest grammar refuses `..`, but a symlinked `document_root` is the interesting case); noted, not filed (grammar-level twin exists in `critical_php_manifest`) |
| `stage1.rs`'s `has_app \|\| route.php().is_some()` | **no** in a protected file on a PHP-free host (G7); the `route.rs` unit test covers the rule |
| `probe_bindings`'s slot/route length mismatch refusal | **no** — unreachable by construction today |

## Tasks filed

| Node | Title | Tier of the gap |
|---|---|---|
| `MB.3.37` | Protected oracle gaps in `critical_php_embed`: shim inode pin, egress twin, cpu-vs-wall, POST body | critical |
| `MB.3.38` | Charge a contained PHP bailout at least its wall time (epic acceptance) and assert it | critical + code |
| `MB.3.39` | CI witness for the real-pair PHP legs (`php-sapi.yml`) | critical process/e2e |
| `MB.3.40` | Production provenance gate for `libphp`/shim/`manifest_dir` (`R7` + `B2-R5`) with protected legs | critical + code |

**Serialization:** `.37`, `.38` and `.40` all add or change assertions in
`crates/blastit/tests/critical_php_embed.rs`; they must not run in parallel (their
`paths` overlap on that file). `.39` is independent and should land early — it is
what makes the other three observable in CI.

## Files

- `docs/testing/AUDIT-MB.3.13.md` (this report).
- Plan nodes created by this audit: `plan/nodes/MB/MB.3/MB.3.37/`,
  `…/MB.3.38/`, `…/MB.3.39/`, `…/MB.3.40/` (each `NODE.md` + `LOG.md`, as `plan.py new` writes them)
  for a new node; `RESULT.md` is the owner's to write).
- Nothing under `crates/` or `docs/` outside this report was touched.
