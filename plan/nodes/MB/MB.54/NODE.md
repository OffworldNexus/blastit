---
id: MB.54
title: "Write TESTING_PLAN.md: user-story-driven E2E testing plan & drastic test/code pruning blueprint"
kind: task
status: done
owner: architect
priority: 1
depends_on: []
paths: [TESTING_PLAN.md]
attempts: 0
max_attempts: 3
escalation: architect
created: "2026-11-20T13:23"
updated: "2026-11-20T13:32"
---

# Goal
Write `TESTING_PLAN.md` at the repository root for owner review: a crisp, concrete, user-story-driven E2E testing plan and drastic test + production/script code reduction blueprint.

# Required Structure & Principles
1. **Current Baseline (Measured Facts, No Fluff):**
   - Present the exact current footprint: ~`106.7k` prod LOC (`100.3k` Rust + `6.3k` Python + `18.9k` `scripts/`) vs ~`164.9k` test/meta-test LOC (`81.6k` `crates/*/tests/*.rs` across 127 files, `45.3k` inline `#[cfg(test)]`, `18.1k` `python/tests/`, `10.0k` `e2e/`, `9.7k` `scripts/` gate/workflow meta-tests).
   - Explain plainly why this bloated: per-slice micro-tests, prose/substring assertions, duplicate unit + integration + e2e coverage of the same paths, and "tests of tests" (`check_workflows.py` 2.2k LOC, `test_packaging_gates.py` 2.8k LOC, `test_docs_assets.py` 2.2k LOC, `runnable_docs/harness.py` 3.9k LOC).
2. **Core Testing Rules Going Forward:**
   - **User-Story E2E Priority:** Every test must trace directly to a user story below and exercise the real `blastit` / `blastit-lsp` binary end-to-end. No test may exist if it doesn't protect a user story.
   - **Unit-Test Exception Rule (Strict):** Do NOT write unit tests unless a specific pure function/algorithm is genuinely impractical to cover combinatorially via E2E (specifically: SQL AST/policy edge cases in `blastit-sqlpolicy`, `Blastfile` grammar/formatter corner cases in `blastit-manifest`, HTTP wire-smuggling edge cases in `blastit-http`, and malformed binary ELF/wire frames in `blastit-sys`/`blastit-frame`). Zero getter/setter/wiring/prose-matching unit tests!
   - **Zero Meta-Tests:** Delete regex/AST linters over GitHub Actions YAML, packaging scripts, and Markdown prose (`check_workflows.py`, `test_packaging_gates.py`, `test_docs_assets.py`, etc.) — real CI jobs building & running the real packages ARE the test.
3. **Complete User Stories (`US-1`..`US-5`) & Their Exact E2E Verification:**
   - List every user story concisely with **(a) User action**, **(b) Expected outcome + security/refusal boundary**, and **(c) The ONE E2E test suite that owns it**:
     * **US-1: Installation, Packaging & CLI Handoff** (`.deb`/APT, `.rpm`/DNF, `install.sh --user`/Alpine `.tar.gz`, `py3-none` wheel on Python 3.12–3.14, Composer wrapper, local CLI handoff `.venv`/`vendor`/`node_modules` + unsafe-owner refusal)
     * **US-2: Local Development (`blastit dev`)** (Python WSGI 3.13/3.14 + Postgres grants/denials, PHP 8.3–8.6 + Postgres grants/denials + `php2shell` surface absence, Static & SPA mounts + path confinement, `fanotify` live reload, `--learn` & `--dry-run`)
     * **US-3: CI Completeness Audit & Formatting (`blastit check` & `blastit fmt`)** (clean pass exit `0`, missing/dead grant + unrouted + unexercised + body findings exit `1` in human & `--json`, static-only & store-less `check`, `check --learn`, `blastit fmt` & `fmt --check`)
     * **US-4: Editor Experience (`blastit lsp` / `blastit-lsp`)** (stdio LSP session: diagnostics, hover, formatting, `$?` SQL code action)
     * **US-5: Production Zero-Trust Sandboxing (`blastit` root & rootless)** (root `--preset digitalocean-paas` per-view UIDs/seccomp/`PR_SET_DUMPABLE`/secret-stripping/fair-queuing/graceful `SIGTERM`, rootless `--preset compose-traefik` file-caps `blastit-init` + zero child caps, fail-closed boot on unprivileged caller / writable paths / inode swap / `body raw` without `unvalidated`)
4. **Drastic Pruning & Consolidation Plan (Tests + Production/Scripts Code):**
   - Concrete table of **Test Suites to Keep / Merge / Delete** with current LOC -> target LOC (targeting ~`165k` -> ~`20–25k` test LOC, an **~85% reduction**!).
   - Concrete list of **Production & Script Code to Cut/Simplify** too (e.g., retiring legacy 0.1.0 `--blastit-manifest` `pytest_plugin.py` if superseded by `blastit check`, retiring standalone `fold.py` / `blastit-manifest` Python CLI if superseded by `blastit fmt` + `blastit --learn`, trimming `scripts/` from 43 scripts / 19k LOC down to ~8 essential build/install scripts, and stripping essay-length comments that restate plan node IDs).
   - Proposed execution order once approved by the owner (including retiring `tools/critical_guard.py` or rescoping it to the new consolidated user-story E2E suites).

# Acceptance
- `TESTING_PLAN.md` written clearly, scannably, and concisely (<350 lines) with real measured LOC numbers from this repo.
