---
name: Security invariants & CVE bar
description: MANDATORY security invariants for Rust crates, WASM validators, protocol proxies, privilege modes, and CVE regressions in Blastit.
---

# Security invariants & CVE bar

Blastit's promise is simple: **an exploitable bug in a route — even on an unpatched PHP, Node, Python, or JVM runtime — is worth at most that route's narrow `Blastfile` grants, never a shell with secrets or full database access, and malformed/malicious inputs never reach the runtime's parser raw.**

Every change must uphold these non-negotiable invariants:

## 1. Production Mode vs Local/CI Dev Simulation Mode

- **Production (`blastit`):** **Fails closed.** Requires either `uid 0` or the explicit Linux capability set (`CAP_SETUID`, `CAP_SETGID`, `CAP_CHOWN`, `CAP_FOWNER`, `CAP_DAC_OVERRIDE`, `CAP_KILL`, optional `CAP_SETPCAP`) plus `seccomp` (unless `BLASTIT_SECCOMP=0` is explicitly set and warned). Never silently falls back to single-UID execution!
- **Developer / CI simulation (`blastit dev`, `blastit check`):** Runs completely unprivileged under the caller's own UID so developers and CI runners never need `sudo` or `setcap`. Crucially, **it still runs the real multi-process architecture**:
  - Stage-0 `execve` barrier still strips real proxy credentials and `BLASTIT_*` variables from the app's environment.
  - Private per-route runtime directories still give each `view` only the proxy sockets it has grants for.
  - Ingress WASM decode/re-encode, route matching, and egress protocol brokers still validate every byte and every query identically to production.
  - Emits an unmistakable `privilege: "unprivileged-dev"` warning and binds loopback by default unless `--host` is explicitly passed.

## 2. Wall 1: Guarantee-Safe Budgeted WASM Ingress (Milestone C+)

- **Decode, validate, re-encode.** No raw untrusted byte stream crosses into the backend when an ingress codec applies:
  - **Compression:** `Content-Encoding` (`gzip`, `deflate`, `br`, `zstd`) is decompressed in memory-safe code with strict compressed-size, output-size, and expansion-ratio caps (zip-bomb immunity).
  - **Structured payloads (JSON, XML, form-urlencoded, multipart):** Parsed in memory-safe / WASM code, validated against the route's JSON Schema and optional **stateless** WASM validator functions (rejecting duplicate keys, trailing bytes, NaN/Infinity, XML DTD/XXE/entity expansion), and **re-serialized canonically** from the parsed AST.
  - **Media & documents (JPEG, PNG, WebP, GIF, BMP, TIFF, HEIC/HEIF, JP2, PDF):** Decoded to raw pixels/pages inside a zero-filesystem `wasm32-wasip1` sandbox (`wasmtime`) driven **one page/frame at a time** (`O(1 page)` bounded linear memory), verified by magic bytes + MIME allowlist, and **re-encoded from scratch** — stripping EXIF/XMP/ICC metadata, scripts, annotations, and polyglot trailers.
  - **Headers & upstream protocol semantics:** Drop unexpected headers by default; reject `Content-Length` + `Transfer-Encoding` ambiguity, invalid `Host`, CR/LF, `HTTP_PROXY` (`httpoxy`), and CGI/proxy parameter smuggling (`REDIRECT_STATUS`, `PHP_VALUE`, `PHP_ADMIN_VALUE`, encoded query-string `%ADd` / `-d` flags).
  - **Scanner rules & refusal paths:** Reject `..` traversal, dotfile (`/.env`, `/.git`) probes, and `.php` tails on static mounts before any view runs — and never learn scanner probes in `--learn`.
- **Strict WASM budgets:** Pre-compile `wasmtime::Engine` + `Module` once; create a fresh `Store` per request/page with hard `memory_size` ceiling and instruction `fuel` cap. Never grant WASI filesystem, network, or host env access.

## 3. Wall 2: Per-Route Sandbox & Capability Brokers

- **Stage-0 `execve` secret barrier:** Real upstream credentials (`DATABASE_URL`, MySQL, Mongo, Redis, outbound API tokens) exist **only** in stage 0 and are piped to their Rust broker (`execve` with empty `envp`). Stage 1 (`execve` with swapped `envp`) and view workers never hold real secrets in `environ`, `argv`, or heap.
- **Socket-derived identity:** Never trust `SO_PEERCRED` alone (broken under gVisor). Identity comes from which per-route `0600` listener accepted the connection, cross-checked with `SO_PASSCRED` / `SCM_CREDENTIALS` in real-UID modes.
- **No parser differentials in brokers:** SQL table/column grants use the database's true grammar (`libpg_query` for PostgreSQL; dialect-accurate parsing for MySQL); Redis/Mongo/HTTP brokers parse full wire frames and serialize clean upstream frames.
- **Outgoing HTTP secret-injecting proxy:** Views connect to a local unauthenticated socket without holding API keys; the Rust broker checks `(view, host, method, path)` against `Blastfile`, establishes upstream TLS itself, and injects secrets only on allowed outbound legs.

## 4. Real-World CVE Regression Mandate

Every backend and ingress milestone ships automated exploit-regression tests proving that famous RCE/injection classes fail harmlessly under Blastit:
- **`php2shell` class:** PHP CGI parameter & argv injection (CVE-2024-4577 `%ADd allow_url_include=1 -d auto_prepend_file=php://input`, `PHP_VALUE`/`PHP_ADMIN_VALUE` overwrite, `SCRIPT_FILENAME` traversal/swap, polyglot `.jpg`/`.pdf` upload RCE) — eliminated structurally via embedded NTS `libphp` over `view.chan` with pinned `(st_dev, st_ino, st_size, st_ctime)` entrypoints.
- **`node2shell` class:** Prototype pollution (`__proto__`, `constructor.prototype`), `NODE_OPTIONS` / env isolation, egress block preventing reverse shells & SSRF metadata theft, Vite `/@fs/` traversal probes.
- **Database & SSRF escalation:** SQLi blocked by parameterized allowlists / table-column grants; SSRF to `169.254.169.254` blocked by seccomp `AF_UNIX`-only socket filters and explicit outbound HTTP proxy grants.

## 5. Never Leak, Never Panic

- Zero `unwrap()`, `expect()`, `panic!()`, or unchecked slice indexing on any path reachable from network bytes, `view.chan` frames, WASM memory, or wire-protocol peers.
- Never log query strings, header values, cookies, request/response bodies, SQL/NoSQL parameter values, or secret credentials (`DATABASE_URL`, passwords, tokens). Every log line stays <= 2048 bytes.
