---
id: MC
title: "Guarantee-Safe Budgeted WASM Ingress & CDR (0.3.x)"
kind: milestone
status: ready
priority: 3
depends_on: [MB]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T00:50"
updated: "2026-11-12T00:50"
---

# Goal

Make Blastit's ingress component validate and reconstruct inputs across focused `0.3.x` sub-releases so **every single byte received by an application backend has been decoded and re-encoded by guaranteed memory-safe code inside budgeted WASM VMs**:
1. **Memory-safe decompression** (`gzip`, `deflate`, `br`, `zstd`) with strict size and expansion-ratio bomb limits.
2. **Structured payload validation & canonical re-encoding:** JSON and XML decoded, validated against JSON Schema and custom **stateless** WASM functions, and cleanly re-encoded (eliminating duplicate-key ambiguity, `__proto__` pollution, XXE/entity bombs, and parser differentials).
3. **Media & PDF Content Disarm & Reconstruction (CDR):** Uploaded images (JPEG, PNG, WebP, GIF, BMP, TIFF, HEIC/HEIF, JP2) and PDFs decoded to raw page/frame pixels inside a zero-filesystem `wasm32-wasip1` reactor sandbox one page at a time (`O(1 page)` bounded linear memory + per-page fuel cap) and re-encoded cleanly from scratch (stripping all metadata, scripts, embedded objects, and polyglots).
4. **Strict HTTP header mechanics:** Drop unexpected headers, canonicalize expected headers, and statelessly validate cookies, auth schemes, content types, and query parameters.
5. **Scanner defense:** Built-in rules and/or honeypots/tarpits against vulnerability scanners before any backend code executes.
6. **Pooled inbound body-RAM budget & byte-permit queueing (`owner: architect`):** replace the unbounded `BLASTIT_MAX_BODY_BYTES × connections` worst-case memory multiplication across buffered codecs (`json`/`form`/`xml`), WASM CDR staging, and PHP `RequestV1` / lazy `bt_read_post` with a shared, bounded inbound body-memory pool where requests acquire byte permits (with TCP read backpressure, bounded queue wait, and deadlock-free progress when a single valid body approaches `max_body_bytes` <= pool size).
7. **Automatic system-memory detection & holistic RAM governor (`researcher` -> `architect`):**
   1. A dedicated `researcher` investigation covering Linux cgroup v2 (`memory.max`, `memory.high`, `memory.current`, PSI `memory.pressure`) vs `/proc/meminfo` ceiling detection, PSS vs RSS accounting across forked/CoW workers, WASM linear-memory pooling (`wasmtime` pooling allocator), and dynamic memory-budget balancing algorithms across competing subsystems without deadlock or config-knob soup.
   2. An `architect`-owned unified memory-governor design + ADR that automatically detects available container/host RAM and dynamically balances allocations & backpressure across live view workers, inbound body buffers, decompression, WASM CDR sandboxes, and proxy buffers so Blastit stays strictly below the OOM ceiling with **zero manual sizing variables**.

# Context

- Vision & rules: `docs/VISION.md`, skill `dx-first`, skill `security-invariants`.
- **Mandatory DX-First law:** Start by writing the lean user documentation showing how developers and AI agents declare route input schemas, media upload rules, custom stateless WASM validators, and header expectations in a `Blastfile` — and how they test and audit them on the **local machine**, in **CI/CD**, and in **deployments** — before finalizing internal WASM ABI designs or writing Rust code!
- Permissive licensing rule: every Rust crate and WASM-compiled C/C++/Rust decoder (`wasm32-wasip1`) must carry a verified Apache-2.0-compatible license (skill `research`).
- **Memory governance is a first-class ingress requirement (owner mandate, out of `0.2.0` scope):** today a buffered body may reserve `BLASTIT_MAX_BODY_BYTES` (default `100 MiB`) per in-flight request (`docs/decisions/ADR-0006-route-ingress-body-gate.md`; `docs/reference/settings.md`), and `0.3.x` adds decompression, XML CDR staging, and page-wise WASM sandboxes on top. The `× connections` worst case is unacceptable, so `0.3.x` must replace it with pooled byte permits plus an auto-detected, self-balancing RAM governor — no operator sizing knobs.

# Acceptance

- [ ] Accepted lean user documentation and `dx` + `ai-ergonomics` + `security` critic reviews precede every `0.3.x` sub-epic, ensuring route input validation takes minimal `Blastfile` lines and integrates into `--learn` / `blastit check`.
- [ ] `wasmtime`-backed ingress sandbox pre-compiles WASM modules once and enforces per-invocation memory ceilings and instruction `fuel` budgets (reset per page up to `max_pages` for multi-page documents), trapping runaway inputs deterministically without starving other streams.
- [ ] Compressed requests are safely decoded (with zip-bomb rejection critical tests); JSON and XML bodies are validated (JSON Schema + custom stateless WASM functions) and re-serialized canonically before reaching any backend.
- [ ] Image and PDF uploads (single-page and multi-page) round-trip through the WASM CDR pipeline preserving visual fidelity (aspect ratio, page count, dominant colours) while provably stripping EXIF/XMP, PDF JavaScript/open-actions/embedded files, and polyglot trailers within `O(1 page)` bounded memory.
- [ ] Unexpected HTTP headers/semantics are dropped or rejected statelessly, and scanner probes trigger embedded rules/honeypots without touching backend workers.
- [ ] Automated CVE regression suite proves `php2shell`, `node2shell`, parser-differential smuggling, and malicious image/PDF polyglots fail harmlessly even against unpatched backend runtimes.
- [ ] **Pooled inbound body-RAM budget (`owner: architect`):** an accepted design + ADR replaces per-request `BLASTIT_MAX_BODY_BYTES × connections` reservation with one shared, bounded inbound body-memory pool; requests acquire byte permits with TCP read backpressure and bounded queue wait, and **critical** stress/deadlock/OOM tests prove peak buffered bytes never exceed the pool, that a single valid body near `max_body_bytes` (<= pool size) always completes, and that N concurrent maximal bodies either queue or shed with `503`/`413` instead of OOM.
- [ ] **Automatic memory detection & holistic RAM governor (`researcher` -> `architect`):** cited `researcher` notes on cgroup v2 (`memory.max`/`memory.high`/`memory.current`/PSI `memory.pressure`) vs `/proc/meminfo`, PSS-vs-RSS accounting across forked/CoW `view` workers, and `wasmtime` pooling allocator behaviour precede an accepted `architect` design + ADR where Blastit auto-detects container/host RAM and dynamically balances budgets & backpressure across `view` workers, inbound body buffers, decompression, WASM CDR sandboxes, and proxy buffers with **zero manual sizing variables**; critical tests pin peak memory strictly below the detected ceiling under adversarial concurrency and prove graceful shedding (never an OOM kill) when the ceiling is approached.
- [ ] `make check`, `make e2e`, `visual-checker`, and `test-curator` mutation audit (≥ 75 % on ingress/WASM policy crates) pass.

# Notes

- **Sequencing for goals 6–7 (owner mandate, scheduled here, not in `0.2.0`):** when `0.3.x` decomposes, run `researcher` (cgroup v2 / PSI / PSS-vs-RSS / `wasmtime` pooling / balancing-algorithm notes under `docs/research/`) **before** the `architect` design + ADR, and land both designs before any pool or governor implementation leaf. Byte permits and the governor share one accounting model — never two independent ceilings.
- Never reference external private/client project names anywhere in code, docs, or tests.
