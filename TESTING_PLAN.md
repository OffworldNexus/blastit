# Blastit Testing & Code-Reduction Plan (`TESTING_PLAN.md`)

> **Goal:** Cut test/meta-test code by **~89%** (`174.3k` → `~19.5k` LOC) and production/script code by **~46%** (`109.2k` → `~58.5k` LOC) by testing **only real user stories end-to-end**, restricting unit tests to 4 narrow pure-parser/wire boundaries, and deleting all meta-tests and retired `0.1.0` scaffolding.

---

## 1. Current Footprint (Measured Reality)

The repository currently carries **60% more test/meta-test code (`174.3k` LOC) than product/build code (`109.2k` LOC)** — `283.4k` lines across `crates/`, `python/`, `e2e/`, and `scripts/`:

| Area | Prod / Build LOC | Test / Meta-Test LOC | Where the Bloat Lives |
|---|---:|---:|---|
| `crates/*/src/` (20 crates) | `95,362` *(incl. `29.8k` comment lines + `1.2k` C/`build.rs`)* | `51,460` | `45,336` inline `#[cfg(test)]` + `6,124` `src/**/tests*` files (`stage0/unit_tests.rs` `2.6k`, `audit/src/tests/*` `1.9k`, `config/tests.rs` `1.7k`). **31% of Rust `src/` is essay comments.** |
| `crates/*/tests/` (127 files) | — | `81,622` | Duplicate regular + `critical_*` twins (`blastfile_body.rs` + `critical_blastfile_body.rs`), mutant-chasing (`mutation_gaps.rs`, `edit_branches.rs`), retired `0.1.1` TOML (`critical_manifest_011.rs`), artifact-prose checks (`critical_artifacts.rs` `2.6k`). |
| `python/blastit/` (7 files) | `6,339` | — | **71% (`4,525` LOC) is legacy `0.1.0` tooling:** `pytest_plugin.py` (`2,646` — disabled by `blastit check` via `-p no:blastit`), `fold.py` (`1,351`), and `logs.py` (`528`). |
| `python/tests/` (59 files) | — | `18,563` | `runnable_docs/` harness (`6.0k`), Markdown/SVG/prose linter `test_docs_assets.py` (`2,240`), legacy `test_fold*` + `test_pytest_plugin*` (`4,208`), `test_release_scripts.py` (`808`). |
| `e2e/` (59 files) | — | `11,163` | 16 overlapping single-purpose Python drivers (`dx_replay.py`, `scenario.py`, `live.py`, `run.py`, `grants.py`, `php_sapi_probe.py`) spawning duplicate Docker stacks. |
| `scripts/` (43 files) | `7,480` *(21 build/pkg scripts)* | `11,457` *(22 gate/meta-test scripts)* | YAML/shell/prose meta-linters and self-tests of linters: `test_packaging_gates.py` (`2,781`), `check_workflows.py` (`2,218`), `check_zed_extension.py` + test (`2,004`), `test_publish_gates.py` (`741`). |
| **Total** | **`109,181`** | **`174,265`** | **4 duplicate LSP suites; 3–4 layers re-testing every route/grant path; `19k+` LOC linting YAML, scripts, and docs prose.** |

### Per-Crate X-Ray (`crates/*`)

| Crate | Prod LOC *(comments)* | `src/` Test LOC | `tests/` LOC *(files)* | Target Test Disposition |
|---|---:|---:|---:|---|
| `blastit` | `21,517` *(`6.3k`)* | `12,187` | `30,381` *(31)* | **Delete** all `src/` unit tests; **merge** `tests/` into 4 E2E binary suites (`~5,500` LOC). |
| `blastit-manifest` | `11,889` *(`3.4k`)* | `4,493` | `15,017` *(34)* | **Exception #2:** Collapse `19.5k` test LOC into `<= 1,500` LOC grammar/fmt/edit unit suite. |
| `blastit-scheduler` | `7,521` *(`2.5k`)* | `1,573` | `12,244` *(17)* | **Delete** `13.8k` mock-channel unit/harness tests; covered via `e2e_dev.rs` & `us5_prod_sandbox.py`. |
| `blastit-sqlpolicy` | `7,441` *(`2.4k`)* | `5,932` | `3,792` *(7)* | **Exception #1:** Collapse `9.7k` test LOC into `<= 1,500` LOC SQL AST/policy/lexer unit suite. |
| `blastit-sys` | `6,612` *(`2.6k`)* | `5,344` | `1,816` *(3)* | **Exception #4 (half):** Keep `<= 400` LOC malformed ELF header tests; delete `6.8k` syscall/unit duplicates. |
| `blastit-dbproxy` | `4,254` *(`1.3k`)* | `2,716` | `3,296` *(4)* | **Delete** `6.0k` isolated proxy tests; covered end-to-end via `blastit dev`, `check`, and prod E2E. |
| `blastit-http` | `3,228` *(`1.0k`)* | `2,139` | `3,854` *(9)* | **Exception #3:** Keep `<= 800` LOC raw wire-smuggling/framing tests; delete `5.2k` duplicates. |
| `blastit-audit` | `4,561` *(`1.5k`)* | `2,253` | `255` *(1)* | **Retire crate:** Delete `fold`/`pytest_plugin` shims + `2.5k` tests; inline `check` diffing into `blastit`. |
| `blastit-view-python` | `3,868` *(`1.4k`)* | `1,921` | `3,115` *(8)* | **Delete** `5.0k` worker micro-tests; covered via `e2e_dev.rs` & `us2_runtimes.py`. |
| `blastit-events` | `2,057` *(`0.4k`)* | `4,896` | `92` *(1)* | **Delete** all `5.0k` JSON formatting unit tests; E2E suites observe real event lines. |
| `blastit-frame` | `2,726` *(`1.0k`)* | `2,343` | `0` *(0)* | **Exception #4 (half):** Keep `<= 300` LOC corrupt/truncated frame tests; delete `2.0k`. |
| `blastit-view-php` + `php-sapi` | `9,302` *(`2.8k`)* | `2,148` | `2,042` *(4)* | **Delete** `4.2k` mock/ABI micro-tests; covered via real `libphp` in `e2e_dev.rs` & `us2_runtimes.py`. |
| `blastit-view-static` | `1,287` *(`0.5k`)* | `0` | `3,388` *(4)* | **Delete** standalone static suites; covered via `blastit dev` & `check` E2E (`US-2.3`, `US-3.1`). |
| `blastit-lsp` | `1,658` *(`0.4k`)* | `398` | `413` *(1)* | **Delete** `src/` tests; **keep** single `tests/e2e_lsp.rs` stdio suite (`~600` LOC) for `US-4`. |
| `env`, `ingress`, `view`, `py-sapi`, `py-shim` | `7,441` *(`2.2k`)* | `3,117` | `1,917` *(3)* | **Delete** all `5.0k` unit/shim micro-tests; covered by E2E boot, wheel, and prod refusal suites. |

---

## 2. Testing Rules Going Forward

1. **User-Story E2E Priority:** Every test in the repository must trace to a named User Story (`US-1`..`US-5` below) and run against the real `blastit` or `blastit-lsp` binary over real sockets, files, and processes (both happy path and security/fail-closed refusal). **If a test does not protect a user story, delete it.**
2. **No Unit Tests by Default (4 Strict Exceptions Only, Hard `<= 4,500` LOC Ceiling):**
   Unit tests are forbidden everywhere except 4 pure-parser/wire boundaries where combinatorial malformed-input cases are impractical to test by booting a full process tree per row:
   - **`blastit-sqlpolicy` (`<= 1,500` LOC):** SQL lexer boundaries, PostgreSQL AST table/column/function extraction, `$1` → `$?` normalization, and fail-closed parser refusals.
   - **`blastit-manifest` (`<= 1,500` LOC):** `Blastfile` lexer/parser recovery, canonical formatter round-trips, and comment-preserving surgical grant edits.
   - **`blastit-http` (`<= 800` LOC):** Malformed HTTP/1.1 wire bytes, `Content-Length` / `Transfer-Encoding` smuggling rejection, header caps, and early abort.
   - **`blastit-sys` + `blastit-frame` (`<= 700` LOC combined):** Malformed ELF header/`.dynsym` bytes and truncated/corrupt `view.chan` wire frames.
3. **Zero Meta-Tests, YAML Linters, or Prose Guards:** Delete all Python/Rust tests that regex-grep or AST-lint GitHub Actions YAML, packaging shell scripts, Zed `.toml`/`.scm` files, ELF scripts, or documentation prose. Real CI jobs building and installing the real packages are the test.

---

## 3. Complete User Stories & E2E Ownership (`US-1`..`US-5`)

Every user story below has **one** authoritative E2E owner. No secondary unit or wrapper suite may re-test it.

### US-1: Installation, Packaging & CLI Handoff

| ID | User Story | E2E Verification (Happy Path + Refusal Boundary) | Single Owning Suite |
|---|---|---|---|
| **US-1.1** | Install native `.deb` (Debian/Ubuntu) or `.rpm` (Fedora/EL) | `apt install` / `dnf install` places `blastit`, `blastit-lsp`, and CPython/PHP shims; `blastit --version` and `blastit dev` serve a minimal app with zero `libpython` linked into `blastit`. | `e2e/us1_packaging.py` |
| **US-1.2** | Install standalone `.tar.gz` via `install.sh --user` (glibc & Alpine musl) | Unprivileged install into `~/.local`; dynamic musl binary loads Alpine `libphp84`/`85` via `dlopen` (`INTERP` present); checksum mismatch fails closed. | `e2e/us1_packaging.py` |
| **US-1.3** | Install `py3-none` universal wheel (`pip` / `uv`) on Python 3.12–3.14 | Same wheel runs CLI/`check`/`fmt` on 3.12–3.14, serves WSGI on 3.13 & 3.14, and refuses WSGI on 3.12 with a clear upgrade message. Pure stdlib (`dependencies = []`). | `e2e/us1_packaging.py` |
| **US-1.4** | Install PHP Composer binary wrapper (`composer require`) | `vendor/bin/blastit` verifies SHA-256 and delegates to the platform binary; corrupt archive is refused before execution. | `e2e/us1_packaging.py` |
| **US-1.5** | Run `blastit` CLI & local project binary handoff | Global `blastit` hands off `dev`/`check`/`fmt`/`lsp` to `.venv/bin/blastit` or `vendor/bin/blastit` when safe; skips group/world-writable project paths with warning; `$BLASTIT_BIN` unsafe path exits `1`; unknown `--preset` or bad flags exit `2`; internal `--stage`/`--role` flags hidden from `--help`. | `crates/blastit/tests/e2e_cli.rs` |

### US-2: Local Zero-Sudo Development (`blastit dev`)

| ID | User Story | E2E Verification (Happy Path + Refusal Boundary) | Single Owning Suite |
|---|---|---|---|
| **US-2.1** | Develop a Python WSGI app (Django / Flask / SQLAlchemy) with PostgreSQL | Zero-`sudo` `blastit dev` boots on loopback, strips `DATABASE_URL` from worker env, allows granted parameterized `$1` & `trait` queries, and denies ungranted SQL/tables/columns (`SQLSTATE 42501`) with zero secret/literal leak in logs. | `crates/blastit/tests/e2e_dev.rs` (+ `e2e/us2_runtimes.py` for 3.13/3.14 matrix) |
| **US-2.2** | Develop an embedded NTS PHP 8.3–8.6 app (Symfony / Laravel / PSR) | `entrypoint php "public/index.php"` serves concurrent views via embedded `libphp`, isolates `exit()`/fatals without killing worker pool, enforces PostgreSQL grants/denials, and removes `php2shell` (`%ADd` / `auto_prepend_file` / FastCGI param surface absent by construction). | `crates/blastit/tests/e2e_dev.rs` (+ `e2e/us2_runtimes.py` for PHP 8.3–8.6 & multi-pkg Alpine) |
| **US-2.3** | Serve Static assets & SPA front-controller mounts | `static` mount serves files with `ETag`/`Range`/`immutable` cache headers and SPA index fallback; `openat2` confines reads strictly beneath root (rejects `..`, symlinks, dotfiles, FIFOs). Zero interpreter required for static-only `Blastfile`. | `crates/blastit/tests/e2e_dev.rs` |
| **US-2.4** | Edit code or `Blastfile` with live reload | Modifying `.py`, `.php`, or `./Blastfile` triggers `fanotify` hot reload of workers/routing without dropping the listener; syntax errors keep the server alive and report cleanly; `SIGINT`/`SIGTERM` reaps all children cleanly. | `crates/blastit/tests/e2e_dev.rs` |
| **US-2.5** | Populate `Blastfile` in one pass with `blastit dev --learn` & `--dry-run` | Single test pass across Python or PHP records routes + parameterized SQL grants preserving comments; `--learn --dry-run` prints proposed diff without modifying `./Blastfile`; scanner probes (`..`, `/.env`, `/.git`) are never learned. | `crates/blastit/tests/e2e_dev.rs` |
| **US-2.6** | Prevent accidental LAN exposure of `blastit dev` | Non-loopback `--host 0.0.0.0` refuses to start unless `BLASTIT_DEV_EXPOSE=1` is set (and prints `EXPOSED, no auth` banner when set). | `crates/blastit/tests/e2e_dev.rs` |

### US-3: CI Completeness Audit & Formatting (`blastit check` & `blastit fmt`)

| ID | User Story | E2E Verification (Happy Path + Refusal Boundary) | Single Owning Suite |
|---|---|---|---|
| **US-3.1** | Gate CI on `blastit check` (clean suite passes) | `blastit check` runs `pytest` against live `dbproxy` for Django and Flask/SQLAlchemy apps (plus store-less and static-only apps with no DB) and exits `0` when routes, `body` policies, and SQL grants are exact and exercised. | `crates/blastit/tests/e2e_check_fmt.rs` |
| **US-3.2** | Catch drift in CI (`blastit check` & `blastit check --json`) | Exits `1` with actionable human + `--json` parity on every defect class: `missing_grant`, `dead_grant` (naming `sql`, `proxy`, and `view <name> postgres`), `unrouted`, `unexercised_view`, and missing `body` policy. | `crates/blastit/tests/e2e_check_fmt.rs` |
| **US-3.3** | Bootstrap new views in CI via `blastit check --learn` | Proposes new `view` blocks with observed grants while refusing to silently widen grants on already-declared views. | `crates/blastit/tests/e2e_check_fmt.rs` |
| **US-3.4** | Canonicalize `Blastfile` via `blastit fmt` & `blastit fmt --check` | `blastit fmt` formats `./Blastfile` idempotently preserving comments; `blastit fmt --check` exits `0` when canonical and exits `1` printing a unified diff when unformatted. | `crates/blastit/tests/e2e_check_fmt.rs` |

### US-4: Editor Experience (`blastit lsp` / `blastit-lsp`)

| ID | User Story | E2E Verification (Happy Path + Refusal Boundary) | Single Owning Suite |
|---|---|---|---|
| **US-4.1** | Live `Blastfile` diagnostics, hover & formatting over stdio LSP | Real `blastit-lsp` child process over stdio JSON-RPC publishes parse/semantic diagnostics on `didOpen`/`didChange`, answers `$?`/`...` hover docs, and returns lossless `textDocument/formatting` edits. | `crates/blastit-lsp/tests/e2e_lsp.rs` |
| **US-4.2** | Quick-fix unparameterized SQL literals in editor | `textDocument/codeAction` on a SQL grant containing a quoted/numeric literal returns the exact `$?` parameterization edit. | `crates/blastit-lsp/tests/e2e_lsp.rs` |

### US-5: Production Zero-Trust Sandboxing (`blastit` Root & Rootless)

| ID | User Story | E2E Verification (Happy Path + Refusal Boundary) | Single Owning Suite |
|---|---|---|---|
| **US-5.1** | Run production container as root (`--preset digitalocean-paas`) | Stage 0 → stage 1 `execve` barrier strips real DB secrets from `/proc/<view_pid>/environ`; views run under distinct `UID_BASE+1000+i` with `PR_SET_DUMPABLE(0)`, `0700`/`0600` per-view sockets + `SCM_CREDENTIALS`, seccomp outbound-socket block, fair CPU queuing across slow views, and zero-drop `SIGTERM` drain. | `e2e/us5_prod_sandbox.py` |
| **US-5.2** | Run rootless production container (`--preset compose-traefik`) | Dedicated `0500` `blastit-init` with file capabilities (`cap_setuid`, `cap_setgid`, `cap_chown`, `cap_fowner`, `cap_dac_override`, `cap_kill`) isolates per-view UIDs; view children have empty capability sets and `NO_NEW_PRIVS`. | `e2e/us5_prod_sandbox.py` |
| **US-5.3** | Fail closed on unsafe production environment or tampered files | Production `blastit` exits `1` before serving when: run unprivileged without caps; shared `python`/`blastit` carries file caps; binary or ancestor dir is group/world-writable; `Blastfile` is a FIFO/symlink; PHP entry script/shim `(dev, ino, size, ctime)` mutates after boot; or `body raw` lacks `unvalidated`. | `crates/blastit/tests/e2e_prod_refusals.rs` (+ root legs in `e2e/us5_prod_sandbox.py`) |

---

## 4. Drastic Pruning & Consolidation Map

### 4A. Tests & Meta-Tests: `174,265` LOC → `~19,500` LOC (**~89% Reduction**)

| Location | Current LOC | Target LOC | Action (Keep / Merge / Delete) |
|---|---:|---:|---|
| `crates/*/src/` inline `#[cfg(test)]` + `src/**/tests*` | `51,460` | `~4,500` | **Delete `~47k` LOC** across 15 crates. **Keep only** the 4 capped pure-logic suites: `sqlpolicy` (`<=1.5k`), `manifest` (`<=1.5k`), `http` (`<=800`), `sys`+`frame` (`<=700`). |
| `crates/blastit/tests/` (31 files) | `30,381` | `~5,500` | **Delete** `critical_artifacts.rs` (`2.6k`), `critical_internal_json.rs`, `dx_capture.rs`, `dx_invariance.rs`, `critical_lsp.rs` (`2.5k`, duplicate). **Merge** remaining 25 files into **4 E2E suites**: `e2e_cli.rs` (`~700`), `e2e_dev.rs` (`~2,500`), `e2e_check_fmt.rs` (`~1,300`), `e2e_prod_refusals.rs` (`~1,000`). |
| `crates/blastit-manifest/tests/` (34 files) | `15,017` | `0` *(in `src/` cap)* | **Delete** `critical_manifest_011.rs` (retired TOML), `docs_fences.rs`, `edit_branches.rs`, `mutation_gaps.rs`, and all 30 split `blastfile_*` / `critical_blastfile_*` twins; fold unique grammar/edit cases into the `1,500` LOC `blastit-manifest` unit budget. |
| Other `crates/*/tests/` (95 files across 14 crates) | `35,811` | `0` | **Delete** in-process mock harnesses (`scheduler/tests/common/mod.rs` `1.7k`, `head_allocs.rs`, `static_feature_off.rs`, `grant_diff_parity.rs`, `lifecycle_build.rs`). Migrate any uncovered security/rewrite invariants into `e2e_dev.rs` and `us5_prod_sandbox.py`. |
| `crates/blastit-lsp/tests/` (1 file) | `413` | `~600` | **Keep & expand slightly** as `e2e_lsp.rs` (absorbing unique stdio cases from `blastit/tests/critical_lsp.rs`, `python/tests/critical/test_lsp_stdio.py`, and `scripts/lsp_smoke.py`). |
| `python/tests/` (59 files) | `18,563` | `~900` | **Delete** `runnable_docs/` (`6.0k`), `test_docs_assets.py` (`2,240`), `test_fold*` (`2,326`), `test_pytest_plugin*` (`1,882`), `test_release_scripts.py` (`808`), `test_lsp_stdio.py` (`397`). **Keep** one lean `test_python_wheel_bridge.py` (`~500` LOC) + minimal WSGI fixture (`~400` LOC). |
| `e2e/` (59 files) | `11,163` | `~8,000` | **Merge** 16 overlapping Python drivers into **3 container E2E suites**: `e2e/us1_packaging.py` (`~2,000`), `e2e/us2_runtimes.py` (`~3,000`, incl. `php2shell` & PHP 8.3–8.6 / Python 3.13–3.14), `e2e/us5_prod_sandbox.py` (`~3,000`, root + rootless + swap refusals). |
| `scripts/` meta-tests & gate linters (22 files) | `11,457` | `0` | **Delete all 22** (`test_packaging_gates.py`, `check_workflows.py`, `check_zed_extension.py`, `test_check_zed_extension.py`, `test_publish_gates.py`, `lsp_smoke.py`, `check-no-test-seam.sh`, `wheel-smoke.sh`, etc.). Fold the 20-line `readelf` portability check directly into `build-standalone.sh`. |
| **Total Test / Meta-Test LOC** | **`174,265`** | **`~19,500`** | **~155k LOC deleted (~89% cut); 100% of `US-1`..`US-5` covered end-to-end.** |

### 4B. Production & Script Code: `109,181` LOC → `~58,500` LOC (**~46% Reduction**)

| Area | Current LOC | Target LOC | Concrete Cuts |
|---|---:|---:|---|
| `python/blastit/` | `6,339` | `~1,300` | **Delete** legacy `0.1.0` `pytest_plugin.py` (`2,646`), `fold.py` (`1,351`), and `logs.py` (`528`). Keep only `__init__.py`, `__main__.py`, slimmed `_blastit.py` launcher, and `_check_pytest.py`. |
| `crates/blastit-audit` + `blastit/src/cli/internal_json.rs` | `~5,000` | `~1,800` | Delete JSON bridge ops that existed only for `fold.py` and `pytest_plugin.py` (`covering`, `audit`, `redact_literals`, `has_quoted_literal`, `blastfile_model`); inline `check` audit diffing into `blastit` and retire `blastit-audit` as a separate crate. |
| `crates/*` Rust `src/` comments & ceremony | `90,362` *(excl. `audit`)* | `~53,200` | Strip `~22k` LOC of multi-paragraph historical diary comments (keep concise doc-comments on public items + `SAFETY:` invariants) and simplify over-split single-caller adapter modules (`supervise.rs`, `dev_reload.rs`, `error.rs`, `worker.rs`). |
| `scripts/` (43 → 8 build/release scripts) | `7,480` *(build)* | `~2,200` | Collapse 21 build/helper scripts into **8 clean scripts**: `install.sh`, `build-standalone.sh` (absorbs `in-image` + ELF check), `build-wheel.sh` (absorbs `repack_universal_wheel.py` + `musl`), `build-deb.sh`, `build-rpm.sh`, `publish_packages.py`, `dev-version.sh`, `release-notes.sh`. |
| `tools/critical_guard.py` | `286` | `0` *(or 50-line list)* | Retire `critical_guard.py` (which froze 60+ fragmented `critical_*.rs` files in place) or rescope it strictly to the 8 consolidated `e2e_*` / `us*` suites. |

---

## 5. Open Decisions for Owner Review

Please confirm **Yes / No** on these 5 cuts before anything is deleted:

1. **D1 — Retire `0.1.0` `pytest --blastit-manifest` plugin (`pytest_plugin.py`, `2,646` prod + `1,882` test LOC)?**
   *Recommendation: **Yes.*** `blastit check` superseded it (and explicitly passes `-p no:blastit` to disable it). Removing the `pytest11` entry point eliminates 4.5k LOC and zero-cost overhead on unrelated `pytest` runs.
2. **D2 — Retire `blastit-manifest fold` Python CLI (`fold.py` + `logs.py`, `1,879` prod + `2,326` test LOC + `blastit-audit` JSON shims)?**
   *Recommendation: **Yes.*** `blastit dev --learn`, `blastit check --learn`, and `blastit fmt` handle route/grant learning and canonical formatting natively in Rust. (If log-folding is ever wanted later, a 150-line Rust subcommand reuses `dev_learn` directly.)
3. **D3 — Delete `python/tests/runnable_docs/` (`6,000` LOC) and `test_docs_assets.py` (`2,240` LOC)?**
   *Recommendation: **Yes.*** Replace `8.2k` LOC of custom doc-fence orchestration with `zensical build --strict` (broken links/anchors) plus a 60-line check in `e2e_cli.rs` parsing every `Blastfile` fence in `docs/` through `blastit fmt --check`.
4. **D4 — Delete `scripts/check_workflows.py`, `test_packaging_gates.py`, and the other 20 script meta-tests (`11,457` LOC)?**
   *Recommendation: **Yes.*** Real CI jobs building and testing `.deb`, `.rpm`, `.tar.gz`, and wheels in `e2e/us1_packaging.py` verify the actual artifacts instead of regex-linting YAML and bash text.
5. **D5 — Retire `tools/critical_guard.py` and `cargo-mutants` Makefile machinery (`mutants-*.sh`)?**
   *Recommendation: **Yes.*** Both incentivized splitting tests into dozens of un-mergeable `critical_*.rs` files and writing `mutation_gaps.rs` filler tests instead of clean E2E user-story suites.

---

## 6. Proposed Execution Order (After Owner Sign-Off)

1. **Slice 1 — Delete Meta-Tests, Legacy Python (`D1`–`D5`), and `scripts/` Bloat:** Remove `pytest_plugin.py`, `fold.py`, `logs.py`, `runnable_docs/`, `test_docs_assets.py`, 35 `scripts/` files, and `critical_guard.py`; retire `blastit-audit` by inlining `check` diffing into `blastit`.
2. **Slice 2 — Consolidate Rust & Container E2E Suites (`US-1`..`US-5`):** Merge fragmented `crates/*/tests/` and `e2e/` drivers into the 8 user-story E2E suites (`e2e_cli.rs`, `e2e_dev.rs`, `e2e_check_fmt.rs`, `e2e_lsp.rs`, `e2e_prod_refusals.rs`, `us1_packaging.py`, `us2_runtimes.py`, `us5_prod_sandbox.py`).
3. **Slice 3 — Prune Unit Tests to the 4 Exceptions (`<= 4,500` LOC):** Delete non-exception inline `#[cfg(test)]` and `src/**/tests*` modules; trim `sqlpolicy`, `manifest`, `http`, and `sys`/`frame` unit tests to their caps.
4. **Slice 4 — Strip Essay Comments & Simplify Oversized Rust Modules:** Remove historical diary comment blocks (`~22k` LOC) and tighten oversized `blastit` / `blastit-manifest` / `blastit-view-php` modules while keeping all `US-1`..`US-5` E2E suites green.
