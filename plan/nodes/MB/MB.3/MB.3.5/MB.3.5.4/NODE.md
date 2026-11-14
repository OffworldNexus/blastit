---
id: MB.3.5.4
title: "Add [route.php] (PhpPolicy, Executor::Php) and narrow [route.fastcgi] to external-only; drop FastCgiPoolSocketInvalid"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.5.1]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - "crates/blastit-manifest/tests/parse.rs"
  - crates/blastit/src/stage0.rs
  - crates/blastit/tests/no_python_build.rs
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T16:39"
---

# Goal

Add first-class `[route.php]` and narrow `[route.fastcgi]` to the external fallback, per
ADR-0010 §5: `PhpPolicy { entry, workers }`, `Executor::Php`, and the drop of
`FastCgiPoolSocketInvalid`.

# Context

- ADR-0010 is the design of record. PHP now runs embedded (host NTS `libphp.so` + a
  per-minor C SAPI shim) inside the per-route `view` worker; there is no blastit-managed
  pool, so `[route.fastcgi]` stops describing one.
- `[route.php]`: `entry` (required, the §4.1 rules unchanged) + optional downward
  `workers` capped by `view_worker_cap()` — exactly the shape `MB.3.9.2` accepted and
  `MB.3.5.1` implemented for `fastcgi`; it transfers verbatim. No `listen`.
- `[route.fastcgi]` **stops existing as a key** (owner decision 2026-11-14, "No fallback
  please"). A manifest that writes it is refused as an **unknown key naming the key** — not
  a defaulted-off block, not a deprecation warning. The external fallback, its `workers`
  rule and the `BLASTIT_ALLOW_EXTERNAL_FASTCGI` switch are deleted by `MB.3.17`; this leaf
  must not keep any `fastcgi` policy type or parser arm alive.
- `Executor::Php` joins `ALL_KINDS`; `Executor::View` stays the WSGI kind.
- Drop `FastCgiPoolSocketInvalid` (the ADR deletes the bind window it described); keep
  `FastCgiListenDerived` for the narrowed block.
- Boot predicates: `can_boot_without_app()` includes `[route.php]`; `is_all_static()` is
  false for a php route exactly as it is for a fastcgi one.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit-manifest --test docs_fences --locked
$ make check
```
- A one-line `[route.php] entry = "public/index.php"` parses, validates and reports
  `Executor::Php`; `workers = 4` is accepted as a cap and `workers = 200` refused.
- `[route.fastcgi]` is refused as an unknown key naming it (the block no longer exists); `[route.php]` parses.
- `grep -rn FastCgiPoolSocketInvalid crates/` is empty; no dead variant remains.
- Every rule has a denied case and its accepted twin (skill `testing` §3).
- The page is `MB.3.10`'s; do not touch `docs/**` here beyond the fence count if it moves.

# Notes

- Do not add the `libphp.so` probe here (that is `MB.3.5.2`, `MB.3.13`): this leaf is
  grammar + executor kinds only.
- `MB.3.5.3` (route-key list) lands with or after this, so the diagnostic names `php` and
  `fastcgi`.

## Scope guards (owner, 2026-11-14) — do not invent a second grammar

1. **One key:** `[route.php] entry`, resolved relative to `blastit.toml` and **under
   `static.root` when both blocks are set** — the same rule and the same error-text shape
   the retired key had. `workers` stays **optional and downward only** (ADR-0010 §4: a cap
   on the derived concurrency, never required, never a `listen`, never a local
   `pm.max_children` idea).
2. **`Executor::Php` is a typed plan**, not a backend string. `[route.fastcgi]` stays an
   **unknown key**: keep the accepted twin in the same test (`[route.php]` parses,
   `[route.fastcgi]` refuses naming the key).
3. **Inherit the rule tests** that left with `check_fastcgi` — entry rules, optional
   `workers` range, `spa = true` conflict, `CONNECT`/`TRACE` refusal. They must not vanish
   with the crate.
4. **Boot refusal:** no `app` is required for a PHP route (it is its own executor), but a
   manifest with neither `app` nor a static/PHP route must still refuse — and that message
   must **not** mention `uv add` on a PHP-only manifest (a MINOR we already fixed once in
   `MB.3.11`; do not reintroduce it).
5. **`docs_fences`' `STALE` row stays** until `MB.3.20` deletes it; do not remove it here,
   and do not edit `docs/frameworks/**`.
