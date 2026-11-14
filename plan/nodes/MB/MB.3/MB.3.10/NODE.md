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
  - "docs/frameworks/static-and-spa.md"
  - docs/reference/manifest.md
  - docs/reference/settings.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/php-dev-local.review-*.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T19:49"
---

# Goal

Retarget the PHP page and reference onto embedded PHP only: `[route.php]`,
`libphp-embed` + `BLASTIT_LIBPHP`. Owner directive 2026-11-14: *"either PHP is
supported, either it's not, but don't go half ass in there"* — PHP **is**
supported (embedded `libphp`, ADR-0010), so `php-fpm` / `FastCGI` /
`[route.fastcgi]` disappear from every page that describes what Blastit does.
No external fallback note, no `listen`, no `php-fpm.conf`, no "run the pool".

# Context

- ADR-0010 §5 renames `docs/frameworks/php.md`: `[route.fastcgi]` → `[route.php]`
  (`entry = "public/index.php"`, optional `workers`, no `listen`), the runtime line becomes
  `libphp-embed` (distro package names) with `BLASTIT_LIBPHP=/path/to/libphp.so` as the
  override. Embedded `libphp` is the **only** PHP story; the page names the boot refusal
  when the host has no usable NTS `libphp`.
- `docs/reference/settings.md`: no `BLASTIT_PHP_FPM` row; the `BLASTIT_LIBPHP` row is
  `MB.3.13.2`'s (one owner). If the `#backends` anchor would be empty before that lands,
  keep `php.md` linking to `settings.md` without the anchor rather than duplicating the row.
- `docs/reference/manifest.md`: add the `[route.php]` row/section; **do not** add a
  `[route.fastcgi]` row; keep every `[route.static]` sentence exactly as it is.
- `docs/frameworks/static-and-spa.md:129`: "the FastCGI worker that runs
  `public/index.php`" becomes the embedded PHP worker.
- The page's H1 and the nav label still say "PHP-FPM": retitle to PHP (Symfony, Laravel)
  in `docs/frameworks/php.md`, `docs/frameworks/index.md` and `zensical.toml`.
- **There is no external fallback** (owner decision 2026-11-14, "No fallback please"):
  `[route.php]` is the only PHP block in the page and the reference. Do not document
  `[route.fastcgi]`, a `listen` key, or a `BLASTIT_ALLOW_EXTERNAL_FASTCGI` switch anywhere;
  if the page needs to say what happened to it, one clause at most, pointing at
  `[route.php]`. The removal itself is `MB.3.17`'s — this leaf only stops teaching it.
- Keep the page lean: two steps in Local (write the one-line manifest, run `blastit dev`),
  install line honest, fences accounted or executed truthfully.

# Acceptance

```console
$ make docs-check
$ make docs-test
$ make check
```
- `grep -rniE 'php[-_ ]?fpm|fastcgi|route\.fastcgi' docs/frameworks/php.md
  docs/frameworks/index.md docs/frameworks/static-and-spa.md zensical.toml` = **0**.
- `docs/frameworks/php.md` names `[route.php]` (entry + optional `workers`) as the only
  PHP block, `libphp-embed` + `BLASTIT_LIBPHP` as the runtime, and the boot refusal when
  the host has no usable NTS `libphp`. No `listen`, no `php-fpm.conf`, no pool step.
- `docs/frameworks/static-and-spa.md:129` says "the embedded PHP worker", not "the
  FastCGI worker".
- Fresh `dx` + `ai-ergonomics` critics PASS (a Symfony developer sees what they write and
  what blastit provides; nothing is a knob they should not own).
- Every fence is executed or explicitly accounted with a true note.

# Notes

- Do not invent package-manager commands beyond the ADR's distro package names
  (`libphp-embed`, `php-embedded`); the wheel/standalone split is the ADR's §4 table and is
  `MB.3.8`'s or the distribution node's, not this page's.

## Release fold (owner, 2026-11-14)

There is no 0.2.1 release: everything goes into **0.2.0**. Every sentence of the form
"`0.2.1` adds …" on the pages you touch becomes **0.2.0**. Add a grep to the acceptance
(`grep -rn '0\.2\.1' docs/frameworks/php.md docs/reference/`) so it is verified, not
remembered.
