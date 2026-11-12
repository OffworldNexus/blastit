---
name: DX-First (Developer Experience & Docs Before Code)
description: MANDATORY for every planner, doc-writer, designer, architect, reviewer, critic, lead and strategist in Blastit. Everything in Blastit stems from the developer experience: lean user documentation for Local, CI/CD and Deployments is written and critiqued BEFORE any internal design or code.
---

# DX-First: Everything Stems From Developer Experience

Blastit succeeds only if adopting it feels **effortless, foolproof, and lean** — both for a human developer at 5 pm and for an AI coding agent maintaining the project.

## 1. The Iron Law: Docs Before Design, Design Before Code

Before writing any internal Rust/Python architecture or production code for a feature, backend, ingress validator, or outgoing proxy:

1. **Write the user-facing documentation FIRST** (`docs/`), showing **exact, runnable commands and files** in all three environments:
   - **On the local developer machine:** Zero `sudo`, zero manual `chmod` on home directories, zero `setcap` ceremony required for daily coding. Show how the developer starts `blastit dev` (unprivileged single-UID simulation that runs the real process topology, `execve` secret swap, ingress WASM validators, and egress brokers so any misconfiguration fails loudly on `localhost` for an honest program), and how optional real-capability local testing works when desired.
   - **In CI/CD:** One fast, unprivileged, deterministic command (`blastit check` / `blastit test`) that runs the app's test suite through Blastit's brokers/validators and fails if **any** route is unexercised, **any** SQL query / Redis key / Mongo op / HTTP call is undeclared, **any** grant is dead, or **any** route lacks expected input rules.
   - **Across deployment scenarios:** Foolproof presets (`digitalocean-paas`, `compose-traefik`, plus clean 12-factor container/systemd recipes) where production `blastit` **fails closed** if real OS isolation (root or capabilities + seccomp) is missing.
2. **Critique the DX with fresh eyes** (`critic` subagents with the `dx` and `ai-ergonomics` lenses). Every extra step, redundant environment variable, manual copy-paste loop, or paragraph of kernel trivia is a defect.
3. **Only after the DX spec is accepted**, write the technical design (`docs/design/<slug>.md`) and implement the code so the documented commands work verbatim.

## 2. Target Audience: Modern 12-Factor Frameworks Only

- **We do NOT support legacy un-12-factorizable monoliths** (such as WordPress, FTP-mutated docroots, or untested spaghetti). Never complicate the manifest or CLI to accommodate them.
- **We DO play natively with modern, tested, 12-factor frameworks and their idioms:**
  - **Python (WSGI):** Django, Flask, SQLAlchemy, psycopg 3 — zero-boilerplate connection/parameter diagnostics.
  - **PHP (PHP-FPM / FastCGI):** Symfony, Laravel, Slim, PSR-7 / PSR-15 front controllers (`public/index.php`), clean URL rewriting without nginx regex soup, safe static asset serving.
  - **Node & Frontend (Vite / HTTP upstream):** Vite dev/preview & static assets, SvelteKit, React Router / Remix, Nuxt, Hono, Next standalone — seamless asset + SSR split.
  - **JVM (Tomcat / Servlet / Spring Boot):** 12-factor embedded or standalone Tomcat / Spring Boot apps talking over local sockets/loopback, JDBC/Redis/HTTP proxying without changing app code.

## 3. Human + AI Manifest Ergonomics (`blastit.toml`)

Every manifest feature must pass two tests:

1. **The Human Test:** Can a developer read `blastit.toml` in 30 seconds and immediately see what each route can touch? Are defaults smart enough that `./blastit.toml` is discovered automatically and common wiring needs minimal lines?
2. **The AI Agent Test:** How does an developer OR an AI agent fill in `blastit.toml` and **know for sure nothing was forgotten**?
   - **Single-pass wire learning (`--learn` / `--record`):** Because Blastit owns the ingress gate and every outgoing proxy (`postgresql`, `mysql`, `mongodb`, `redis`, `http`), it observes every route hit, header, payload shape, and outgoing query/call **on the wire** across *every* backend language (Python, PHP, Node, Java) — without needing per-language monkeypatching! In `--learn` mode, Blastit logs-and-allows instead of aborting on the first denial, so one run of the test suite captures *every* statement and call across the entire app in one pass.
   - **Strict completeness verification:** CI mode checks 4-way coverage:
     1. Every observed request matched an intended route (no accidental catch-all drift).
     2. Every observed outgoing operation is granted (zero missing queries/endpoints).
     3. Every route and grant in `blastit.toml` was actually exercised by tests (or explicitly marked), catching dead grants and untested edge cases.
     4. Every route accepting bodies/uploads declares explicit ingress validation (or explicit opt-out, flagged loudly).
   - **Actionable diagnostics:** Both pretty human output (naming the exact config line or framework setting to change) and `--json` output (structured for AI agents to apply cleanly in one edit).

## 4. Ruthless Documentation Leanness Rules

Today's developer stops reading the moment documentation lectures them. Enforce these rules on every page under `docs/` and `README.md`:

- **Lead with the working 10-line example.** Never open a guide with caveats, kernel internals, `sys.path` edge cases, or gVisor war stories. Put architecture and OS mechanics in `how-it-works.md`, `security.md`, and `ARCHITECTURE.md` — nowhere else.
- **Count the steps.** Getting started must take **≤ 3 steps** and **Zero `sudo`** (`uv add blastit` → 6-line `blastit.toml` → `uv run blastit dev`).
- **One canonical way per task.** Don't show 4 ways to pass environment variables in a tutorial; show the cleanest one and link to `reference/` for overrides.
- **Short sentences, active voice, scannable tables.** Cut every paragraph that doesn't change what the reader types or decides. Aim for user guide pages under 150 lines.
- **Every snippet is tested.** Commands, `blastit.toml` blocks, Compose files, and CI snippets in `docs/` must actually parse and run.
