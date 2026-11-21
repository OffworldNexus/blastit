# Log MB.3.10

## 2026-11-14T14:39 [lead]

created (task) Retarget the PHP page and reference onto [route.php] + libphp-embed + BLASTIT_LIBPHP (ADR-0010)

## 2026-11-14T15:14 [lead]

RETARGETED (owner 2026-11-14, 'No fallback please'): the docs retarget teaches [route.php] + libphp-embed + BLASTIT_LIBPHP ONLY. The fail-closed switch (BLASTIT_ALLOW_EXTERNAL_FASTCGI) and the [route.fastcgi] fallback note I added to this body earlier are SUPERSEDED by that decision -- do not document a switch whose removal is MB.3.17's. Still HELD: docs/frameworks/** is the architect's path while its lenses run.

## 2026-11-14T19:49 [lead]

edited fields

## 2026-11-14T19:49 [lead]

RETARGETED by lead to the owner's 2026-11-14 directive: PHP is supported (embedded libphp) and php-fpm/FastCGI/[route.fastcgi] must disappear from what blastit does. Removed the stale 'short documented external [route.fastcgi] fallback' from the Goal and the acceptance (it contradicted the Context's own 'no fallback' note), added docs/frameworks/static-and-spa.md (line 129 'the FastCGI worker' -> embedded PHP worker), and made the acceptance a ZERO-hit grep on php-fpm/php_fpm/FastCGI/route.fastcgi across php.md+index.md+static-and-spa.md+zensical.toml. settings.md stays MB.3.13.2's row (one owner); php.md links without the #backends anchor if it would be empty. The durable guard is MB.3.30.

## 2026-11-14T20:37 [lead]

edited fields

## 2026-11-14T20:37 [lead]

OWNER 2026-11-14: the docs_fences STALE row for php.md is deleted in THIS change (not kept as a compat shim) since the page is retargeted here; added crates/blastit-manifest/tests/docs_fences.rs. MB.3.20 no longer owns that row.

## 2026-11-14T20:43 [lead]

status ready -> active

## 2026-11-14T20:43 [lead]

attempt 1/3 started

## 2026-11-14T20:43 [lead]

base e489e4e; .28 landed 39c02f7 (php refused until .3 wires it); owner directive: PHP IS supported via embedded libphp, no fallback, zero php-fpm/FastCGI hits in the three framework pages, and the docs_fences STALE row goes in this same change.

## 2026-11-14T21:25 [lead]

status active -> done
