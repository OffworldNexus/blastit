# Result MB (`v0.2.0` Sub-Release Gate) — ACCEPTED

## Summary

The **`v0.2.0` sub-release gate** of Milestone `MB` is **ACCEPTED** by `strategist` on branch `release/0.2.0` (base feature tip `9784965`, plus this gate's plan triage and release-material sync).

`v0.2.0` ships the combined static/SPA + embedded-PHP sub-release (`0.2.1` folded into `0.2.0`) plus the owner-mandated `0.2.0` platform & DX upgrades:
- **`Blastfile` DSL & `blastit fmt` (`MB.21`, `ADR-0012`):** replaces `blastit.toml` permanently across every CLI entry point, Python bridge, doc fence, and `e2e/` fixture, with lossless comment-preserving formatting (`blastit fmt [--check]`), surgical `--learn` edits, and permanent purity/retired-vocabulary guards.
- **Static mounts & SPA/front-controller routing (`MB.2`, `MB.20`, `MB.31`, `ADR-0007`):** `openat2`-confined static serving via `blastit-view-static`, unified `immutable` pattern grammar (bare / `"/assets/*"` / `regex '…'`, `ImmutableGlob` deleted), `.php`/`.phar`/`.phtml` source denial (`404`), and Python-free all-static `blastit dev` + `blastit check [--json]`.
- **Embedded PHP 8.3, 8.4 & 8.5 (`MB.3`, `MB.24`, `MB.26.8`, `ADR-0010`, `ADR-0015`):** host NTS `libphp` (`8.2` dropped and refused at boot) inside unprivileged per-route `blastit-view-php` workers over `view.chan` via per-minor Apache-2.0 C SAPI shims (`crates/blastit-php-sapi`), zero external `php-fpm`/FastCGI daemon, zero CGI parameter stream (`php2shell` surface absent by construction), boot-time `(st_dev, st_ino, st_size, st_ctime)` pinning, production provenance gates, on-demand multi-process per-route concurrency up to `BLASTIT_MAX_WORKERS`, and `outcome: "bailed"` wall-time accounting.
- **Live reload & runtime diagnostics (`MB.23`, `MB.25`, `ADR-0014`):** unprivileged `fanotify` live reload (`--no-reload` opt-out, zero `inotify` symbols, `:8080` held open across view/preload/`Blastfile` reloads) and structured `INFO runtime` boot/reload events retiring `WARN manifest.warning` abuse.
- **SQL template validation, `blastit-lsp` & Zed extension (`MB.22`, `MB.32`–`MB.35`, `ADR-0013`):** PostgreSQL 18 template validation (`$?`, `IN ($?, ...)`, `VALUES (…), ...`) with exact inner `line:col` spans across `blastit-sqlpolicy`, `blastit-manifest`, `check`, and LSP; `$?`/`...` hover; `"Parameterize SQL for Blastfile ($?)"` Quick Fix; Tree-sitter + Zensical WCAG-AA syntax highlighting; and standalone `blastit-lsp` + `uv`-free Zed resolution (`LspSettings` -> `PATH` -> GitHub Release).
- **Four-crate view architecture (`MB.36`, `ADR-0016`):** `blastit-view` (pure-`std` `view.chan` transport, zero `pyo3`/SAPI deps) + `blastit-view-{static,python,php}` composed by Cargo features (`WorkerRuntime`/`Upstream` deleted and guarded by `critical_artifacts`).

---

## 1. Release Scope Statement & Plan Status Note

Milestone `MB` spans three sequential `0.2.x` releases (`plan/ROADMAP.md`):
1. **`v0.2.0` (this release — COMPLETE):** Static mounts & URL rewriting (`MB.2`), Embedded PHP 8.3–8.5 & `php2shell` defense (`MB.3`), `Blastfile` (`MB.21`), `/deploy` & `/frameworks` restructures (`MB.11`–`MB.13`, `MB.18`–`MB.20`), `blastit lsp` + portable `blastit-lsp` + Zed (`MB.22`, `MB.33`, `MB.35`), `fanotify` live reload (`MB.23`), parallel PHP workers (`MB.24`, `MB.26.8`), `INFO runtime` (`MB.25`), unified `immutable` grammar (`MB.31`), Zensical highlighting (`MB.32`), SQL template validation (`MB.34`), and four view crates (`MB.36`). Every `0.2.0` epic (`MB.1`, `MB.2`, `MB.3`, `MB.12`, `MB.21`–`MB.25`, `MB.31`, `MB.32`, `MB.34`–`MB.36`) is **`done`**.
2. **`v0.2.2` (`MB.4` — LIVE, NOT `0.2.0` scope):** Vite & Node HTTP upstream & `node2shell` defense (`MB.4`, `MB.4.1` remain `ready`).
3. **`v0.2.3` (`MB.5` — LIVE, NOT `0.2.0` scope):** Tomcat / JVM 12-factor backend & unified cross-backend `blastit check -- <suite>` gate (`MB.5` remains `ready`).
4. **Post-`0.2.0` hardening backlog (`MB.26` — LIVE, `proposed` leaves `MB.26.1`–`MB.26.56`, with `MB.26.8` `done`):** non-blocking mutation survivors, tier promotions, and CI/e2e witnesses.

**Mechanical `tools/plan.py` note:** `tools/plan.py` (`set_status:487`, `validate:445`) enforces `any(c.status not in ("done", "superseded") for c in node.children)` as a hard error whenever a container is marked `done`, and `ancestors_open:348` suppresses all children under a `done` milestone while unlocking `MC` (`depends_on: [MB]`). Keeping `MB` at `status: ready` while closing every `0.2.0` epic (`MB.3`, `MB.12`, `MB.34`, etc.) to `done` keeps `plan.py validate` and `make check` green, preserves `MB.4` (`0.2.2`), `MB.5` (`0.2.3`), and `MB.26` as live work after `v0.2.0` merges, and triggers `conductor`'s Release PR procedure via `RELEASE_READY: 0.2.0` (`conductor.md:125`).

---

## 2. Two Mandatory Unproven / CI-Only Facts (Stated Honestly, Not Claimed Locally)

1. **Cross-platform `blastit-lsp` legs (`macOS` & `Windows`, plus `aarch64` Linux) have NOT run:**
   - `scripts/platform_matrix.py` and `.github/workflows/{ci,publish-pypi}.yml` declare five native `blastit-lsp` targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, and `x86_64-pc-windows-msvc`.
   - **Locally proved (`1/5`):** `x86_64-unknown-linux-gnu` (`manylinux_2_28` build + `scripts/lsp_smoke.py` + `scripts/check_lsp_portable.py` token-stream portability guard across 59 fixtures, `docs/testing/E2E-MB.35.md`).
   - **Declared & workflow-checked, NOT yet executed (`4/5`):** `aarch64-unknown-linux-gnu` (`ubuntu-24.04-arm`), `aarch64-apple-darwin` (`macos-latest`), `x86_64-apple-darwin` (`macos-15-intel`), and `x86_64-pc-windows-msvc` (`windows-latest`) run on GitHub Actions when `conductor` opens the `release/0.2.0 -> develop` PR. Likewise, interactive Zed GUI clicks/screenshots are `NOT RUN` locally (no Zed binary on this host), with wire-level JSON-RPC transcripts and `tree-sitter` parse/highlight proofs recorded in `docs/testing/E2E-MB.34.md`.
2. **`aarch64` (`gnu` + `musl`) Linux serving legs are CI-only:**
   - **Locally proved on `x86_64`:** `4/8` PyPI wheels (`x86_64` `manylinux_2_28` & `musllinux_1_2` × CPython `3.13`/`3.14`), `2/4` standalone `blastit` archives (`x86_64-unknown-linux-{gnu,musl}`, smoked with `--require-no-python` in Debian bookworm/trixie and Alpine 3.21), and `6/12` PHP SAPI shims (`8.3`, `8.4`, `8.5` × `x86_64-{gnu,musl}`, all 6 reaching `served`).
   - **CI-only (`ubuntu-24.04-arm`, NOT executed locally):** `4/8` `aarch64` wheels, `2/4` `aarch64` standalone archives, and `6/12` `aarch64` PHP 8.3/8.4/8.5 SAPI shims (`php-sapi.yml` and `publish-pypi.yml`). Additionally, DigitalOcean App Platform gVisor cloud deployment (`live.yml`) runs on `develop` merge before `publish-pypi.yml`'s `validate` gate releases to PyPI.

---

## 3. `ROADMAP.md` `MB` Exit Criteria Verification Matrix

| # | `ROADMAP.md` `MB` Exit Criterion | `v0.2.0` Verdict | Evidence & Sub-Release Scope |
|---|---|---|---|
| **1** | **Docs-First per sub-release:** lean Local / CI-CD / Deployment guides written & DX-critiqued before code for each backend | **PASS (`0.2.0`)** | Static (`MB.2.1` + `VISUAL-MB.2.md`), PHP (`MB.3.2` -> `MB.3.9.1` -> `MB.3.10` -> `MB.20`), `/deploy` (`MB.11`, `MB.13`, `MB.18`), `/frameworks` (`MB.19`, `MB.20`), `Blastfile` (`MB.21.1` + `VISUAL-MB.21.md`), LSP/Zed (`MB.22.1`, `MB.35.2`), live reload (`MB.23.1`), concurrency (`MB.24.1`), `INFO runtime` (`MB.25.1`), `immutable` (`MB.31.1`), highlighting (`MB.32.2` `VISUAL-MB.32.md`), SQL templates (`MB.34.1`, `MB.34.8`). All 30 user pages pass `make doc-tone`, `make docs-check`, `make docs-unit`, and `make docs-test` (`48` fences, `0` failed). |
| **2** | **Static assets & URL rewriting:** built-in safe static asset serving and clean front-controller / SPA URL rewriting without external nginx boilerplate | **PASS** | `crates/blastit-view-static` (`openat2` confinement, MIME/`ETag`/`304`, bare/pattern/`regex` `immutable`, `fallback` SPA answer, `.php`/`.phar`/`.phtml` `404`), protected `critical_static_{resolve,assets,entry}.rs` + `critical_rewrite.rs` + `critical_static_cli.rs`, and Docker `e2e/static_assets.py` (SPA, non-SPA, caching, 9 traversal/dotfile/symlink refusals, all-static `check` + `--json`). |
| **3** | **Embedded PHP (`libphp` over `view.chan`, no `php-fpm`/FastCGI):** first-class PSR-7/15, Symfony & Laravel support in unprivileged per-route view workers, `php2shell` class structurally absent | **PASS** | `entrypoint php "public/index.php"` (`crates/blastit-view-php` + `crates/blastit-php-sapi` shims for PHP 8.3, 8.4, 8.5; `8.2` refused; `blastit-fastcgi` deleted in `MB.3.17` and guarded by `MB.3.30`); parallel per-route view pools (`MB.24` + `MB.26.8`); boot-time inode/size/ctime pinning (`MB.3.56`) + provenance (`MB.3.40`); `docs/frameworks/php/{index,symfony,laravel}.md`. |
| **4** | **Vite & Node (`0.2.2`):** HTTP upstream per route/worker, seamless static + SSR split | **DEFERRED TO `0.2.2` (`MB.4`)** | Explicitly scoped to next sub-release `0.2.2`; `MB.4` and `MB.4.1` remain live (`ready`) under `MB`. |
| **5** | **Tomcat / JVM (`0.2.3`):** clean 12-factor Servlet / Spring Boot integration | **DEFERRED TO `0.2.3` (`MB.5`)** | Explicitly scoped to sub-release `0.2.3`; `MB.5` remains live (`ready`) under `MB`. |
| **6** | **Unified manifest tooling & baseline CVE bar:** `dev --learn` and `check` across backends; `php2shell` and `node2shell` suites proven blocked in `e2e/` | **PASS for `0.2.0` (Python + Static + PHP); Node/JVM & non-Python `check -- <suite>` in `0.2.2`/`0.2.3`** | `blastit dev --learn` works across Python and PHP (`critical_learn_executor_blocks.rs`, `e2e/php2shell.py`); `blastit check [--json]` audits Python WSGI suites and all-static `Blastfile`s, while PHP CI uses `blastit fmt --check` + `check` boot/root/libphp gates in `0.2.0` (stated explicitly in `docs/frameworks/php/index.md:90-99` and `CHANGELOG.md`; cross-language `check -- <suite>` carried into `MB.5`). `e2e/php2shell.py` (24 client-reachable rows covering attacks `#1`–`#20`: CVE-2012-1823, CVE-2024-4577, CVE-2024-8926, CVE-2024-8927, `PHP_VALUE`/`PHP_ADMIN_VALUE`, `httpoxy`, `.user.ini`, polyglot/`.php` static `404`, smuggling) + `e2e/rootless/php_entry_swap.py` pass (`411` `e2e` checks ok, `0` fail). `node2shell` lives in `MB.4` (`0.2.2`). |

---

## 4. Complete Triage Table of Remaining Non-Closed `MB` Nodes

Every node under `MB` that was non-closed entering the gate is accounted for below (zero nodes deleted; every `superseded` node carries either its `MB.26.<N>` successor log line or its architectural supersession reason):

| Node ID | Previous Status | Gate Decision | Successor / Reason |
|---|---|---|---|
| `MB.3` | `epic/ready` | **`done`** | `0.2.0` embedded PHP 8.3–8.5 + `php2shell` defense delivered (`plan/nodes/MB/MB.3/RESULT.md`). |
| `MB.3.5` | `epic/ready` | **`superseded`** | Obsolete TOML `[route.php]` (`entry` + `workers`) epic; superseded by `MB.21` (`Blastfile` `entrypoint php` + `critical_php_manifest.rs` / `critical_blastfile_entrypoint.rs`). |
| `MB.3.5.2` | `task/ready` | **`superseded`** | Obsolete TOML `EntryProbe` + retired `docs/reference/manifest.md` brief; superseded by `MB.21`, `MB.3.36`/`MB.3.40`/`MB.3.56`, and `MB.36.5` (`blastit-view-php`). |
| `MB.3.5.3` | `task/ready` | **`superseded`** | Obsolete TOML `parse.rs` `ROUTE_KEY_EXPECTED` task (`parse.rs` and `tests/parse.rs` deleted in `MB.21`). |
| `MB.3.6` | `epic/ready` | **`superseded`** | Obsolete `MB.9` `Upstream`-trait + `[route.php]` dispatch epic; superseded by `MB.3.13` + `MB.21` + `MB.24` + `MB.36.5` (`ADR-0016`), with non-Python `check -- <suite>` carried into `MB.5` (`0.2.3`). |
| `MB.3.6.4` | `epic/ready` | **`superseded`** | Obsolete `MB.9`-coupled PHP `check -- <suite>` sub-epic; `0.2.0` ships `dev --learn` + `fmt --check` (`docs/frameworks/php/index.md:90-99`), unified `check -- <suite>` carried into `MB.5` (`0.2.3`). |
| `MB.3.6.4.1` | `task/ready` | **`superseded`** | Written against deleted `MB.9.1.1` `Observation` types and `[route.static]`; carried into `MB.5` (`0.2.3`). |
| `MB.3.6.4.2` | `task/ready` | **`superseded`** | Written against `MB.3.6.4.1` and `[route.php]`; carried into `MB.5` (`0.2.3`). |
| `MB.3.9` | `epic/ready` | **`done`** | Embedded PHP `blastit dev` supervision & cleanup delivered over `view.chan` (`plan/nodes/MB/MB.3/MB.3.9/RESULT.md`). |
| `MB.3.15` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.30`** (`proposed`): promote R12 cross-build capability refusal matrix to protected `critical_backend_capability.rs`. |
| `MB.3.16` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.31`** (`proposed`): pin Python-less standalone all-static `blastit check` + `--json` in `no_python_build.rs`. |
| `MB.3.20` | `task/proposed` | **`superseded`** | Obsolete held docs pass for `MB.3.17`; completed by `MB.3.10`, `MB.3.30`, `MB.3.32`, `MB.20`, and `MB.21.5.1`. |
| `MB.3.22` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.32`** (`proposed`): `BLASTIT_APP` env refusal on `entrypoint php` files + `PhpDeadDeclaration::key` cleanup. |
| `MB.3.26` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.33`** (`proposed`): post-`0.2.0` `PR_SET_CHILD_SUBREAPER` + `WNOWAIT`, `BLASTIT_ALLOW_EGRESS=1` switch, and 4×3 residue matrix. |
| `MB.3.34` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.34`** (`proposed`): `blastit-php-sapi/build.rs` transient `EAGAIN`/`ETXTBSY` retry. |
| `MB.3.55` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.35`** (`proposed`): `MB.3.54` reviewer MINOR precision follow-ups on `critical_raw_ingress.rs` / `ServeGates`. |
| `MB.3.68` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.36`** (`proposed`, paths updated to `crates/blastit-view-php/src/discovery.rs`): diagnostic edge case when stray 8.2 coexists with unprobed supported spelling. |
| `MB.3.69` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.37`** (`proposed`): manual `Dockerfile.two-package` container witness run + `MB.3.65/RESULT.md` note. |
| `MB.4`, `MB.4.1` | `epic/ready`, `task/ready` | **Stay `ready` (`0.2.2`)** | Next sub-release (`0.2.2` Vite & Node HTTP upstream + `node2shell`), unlocked now that `MB.3` is `done`. |
| `MB.5` | `epic/ready` | **Stay `ready` (`0.2.3`)** | Sub-release `0.2.3` (Tomcat / JVM + unified cross-backend `blastit check`). |
| `MB.6` | `task/ready` | **`superseded`** | Pre-`Blastfile` static framing task; delivered by `MB.20` (`django.md` §Static files, `php/index.md` §public/) and `MB.31` (`immutable` + hashed-asset regex). |
| `MB.8` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.38`** (`proposed`): combined Django `collectstatic` + WSGI container leg in `e2e/`. |
| `MB.10` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.39`** (`proposed`): disk-backed agent scratch helper (`MUTANTS_TMP` and `.worktrees/` already fixed in `MB.1.11`). |
| `MB.11` | `task/ready` | **`done`** | Landed in `f430031` and carried into `Blastfile` `/deploy` by `MB.18` + `MB.21.6` (`plan/nodes/MB/MB.11/RESULT.md`). |
| `MB.12` | `epic/ready` | **`done`** | Landed in `5fab787` (`ADR-0009`, `ADR-0010`, `backend-runtimes.md` + 4 lenses; trait half superseded by `ADR-0016`, `plan/nodes/MB/MB.12/RESULT.md`). |
| `MB.13` | `task/ready` | **`done`** | Landed in `aa94c58` and distilled in `MB.18` (`6ba10d3`, `plan/nodes/MB/MB.13/RESULT.md`). |
| `MB.16` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.40`** (`proposed`): deterministic retry on `supervise.rs:1045` unit test. |
| `MB.26` (`MB.26.1`–`.29`) | `epic/ready` (`proposed` + `.8` `done`) | **Stay live (`MB.26`)** | Post-`0.2.0` hardening & coverage backlog (grown to `MB.26.1`–`MB.26.56`; `MB.26.8` PHP retire race already `done`). |
| `MB.27` | `task/proposed` | **`superseded` (re-filed)** | Re-filed to **`MB.26.41`** (`proposed`): compile & run `critical_parallel_views` inside `e2e/php-sapi/Dockerfile`. |
| `MB.28` | `task/proposed` | **`superseded` (re-filed)** | `ARCHITECTURE.md` and `docs/reference/settings.md` parallel-views deltas landed in this gate commit; remaining `crates/blastit-view-php/src/worker.rs` doc comment re-filed as **`MB.26.42`** (`proposed`). |
| `MB.29` | `task/proposed` | **`superseded` (re-filed)** | Re-filed to **`MB.26.43`** (`proposed`): real-`ulimit -u` container e2e for graceful `RLIMIT_NPROC` fork backoff. |
| `MB.30` | `task/proposed` | **`superseded` (re-filed)** | Re-filed to **`MB.26.44`** (`proposed`): deduplicate `VIEW_PROCESS_MARGIN` and sync `parallel-views.md` §4.6/§4.7. |
| `MB.34` | `epic/ready` | **`done`** | SQL `$?`/`...` template validation, inner spans, Quick Fix, hover & highlighting delivered (`plan/nodes/MB/MB.34/RESULT.md`). |
| `MB.34.7` | `epic/review` | **`done`** | Outside-in `docs/testing/E2E-MB.34.md` proof (`reviewer: VERDICT PASS`); 8 audit-coverage children re-filed to `MB.26.48`–`MB.26.55`. |
| `MB.34.7.2`–`MB.34.7.9` | `task/ready` (×8) | **`superseded` (re-filed)** | Re-filed to **`MB.26.48`–`MB.26.55`** (`proposed`): `AUDIT-MB.34` `G1`–`G8` test-tier promotions & `make zed-check` corpus runner. |
| `MB.37` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.45`** (`proposed`): rare parallel-run `BrokenPipe` race in `critical_learn_and_redaction.rs:518`. |
| `MB.38` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.46`** (`proposed`): custom `Debug` redaction on `PgGrant::queries`. |
| `MB.39` | `task/ready` | **`superseded` (re-filed)** | Re-filed to **`MB.26.47`** (`proposed`): runnable-docs `SIGINT` timing determinism on `docs/local.md` `dev-learn` fence. |
| *(New)* `MB.26.56` | `task/proposed` | **Created (`proposed`)** | Run `make mutants-rust PKG=blastit-php-sapi` (`AUDIT-MB.3.13-CLOSURE.md` milestone recommendation). |

---

## 5. `test-curator` & Mutation Audit Synthesis + Escalation Review

- **Protected suite inventory:** **52** Rust `crates/*/tests/critical_*.rs` suites + **4** Python `python/tests/critical/` suites across **18** workspace crates; `ci.yml`'s branch-wide `tools/critical_guard.py --base origin/develop` passes (`all 50 protected change(s) are authorized`).
- **Audit ledger (`14` `AUDIT-MB*.md` + `2` `E2E-MB*.md` + `3` `VISUAL-MB*.md`):** **0 `BLOCKER` findings**. Every `MAJOR` product/security finding in `AUDIT-MB.1`, `AUDIT-MB.2.4`, `AUDIT-MB.3.13`, `AUDIT-MB.3.13-CLOSURE`, `AUDIT-MB.21` (`MB.21.5.5`, `.7`), and `AUDIT-MB.36` (`MB.36.11`) is **`done`**.
- **Measured `cargo-mutants` kill rates on critical policy, parser & view crates (all `>= 75 %` bar):**
  - `blastit-sqlpolicy`: **921/921** original mutants with **0 timeouts** (`literals` 100%, `statements` 100%, `tables` 100%, `lexer` 99.5%, `normalize` 99.0%, `learn` 98.9%, `analysis` 84.4%) + `template.rs` (`MB.34.3`): **`122/128` = `95.3 %`** (6 equivalent/unreachable, 0 timeouts).
  - `blastit-manifest`: Blastfile parser/CST/printer/editor (`AUDIT-MB.21.md`): **`688/748` = `92.0 %`** (`tokens` 100%, `parser` 98.1%, `pattern` 94.9%, `lower` 93.0%, `syntax` 92.5%, `write` 89.8%, `lexer` 88.6%, `print` 85.7%, `edit` 84.6%); `QuerySite` span helpers (`AUDIT-MB.34.md`): **`20/21` = `95.2 %`** (100% across standard + critical tiers).
  - `blastit-env`: **`144/153` = `94.1 %`** (0 timeouts).
  - `blastit/src/dev_reload.rs` (`AUDIT-MB.23.md`): **`123/145` = `84.8 %`** (0 timeouts).
  - `blastit-view` (`AUDIT-MB.36.md`): **`191/222` = `86.0 %`**.
  - `blastit-view-static` (`AUDIT-MB.36.md`): **`218/256` = `85.2 %`** (`static_response.rs` 95.6%, `static_asset.rs` 75.0%).
  - Remaining non-blocking mutation/tier follow-ups are explicitly tracked in `MB.26`: `MB.26.1`–`.7` (`Blastfile` editor/printer survivors), `MB.26.9`–`.14` (LSP/Zed negative guard twins), `MB.26.15`–`.23` (`dev_reload` reload-taint/reconcile/container witnesses), `MB.26.24` (`check_lsp_portable` review escapes), `MB.26.25`–`.29` (`blastit-view*` transport suite & `--test-package` consumer runner for `blastit-view-php`), `MB.26.30`–`.55` (triage above), and `MB.26.56` (`blastit-php-sapi` mutation run).
- **Escalation review (`plan/ESCALATIONS.md`):** `14` total escalations across `250` completed tasks (`7` in `MA`, `7` in `MB`: `2` `architect` designs, `1` `strategist` `0.2.0` gate, and `4` `expert` rescues across ~206 `MB` tasks = **`1` non-gate escalation per `~51` `MB` tasks**, well inside the `<= 1 per 15` target). Two systemic lessons were already codified in `LESSONS.md`: single-writer ownership on design docs (`MB.3.14`) and token-stream lexing (never line-regex scanning) for Rust source-portability guards (`MB.35.6`).

---

## 6. Release Invariants & Gate Evidence

```console
$ scripts/check-release-tag.sh v0.2.0
tag v0.2.0 names the workspace version 0.2.0          # EXIT=0

$ scripts/release-notes.sh --require-dated v0.2.0 > /dev/null
# EXIT=0 (CHANGELOG.md "## 0.2.0 (2026-11-18)" covers Blastfile, static, PHP 8.3-8.5, parallel workers, fanotify reload, INFO runtime, SQL templates, blastit-lsp + Zed, four view crates, raw-body gate, poolers)

$ uv run --no-project tools/plan.py validate
plan OK                                                # EXIT=0

$ uv run --no-project tools/guard.py
guard: OK                                              # EXIT=0

$ make check
# EXIT=0 (all Rust workspace fmt/clippy/doc/unit/integration/critical suites, rs-no-python, rs-no-default, lsp-portable, zed-check, platform-matrix-check, workflow-check, packaging-gates-test, doc-tone, manifest-purity, Python ruff + critical + docs-unit, zensical build --strict, wheel build + 48 runnable doc fences, plan.py validate)

$ make e2e   # verified at 9784965 (zero Rust/Python/e2e code changed since 9784965)
PASS: 0 failure(s)                                     # EXIT=0 (411 ok, 0 FAIL)
```

## Files Touched at Gate

- `CHANGELOG.md` (updated `## 0.2.0 (2026-11-18)` to match shipped `0.2.0` scope)
- `RELEASING.md` (synced `0.2.0` 29-artifact + `sdist` matrix and CI-only legs)
- `ARCHITECTURE.md` (synced multi-backend opening, entry points, `MB.28` parallel-views slots/caps, `runtime`/`dev.reload` events, and shipped §11 items)
- `docs/{index,ci,local,security,troubleshooting,changelog}.md`, `docs/deploy/{index,anything-else,compose-traefik}.md`, `docs/frameworks/{php,python}/index.md`, `docs/reference/{cli,settings}.md` (removed surviving `0.1.1`/WSGI-only/`egress = true`/`prefork = true`/`php84-embed` contradictions and added `blastit fmt` to `cli.md`)
- `plan/ROADMAP.md`, `plan/STATE.md`, `plan/JOURNAL.md`, `plan/ESCALATIONS.md`, `plan/BUDGET.md`, `plan/nodes/MC/NODE.md`, `plan/nodes/MB/**`
