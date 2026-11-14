---
id: MB.3.5.4
title: "Add [route.php] (PhpPolicy, Executor::Php) and narrow [route.fastcgi] to external-only; drop FastCgiPoolSocketInvalid"
kind: task
status: ready
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
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T14:39"
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
- `[route.fastcgi]`: `entry` + `listen`, external/operator-managed only, **no `workers`**
  (blastit does not size a pool it does not own), and a loud weaker-mode warning at boot
  (no `execve` env strip, no CPU metering). A manifest that sets `workers` on
  `[route.fastcgi]` is an unknown key naming the key.
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
- `[route.fastcgi]` with `workers` is refused naming the key; with `listen` alone it parses.
- `grep -rn FastCgiPoolSocketInvalid crates/` is empty; no dead variant remains.
- Every rule has a denied case and its accepted twin (skill `testing` §3).
- The page is `MB.3.10`'s; do not touch `docs/**` here beyond the fence count if it moves.

# Notes

- Do not add the `libphp.so` probe here (that is `MB.3.5.2`, `MB.3.13`): this leaf is
  grammar + executor kinds only.
- `MB.3.5.3` (route-key list) lands with or after this, so the diagnostic names `php` and
  `fastcgi`.
