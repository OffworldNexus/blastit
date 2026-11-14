# Log MB.3.30

## 2026-11-14T19:49 [planner]

created (task) Docs guard: php-fpm/FastCGI/[route.fastcgi] cannot return to user-facing pages

## 2026-11-14T20:33 [lead]

edited fields

## 2026-11-14T20:33 [lead]

OWNER REITERATION 2026-11-14: the guard now also covers the PLAN. No live NODE.md (status not done/superseded; LOG/RESULT are records) may contain php-fpm/PHP-FPM/php_fpm/route.fastcgi; FastCGI only on an attack-class line (attack|desync|parameter trick|smuggling|CVE), same rule as VISION; skip MB.3.30/NODE.md (defines the list). Also flag PHP-fallback OFFERS (e.g. 'fallback to php', 'use php-fpm as a fallback') but not denials ('there is no fallback'). New prerequisite MB.3.31 performs the prose purge across all live briefs; this node depends on it. Guard runs in make check via docs-unit (python/tests/test_docs_assets.py).

## 2026-11-14T20:33 [lead]

edited fields

## 2026-11-14T20:36 [lead]

OWNER 2026-11-14: guard now also fails on crates/**/*.rs citing docs/design/php-fastcgi.md (dead-link trap). Runs in make check via docs-unit. No attack-class allowance for a path citation.
