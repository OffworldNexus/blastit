# Blastit: Vision

> **Small bird. Small blast radius. Effortless developer experience.**

## Why Blastit Exists

Modern web applications share one fatal architectural flaw: every route runs in the same trust domain with full access to real database credentials, cache credentials, API keys, outbound network sockets, and raw untrusted bytes parsed directly by complex language runtimes. One bug in the least important endpoint — a SQL injection, an SSR/deserialization flaw, a FastCGI/CGI parameter trick (`php2shell`), a Node prototype/command gadget (`node2shell`), or a malformed PDF/image parser bug — hands the attacker the entire application.

Blastit fixes this **outside the application code**, without asking developers to rewrite their app or become Linux security engineers:

1. **Every inbound byte is decoded, validated, and re-encoded by guaranteed-safe budgeted WASM/Rust code** before the backend ever sees it.
2. **Every route runs in its own sandbox** with zero real credentials in memory.
3. **Every outgoing connection goes through a local capability proxy** that authenticates the route by socket identity, enforces per-route allowlists (`blastit.toml`), and injects real credentials only upstream.

## The Guiding Principle: Everything Stems From Developer Experience

Security tools that require 40 environment variables, `sudo` on a laptop, manual SQL copy-pasting, or 20-page kernel manuals do not get adopted. **In Blastit, Developer Experience is the primary design driver:**

1. **Documentation First, Always.** Before we design internals or write code for any feature, we write the **lean, crystal-clear user documentation** showing how it works:
   - **On a developer's laptop:** Zero `sudo`, zero `setcap` rituals, zero `chmod` on `$HOME`. `blastit dev` runs an unprivileged single-UID simulation that still executes the full multi-process topology (`http`, ingress gate, `scheduler`, route workers, local proxies, `execve` secret stripping, per-route socket dirs). To an honest program, every misconfiguration shows up locally right away. (Testing real multi-UID capabilities locally is also a one-liner when wanted.)
   - **In CI/CD:** One fast, unprivileged command runs the test suite through Blastit and proves **nothing was forgotten**: zero unrouted paths, zero missing SQL/NoSQL/Redis/HTTP grants, zero dead grants, zero untested routes, and valid input schemas.
   - **In production deployments:** Foolproof presets (`digitalocean-paas`, `compose-traefik`, plus clean container/systemd setups) that **fail closed** if real OS isolation (root or capabilities + seccomp) is absent.
2. **Lean Documentation & Minimal Configuration.** `./blastit.toml` is found automatically. Common framework setups take a handful of readable lines. User docs lead with 10-line working examples, never kernel trivia.
3. **Built for Both Humans and AI Agents.** Developers and AI coding agents fill and maintain `blastit.toml` effortlessly:
   - **Single-pass wire learning (`--learn`):** Because Blastit sits on the HTTP ingress and every outgoing wire protocol, it records every route hit and outgoing query/call across an entire test run in one pass — language-agnostically (Python, PHP, Node, Java) — instead of failing on the first query per request.
   - **Machine-readable verification (`--json`):** Structured audit output lets an AI agent or CI job verify completeness and fix drift deterministically without resorting to `unrestricted = true`.

## Target Applications (Modern 12-Factor Frameworks Only)

Blastit deliberately targets **modern, testable, 12-factor applications** — not legacy un-12-factorizable monoliths such as WordPress. We embrace modern framework idioms natively:

- **Python (WSGI):** Django, Flask, SQLAlchemy, psycopg 3.
- **PHP (PHP-FPM / FastCGI):** Symfony, Laravel, Slim, PSR-7 / PSR-15 front controllers (`public/index.php`), clean URL rewriting, and static asset serving without nginx boilerplate.
- **Node & Frontend (Vite + HTTP SSR/API):** Vite assets & dev/preview integration, SvelteKit, Remix / React Router, Nuxt, Hono, Next standalone.
- **JVM (Tomcat / Spring Boot):** Modern 12-factor Servlet / Spring Boot services.

## The Four Milestones

### Milestone A (`0.1.1` — Radical DX & Lean Docs for Current Scope)
Reimagine and ship the sharpest possible developer experience for Python WSGI + PostgreSQL (`v0.1.0` is already tagged, so Milestone A ships as `0.1.1`):
- Rewrite the documentation and README from the ground up to be **radically leaner**, clear, and show-the-world ready across Local, CI/CD, and Deployments.
- Ship zero-`sudo` local/CI execution (`blastit dev`), auto-discovery of `./blastit.toml` (with optional non-secret app wiring in `blastit.toml`), single-pass wire-level `--learn` & framework-agnostic `blastit test` / `blastit check` (solving Flask/SQLAlchemy alongside Django!), and crisp human + `--json` diagnostics.

### Milestone B (`0.2.x` — Modern Backends: PHP-FPM, Vite/Node, Tomcat & Static Assets)
Expand Blastit across several focused `0.2.x` sub-releases from a Python-only WSGI server into a multi-backend zero-trust harness:
- First-class static asset serving & clean SPA/front-controller URL rewriting.
- **PHP-FPM** (FastCGI wire protocol, PSR idioms, Symfony/Laravel zero-footgun defaults, `SCRIPT_FILENAME` & FastCGI param lockdown).
- **Vite & Node** (HTTP upstream per route/worker, SSR + static split, clean dev & prod workflows).
- **Tomcat / JVM** (12-factor Servlet / Spring Boot integration) and other high-value modern HTTP backends where appropriate.
- Language-agnostic wire learning (`--learn`) and CI completeness checks across every backend.

### Milestone C (`0.3.x` — Guarantee-Safe Budgeted WASM Ingress & CDR)
Ensure **every single byte** reaching an application backend has been decoded, validated, and re-encoded by guaranteed memory-safe code inside fuel- and memory-budgeted WASM VMs (`wasmtime`):
- **Safe decompression:** Memory-safe gzip/deflate/brotli/zstd decompression with strict size and expansion-ratio caps.
- **Structured payloads:** JSON and XML decoded, validated against JSON Schema and custom **stateless** WASM functions, and canonically re-encoded (eliminating parser differentials, duplicate keys, XXE, and prototype-pollution keys).
- **Media & PDF Content Disarm & Reconstruction (CDR):** Images (JPEG, PNG, WebP, GIF, BMP, TIFF, HEIC/HEIF, JP2) and PDFs decoded to raw page/frame bitmaps inside a zero-filesystem WASM reactor sandbox one page at a time (`O(1 page)` bounded memory, per-page instruction fuel caps) and cleanly re-encoded from scratch — stripping all metadata, scripts, embedded objects, and polyglots.
- **Strict HTTP header & protocol mechanics:** Drop unexpected headers, canonicalize expected headers, statelessly validate cookies/auth/content-types, and block CGI/FastCGI/proxy header tricks.
- **Scanner rules & honeypots:** Fast built-in rejection/honeypots against vulnerability scanners before any backend code runs.

### Milestone D (`0.4.x` — Outgoing Capability Proxies)
Extend Blastit's stage-0 credential barrier, per-route local sockets, allowlist enforcement, and `--learn` wire tooling to the outgoing services modern apps rely on:
- **MySQL / MariaDB** wire proxy (parameterized SQL & table/column grants).
- **MongoDB** `OP_MSG` wire proxy (collection, command, and operator-shape grants).
- **Redis / Valkey** RESP2/RESP3 proxy (key-prefix & command grants).
- **Outgoing HTTP / HTTPS secret-injecting proxy:** Routes call a plain local proxy without ever holding API keys or tokens; for each allowed `(route, host, method, path)` upstream, the broker upgrades to TLS and injects the secret (`Authorization`, API-key header, query param, or HMAC) on the outbound leg.

## Standing Security Benchmark: Neutralizing Real CVEs

Across Milestones B, C, and D, Blastit maintains an automated CVE exploit-regression suite (`php2shell`, `node2shell`, polyglot image/PDF RCEs, HTTP/FastCGI desync, SSRF) proving that **even an unpatched version of PHP, Node, Python, or Java running behind Blastit cannot be exploited**.
