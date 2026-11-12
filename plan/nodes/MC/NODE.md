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

# Context

- Vision & rules: `docs/VISION.md`, skill `dx-first`, skill `security-invariants`.
- **Mandatory DX-First law:** Start by writing the lean user documentation showing how developers and AI agents declare route input schemas, media upload rules, custom stateless WASM validators, and header expectations in `blastit.toml` — and how they test and audit them on the **local machine**, in **CI/CD**, and in **deployments** — before finalizing internal WASM ABI designs or writing Rust code!
- Permissive licensing rule: every Rust crate and WASM-compiled C/C++/Rust decoder (`wasm32-wasip1`) must carry a verified Apache-2.0-compatible license (skill `research`).

# Acceptance

- [ ] Accepted lean user documentation and `dx` + `ai-ergonomics` + `security` critic reviews precede every `0.3.x` sub-epic, ensuring route input validation takes minimal `blastit.toml` lines and integrates into `--learn` / `blastit check`.
- [ ] `wasmtime`-backed ingress sandbox pre-compiles WASM modules once and enforces per-invocation memory ceilings and instruction `fuel` budgets (reset per page up to `max_pages` for multi-page documents), trapping runaway inputs deterministically without starving other streams.
- [ ] Compressed requests are safely decoded (with zip-bomb rejection critical tests); JSON and XML bodies are validated (JSON Schema + custom stateless WASM functions) and re-serialized canonically before reaching any backend.
- [ ] Image and PDF uploads (single-page and multi-page) round-trip through the WASM CDR pipeline preserving visual fidelity (aspect ratio, page count, dominant colours) while provably stripping EXIF/XMP, PDF JavaScript/open-actions/embedded files, and polyglot trailers within `O(1 page)` bounded memory.
- [ ] Unexpected HTTP headers/semantics are dropped or rejected statelessly, and scanner probes trigger embedded rules/honeypots without touching backend workers.
- [ ] Automated CVE regression suite proves `php2shell`, `node2shell`, parser-differential smuggling, and malicious image/PDF polyglots fail harmlessly even against unpatched backend runtimes.
- [ ] `make check`, `make e2e`, `visual-checker`, and `test-curator` mutation audit (≥ 75 % on ingress/WASM policy crates) pass.

# Notes

- Never reference external private/client project names anywhere in code, docs, or tests.
