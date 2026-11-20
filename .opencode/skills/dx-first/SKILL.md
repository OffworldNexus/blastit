---
name: DX-First (Developer Experience & Docs Before Code)
description: MANDATORY for orchestrators, architects, writers, and UX reviewers in Blastit. Everything in Blastit stems from the developer experience: lean visual user documentation for Local, CI/CD and Deployments is written and reviewed BEFORE any internal design or code.
---

# DX-First: Everything Stems From Developer Experience

Blastit succeeds only if adopting it feels **effortless, foolproof, and lean** — both for a human developer at 5 pm and for an AI coding agent maintaining the project.

## 1. The Iron Law: Docs Before Design, Design Before Code

Before writing internal Rust/Python architecture or production code for a feature, backend, ingress validator, or outgoing proxy — **and before its tests** — the pure user-story `TESTING_PLAN.md` update is proposed (zero file/crate/code references) and blocked on human validation; no tests or code proceed until it is validated:

1. **Write the user-facing documentation FIRST** (`docs/`, owned by `writer` or `architect`), showing **exact, runnable commands and files** in all three environments:
   - **On the local developer machine:** Zero `sudo`, zero manual `chmod` on home directories, zero `setcap` ceremony required for daily coding. Lead immediately with 3 zero-`sudo` commands (`blastit dev` runs the real multi-process topology, stage-0 `execve` secret swap, ingress validators, and egress brokers so any misconfiguration fails loudly on `localhost`).
   - **In CI/CD:** One fast, unprivileged, deterministic command (`blastit check`) that runs the app's test suite through Blastit's brokers/validators and fails if **any** route is unexercised, **any** SQL query / Redis key / Mongo op / HTTP call is undeclared, **any** grant is dead, or **any** route lacks expected input rules (and passes cleanly on 0-proxy store-less apps).
   - **Across deployment scenarios:** Foolproof presets (`digitalocean-paas`, `compose-traefik`, plus clean 12-factor container/systemd recipes) where production `blastit` **fails closed** if real OS isolation (root or capabilities + seccomp) is missing.
2. **Review with `review-ux` and `review-arch`.** Every extra step, redundant environment variable, manual copy-paste loop, missing Mermaid diagram, or paragraph of kernel trivia is a defect.
3. **Only after the DX spec is clean**, implement the code in coarse vertical slices so the documented commands work verbatim.

## 2. Target Audience: Modern 12-Factor Frameworks Only

- **We do NOT support legacy un-12-factorizable monoliths** (such as WordPress, FTP-mutated docroots, or untested spaghetti). Never complicate `Blastfile` or CLI flags to accommodate them.
- **We DO play natively with modern, tested, 12-factor frameworks and their idioms:**
  - **Python (WSGI):** Django, Flask, SQLAlchemy, psycopg 3 — zero-boilerplate connection/parameter diagnostics (`entrypoint python "app:app"`).
  - **PHP (embedded NTS `libphp` `8.3, 8.4, 8.5`):** Symfony, Laravel, Slim, PSR-7 / PSR-15 front controllers (`entrypoint php "public/index.php"`), automatic host NTS `libphp` discovery (`BLASTIT_LIBPHP` override only), clean URL rewriting without nginx boilerplate, safe static asset serving — zero external PHP daemon or CGI parameter surface.
  - **Node & Frontend (Vite / HTTP upstream):** Vite dev/preview & static assets, SvelteKit, React Router / Remix, Nuxt, Hono, Next standalone — seamless asset + SSR split.
  - **JVM (Tomcat / Servlet / Spring Boot):** 12-factor embedded or standalone Tomcat / Spring Boot apps talking over local sockets/loopback, JDBC/Redis/HTTP proxying without changing app code.

## 3. Human + AI Manifest Ergonomics (`Blastfile`)

Every `Blastfile` feature must pass two tests:

1. **The Human Test:** Can a developer read `Blastfile` in 30 seconds and immediately see what each `view` can touch? Declare `entrypoint` once at top level, nest capability blocks inside each `view`, use clean `"/*"` / `regex '...'` / bare `immutable`, and auto-discover `./Blastfile`.
2. **The AI Agent Test:** How does a developer OR an AI agent fill in `Blastfile` and **know for sure nothing was forgotten**?
   - **Single-pass wire learning (`--learn` / `--learn --dry-run`):** Observes every route hit and outgoing query/call on the wire across every backend language in one pass without learning `..` or dotfile scanner probes.
   - **Strict completeness verification (`blastit check` / `--json`):** Verifies zero unrouted requests, zero missing grants, zero dead grants, and explicit body policies — printing dead-grant `sql`, `proxy`, and `fix.to` (`view <name> postgres`) in both human and `--json` output with proper singular/plural (`1 request`).

## 4. Visual Hierarchy & Ruthless Leanness Rules (`<130` lines/page)

Today's developer closes bloated documentation immediately. Enforce on every page under `docs/` and `README.md`:

- **Immediate 3-step zero-`sudo` happy path:** Right under `# Title` + 1–2 line scope statement, show the 3 runnable commands. Put caveats in `!!! warning` *after* Command 1 and platform/wheel matrices in collapsed `??? note` near the bottom — **never** before Command 1.
- **Visual hierarchy on every guide/spec:** Include at least **1 Mermaid diagram** (`flowchart LR`/`TB` or `sequenceDiagram`), at least **1 admonition** (`!!! note` / `!!! warning`), compact tables, non-empty `Blastfile` grants, and forward `## Where next` links.
- **Zero diary/apology bloat:** Keep pages `<130` lines (target 75–95 lines). Zero diary headings (`What we thought...`), zero roadmap excuses (`Until Milestone...`, `not supported yet`), and zero host-package apologies.
- **Every snippet is tested:** `make docs-check` (`uvx --offline zensical build --strict`) verifies every link and anchor, and `make docs-unit` runs the package's offline Python suites.
