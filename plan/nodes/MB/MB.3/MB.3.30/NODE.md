---
id: MB.3.30
title: "Docs guard: php-fpm/FastCGI/[route.fastcgi] cannot return to user-facing pages"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.3.10]
paths: [python/tests/test_docs_assets.py]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T19:49"
updated: "2026-11-14T19:49"
---

# Goal

Make the PHP-naming sweep durable: a docs test that fails if a user-facing page
describes Blastit's PHP as `php-fpm` / `FastCGI`, or teaches `[route.fastcgi]`.
PHP is supported via embedded `libphp` (ADR-0010); the old words are history.

# Context

- `MB.3.10` removes the four `php.md` mentions, the `docs/frameworks/index.md`
  nav label, and `docs/frameworks/static-and-spa.md:129` ("the FastCGI worker").
  Without a test the words come back the first time someone copies an old
  paragraph.
- `make docs-unit` already runs `python/tests/test_docs_assets.py` (offline,
  no wheel); this guard belongs there, not in a new file that the Makefile
  would have to learn.
- The distinction is **attack class vs our transport**: `docs/decisions/**`
  (ADR-0008 is the superseded record), `docs/design/**`, `docs/research/**`
  and `docs/testing/**` may say `FastCGI` freely. `docs/VISION.md` may say it
  only when naming the attack ("FastCGI/CGI parameter trick", "HTTP/FastCGI
  desync"), never as what Blastit runs.

# Acceptance

```console
$ make docs-unit
$ make check
```
- Scanned: `docs/frameworks/**`, `docs/deploy/**`, `docs/reference/**`,
  `README.md`. Fail on any of, case-insensitive: `php[-_ ]?fpm`,
  `php-fpm.conf`, `[route.fastcgi]`, `route.fastcgi`, `fastcgi`.
- `docs/VISION.md`: fail on `php[-_ ]?fpm`, `php_fpm`, `route.fastcgi` always;
  allow `FastCGI` only on a line with an explicit attack-class marker
  (`attack`, `desync`, `parameter trick`, `smuggling`). Keep the marker list in
  one named constant with a comment naming ADR-0010.
- Not scanned: `docs/decisions/**`, `docs/design/**`, `docs/research/**`,
  `docs/testing/**`.
- Failure message names `file:line`, the matched word, and the replacement
  ("embedded `libphp`, `[route.php]`; see ADR-0010").
- Tests in `python/tests/test_docs_assets.py` cover both directions with
  crafted strings (a product-docs hit fails; a decisions/design hit passes; a
  VISION attack-class line passes; a VISION "runs php-fpm" line fails) **and**
  run the guard over the real tree, so the residue cannot reappear.
- Mutation proof in RESULT: add `php-fpm` to a scanned page, `make docs-unit`
  fails naming it; restore and it passes.

# Notes

- Keep it stdlib-only and fast (no network, no wheel, no subprocess).
- Do not edit `docs/decisions/**` to satisfy the guard; those files are the
  record.
