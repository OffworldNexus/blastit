---
name: Security invariants & CVE bar
description: MANDATORY before designing, implementing or reviewing any Rust crate, WASM validator, protocol proxy, privilege mode, or security documentation in Blastit.
---

# Security invariants & CVE bar

Blastit's promise is simple: **an exploitable bug in a route — even on an unpatched PHP, Node, Python, or JVM runtime — is worth at most that route's narrow manifest grants, never a shell with secrets or full database access, and malformed/malicious inputs never reach the runtime's parser raw.**

Every change must uphold these non-negotiable invariants:

## 1. Production Mode vs Local/CI Dev Simulation Mode

- **Production (`blastit`):** **Fails closed.** Requires either `uid 0` or the explicit Linux capability set (`CAP_SETUID`, `CAP_SETGID`, `CAP_CHOWN`, `CAP_FOWNER`, `CAP_DAC_OVERRIDE`, `CAP_KILL`, optional `CAP_SETPCAP`) plus `seccomp` (unless `BLASTIT_SECCOMP=0` is explicitly set and warned). Never silently falls back to single-UID execution!
- **Developer / CI simulation (`blastit dev`, `blastit test`, `blastit check`):** Runs completely unprivileged under the caller's own UID so developers and CI runners never need `sudo` or `setcap`. Crucially, **it still runs the real multi-process architecture**:
  - Stage-0 `execve` barrier still strips real proxy credentials and `BLASTIT_*` variables from the app's environment.
  - Private per-route runtime directories (`/tmp/...` or `$XDG_RUNTIME_DIR/blastit-.../views/<route>/`) still give each route only the proxy sockets it has grants for.
  - Ingress WASM decode/re-encode, routing regexes, and egress protocol brokers still validate every byte and every query identically to production.
  - Emits an unmistakable `privilege: "unprivileged-dev"` boot warning (and refuses to bind non-loopback addresses unless `--host` is explicitly passed) so nobody mistakes dev simulation for OS-level multi-UID isolation in production.

## 2. Wall 1: Guarantee-Safe Budgeted WASM Ingress (Milestone C+)

- **Decode, validate, re-encode.** No raw untrusted byte stream crosses into the backend when an ingress codec applies:
  - **Compression:** `Content-Encoding` (`gzip`, `deflate`, `br`, `zstd`) is decompressed in memory-safe code with strict compressed-size, output-size, and expansion-ratio caps (zip-bomb immunity), then forwarded uncompressed (or cleanly framed) to the backend.
  - **Structured payloads (JSON, XML, form-urlencoded, multipart):** Parsed in memory-safe / WASM code, validated against the route's JSON Schema and optional **stateless** WASM validator functions (rejecting duplicate keys, trailing bytes, NaN/Infinity, XML DTD/XXE/entity expansion), and **re-serialized canonically** from the parsed AST. Backend parsers never see ambiguous syntax or parser-differential payloads.
  - **Media & documents (JPEG, PNG, WebP, GIF, BMP, TIFF, HEIC/HEIF, JP2, PDF):** Decoded to raw pixels/pages inside a zero-filesystem `wasm32-wasip1` sandbox (`wasmtime`) driven **one page/frame at a time** (pull-based export ABI so linear memory holds at most **one** decoded page/frame bitmap, never all N pages at once), verified by magic bytes + MIME allowlist, and **re-encoded from scratch** — stripping all EXIF/XMP/ICC metadata, embedded scripts, fonts, annotations, JavaScript, polyglot trailers, and malformed chunks.
  - **Headers & upstream protocol semantics:** Strict allowlist of expected headers and HTTP semantics. Drop unknown/unexpected headers by default; reject `Content-Length` + `Transfer-Encoding` ambiguity, invalid `Host`, CR/LF, `HTTP_PROXY` (`httpoxy`), and FastCGI/CGI parameter smuggling (`REDIRECT_STATUS`, `PHP_VALUE`, `PHP_ADMIN_VALUE`, encoded query-string `-d` flags).
  - **Scanner rules & honeypots:** Built-in, zero-Python/PHP/Node fast rejection or tarpit/honeypot responses for automated exploit scanners (`/.env`, `/.git`, `/wp-admin`, `/phpinfo.php`, `/actuator/env`, CGI query-string gadget probes).
- **Strict WASM budgets:** Pre-compile `wasmtime::Engine` + `Module` once; create a fresh `Store` per request/page with hard `memory_size` ceiling and instruction `fuel` cap (reset per page up to `max_pages` for multi-page documents). Never grant WASI filesystem network or host env access.

## 3. Wall 2: Per-Route Sandbox & Capability Brokers

- **Stage-0 `execve` secret barrier:** Real upstream credentials (`DATABASE_URL`, MySQL, Mongo, Redis, outbound API tokens) exist **only** in stage 0 and are piped to their Python/PHP/Node/JVM-free Rust broker (`execve` with empty `envp`). Stage 1 (`execve` with swapped `envp`) and route processes never hold real secrets in `environ`, `argv`, or heap.
- **Socket-derived identity:** Never trust `SO_PEERCRED` alone (broken under gVisor). Identity comes from which per-route `0600` listener in `/run/blastit/views/<uid>/` accepted the connection, cross-checked with `SO_PASSCRED` / `SCM_CREDENTIALS` in real-UID modes.
- **No parser differentials in brokers:** SQL table/column grants use the database's true grammar (`libpg_query` for PostgreSQL; dialect-accurate lexing/parsing for MySQL); Redis/Mongo/HTTP brokers parse full wire frames and serialize clean upstream frames rather than forwarding raw bytes after regex peeks.
- **Outgoing HTTP secret-injecting proxy:** Route processes connect to a local unauthenticated per-route socket/port without knowing API keys. The Rust broker checks `(route, target host, method, path pattern)` against `blastit.toml`, establishes upstream TLS itself, strips unexpected headers, and injects the secret (`Authorization: Bearer ...`, custom header, query param, HMAC) **only** for matching upstream endpoints — preventing credential exfiltration to attacker-controlled hosts!

## 4. Real-World CVE Regression Mandate

Every backend and ingress milestone must ship automated exploit-regression tests proving that famous RCE/injection classes fail harmlessly under Blastit **even if the backend runtime itself were vulnerable**:
- **`php2shell` class:** PHP CGI/FastCGI argument & env injection (CVE-2024-4577 soft-hyphen/best-fit `-d allow_url_include=1 -d auto_prepend_file=php://input`, FastCGI `PHP_VALUE`/`PHP_ADMIN_VALUE` overwrite, `SCRIPT_FILENAME` traversal, polyglot `.jpg`/`.pdf` LFI/upload RCE).
- **`node2shell` class:** Node/SSR prototype pollution (`__proto__`, `constructor.prototype` in JSON/query/form payloads), child-process env/argument injection (`NODE_OPTIONS` isolation, egress block preventing reverse shells & SSRF metadata theft), malicious Vite `/@fs/` traversal probes, RSC/SSR flight/action payload validation.
- **Database & SSRF escalation:** SQLi blocked by parameterized allowlist / column grants; SSRF to cloud metadata (`169.254.169.254`) blocked by seccomp `AF_UNIX`-only socket filter and explicit outbound HTTP proxy allowlists.

## 5. Never Leak, Never Panic

- Zero `unwrap()`, `expect()`, `panic!()`, or unchecked slice indexing on any path reachable from network bytes, backend frames, WASM memory, or wire-protocol peers.
- Never log query strings, header values, cookies, request/response bodies, SQL/NoSQL parameter values, or secret templates. Every log line stays ≤ 2048 bytes.
