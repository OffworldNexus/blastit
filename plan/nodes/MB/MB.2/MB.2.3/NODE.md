---
id: MB.2.3
title: "Parse and validate [static] route blocks in blastit-manifest"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.1, MB.2.2]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/tests/critical_static_manifest.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - docs/reference/manifest.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T15:11"
updated: "2026-11-13T15:11"
---

# Goal

Teach `blastit-manifest` the `[static]` grammar exactly as the accepted page and design define it: model, parse, boot validation and error messages, plus the `docs/reference/manifest.md` entry and one new protected suite. Additive only — every existing `version = 1` manifest must still parse unchanged.

# Context

- Files: `model.rs` (types and route exposure), `parse.rs` (keys and shape), `validate.rs` (boot rules), `error.rs` (messages that name `file:line`), `lib.rs` (exports), `docs/reference/manifest.md` (reference entry for the accepted block), and a new `crates/blastit-manifest/tests/critical_static_manifest.rs`.
- **Fatal at boot** (the design pins the exact list; these are the minimum): unknown key in the block; empty, absolute or `..`-bearing `root`; `root` that does not exist or is not a directory; `index`/fallback target containing `..`, NUL or a leading `/` where a relative path is required; fallback that is not a rooted path when the design says it is; malformed `immutable` patterns; a static route with no `path`; a static block combined with a request-body `[route.ingress]` when the design forbids it.
- **Warn and run** only where the design says so; new warnings use the existing `ManifestWarning` shape.
- Additive compatibility: `critical_manifest_011.rs`, `critical_manifest_edit.rs`, `grants.rs`, `routing.rs`, `uid.rs`, `validate.rs` and `parse.rs` test binaries stay green **untouched** (they are not in this node's `paths`).
- `docs_fences.rs`: add `docs/frameworks/static-and-spa.md` to `FILES`, bump the array and comment from six to seven, so the page's complete manifest is machine-validated from now on.
- No dependency changes: `Cargo.toml`/`Cargo.lock` belong to `MB.2.8`; if a crate really needs a new dependency, stop and log a follow-up instead of editing the workspace.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test critical_static_manifest --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked      # every existing suite, untouched
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test docs_fences --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.2.3
$ make rs-test
```

# Notes

- **Protected tests: add, never weaken.** `critical_static_manifest.rs` is a new file (the guard allows additions freely). If an existing `critical_*.rs` would have to change, stop and record a follow-up — it is not in `paths`.
- The critical suite proves the negative side: each fatal rule has a manifest that fails with a message naming the offending line, and the accepted page's manifest parses and validates.
- Keep parse/model/validate changes small and table-driven; budget ≤ 600 changed lines including tests. If the grammar grows past that, stop and log a follow-up rather than bundling a second concern.
- Do not touch `crates/blastit-scheduler/**` (MB.2.4/MB.2.5) or `crates/blastit/**` (MB.2.6).
