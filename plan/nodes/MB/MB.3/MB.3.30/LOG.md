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

## 2026-11-14T22:30 [lead]

edited fields

## 2026-11-14T22:30 [lead]

status ready -> active

## 2026-11-14T22:30 [lead]

attempt 1/3 started

## 2026-11-14T22:30 [lead]

base 66fccd512b4855ca43ba461cfb1627da93024801; lead widened paths with docs/VISION.md: its two denial lines (33,47) name the retired words and the always-fail rule needs them rewritten

## 2026-11-14T22:38 [lead]

status active -> review

## 2026-11-14T22:40 [lead]

REVIEW FAIL (attempt 1): (1) MAJOR the fallback-offer scan is line-based and cuts at any '.', so realistic offers pass ('Fall back to a daemon, e.g. for PHP routes.', 'Use the daemon as a fallback (see docs/...) for PHP.'); (2)+(3) MINORs: no test pins the transport-vs-attack lastgroup rule for php-fpm/route.fastcgi on attack-marked lines, and the crates dead-citation scan has no crafted negative.

## 2026-11-14T22:40 [lead]

status review -> ready

## 2026-11-14T22:40 [lead]

status ready -> active

## 2026-11-14T22:41 [lead]

attempt 2/3 started

## 2026-11-14T22:48 [lead]

REVIEW FAIL (attempt 2): FALLBACK_DENIAL (:203) treats any no|not|never|without|zero earlier in the sentence as a denial, so conditional offers pass silently: 'If libphp is not available, fall back to a daemon for PHP.', 'Without libphp, fall back to a daemon for PHP routes.', 'Do not stop at PHP: use a daemon as a fallback.'

## 2026-11-14T22:48 [lead]

status active -> ready

## 2026-11-14T22:48 [lead]

status ready -> active

## 2026-11-14T22:48 [lead]

attempt 3/3 started

## 2026-11-14T22:55 [lead]

REVIEW FAIL (attempt 3): denial is sentence-scoped so a contrasting clause's offer passes ('no fallback — you may fall back to a daemon for PHP', 'no fallback exists; use a daemon as a fallback', ', but you may use', ': fall back to'); MINOR: verb-negated denials ('Never run a daemon as a fallback') read as offers; MINOR: noun-form offers ('The fallback for PHP routes is a daemon') are invisible. Lead raised max_attempts to 4 and specified a clause-scoped rule with an approved-denial shape list; if this attempt fails the node goes to expert.

## 2026-11-14T22:55 [lead]

edited fields

## 2026-11-14T22:55 [lead]

status active -> ready

## 2026-11-14T22:55 [lead]

status ready -> active

## 2026-11-14T22:55 [lead]

attempt 4/4 started

## 2026-11-14T23:03 [lead]

edited fields

## 2026-11-14T23:03 [lead]

lead: paths widened for the two prose edits the rule forced (docs/reference/manifest.md:314, MB.3.20 brief) — reviewer flagged them as silent edits

## 2026-11-14T23:06 [lead]

status active -> done
