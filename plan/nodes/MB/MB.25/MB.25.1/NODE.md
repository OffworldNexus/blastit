---
id: MB.25.1
title: "Document the INFO runtime event and the two dev lines (PHP version + php.ini, Python version + environment) with dx + ai-ergonomics critics"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MB.23.1]
paths:
  - "docs/reference/log-events.md"
  - docs/local.md
  - "docs/design/runtime-diagnostics.review-dx.md"
  - "docs/design/runtime-diagnostics.review-ai-ergonomics.md"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - python/tests/runnable_docs/inventory.toml
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T01:28"
updated: "2026-11-17T06:13"
---

# Goal

Write the event contract before the code: one new `INFO` runtime event, its exact fields per
entrypoint, the `dev` line a developer reads, and the reminder that `WARN manifest.warning`
is only for things that need attention. No Rust in this task.

After this task `docs/reference/log-events.md` documents the runtime event beside `boot`,
`ready` and `shutdown`, and `docs/local.md` shows the one line `blastit dev` prints for a
PHP file and for a Python file. `MB.25.2` implements exactly these words.

# Context

- Today the table row is `manifest.warning` → *"the manifest is usable but has a problem"*
  (`docs/reference/log-events.md:37`), and nothing documents runtime discovery at all.
- The line to replace is the owner's live output from `0.2.0`:
  `WARN manifest.warning the entrypoint php runtime is libphp /usr/lib/libphp8.5.so and shim .../libblastit_php85-x86_64-gnu.so (PHP 8.5, x86_64-gnu)`. It names an internal
  shim path, carries no `php.ini`, and is a warning about something that worked.
- Proposed event (settle the name here, implement it verbatim): `runtime`, `level: info`,
  `component: "init"`, with `entrypoint` (`"php"|"python"|"static"`) and only the fields
  that entrypoint has:
  * `php`: `version` (`"8.5.4"`), `libphp` (resolved path), `php_ini`
    (`/etc/php/8.5/embed/php.ini` or `null`), and one note that `PHPRC` /
    `PHP_INI_SCAN_DIR` are ignored;
  * `python`: `version` (`"3.14.0"`) and `environment`
    (`VIRTUAL_ENV` when set, else `sys.prefix`);
  * `static`: neither.
  No shim path, no build tuple, no library soname list.
- `docs/local.md` and `docs/reference/log-events.md` are owned by `MB.23.1` until it lands;
  depend on it and never edit concurrently. Add no new page and no `zensical.toml` nav
  entry, so `python/tests/runnable_docs/inventory.toml` is untouched unless a fence is
  added — in which case register it.
- Lean rules: the pretty `dev` line is **one** line under 160 characters; the JSON shape is
  a 4-row table, not prose.

# Acceptance

```console
$ grep -n "runtime" docs/reference/log-events.md | head
$ ! grep -rn "shim" docs/local.md && echo ok      # no internal path in the developer page
$ make docs-check
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/runnable_docs -q
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ uv run --no-project tools/plan.py validate
```

Then a fresh-context `critic` pass with the `dx` and `ai-ergonomics` lenses, recorded as
`docs/design/runtime-diagnostics.review-dx.md` and
`docs/design/runtime-diagnostics.review-ai-ergonomics.md`, both ending `VERDICT: PASS`.
Quote the final table rows and the two `dev` lines in `RESULT.md`.

# Notes

- Say plainly when a warning *is* expected, so the page teaches the difference:
  `egress any`, `postgres` without a policy, an unacknowledged `raw` body, an unused
  `trait`, and `dev.reload.unavailable`.
- Never name the retired manifest format in a page; never pair PHP with a second transport.
