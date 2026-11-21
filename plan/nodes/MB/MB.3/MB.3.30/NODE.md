---
id: MB.3.30
title: "Docs + plan guard: the retired PHP transport vocabulary and PHP fallbacks cannot return"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.10, MB.3.31]
paths:
  - python/tests/test_docs_assets.py
  - docs/VISION.md
  - docs/reference/manifest.md
  - plan/nodes/MB/MB.3/MB.3.20/NODE.md
attempts: 4
max_attempts: 4
escalation: none
created: "2026-11-14T19:49"
updated: "2026-11-14T23:06"
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

## Owner reiteration (2026-11-14): the guard covers the plan too

"No fallback" is a hard invariant, not a docs style. Extend the same test:

- **Live plan briefs.** Scan `plan/nodes/**/NODE.md`; skip a file whose
  `status` is `done` or `superseded` (history), and skip
  `plan/nodes/MB/MB.3/MB.3.30/NODE.md` (it defines the list). Fail on
  `php[-_ ]?fpm`, `php_fpm`, `route.fastcgi` anywhere, and on `fastcgi` unless
  the same line names the attack class (`attack`, `desync`, `parameter trick`,
  `smuggling`, `CVE`) — the same distinction the docs scan makes for
  `docs/VISION.md`. `LOG.md`/`RESULT.md` are records and are not scanned.
- **No fallback offer.** In user-facing docs and live briefs, fail on a
  *fallback offer* near PHP (`fallback to` / `as a fallback` / `use … as a
  fallback` within one sentence of `php`/`fastcgi`), but not on a denial
  ("there is no fallback; embedded `libphp` is the only way"). A denial is the
  message we want; an offer is the defect.
- **Green only with `MB.3.31`.** The purge node rewrites the live briefs first;
  this guard lands after it and keeps them purged. Failure names `file:line`,
  the word, and the truth to use instead (embedded `libphp`, `[route.php]`,
  ADR-0010).

## Owner: also fail on a code comment citing the deleted design (2026-11-14)

Add a third scan: any `crates/**/*.rs` that references
`docs/design/php-fastcgi.md` fails, naming `file:line`. A comment citing a
deleted file as authority is the exact trap that costs someone an hour (it is
not a word-style issue, so no attack-class allowance). Keep every existing
exemption. This scan needs no extra path: the guard only reads.
