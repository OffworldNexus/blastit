---
id: MB
title: "Modern 12-Factor Backends: Static, PHP-FPM, Vite/Node, Tomcat (0.2.x)"
kind: milestone
status: ready
priority: 2
depends_on: [MA]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-12T00:50"
---

# Goal

Expand Blastit across focused `0.2.x` sub-releases into a **multi-backend zero-trust application harness** supporting modern 12-factor ecosystems beyond Python WSGI:
1. Safe built-in **static asset serving** and clean front-controller / SPA **URL rewriting** (no nginx regex soup).
2. **PHP-FPM** (`FastCGI` protocol, PSR-7/15 front controllers, Symfony & Laravel idioms, locked-down `SCRIPT_FILENAME` & FastCGI params).
3. **Vite & Node** (HTTP upstream per route/worker, seamless Vite asset + SSR split for SvelteKit, Remix/React Router, Nuxt, Hono, Next standalone).
4. **Tomcat / JVM** (modern 12-factor Servlet / Spring Boot integration) and other evaluated popular 12-factor HTTP backends.
5. Unified language-agnostic `blastit dev --learn` and `blastit check` workflows across Python, PHP, Node, and JVM, plus baseline `php2shell` and `node2shell` exploit neutralization tests.

# Context

- Vision & rules: `docs/VISION.md`, skill `dx-first`, skill `security-invariants`.
- **Mandatory DX-First law:** Before designing internals or coding any sub-release (`0.2.0`, `0.2.1`, `0.2.2`...), **start by writing the lean user documentation** showing how that backend looks on the **local machine** (zero `sudo`), in **CI/CD** (completeness verification), and in **deployments** — critiqued by `dx`, `ai-ergonomics`, and `security` goldfish critics!
- Modern 12-factor scope only: play natively with PSR, modern frameworks, static assets, and URL rewriting, without supporting legacy un-12-factorizable monoliths like WordPress.

# Acceptance

- [ ] Each `0.2.x` sub-epic begins with accepted lean user documentation (`docs/frameworks/...`) covering Local, CI/CD, and Deployments, verified by `dx` and `ai-ergonomics` critics before Rust code is written.
- [ ] Static asset serving + URL rewriting handle SPA fallbacks, hashed immutable assets, and PHP `public/index.php` front-controller routing safely (immune to path traversal, null bytes, symlink escapes, and dotfile leaks).
- [ ] PHP-FPM FastCGI backend serves real PSR / Symfony- or Laravel-style routes isolated per route, strips real DB secrets via stage-0 `execve`, enforces `blastit.toml` PostgreSQL grants, and blocks `php2shell` / CVE-2024-4577 FastCGI/CGI argument & `PHP_VALUE` injection attacks in automated `e2e/` tests.
- [ ] Vite + Node SSR/API backend serves static + dynamic routes cleanly in `blastit dev` and production containers, enforces per-route DB grants & egress lockdown, and blocks `node2shell` reverse-shell / env / `/@fs/` traversal vectors in `e2e/`.
- [ ] Tomcat / JVM 12-factor backend runs cleanly under Blastit with per-route sandboxing/routing, env secret swapping, and wire-level `--learn` + `blastit check`.
- [ ] `make check`, `make docs-check`, `visual-checker`, `make e2e`, and `test-curator` milestone audit all pass.

# Notes

- May be shipped as multiple `0.2.x` sub-releases (`0.2.0`, `0.2.1`, `0.2.2`, `0.2.3`); `strategist` refreshes concrete pointers here at the `MA` gate.

## Starting points (written at the MA gate, 2026-11-13)

**Begin from what MA (`0.1.1`, incl. `MA.6` hot-path merge) proved.** The `0.1.1` runtime is the template for every backend:
- `blastit dev` (real 5-role topology under `geteuid()`, loopback-only unless `--host` + `BLASTIT_DEV_EXPOSE=1`, loud 3-line banner),
- the stage-0 `execve` secret barrier with `BLASTIT_EXEC` pointing at a capability-free binary copy,
- per-route `0600` listeners in private view directories,
- wire-level `--learn` + `blastit check` (`--json` v1 schema), and
- `MA.6`'s zero-copy `ReqHeadView` / `RawFrame` forwarding, 64-frame / 256 KiB channel batching (`chan.rs`), lazy 64 KiB response credit (`WINDOW_LOW_WATER`), and pooled view threads (`blastit-view`).

A Static, PHP-FPM (`FastCGI`), Vite/Node (HTTP upstream), or Tomcat/JVM view is a **new backend kind behind the existing supervisor and `RawFrame` scheduler channel**, not a second supervisor: reuse the role split, UID layout (`UID_BASE + 1000 + i`), seccomp baseline, per-route socket rule, and secret barrier before inventing anything.

**Mandatory DX-First sequence per sub-release (non-negotiable).**
1. One lean user page per backend under `docs/frameworks/` covering Local (zero `sudo`), CI/CD (`blastit check`), and Deployments → `dx` + `ai-ergonomics` goldfish critics.
2. Register every new public doc page in `python/tests/runnable_docs/inventory.toml` **in the same commit** (and bump `crates/blastit-manifest/tests/docs_fences.rs`'s `count == 6` whenever a complete `blastit.toml` fence is added).
3. Technical design (`docs/design/*.md`) + ADR → `security` + `implementability` + `testability` critics.
4. Small leaf tasks (≤ 5–8 files, ≤ 600 lines) bundling their own `crates/*/tests/critical_*.rs` tests. **MA escalation lesson (`MA.2.3.3`):** never bundle process-lifecycle/cleanup logic and `--learn` wire attribution into the same leaf task — split them so cheap passes succeed on attempt 1.
5. `e2e/` + baseline CVE exploit regression (`php2shell` CVE-2024-4577 / `PHP_VALUE` / `SCRIPT_FILENAME`; `node2shell` egress/env/`/@fs/`).

**MA lessons to keep in mind:**
- Clean `git merge-tree` auto-merges can still break semantics when helper signatures move (`blastit-events/src/json.rs` in `MA.6`): read every file touched by both sides whole.
- Local wheel rebuilds (`scripts/build-wheel.sh`) require `uv cache clean blastit --force` or a per-run `UV_CACHE_DIR` so `uv` never tests a cached `_blastit*.so`.
- Keep `/tmp` clean during heavy doctest or `cargo-mutants` runs (a full `/tmp` tmpfs surfaces as `ld terminated with signal 7 [Bus error]`).
- Multi-node `tools/critical_guard.py` (`--node` repeatable) and the single-pass union check in `.github/workflows/ci.yml` (`guards`) landed in `70a4357` — **done**, do not re-open.

**Queued hygiene, diagnostics, perf & mutation gaps — execute first in `MB.1` before `0.2.0` feature code:**
1. **Mutation & timeout gap (`MB.1`):** lift whole-crate `blastit-manifest` from 69.4 % (437/630 viable; policy modules already 95.0 %) past 75 % by testing `edit.rs` `verify()` (`:961`) and comment/bracket patcher branches; run the unreached 511/1019 `blastit-sqlpolicy` mutants (`lexer.rs` tail, `literals.rs`, `lru.rs`, `normalize.rs`, `statements.rs`, `tables.rs`) via file-scoped targets; and eliminate or bound the cursor-progress infinite-loop mutants behind the 20 timeouts (11 in `blastit-manifest`, 9 in `blastit-sqlpolicy`).
2. **Missing `./blastit.toml` startup message (`crates/blastit/src/stage0.rs:444,666`):** deferred from `MA` (`docs/troubleshooting.md:13` covers `0.1.1`) — check manifest existence/readability before `config.app.is_none()` so running `blastit dev` in an empty directory names the missing `./blastit.toml` rather than `BLASTIT_APP`, and pin in `critical_config_and_presets.rs`.
3. **`scripts/build-wheel.sh` README links:** run `scripts/readme-absolute-links.py` (restoring `README.md` via trap) during local wheel builds so `dist/*.whl` long description matches PyPI's build.
4. **`CARGO_NET_OFFLINE=true` spelling:** replace `CARGO_NET_OFFLINE=1` in `Makefile` / scripts / task acceptances that can reach `maturin >= 1.16` (maturin parses it as strict `"true"`/`"false"`).
5. **Permanent `e2e/` negative & empty-grant jobs:** add an `e2e/` route fixture exercising `queries = []` (`42501` denying listener) and a permanent negative-boot check (`docker run -u 10000 --cap-drop ALL` exits `1` naming missing capabilities).
6. **Learn-refusal contract sync:** eliminate the rare timing race in `python/tests/critical/test_check_contract.py`'s learn-refusal test (`server.statements` drain before exit).
7. **Hot-path `Scheduler::plan_request_body` (`crates/blastit-scheduler/src/scheduler.rs`):** return `Ok(BodyPlan::AllowEmptyOnly)` as soon as `policy.is_none()` instead of allocating method + 2 header `String`s first (measure when bringing in the `benchmark/` harness).
8. **`crates/blastit-events/Cargo.toml` unused `serde` dependency:** drop the unused `serde` dep in a dedicated sequential `Cargo.toml`/`Cargo.lock` task.
9. **Light-scheme header logo contrast (`docs/brand/logo.svg`):** address `VISUAL-MA.3.md` minor contrast note (`fill="#fbfaf6"` on light `#f4efe3` header).
