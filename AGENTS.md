# BLASTIT — Engineering & DX Charter

Blastit is a developer-first zero-trust application harness and sandbox for modern 12-factor web apps (Python WSGI, embedded NTS PHP, static sites, Node/JVM views). It isolates each `view` in its own process, enforces inbound codecs, keeps real credentials behind stage-0 capability proxies (`postgres`, `redis`, outgoing `http`), and verifies completeness in CI via `blastit check`.

## Core Principles (Inlined — Act Directly Without Ceremony)

1. **Zero Boot Ceremony:** Leaf agents (`implementer`, `writer`, `review-arch`, `review-ux`, `planner`) act directly without wasting tool calls loading skills, snooping `.opencode/`, or running `ls`/`find`. Read `NODE.md` (when assigned a plan node), `docs/knowledge/LESSONS.md`, and the target workspace files directly in 1–2 batched steps. Orchestrators (`conductor`, `lead`, `strategist`) read `docs/VISION.md` and `plan/STATE.md` (`uv run --no-project tools/plan.py state`).
2. **True Constructor IoC & Cohesive Architecture:**
   - **Single Composition Root:** Read `std::env` / `os.environ` ONLY at top-level CLI/stage-0 boot (`BootMode::{Production, Dev, Check}`), then pass typed dependencies through constructors.
   - **Zero Globals & Zero Mock-Trait Ceremony:** Never introduce `OnceLock<Mutex<...>>`, `static Mutex`, hidden `env::var`, `#[cfg(any(test, debug_assertions))]` env-var hooks, or single-implementation mock traits (`Acceptor`, `RequestIo`, `Clock`, `Editor`). Inject concrete collaborators or closures/channels.
   - **Cohesion & Reuse:** Keep files `<500` prod LOC, structs cohesive (nested `#[derive(Deserialize)]` configs, never 30+ field flat `Config` bags or hand-rolled `json_*` accessors), and functions `<=6` args. Search the workspace BEFORE writing helpers: extend the canonical owner module (`sqlpolicy`, `elf`, Rust analyzer FFI) instead of copy-pasting enums, lexers, or audit schemas; delete dead fallbacks (retired TOML manifests, `inotify`, dead legacy schedulers).
   - **Protocol Crate Boundaries:** Isolate language runtimes as separate crates (`blastit-view-*`) speaking `view.chan` wire frames with per-flow CPU time reporting — never cross-language `WorkerRuntime` mega-traits.
3. **Lab-First Tests & Security Walls:**
   - Test real components over `UnixStream::pair()` / `UnixListener` / wire frames — never `MockPgServer`, `StubPolicy`, or `AlwaysAllowPolicy`.
   - Always test BOTH allowed (including table/column parameterized `$1` grants and 0-proxy store-less apps) AND denied/adversarial paths (SQLi, ungranted columns, `php2shell` `%ADd`, path traversal `..`/`.env`), verifying zero secret/password leaks in error text and zero `unwrap()`/`expect()` on untrusted inputs. Production `blastit` fails closed without privileges; `blastit dev` runs explicit local single-UID simulation.
   - Enforce invariants with crate/type boundaries, NEVER 400-line `scripts/check_*.py` source-grep regex guards.
4. **CLI Terminal Craft & Visual Documentation:**
   - **CLI:** Hide internal `--stage=1`/`--role=dbproxy`/`--config-fd` flags from `--help`; support `--version` and `help`; reject unknown `--preset` naming valid presets (`digitalocean-paas`, `compose-traefik`); exit `2` on CLI usage errors; honour `NO_COLOR` and `TERM=dumb` using `std::io::IsTerminal` per stream (`stdout` vs `stderr`).
   - **Live UX & Reports:** Print clickable `http://host:port` (`http://[::1]:port` for IPv6), `--learn` badge, route table, phase lines before waits, fixed-width aligned event columns, and clean stderr boot refusals. Human `blastit check` must print dead-grant `sql`, `proxy`, and `fix.to` (`view <name> postgres`) matching `--json`, using `view`/`trait` nouns and proper singular/plural (`1 request`). `fmt --check` shows a diff; `--learn --dry-run` shows the proposed `Blastfile` snippet and never learns `..`/dotfile scanner probes.
   - **Docs (<130 lines/page):** Lead immediately with 3 zero-`sudo` runnable steps (put caveats/platform matrices in `!!! warning` or collapsed `??? note`, NEVER before command 1!). Include a Mermaid `flowchart` or `sequenceDiagram` on every guide/spec, runnable non-empty `Blastfile` grants, forward navigation links, factual support statements (`8.3, 8.4, 8.5` NTS), and ZERO diary headings (`What we thought...`), roadmap excuses, or host-package apologies.
5. **Pre-1.0 Minimalism & Distribution Purity:**
   - `Blastfile` has no `version = 2`, declares `entrypoint` once, nests grants inside each `view`, uses `"/*"` / `regex '...'` / bare `immutable` (zero 6-glob staircases), and embeds NTS `libphp` automatically (`BLASTIT_LIBPHP` override only; zero `php-fpm`, FastCGI, `listen =`, or `workers =`).
   - Standalone `blastit` and `blastit-lsp` link zero `libpython` or `uv` `RUNPATH`; editor extensions resolve `blastit-lsp` via settings -> `PATH` -> GitHub release asset, never `uv`.

## Mandatory Repository & Harness Rules

1. **Plan & Scope Discipline (`plan/`):** All autonomous work flows through `plan/nodes/`. Mutate plan state **only** via `uv run --no-project tools/plan.py --agent <role> ...` — never hand-edit managed frontmatter (`status`, `attempts`, `updated`). Touch only the globs in your node's `paths`, and record out-of-scope discoveries under `## Follow-ups` in `RESULT.md`.
2. **1 Sequential Cargo Writer & Gate Split (`fast-check` vs `check`):**
   - Run **at most 1** Cargo/wheel/Docker writer at a time per worktree (never run parallel builds touching `target/` or `Cargo.lock`).
   - Leaf slices verify with `make fast-check` (or `make docs-check` for doc-only tasks) and record brief command + output-tail evidence in `RESULT.md`.
   - Full `make check` (including wheel build, runnable-docs harness, standalone ELF matrix, and Zensical build) runs **once** at epic/release gates — never 3x per leaf.
3. **Protected Critical Tests (`tools/critical_guard.py`):** Existing files in `crates/*/tests/critical_*.rs` and `python/tests/critical/` are protected by `uv run --no-project tools/critical_guard.py --base <sha> --node <id>`. Adding new critical tests is always welcome; never modify, delete, or skip (`#[ignore]`, `skip`) existing critical tests unless explicitly authorized in your node's `paths` and `# Goal`.
4. **Python & Rust Execution (`uv` + `Makefile`):** Never invoke bare `python`, `python3`, or `pip`. Use `uv run --no-project` for standalone scripts (`tools/`, `scripts/`, `e2e/`), `uvx --offline` for cached CLI tools (`ruff`, `zensical`), and `make` targets (which export `PYO3_PYTHON`, `PYTHONHOME`, and `LD_LIBRARY_PATH` automatically). Published `python/blastit/` stays 100% stdlib (`dependencies = []`).
5. **Git & Release PR Workflow:** Work and commit locally on `release/<version>` branches (`lead` and `conductor` only, staging explicit file paths — never `git add -A`). Leaf subagents never run `git add`, `git commit`, `git push`, `git tag`, or `gh pr`. When a release gate passes, `conductor` pushes `release/<version>` and opens a PR into `develop` for human merge. Never push to `develop`, never force-push, never run `git tag`, and never run `gh pr merge`.
6. **Autonomy & Confidentiality:** Never ask the human directly unless you are `conductor` and every branch is blocked (open an item via `uv run --no-project tools/plan.py --agent <you> human add "..."` instead). **Never mention external client, proprietary, or sibling private project names** anywhere in code, docs, comments, or commit messages.

## Repository Map

| Path | What |
|---|---|
| `crates/` | Rust workspace (`blastit`, `blastit-http`, `blastit-scheduler`, `blastit-dbproxy`, `blastit-sqlpolicy`, `blastit-manifest`, `blastit-lsp`, `blastit-env`, `blastit-frame`, `blastit-events`, `blastit-view`, `blastit-view-php`, `blastit-view-static`, `blastit-php-sapi`, `blastit-sys`, `blastit-py`) |
| `python/` | Zero-dependency Python CLI/runtime package (`blastit`) + unit, critical & runnable-docs suites |
| `docs/` & `README.md` | Lean public user documentation site (`zensical.toml` -> `make docs-check`) |
| `ARCHITECTURE.md` | Technical specification of internals & trust boundaries (`docs/{VISION.md,design/,decisions/,research/,knowledge/LESSONS.md,testing/}` excluded from public build) |
| `editors/` | Tree-sitter `Blastfile` grammar & Zed extension (`make zed-check`) |
| `e2e/` & `scripts/` | Docker/rootless/PHP-SAPI/CVE harnesses and packaging/ELF/workflow build scripts |
| `plan/` & `tools/` | Autonomous plan tree (`STATE.md`, `ROADMAP.md`, `nodes/`) + stdlib tools (`plan.py`, `guard.py`, `critical_guard.py`) |
