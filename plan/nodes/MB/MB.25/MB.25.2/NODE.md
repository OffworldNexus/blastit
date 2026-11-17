---
id: MB.25.2
title: "Emit the INFO runtime event: PHP version + php_ini_opened_path via the C SAPI, Python version + environment, static-only summary; retire the WARN line"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.25.1]
paths:
  - "crates/blastit-events/src/**"
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/php_view.rs
  - crates/blastit/src/zygote.rs
  - crates/blastit/src/immortal.rs
  - "crates/blastit-php-sapi/c/**"
  - "crates/blastit-php-sapi/src/**"
  - "crates/blastit-php-sapi/tests/critical_shim_contract.rs"
  - crates/blastit/tests/critical_php_backend.rs
  - crates/blastit/tests/critical_php_embed.rs
  - "docs/design/runtime-diagnostics.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T01:30"
updated: "2026-11-17T01:30"
---

# Goal

Emit the `INFO` runtime line exactly as `MB.25.1` documented it, in both the JSON stream and
the `blastit dev` pretty line, for all three boot shapes — and stop using
`WARN manifest.warning` for a runtime that resolved. Warnings stay for actionable problems
only.

Concretely: a new `EventKind` (name, fields and level from the docs), PHP's active
`php.ini` path exposed by the C SAPI (`php_ini_opened_path`), Python's version and
environment, a static-only variant, and the existing critical/standard tests updated to
assert the new level, the new fields and the absence of the shim path in normal output.

# Context

- Emit sites to replace: `crates/blastit/src/stage0.rs:2584-2597` (serve path) and
  `:2870-2888` (`dev`), both currently
  `EventKind::ManifestWarning(WarningEvent::new(php_runtime_line(&runtime)))`; the sentence
  helper is `php_runtime_line` (`stage0.rs:2295-2316`).
- The event model: `EventKind` + `default_level` (`crates/blastit-events/src/lib.rs:1300-1370`),
  JSON encode/decode and the round-trip fixtures (`json.rs:206,784,1157`), the pretty
  renderer (`pretty.rs:119`). The runtime event must round-trip and appear in the pretty
  format; keep `MAX_EVENT_BYTES` and 2048-byte truncation rules unchanged.
- PHP `php.ini`: the shim today sets `php_ini_ignore = 0`, `php_ini_ignore_cwd = 1`
  (`crates/blastit-php-sapi/c/blastit_php_sapi.c:446-458`). Add one exported accessor for
  `php_ini_opened_path` (a `main/php_ini.h` global) next to the version symbol, read it
  after `php_module_startup`, and carry it to the boot through the existing probe/report
  channel (`crates/blastit-php-sapi/src/{loader,abi}.rs`, `crates/blastit/src/php_view.rs`).
  A host where the ini cannot be read reports `null`, never a guessed path.
- Python: `py.version_info()` and `sys.prefix`; `VIRTUAL_ENV` when set. `immortal.rs:340`
  already reads the version. Report from the earliest point the interpreter exists; do
  **not** change `stage1.rs` or `supervise.rs` (owned by `MB.24.4`/`MB.24.3`).
- `PHPRC`/`PHP_INI_SCAN_DIR` are stripped from the view environment; the docs say so once,
  the code just has to report the file it actually opened.
- Tests that must move with the change: `crates/blastit-php-sapi/tests/critical_shim_contract.rs`
  (the C-source and symbol contract, `:571`), `crates/blastit/tests/critical_php_backend.rs`
  (whole-boot "one PHP backend" scan — it must see the new line and still find no second
  transport), `crates/blastit/tests/critical_php_embed.rs` (boot-output assertions). These
  files are listed in `paths`, and this task explains the edit: the line they pinned moved
  from `manifest.warning` to `runtime`.
- No new dependency; `Cargo.toml`/`Cargo.lock` unchanged. `crates/blastit-events` stays
  `#![forbid(unsafe_code)]`; the `unsafe` stays in `blastit-php-sapi`'s loader.

# Acceptance

```console
$ cargo test -p blastit-events
$ cargo test -p blastit-php-sapi
$ cargo test -p blastit --test critical_php_backend
$ BLASTIT_LIBPHP=/usr/lib/libphp8.5.so cargo test -p blastit --test critical_php_embed
$ make check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
```

And the three boot tails in `RESULT.md`, verbatim, each showing `"level":"info"`,
`"event":"runtime"`, the documented fields, **no** `manifest.warning` for a resolved
runtime, and **no** shim path:

1. `entrypoint php` fixture — `version`, `libphp`, `php_ini` (a real path, or `null` with
   the reason);
2. `entrypoint python` fixture — `version`, `environment` (the active prefix);
3. static-only fixture — `entrypoint: "static"` and nothing else.

A PHP host with `BLASTIT_LIBPHP` unset still fails closed with the existing package
sentence, and that path emits no runtime line (assert it).

# Notes

- Do not soften a real warning to keep a test green; if a test asserted the old wording of a
  genuine warning, fix the test's message, not the warning.
- Keep every field free of secrets and of internal URLs; the libphp path is public
  knowledge, the shim path is not reported.
- Never name the retired manifest format; never pair PHP with a second transport.
