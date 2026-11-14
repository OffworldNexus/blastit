---
id: MB.3.10
title: "Retarget the PHP page and reference onto [route.php] + libphp-embed + BLASTIT_LIBPHP (ADR-0010)"
kind: task
status: ready
owner: "doc-writer"
priority: 1
depends_on: [MB.3.9.1]
paths:
  - docs/frameworks/php.md
  - docs/frameworks/index.md
  - docs/reference/manifest.md
  - docs/reference/settings.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/php-dev-local.review-*.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T14:39"
---

# Goal

Retarget the PHP page and reference onto embedded PHP: `[route.php]`,
`libphp-embed` + `BLASTIT_LIBPHP`, and a short documented external `[route.fastcgi]`
fallback.

# Context

- ADR-0010 §5 renames `docs/frameworks/php.md`: `[route.fastcgi]` → `[route.php]`
  (`entry = "public/index.php"`, optional `workers`, no `listen`), the runtime line becomes
  `libphp-embed` (distro package names) with `BLASTIT_LIBPHP=/path/to/libphp.so` as the
  override, and the page gains a brief fallback note for operator-managed external
  `php-fpm` sockets (`[route.fastcgi]` + `listen`, weaker mode).
- `docs/reference/settings.md`: replace the `BLASTIT_PHP_FPM` row with `BLASTIT_LIBPHP`
  (what it does → default search: distro NTS `libphp8.4/8.3/8.2/libphp.so` → env-only, never
  a manifest key).
- `docs/reference/manifest.md`: add the `[route.php]` row/section and the narrowed
  `[route.fastcgi]` row; keep every `[route.static]` sentence exactly as it is.
- The page's H1 and the nav label still say "PHP-FPM": retitle to PHP (Symfony, Laravel)
  in `docs/frameworks/php.md`, `docs/frameworks/index.md` and `zensical.toml`.
- Keep the page lean: two steps in Local (write the one-line manifest, run `blastit dev`),
  install line honest, fences accounted or executed truthfully.

# Acceptance

```console
$ make docs-check
$ make docs-test
$ make check
```
- `grep -c 'php-fpm -F' docs/frameworks/php.md` = 0; the page names `[route.php]`, not
  `[route.fastcgi]`, as the first-class block, and mentions the fallback in one short note.
- `grep -rn 'PHP-FPM' docs/frameworks/php.md docs/frameworks/index.md zensical.toml` = 0
  except where the fallback genuinely is php-fpm.
- Fresh `dx` + `ai-ergonomics` critics PASS (a Symfony developer sees what they write and
  what blastit provides; nothing is a knob they should not own).
- Every fence is executed or explicitly accounted with a true note.

# Notes

- Do not invent package-manager commands beyond the ADR's distro package names
  (`libphp-embed`, `php-embedded`); the wheel/standalone split is the ADR's §4 table and is
  `MB.3.8`'s or the distribution node's, not this page's.
