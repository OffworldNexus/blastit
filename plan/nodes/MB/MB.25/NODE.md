---
id: MB.25
title: "Clean runtime startup diagnostics: INFO runtime line (PHP version + php.ini, or Python version + virtualenv) instead of WARN manifest.warning"
kind: epic
status: ready
priority: 1
depends_on: [MB.21.5.2]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T01:28"
updated: "2026-11-17T01:28"
---

# Goal

A `blastit dev` boot currently greets a PHP developer with a **warning** about the thing
that just worked:

```text
00:11:36.867 WARN manifest.warning the entrypoint php runtime is libphp /usr/lib/libphp8.5.so and shim .../libblastit_php85-x86_64-gnu.so (PHP 8.5, x86_64-gnu)
```

Nothing here needs attention, so warning it teaches developers to ignore warnings. Replace
the abuse of `WARN manifest.warning` with one clean **`INFO` runtime line** that says what
they actually need to know, tailored to the entrypoint the file declares:

- **`entrypoint php`** — `PHP 8.5.x`, the `libphp` it will load, and the `php.ini` in force
  (`/etc/php/8.5/embed/php.ini`, or `none` when no file is opened, with Blastit's
  `PHPRC`/`PHP_INI_SCAN_DIR` stripping stated once). No internal shim path in normal output.
- **`entrypoint python`** — `Python 3.x.y` and the environment it serves from (`VIRTUAL_ENV`
  or `sys.prefix`).
- **static only, no entrypoint** — one clean static-only summary.

A warning must again mean *something needs attention* (`egress any`, `postgres
unrestricted`, an unacknowledged `raw` body, an unused `trait`, `dev.reload.unavailable`,
an `entrypoint php` no view reaches). This is owner-reported DX debt from testing `0.2.0`.

Mandatory for `0.2.0`; `depends_on: [MB.21.5.2]`.

# Context

- Both abuses are in `crates/blastit/src/stage0.rs`: the production serve path at `:2590`
  and `dev` at `:2881` emit `EventKind::ManifestWarning(WarningEvent::new(php_runtime_line(..)))`;
  the sentence itself is `php_runtime_line` (`stage0.rs:2295-2316`) — it names the shim
  path, which is internal, and it carries no `php.ini`.
- `docs/reference/log-events.md:37` defines `manifest.warning` as *"the manifest is usable
  but has a problem"*. A resolved runtime is not a problem: the event **kind and level** are
  the defect, not just the wording (`crates/blastit-events/src/lib.rs:1324-1370`).
- Adding an event means wiring all four surfaces: `EventKind` + level in
  `crates/blastit-events/src/lib.rs`, JSON encode/parse in `json.rs`, the pretty line in
  `pretty.rs`, and the docs table.
- Python version/prefix are already reachable: `py.version_info()` is used in
  `crates/blastit/src/immortal.rs:340,706`; `std::env::var("VIRTUAL_ENV")` and
  `sys.prefix` give the environment. PHP has **no** ini path exposed yet: the shim sets
  `php_ini_ignore_cwd = 1` and `php_ini_ignore = 0`
  (`crates/blastit-php-sapi/c/blastit_php_sapi.c:446-458`), so `php_ini_opened_path` (a
  `main/php_ini.h` global) must be surfaced through the C SAPI the way the version already
  is, and the config strips `PHPRC`/`PHP_INI_SCAN_DIR` so the reported file is the truth.
- `critical_php_backend.rs`'s "no alternative transport" contract scans the whole boot
  output; the new line must keep that true (it names one library, never an alternative).
- Docs owners ahead of this epic: `MB.23.1` owns `docs/reference/log-events.md` and
  `docs/local.md`, so `MB.25.1` lands after it. `MB.22`/`MB.23` never touch
  `crates/blastit-events` or the PHP SAPI, so `paths` do not collide.

# Acceptance

```console
$ uv run --no-project tools/plan.py validate
$ cargo test -p blastit-events
$ cargo test -p blastit-php-sapi
$ cargo test -p blastit --test critical_php_backend --test critical_php_embed
$ make check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
```

and one live boot of each shape pasted into `RESULT.md`, showing `"level":"info"` and no
`manifest.warning` for a resolved runtime:

```console
$ uv run blastit dev            # entrypoint php fixture -> INFO runtime, PHP version, php.ini
$ uv run blastit dev            # entrypoint python fixture -> INFO runtime, Python version, env
$ uv run blastit dev            # static-only fixture -> INFO runtime, static-only summary
```

# Notes

- Nothing about discovery, resolution or refusal changes: a host without `libphp` still
  fails closed with the same sentence.
- `WARN` stays for things a developer must act on; do not reclassify a real manifest
  warning to make this task smaller.
- Never name the retired manifest format, and never pair PHP with a second transport or a
  degraded mode.
