# BLASTIT: rules for every agent

Blastit is a **developer-first zero-trust application harness and sandbox** for modern 12-factor web applications. It splits an application into isolated per-route sandboxes, validates and re-encodes every inbound byte inside budgeted WASM VMs, keeps real credentials out of application memory, and mediates outgoing traffic through protocol-aware capability proxies (PostgreSQL, MySQL/MariaDB, MongoDB, Redis/Valkey, and secret-injecting HTTP).

The project runs autonomously: a cheap model (`deepseek-v4.1-flash`) does nearly all the work, and an expensive model (`gemini-4-carbon`) is called only at gates, core architecture crossroads, or hard escalations.

**Read these before doing anything:**

1. `docs/VISION.md`: what we build, who we build it for, and why **everything stems from the developer experience**.
2. `plan/STATE.md`: where we are (`uv run --no-project tools/plan.py state`).
3. The `NODE.md` of the plan node you were assigned, plus `docs/knowledge/LESSONS.md`.

## Non-negotiable rules

1. **Developer Experience First (Docs Before Design, Design Before Code).** Nothing gets designed internally or coded in Rust/Python until the **lean user-facing documentation** is written and critiqued first — showing exact, foolproof steps on **(a) the local developer machine** (zero `sudo`!), **(b) CI/CD** (completeness checks proving nothing in `blastit.toml` was forgotten), and **(c) deployments** (12-factor presets that fail closed). Load skill `dx-first`.
2. **Lean prose & minimal config.** Today's developer closes bloated docs. Keep user pages short, concrete, and free of kernel-internals lectures (save OS/WASM internals for `ARCHITECTURE.md` and `docs/security.md`). Target modern, tested, 12-factor frameworks (Django, Flask/SQLAlchemy, Symfony/Laravel PSR, Vite/Node, Tomcat/Spring Boot) — never legacy monoliths like WordPress.
3. **Human + AI manifest workflow.** Both a human and an AI agent must be able to populate and verify `blastit.toml` in a single pass (`--learn` wire observation across any backend language + strict CI audit with `--json` output) without playing whack-a-mole on one denied query at a time.
4. **The plan lives on disk.** All work flows through `plan/nodes/`. Mutate the plan **only** via `uv run --no-project tools/plan.py ...` (skill `plan-system`). Never hand-edit managed frontmatter fields (`status`, `attempts`, `updated`).
5. **Stay in your lane.** Touch only the globs in your node's `paths`. Record out-of-scope discoveries under `## Follow-ups` in `RESULT.md`.
6. **Evidence or it didn't happen.** Run every `# Acceptance` command and `make check` yourself and paste command + output tails into `RESULT.md`.
7. **Never ask the human directly** unless you are `conductor` and every branch is blocked. Open a request via `uv run --no-project tools/plan.py --agent <you> human add "..."` and continue other work.
8. **Git & Release PR workflow:** Work and commit locally on `release/<version>` branches (`lead` and `conductor` only; skill `git-workflow`) — never force work onto `develop`. Subagents never run `git push`, `git tag`, or `gh pr`. Whenever a release (`0.1.1`, `0.2.x`, `0.3.x`, `0.4.x`) passes its gate, `conductor` pushes `release/<version>` and opens a PR into `develop`, then pauses for a human to review and merge it — merging into `develop` validates CI + Docs + Live e2e and triggers the automated PyPI + GitHub Release pipeline. Never push to `develop`, never force-push, never run `git tag`, and never run `gh pr merge`.
9. **Python & Rust execution:** Never invoke bare `python`, `python3`, or `pip`. Use `uv run --no-project` for scripts (`tools/`), `uvx --offline` for cached CLI tools (`ruff`, `zensical`), and `make` targets (which set `PYO3_PYTHON` and `LD_LIBRARY_PATH` automatically). Published `python/blastit/` stays 100% stdlib (`dependencies = []`).
10. **Security & CVE bar:** Load skill `security-invariants` before touching `crates/` or security docs. Production `blastit` fails closed without privileges; `blastit dev` runs explicit local single-UID simulation. Every backend/ingress milestone proves real CVE classes (`php2shell`, `node2shell`, polyglot media/PDFs, smuggling) are neutralized even on unpatched runtimes.
11. **Confidentiality:** Never mention external client, proprietary, or sibling private project names anywhere in code, docs, comments, or commit messages.
12. **Protected critical tests:** Files in `crates/*/tests/critical_*.rs` and `python/tests/critical/` are guarded by `tools/critical_guard.py`. Never edit or weaken them unless explicitly listed in your node's `paths`.

## Where things are

| Path | What |
|---|---|
| `plan/` | Autonomous plan system: `ROADMAP.md`, `STATE.md`, `HUMAN.md`, `ESCALATIONS.md`, `JOURNAL.md`, `BUDGET.json`, `nodes/` |
| `docs/VISION.md` | North Star vision, DX-first law, scope across Milestones A–D (excluded from public Zensical build) |
| `docs/` & `README.md` | Lean public user documentation site (`zensical.toml` -> `make docs-check`) |
| `ARCHITECTURE.md` | Living technical specification of Blastit's internals and trust boundaries |
| `docs/design/` | Technical design docs & DX blueprints + goldfish `*.review-<lens>.md` files (excluded from public site) |
| `docs/decisions/` | Architecture Decision Records (`ADR-NNNN-<slug>.md`, excluded from public site) |
| `docs/research/` | Cited protocol, framework, WASM codec & CVE research notes (excluded from public site) |
| `docs/knowledge/` | `LESSONS.md` — non-obvious gotchas & host/tooling facts (excluded from public site) |
| `docs/testing/` | `test-curator` audit reports (`AUDIT-*.md`, excluded from public site) |
| `crates/` | Rust workspace crates (`blastit`, `blastit-http`, `blastit-scheduler`, `blastit-dbproxy`, `blastit-sqlpolicy`, `blastit-manifest`, `blastit-env`, `blastit-frame`, `blastit-events`, `blastit-view`, `blastit-sys`, `blastit-py`, plus upcoming ingress/WASM/backend/proxy crates) |
| `python/` | Published zero-dependency Python package (`blastit`) + test suites |
| `e2e/` | End-to-end Docker, rootless-capability, multi-backend, and CVE regression harnesses |
| `tools/` | Stdlib-only harness tools: `plan.py`, `guard.py`, `critical_guard.py` |

## Skills index (load with `skill`)

| Skill | Load when |
|---|---|
| `dx-first` | Planning, writing docs, designing UX/CLI/manifest, or reviewing any change |
| `plan-system` | Reading or mutating `plan/` via `tools/plan.py` |
| `dispatch` | Spawning subagents, writing briefs/reports, or escalating |
| `design-docs` | Writing or critiquing DX specs, design docs, or ADRs |
| `security-invariants` | Designing, coding, or reviewing Rust crates, WASM gates, proxies, privileges, or CVE tests |
| `rust-standards` | Writing or reviewing Rust (`crates/*`) |
| `python-standards` | Writing or reviewing Python (`python/`, `tools/`, `e2e/`) |
| `testing` | Writing tests, reviewing test diffs, or running `test-curator` audits |
| `research` | Investigating protocols, frameworks, WASM libraries/licenses, or CVEs |
| `git-workflow` | Staging and committing completed plan nodes |
