---
id: MA.2.2.1
title: "Leaf 4: manifest 0.1.1 schema, reserved-key guard and boot warnings"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MA.2.1.2, MA.2.1.3]
paths:
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/tests/**"
  - "crates/blastit-py/src/audit.rs"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T14:19"
---

# Goal

Ship the `blastit.toml` 0.1.1 schema in `blastit-manifest`: top-level `app` and `preload`, optional `[route.ingress]`, the reserved-key-inside-a-table guard, `LiteralQuery`/`RedactedQueryPlaceholder` validation, boot warnings, catch-all detection and the shared `is_query_covered_by_tables_or_groups` helper — with a protected critical suite and a standard docs-fence suite.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §4.2 B, §7.1 docs fences, §7.2 row `R7..R8, R11, R24 (manifest)`, §8 Leaf 4; `ADR-0002`, `ADR-0003`; requirements `R7`, `R8`, `R11`, `R24` (manifest half).
- Typed APIs: `Manifest::app`, `Manifest::preload`, `Route::with_ingress`, `Route::is_unrestricted_catch_all` (matches both `path = ".*"` and `path = ['.*']`), `ManifestWarning::{RawIngress, EgressRoute}`, `Manifest::validate(&self, base: u32)`.
- `TOP_LEVEL_KEYS` gains `"app"`/`"preload"`; any `version`/`app`/`preload` inside a table (or `groups` outside `[[route]]`) fails with the exact `… after a [table] is not top-level; move it above the first table` message. `allow_raw` on `json`/`form`/`none` is `UnknownKey`.
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_manifest_011 --locked
$ cargo test -p blastit-manifest --test docs_fences --locked
$ make check
```

- `crates/blastit-manifest/tests/critical_manifest_011.rs` is **new**: positive (`app`, string/array `preload`, route `groups`, `[route.ingress]`, `KNOWN_STATIC_SETUP_SQL`, both catch-all shapes) and negative (exact reserved-key message for `groups`/`app`/`preload`/`version`, `allow_raw` on `json` -> `UnknownKey`, `…` -> `RedactedQueryPlaceholder`, `'secret'` -> `LiteralQuery`, both boot warnings emitted).
- `crates/blastit-manifest/tests/docs_fences.rs` is **new and standard-tier** (not protected): exactly the 6 complete `title="blastit.toml"` fences in `README.md`, `docs/index.md`, `docs/getting-started.md`, `docs/local.md`, `docs/frameworks/django.md`, `docs/frameworks/flask.md` parse cleanly (`count == 6`), plus a negative invalid-regex canary.
- No value-bearing literal from a parsed manifest can survive validation into an allowlist (`R24`).

# Notes

- ~320 changed lines. Do not create `src/edit.rs` (Leaf `MA.2.2.2`); do not add `blastit-ingress` code (Leaf `MA.2.1.2`) — only consume `IngressPolicy`.
- `docs_fences.rs` is deliberately standard-tier so `MA.3` can edit docs without a `critical_guard` conflict; never move it into `critical_*.rs`.
