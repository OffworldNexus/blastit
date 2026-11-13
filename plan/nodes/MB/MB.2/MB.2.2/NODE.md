---
id: MB.2.2
title: "Design static serving, rewrite precedence and path safety (ADR-0007)"
kind: task
status: ready
owner: designer
priority: 2
depends_on: [MB.1, MB.2.1]
paths:
  - "docs/design/static-and-spa.md"
  - "docs/design/static-and-spa.review-security.md"
  - "docs/design/static-and-spa.review-implementability.md"
  - "docs/design/static-and-spa.review-testability.md"
  - docs/design/README.md
  - "docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md"
  - docs/decisions/README.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T15:11"
updated: "2026-11-13T15:11"
---

# Goal

Turn the accepted `docs/frameworks/static-and-spa.md` into the technical design `docs/design/static-and-spa.md` plus `ADR-0007`: the exact `[static]` manifest grammar, immutable hashed caching, SPA-fallback and front-controller rewrite precedence, and the path-safety model — pinned to the existing supervisor / `RawFrame` scheduler path with no second supervisor.

# Context

- Input of record: accepted `docs/frameworks/static-and-spa.md` and its two critic reviews. The design may not contradict the page; if it must, `human add` the conflict instead of silently weakening either.
- **Grammar to pin** (exact keys, defaults, errors): the `[static]` block on a route — at minimum `root`, `index`, the SPA / front-controller fallback target, and the `immutable` filename patterns. Decide and justify per-route `[route.static]` versus a top-level `[static.<name>]` reused by routes (see `[proxy.<name>]` and `[route.ingress]` precedent). State whether a manifest whose routes are all static may omit `app` (MB.2.6 implements it).
- **Rewrite precedence table**, complete and ordered: exact file → directory `index` → declared SPA/front-controller fallback → the route's own app → `404`. Include the rule that a path refused by the safety model is never masked by the fallback (the classic traversal-to-`index.html` bug), `GET`/`HEAD` only (`405` with `Allow`), body on a static route `413`, and behaviour for `//`, trailing slash, `%2F`.
- **Response semantics:** `Content-Type` from extension (closed table), `Content-Length`, quoted `ETag`, `Last-Modified`, `If-None-Match` → `304`, and `Cache-Control: public, max-age=31536000, immutable` **only** for paths matching a configured hashed pattern, `no-cache` otherwise. Say what `HEAD` does.
- **Security model, as a test matrix** (one row per attack, so implementers do not invent coverage): `..`, `%2e%2e`, `..%2f`, `%2F`, `%5c`, double-encoded `%252e`, absolute path, NUL `%00` and other control bytes, over-long/over-size, symlink inside the root pointing outside, directory/socket/fifo/device, dotfiles (`/.env`, `/.git/config`, `/.git/`), and the `/.well-known/` decision. Each row names the expected status.
- **Mechanism:** `crates/blastit-scheduler/src/static_asset.rs` (resolver + response, `MB.2.4`) and dispatch in `crates/blastit-scheduler/src/scheduler.rs` (`MB.2.5`). The boundary is `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS` via the `rustix` dependency the scheduler already has — never a `canonicalize()`-and-compare-string check. No new role, no uid for static, no second supervisor; a size cap and the scheduler's uid apply.
- **ADR-0007** records the lasting decisions only (static responder inside the scheduler; per-route vs named block; fallback vs catch-all regex; fail-closed defaults), one page, `ADR-NNNN-<slug>.md` conventions from skill `design-docs`.
- Fresh-context critics with lenses `security`, `implementability`, `testability` write `docs/design/static-and-spa.review-security.md`, `...review-implementability.md`, `...review-testability.md`; every `BLOCKER`/`MAJOR` is resolved and listed under `## Review Resolution` in `RESULT.md`.

# Acceptance

```console
$ test -f docs/design/static-and-spa.md && test -f docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md && echo "design + ADR exist"
$ grep -qi 'openat2' docs/design/static-and-spa.md && grep -q 'RESOLVE_BENEATH' docs/design/static-and-spa.md
$ grep -q 'public/index.php' docs/design/static-and-spa.md
$ grep -c '^|' docs/design/static-and-spa.md     # precedence table + attack matrix are tables, not prose
$ for f in docs/design/static-and-spa.review-security.md docs/design/static-and-spa.review-implementability.md docs/design/static-and-spa.review-testability.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "three lenses pass"
$ grep -n 'static-and-spa' docs/design/README.md && grep -n 'ADR-0007' docs/decisions/README.md
$ make docs-check
```

# Notes

- No code, no test files, no manifest changes: this node produces two documents, three review artefacts and the two index rows.
- The attack matrix is the contract `MB.2.4` and `MB.2.5` implement as protected `critical_*.rs` tests; keep every row binary (status code in, status code out) so a cheap implementer can table-drive it.
- Keep the design terse: tables and signatures, not narrative. Kernel and privilege detail stays in `ARCHITECTURE.md` (synced by `MB.2.8`), not repeated here.
- `docs/design/` and `docs/decisions/` are excluded from the public Zensical build; `make docs-check` proves only that the site still builds.
