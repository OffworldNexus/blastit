---
name: Research
description: How Blastit researchers investigate protocols, modern framework idioms, WASM libraries/licenses, and CVEs (`php2shell`, `node2shell`, parser differentials). Writes cited notes under `docs/research/`.
---

# Research

## Procedure

1. **Check existing repo knowledge first:** `docs/research/INDEX.md`, `docs/knowledge/LESSONS.md`, `ARCHITECTURE.md`, and `docs/spikes/`. Never duplicate completed research.
2. **Parallelize broad questions:** If a research node has ≥ 3 independent sub-topics (e.g., FastCGI wire framing vs Symfony/Laravel PSR idioms vs PHP CGI/FastCGI CVEs), spawn parallel background `researcher` subagents, each writing `docs/research/<date>-<slug>-<part>.md`, then synthesize the master note.
3. **Primary sources & local verification:**
   - Wire specs (PostgreSQL, MySQL client/server protocol, MongoDB `OP_MSG`, Redis RESP2/RESP3, FastCGI 1.0, RFC 9110/9112 HTTP/1.1).
   - Official framework docs & source code (Django, Flask/SQLAlchemy, Symfony, Laravel, Vite, SvelteKit/Remix/Hono, Tomcat/Spring Boot).
   - Official CVE advisories / NVD / upstream patches for `php2shell`, `node2shell`, HTTP smuggling, and media parser CVEs.
   - When a 2-minute local probe in `/tmp/opencode/` answers an empirical question (e.g. testing how a driver parses a Unix-socket URI or how `wasmtime` fuel behaves), run it and record the exact command and output!
4. **Strict license verification (Apache-2.0 project):**
   - Check the actual `LICENSE` / `Cargo.toml` license expression of every candidate crate or C/Rust library compiled to WASM.
   - **Allowed:** `Apache-2.0`, `MIT`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `MPL-2.0` (file-level, if unmodified crate).
   - **Forbidden in shipped wheels/binaries/WASM:** `GPL-*`, `LGPL-*` (static WASM linking), `AGPL-*`, `SSPL`, non-commercial licenses. Always state the verified SPDX license in the comparison table.

## Note format: `docs/research/YYYY-MM-DD-<slug>.md`

```markdown
# <Question / Topic>
- Node: <plan id>   - Author: researcher   - Date: YYYY-MM-DD

## TL;DR (<= 8 lines)
## Developer Experience Implications (what framework idioms/defaults make this zero-config for users?)
## Technical Findings (with citations / empirical probes)
## Comparison Table (options, SPDX license, maintenance, footprint, security properties)
## Concrete Recommendations for Blastit
## Unverified / Open Questions
## Sources
```

Append one entry to `docs/research/INDEX.md` (`- [YYYY-MM-DD-slug](YYYY-MM-DD-slug.md): one-line summary`). Note that `docs/research/` is excluded from the public Zensical site via `zensical.toml`.
