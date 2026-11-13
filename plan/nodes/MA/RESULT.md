# Result MA — GATE PACKET (DRAFT — pending strategist acceptance)

> **DRAFT — pending strategist acceptance.** This file was assembled by a cheap-tier `lead`
> (`gate-packet preparation` mode) because the expensive tier is unusable (H-003, OpenRouter
> credits). It is **not** an accepted milestone result: `MA` is still `ready`, nothing here marks
> the gate passed, and the decision to close `MA` — including the `cargo-mutants` deferral — is the
> `strategist`'s alone.

## What this packet is

Everything a `strategist` needs to run the `MA` gate as one mechanical call: evidence run at
`e0bc1b3`, the `ROADMAP.md` exit criteria mapped one-by-one, the audit sign-off, a fresh-eyes DX
pass, the release facts, the exact `HUMAN.md` release text to paste, MB starting points, and the
short list of decisions only the `strategist` can make.

## Evidence — run by this `lead`, not copied from child results

Base commit: `e0bc1b3` (`[MA.3] Gate passed: runnable docs, visual QA, e2e and the 0.1.1 release
prep`). Full logs stay out of the repository: `/tmp/opencode/ma-gate/{check.log,e2e.log}`.

```console
$ make check                 # entire Makefile chain, unprivileged, offline
exit 0                       # guard, critical-guard, fmt, clippy -D warnings, doc -D warnings,
                             # workspace tests, 12 critical Rust suites (200 tests), 50 Python
                             # critical tests, ruff check + format, docs-unit (46 tests / 57
                             # subtests), 38 doc fences (26 executed, 12 accounted, 0 failed),
                             # zensical build --strict ("No issues found"), plan validate ("plan OK")

$ make e2e                   # scripts/build-wheel.sh + Docker Django/Flask+SQLAlchemy + rootless
exit 0  (real 0m33s)         # 8 × "PASS: 0 failure(s)": app self-test, Django scenario, SQLAlchemy
                             # app, SQLAlchemy logs, rootless self-test, rootless Django scenario,
                             # rootless logs, plus the rootless capability/boot-event assertions
                             # (`privilege: capabilities`, exactly cap_chown,cap_dac_override,
                             # cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap; the
                             # dedicated `python3.14-init` copy 0500 refused to the worker uid)
```

Per-suite tails from `make check` (`/tmp/opencode/ma-gate/check.log`):

| Protected suite | Result |
|---|---|
| `blastit-dbproxy/tests/critical_learn_and_redaction` | ok, 9 passed |
| `blastit-http/tests/critical_early_abort` | ok, 5 passed |
| `blastit-ingress/tests/critical_ingress` | ok, 55 passed |
| `blastit-manifest/tests/critical_manifest_011` | ok, 17 passed |
| `blastit-manifest/tests/critical_manifest_edit` | ok, 18 passed |
| `blastit-py/tests/critical_check_schema` | ok, 6 passed |
| `blastit-scheduler/tests/critical_ingress_enforcement` | ok, 9 passed |
| `blastit-sqlpolicy/tests/critical_learn_policy` | ok, 10 passed |
| `blastit-sqlpolicy/tests/critical_literals` | ok, 25 passed |
| `blastit/tests/critical_check_engine` | ok, 16 passed |
| `blastit/tests/critical_config_and_presets` | ok, 11 passed |
| `blastit/tests/critical_dev_and_fail_closed` | ok, 19 passed |

One trivial doc defect was fixed during this packet (`docs/troubleshooting.md`, §4 below), so
`make check` was re-run on the tree that this commit contains: **exit 0** again
(`/tmp/opencode/ma-gate/check2.log`, same suites and fence counts). No Rust, Python, test or
workflow file changed in this packet.

The one command **not** run for real: `make mutants-rust` — `cargo-mutants` is absent on this host
(`cargo --list | grep -c mutants` → `0`; `command -v cargo-mutants` → nothing). See §3.

## 1. `ROADMAP.md` MA exit criteria

| # | Criterion (as written in `plan/ROADMAP.md`) | Verdict | Evidence |
|---|---|---|---|
| 1a | Docs-First overhaul: radically leaner `docs/` + `README.md` covering Local / CI-CD / Deployments with zero kernel-trivia bloat | **PASS** | `README.md` 71 lines, `docs/getting-started.md` 63, `docs/local.md` 149, `docs/ci.md` 150, `docs/index.md` 70; nav `zensical.toml:25-47` names exactly those pages; commit `ad671d6` (MA.1) + `dba348c` (MA.3.2 reconciliation) |
| 1b | `zensical build --strict` green | **PASS** | inside `make check` exit 0: `uvx --offline zensical build --strict` → `No issues found` / `Build finished in 0.18s` |
| 1c | `visual-checker` PASS | **PASS** | `docs/testing/VISUAL-MA.3.md:42` → `VERDICT: PASS`; 16 page×viewport rows (9 pages, light/dark/mobile), 3 Mermaid diagrams rendering, 0 404s/console errors, base `dba348c` |
| 2a | Unprivileged `blastit dev` single-UID simulation: real multi-process topology, `execve` env swap, per-route sockets, loud dev banner | **PASS** | `crates/blastit/tests/critical_dev_and_fail_closed.rs` — 19 passed (five roles, `privilege:"unprivileged-dev"`, 3-line banner ×2 + `EXPOSED`, `DATABASE_URL`/`BLASTIT_*`/`ENV_STRIP` stripped, per-route `.s.PGSQL`, `ENOENT` twin, `egress=false` `EPERM|ENOSYS` + twin) |
| 2b | Production `blastit` still fails closed without root/caps | **PASS** | same suite's production twin (exit 1, never `unprivileged-dev`, never `ready`); reproduced live in this packet: `blastit` as uid 1000 → exit 1, `init runs as uid 1000 and is missing cap_chown, …`; e2e rootless leg asserts the capability path holds |
| 2c | Auto-discovered `./blastit.toml`, optional non-secret `app`/`preload` in-manifest | **PASS** | `critical_config_and_presets.rs` 11 passed (CWD discovery, set-but-empty `BLASTIT_*` still wins, both presets); `docs/local.md:18-19`, `docs/getting-started.md:52-53` |
| 3a | Single-pass wire-level `--learn` at dbproxy/dev | **PASS** | `critical_learn_and_redaction` 9 passed (learn + dynamic check + literal redaction); `critical_dev_and_fail_closed` `--learn` per-route attribution + one-pass write; product fix `7440cd5` (MA.3.7) |
| 3b | Framework-agnostic `blastit test`/`blastit check` (FLask/SQLAlchemy + any WSGI alongside Django) | **PASS** | `make e2e` E2b Flask+SQLAlchemy leg `PASS: 0 failure(s)` (3 groups: app, logs, denial by proxy `42501` not by DB grants); `python/tests/critical/test_check_contract.py` (raw WSGI + `.wsgi_app` shapes) green inside `make test-critical` |
| 3c | Crisp text **and** `--json` reports | **PASS** | `critical_check_engine.rs` 16 passed (byte-exact human block, 9 finding kinds, interaction counts); `critical_check_schema.rs` 6 passed (schema version, counts, bridge bounds); `docs/ci.md:55-80` documents one stdout document |
| 4a | Every snippet in `docs/` and `README.md` tested for real | **PASS** | `make check` → `38 fences: 26 executed, 12 accounted, 0 failed`; the 12 accounted fences each carry a specific written reason in `python/tests/runnable_docs/inventory.toml` (cloud/registry/`setpriv`/external-install recipes) and are statically checked (flags, referenced files, YAML/ini/SQL parses) |
| 4b | `make check` green | **PASS** | exit 0, above |
| 4c | `make e2e` green | **PASS** | exit 0, 8 × `PASS: 0 failure(s)`, above |
| 4d | `test-curator` audit passed | **PASS WITH ONE DEFERRAL** | four audits on disk — `AUDIT-MA.2.1` (PASS WITH GAPS → G1..G4 closed by MA.2.1.4-7), `AUDIT-MA.2.2` (PASS WITH GAPS → G1..G4 closed by MA.2.2.5-8), `AUDIT-MA.2.3` (BLOCKED → F1..F8 closed, see §3), `AUDIT-MA.3` (BLOCKED → F1..F8 closed, see §3). No BLOCKER/MAJOR is open. The `cargo-mutants` half of the standing policy is **UNVERIFIED** (§3) |
| 4e | `Cargo.toml` at `0.1.1` | **PASS** | `Cargo.toml:6` `version = "0.1.1"`; all 13 workspace crates in `Cargo.lock` at `0.1.1` |
| 4f | `CHANGELOG.md` ready for the owner tag | **PASS** | `CHANGELOG.md:7` `## 0.1.1 (2026-11-12)` with four user-facing bullets; links/anchors resolve (verified in `site/` by MA.3.6) |
| 4g | `HUMAN.md` sign-off item opened for tagging `v0.1.1` | **FAIL (not yet opened)** | `plan/HUMAN.md` holds H-001 (closed), H-002 (cargo-mutants) and H-003 (credits); no release item exists. Exact text is in §5 — the `strategist` opens it |

**One-line verdict for the roadmap row:** every code/doc/DX criterion is PASS; the only FAIL is the
release sign-off item that the `strategist` itself must open, and the only UNVERIFIED is the literal
`cargo-mutants` run.

## 2. Audit sign-off check

### `docs/testing/AUDIT-MA.2.3.md` — verdict was "BLOCKED at the epic gate"

| Finding | Sev | Closed by | Evidence of closure |
|---|---|---|---|
| F1 — contract suite red under the `pytest-plugin` CI job (7/10) | BLOCKER | **MA.2.3.6** (`39998a5`) | `RESULT.md`: the CI package set reproduced red before, `74 passed` after, identical with `--with pytest-xdist`; the stub `pytest_django` is written for every Project |
| F2 — `test_plugin_helpers.py:61` `NameError` | BLOCKER | **MA.2.3.6** (`39998a5`) | same run (`74 passed`); `psycopg = pytest.importorskip(...)`, misleading `noqa: F821` removed |
| F3 — `test_pytest_plugin.py` fixture drift (7 tests) | BLOCKER | **MA.2.3.7** (`acd9289`) | re-pinned the Django 6.1 single setup statement and the documented `egress` warning; fixture changed, not the warning |
| F4 — `blastit dev` clean `SIGTERM` intermittently exits 1 (3/33) | BLOCKER | **MA.2.3.8** (`1c070e6`) + hardening **MA.2.3.12** (`25b9de3`) | late `ctl.chan` `EPIPE` is no longer fatal during drain; `critical_dev_and_fail_closed` green in every `make check` since (`19 passed` here) |
| F5 — `check --learn` exits 0 after a refused statement | MAJOR | **MA.2.3.9** (`e90b135`) | end-to-end contract for exit 1, `"wrote": false`, byte-identical manifest |
| F6 — `evaluate` may emit `add_query` carrying `…` | MAJOR | **MA.2.3.10** (`a6c1348`) | twin asserts `fix.action == "observe"` for a `query` verdict with `…` |
| F7 — `no_observations` zero-statement clause unprotected | MAJOR | **MA.2.3.10** (`a6c1348`) | routes exercised, no proxy statement → `no_observations` |
| F8 — `would_write` zero-request rule unprotected | MAJOR | **MA.2.3.10** (`a6c1348`) | 0 requests + an unrouted SQL statement → `"wrote": false` |
| F9/F10/F11 — inference extras, `--blastit-manifest=<path>` exits 3 not 2, singular labels/aliases | MINOR | **not filed** | Accepted-deferred, recorded as residual (below) |

### `docs/testing/AUDIT-MA.3.md` — verdict was "BLOCKED at the milestone gate"

| Finding | Sev | Closed by | Evidence of closure |
|---|---|---|---|
| F1 — five unrecognised fence shapes + orphan pages invisible | MAJOR | **MA.3.9** (`4aad339`), unit-pinned by **MA.3.13** (`4338f88`) | `fences.scan_page` walks every block (backticks or tildes, any indent, info-string suffix) and the gate is red on `$ ` in a non-console block, prompts outside a block, an unclosed block and a marker inside a body; `public_pages` = `docs/**/*.md − exclude_docs`. Oracles: `python/tests/test_runnable_docs.py` (orphan at `:289`/`:314`, shapes at `:82`), 46 tests + 57 subtests green in `make check` |
| F2 — executing mode accepted a console fence with zero commands | MAJOR | **MA.3.10** (`631c66a`) + **MA.3.13** | `test_a_fence_without_prompts_is_refused` / `test_an_empty_fence_is_refused` |
| F3 — page published but absent from `nav` never inventoried | MAJOR | **MA.3.9** (`4aad339`) | inventory derives from the docs tree minus `exclude_docs`; `test_an_orphan_page_without_an_entry_is_red` |
| F4 — `static` fences accepted nonexistent commands/flags/files; YAML/ini/Dockerfile/SQL never parsed | MAJOR | **MA.3.10** (`631c66a`) | per-executable flag tables audited against the installed `--help` (`assert_flag_tables`, `harness.py --audit-flags`), path arguments resolved, `_yaml_structure` / `_ini_document` / `_sql_block` parse every copied snippet |
| F5 — the docs gate had no tests of its own | MAJOR | **MA.3.13** (`4338f88`) | `make docs-unit` runs `python/tests/runnable_docs/test_harness.py` (25 tests/38 subtests) + `python/tests/test_runnable_docs.py` inside `make check` |
| F6 — `docs-test`, `critical_guard`, `critical-skip-check` ran in no workflow | MAJOR | **MA.3.8** (`19c37b1`) | `.github/workflows/ci.yml:228` `docs-gate` (45 min) and `:330` `guards` (5 min, `PIPESTATUS`-safe, `--node` per changed task node); `docs.yml` also asserts the orphan class. **No real Actions run yet** — an agent may not push, so this is a YAML+`run:`-body proof only (declared as such in MA.3.8's RESULT) |
| F7 — docs gate not offline/HOME-isolated | MAJOR | **MA.3.10** (`631c66a`) | `UV_OFFLINE=1` in `base_env`, per-run uv cache, `HOME`-scrubbed gate; note the remaining `PATH`/`RUSTUP_HOME` dependence documented in the RESULT and now handled in CI by linking `setup-python`'s interpreter into `/usr/local/bin` and probing it |
| F8 — sqlpolicy policy oracle lived in `src/**` unit tests (0/11 protected kills) | MAJOR | **MA.3.11** (`0cb3baf`) | new protected `crates/blastit-sqlpolicy/tests/critical_learn_policy.rs`, 10 tests / 10 allow-deny pairs; `make check` runs it (`10 passed`) and it kills `SP1..SP11` including the `SP8` survivor (depth 14/15/18 refused) |
| F12/F13 — `shape` mode normalises numbers; staging path local-only | MINOR/INFO | **accepted** | declared design; recorded as residual |

### Mutation testing — the honest state

`cargo-mutants` is **absent** on this host (H-002 still open): `cargo --list | grep -c mutants` →
`0`; `command -v cargo-mutants` → nothing; `make mutants-rust PKG=blastit-manifest` cannot run.
**The milestone gate's literal `make mutants-rust` evidence does not exist and cannot be produced
here.**

Anchored fallback evidence, authored and run by `test-curator` (not by this lead), in a scratch copy
of the tree — labelled as anchored substitutions, not a `cargo-mutants` sweep:

| Crate | Anchored mutants | Killed | Killed by a **protected** suite |
|---|---|---|---|
| `blastit-manifest` | 8 | **8 (8/8)** | 8 |
| `blastit-sqlpolicy` | 12 (11 + the `SP8` re-check) | **12 (12/12)** after MA.3.11 | 12 |
| `blastit` (CLI/check/infer, AUDIT-MA.2.3) | 20 (policy-relevant subset) | 9/20 before the repairs; every meaningful survivor became F5..F8 and is now tasked and closed | — |

**Residual risk, stated plainly:** the fallback measures only a curator-chosen, anchored subset of
one exact string replacement per branch; it does not enumerate the way `cargo-mutants` does, so
un-anchored mutants (operator changes, whole-function deletion, error-path reformatting) in
`blastit-manifest` / `blastit-sqlpolicy` remain unmeasured and the "≥ 75 % killed" bar is evidenced
only on that subset — a real, if narrow, hole in the milestone's mutation policy.

**Other residuals accepted (none blocking):** AUDIT-MA.2.3 F9/F10/F11 (inference extras beyond the
13-row table; `--blastit-manifest=<path>` exits 3 instead of the documented 2; singular/alias
labels) and AUDIT-MA.3 F12/F13. They are MINOR, each with a one-line reproduction in the audit.

## 3. DX leanness spot-check (fresh eyes, this packet)

Read without the authors' context: `README.md`, `docs/index.md`, `docs/getting-started.md`,
`docs/local.md`, `docs/ci.md`, `docs/deploy/*.md`, `docs/frameworks/*.md`,
`docs/troubleshooting.md`, `docs/reference/manifest.md` (+ `settings.md`, `cli.md` headings).

| Rule (`dx-first` §4) | Verdict | Where |
|---|---|---|
| Getting started ≤ 3 steps | **PASS** | `docs/getting-started.md:5` (1. Install), `:15` (2. Write two files), `:38` (3. Run it) |
| Zero `sudo` on localhost | **PASS** | no `sudo`/`setcap`/`chmod` command in README, `getting-started`, `local`, `ci`, `frameworks/*`; the only `chmod`/`setcap` lines are inside the *rootless production* recipe (`deploy/compose-traefik.md:175-182`) |
| One canonical way per task | **PASS (one nit)** | `dev`, `--learn`, `check`, `--json`, presets each appear once per task. Nit: `docs/local.md:63` shows `export DATABASE_URL=…` and `:67` one line for `uv run --env-file .env` — two spellings of "set an env var" on a page that preaches one; the note is short and links nowhere, acceptable but it is the only place the pattern repeats |
| Every command/snippet runnable | **PASS with accounted exceptions** | 26 of 38 fences execute; the 12 accounted ones (`inventory.toml`) are cloud/registry/`setpriv`/external-install recipes with a specific written reason and a static check (real flag tables from `--help`, path existence, YAML/ini/SQL parses) |
| No kernel-trivia lectures in user pages | **PASS** | uid layout, capabilities, seccomp and stage-0 `execve` live in `how-it-works.md`, `security.md` and `deploy/*`; `getting-started`/`local`/`ci` name the property in one sentence and move on |
| Pages scannable, short | **PASS** | user pages 63–150 lines (`reference/manifest.md` 315 and `deploy/compose-traefik.md` 208 are reference/recipe pages, table- and block-dominated) |

**Defects found and fixed in this packet (docs only, no code):**

1. `docs/troubleshooting.md:10` listed the symptom `` `production mode needs root or …` ``. **No
   such message exists** — the production refusal for an unprivileged boot is `init runs as uid
   1000 and is missing cap_chown, …` (reproduced live with `target/debug/blastit` as uid 1000;
   source `crates/blastit/src/stage0.rs:230-260`). Row replaced with the real symptom.
2. Missing row: with no `./blastit.toml` in the directory, `blastit dev` says `BLASTIT_APP: the
   WSGI callable (`module:attribute`) is required` (reproduced), which points a beginner at an env
   var they never set. Row added at `docs/troubleshooting.md:13` naming the real cause.

**Defects found, deliberately not fixed (out of a lead's scope / would touch code or the docs gate):**

3. `blastit dev`'s message for a missing `./blastit.toml` is an `BLASTIT_APP` complaint rather than
   "no ./blastit.toml here" (see #2). The text is pinned by
   `critical_config_and_presets.rs`, so changing it is a product task, not a doc task → MB follow-up.
4. `README.md:61` and `docs/index.md:40` write `blastit --preset …` bare next to `uv run blastit
   dev` on the same page. Correct for the published image (where `blastit` is the entrypoint), but
   a `uv add` reader may copy it. Left alone: the `summary`/`presets` inventory modes pin these
   fences and the deployment pages set the context. Cosmetic.
5. `docs/local.md:87` / `docs/ci.md:36` show counts (`412 requests, 977 statements`) that the gate
   normalises (`AUDIT-MA.3` F12) — declared design, no action.
6. `docs/testing/VISUAL-MA.3.md:38` records a MINOR brand defect: the light-scheme header logo
   (`docs/brand/logo.svg`, `fill="#fbfaf6"`) is low-contrast. Brand asset, not docs prose → follow-up.

## 4. Release readiness facts

| Fact | Value (command / file) |
|---|---|
| Workspace version | `Cargo.toml:6` → `version = "0.1.1"` |
| Lockfile | all 13 workspace crates at `0.1.1` in `Cargo.lock` (no dependency churn; `1c070e6`-era) |
| Wheel metadata | `dist/blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl` → `Name: blastit`, `Version: 0.1.1`, `Requires-Python: >=3.13`, `Requires-Dist:` none (stdlib-only guarantee holds) |
| Wheel provenance | the docs gate compares the wheel it installs file-by-file with `dist/` and refuses a version that is not `Cargo.toml`'s (`MA.3.10`, `harness.wheel_path`) |
| CHANGELOG | `CHANGELOG.md:7` `## 0.1.1 (2026-11-12)` — four user-visible bullets, no internal trivia |
| Release-tag check | `scripts/check-release-tag.sh v0.1.1` → `tag v0.1.1 names the workspace version 0.1.1` (exit 0) |
| Tags present | only `refs/tags/v0.1.0` — **`v0.1.1` is not tagged**, as required |
| PyPI/TestPyPI path | `RELEASING.md` + `.github/workflows/publish-pypi.yml` (trusted publishing, `pypi` environment with required reviewers) |
| e2e/CI caveat | CI workflows (`docs-gate`, `guards`) have **never executed on GitHub** — no push is allowed, `act` is absent; their proof is a YAML parse plus byte-exact `run:` body execution (MA.3.8) |

### Exact `HUMAN.md` text for the single release sign-off item (paste as-is)

```markdown
## H-00x [open] (node MA)

- asked: 2026-11-13T00:00 by strategist

blastit 0.1.1 is ready at <packet commit>: `make check` and `make e2e` are green, every documented
snippet runs, and `Cargo.toml`/`Cargo.lock`/`CHANGELOG.md`/the wheel METADATA all say 0.1.1.
Agents never push or tag, so please do the three release steps (RELEASING.md has the detail):

1. `git push origin develop` — publishes the docs site (GitHub Pages) and starts the DigitalOcean
   live run.
2. `scripts/check-release-tag.sh v0.1.1` must print `tag v0.1.1 names the workspace version 0.1.1`;
   then `git tag -a v0.1.1 -m "blastit 0.1.1" && git push origin v0.1.1` — that builds the 3.13/3.14
   wheels and the sdist and hands the upload to the `pypi` environment.
3. Approve the `pypi` environment in the *Publish* run; the last job creates the GitHub release
   with the 0.1.1 changelog entry as its notes.

Still open for you, separately: H-002 (`cargo-mutants`, the literal mutation audit) and H-003
(OpenRouter credits for the expensive tier).

**Answer:**
```

## 5. MB starting points (for the `strategist` to paste into `plan/nodes/MB/NODE.md`)

Additive only — nothing here changes MB's goals, versions or exit criteria.

```markdown
## Starting points (written at the MA gate, <date>)

**Begin from what MA proved.** The 0.1.1 harness is the template for every backend:
`blastit dev` (real topology, one uid, loud banner), the stage-0 `execve` barrier with `BLASTIT_EXEC`
pointing at a capability-free copy, per-route `0600` sockets, and `blastit check --learn` on the
wire. A PHP-FPM / Node / Tomcat view is a **new backend kind behind the existing supervisor**, not a
new supervisor: reuse the role split, the socket-per-route rule and the secret barrier before
inventing anything.

**Ordering per sub-release (unchanged, non-negotiable).** One lean page per backend under
`docs/frameworks/` (Local zero-`sudo`, CI/CD, Deployments) → `dx` + `ai-ergonomics` critics →
`docs/design/` + ADR → leaf implementation with protected critical tests → `e2e/` proof. Register
each new page in `python/tests/runnable_docs/inventory.toml` **in the same commit** as the page: the
gate is per-page and unprivileged, so a PHP/Node fence runs against a fixture or is accounted with a
written reason (cloud/registry recipes are the established pattern). A seventh complete
`blastit.toml` in the docs means bumping `docs_fences.rs`'s `count == 6` in the same commit.

**Harness hygiene MA left open — do these in MB's first epic; they protect the gate itself:**

1. `tools/critical_guard.py` needs an enforce/union mode. Today (a) a task node changed in the same
   commit can self-authorise a protected-test edit, (b) one node's `paths` cannot authorise two
   protected files changed under two nodes, and (c) the `guards` loop in `.github/workflows/ci.yml`
   exits 0 on the first authorising node, so an earlier node's `exit 2` is masked.
2. `scripts/build-wheel.sh` should run `scripts/readme-absolute-links.py`, so the wheel's long
   description matches what PyPI renders instead of depending on the publish workflow's rewrite.
3. Spell `CARGO_NET_OFFLINE=true` in any acceptance line that may reach maturin ≥ 1.16 (it rejects
   `1`, which cargo accepts).
4. `e2e/` has no fixture for the documented `queries = []` denying listener, and no permanent
   negative-boot job (`docker run -u 10000 --cap-drop ALL` must exit 1 naming each missing
   capability). Both were one-off evidence in MA (`MA.3.7` / `MA.3.4`); make them jobs.
5. One flake seen once: `python/tests/critical/test_check_contract.py`'s learn-refusal test
   (`server.statements []`), green on rerun. Give it a deterministic sync or a bounded retry before
   it becomes MB's background noise.
6. `blastit dev` with no `./blastit.toml` reports `BLASTIT_APP: the WSGI callable … is required`
   instead of naming the missing file (observed 2026-11-13; `docs/troubleshooting.md` now covers the
   symptom, but the message should name the file). The text is pinned by
   `critical_config_and_presets.rs`, so it is a product task.
7. Do not re-litigate MA's DX: getting started stays ≤ 3 steps, `blastit dev`/`check`/`--learn`
   stay the only spellings, and every new backend gets exactly one page per environment.
```

## 6. What the `strategist` must still do (in order)

1. **Accept or reject this evidence** — optionally re-run `make check` and `make e2e` at the packet
   commit (both are unprivileged and take ~8 min and ~1 min respectively on this host).
2. **Decide the `cargo-mutants` deferral** (H-002): either accept the anchored fallback (manifest
   8/8, sqlpolicy 12/12, residual risk stated in §2) as sufficient for `0.1.1` and leave the tool
   for `0.2.x`, or keep `MA` open/blocked until the owner installs it. This is a policy decision on
   `plan/ROADMAP.md`'s standing policy; a cheap lead cannot make it.
3. **Mark `MA` done** once (1) and (2) are settled: `plan.py --agent strategist set MA done`, then
   `plan.py state`. Do not write a second RESULT — this draft becomes the accepted one by editing
   its first line.
4. **Open the single release sign-off item** with the exact text in §4 (it is the only remaining
   FAIL in the roadmap table). Do not push or tag.
5. **Green-light MB**: paste §5 into `plan/nodes/MB/NODE.md` (body only), then
   `plan.py next --under MB` for the first decompose.
6. **Close or re-scope H-003** (§4's item also mentions H-002): with credits restored, run the gate
   yourself; without them, record the deferral on `MA` so the next session does not loop.

## Files

- `docs/troubleshooting.md` (2 rows: real production-refusal symptom, missing-manifest symptom)
- `plan/nodes/MA/RESULT.md` (this draft packet)
- `plan/nodes/MA/LOG.md` (packet log line)

## Follow-ups

- Product: make `blastit dev` name the missing `./blastit.toml` (§3 defect 3) — pinned message, new
  node under MB.
- Brand: light-scheme header logo contrast (`docs/testing/VISUAL-MA.3.md:38`).
- Harness: the six hygiene items in §5 belong to MB's first epic.
- CI: the `docs-gate` and `guards` jobs have never run on GitHub; the first push is their first real
  test.
