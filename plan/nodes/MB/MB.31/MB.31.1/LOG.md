# Log MB.31.1

## 2026-11-17T12:33 [planner]

created (task) Document one immutable pattern grammar (bare, view-syntax, regex) and purge the six-glob staircase from every page

## 2026-11-17T22:12 [lead]

status ready -> active

## 2026-11-17T22:12 [lead]

attempt 1/3 started

## 2026-11-17T22:12 [lead]

base df44f98

## 2026-11-17T22:32 [lead]

edited fields

## 2026-11-17T22:32 [lead]

docs done: three shapes documented against pattern.rs, staircase purged from Django/reference/SPA/Laravel/ADR-0012/ADR-0007/designs, docs_fences pinned to the single pattern, dx+ai-ergonomics critics accept with 0 open BLOCKER/MAJOR. laravel.md added to paths: the dx BLOCKER lived there, outside the original list.

## 2026-11-17T22:32 [lead]

status active -> review

## 2026-11-17T22:46 [lead]

edited fields

## 2026-11-17T22:46 [lead]

reviewer VERDICT PASS (6 MINOR). Fixed the path names to the real critic files (immutable-pattern-grammar.review-*). The other five MINORs are doc-accuracy items on pages MB.31.2/.3/.4 also touch and are handed there: the pattern name rule ([a-z][a-z0-9_]*, once per pattern) is undocumented; django.md's 'reaches every depth' sentence is only true once MB.31.2/.3 land; the hashed-name example is Django-specific; the 256-byte table in design/static-and-spa.md is stale; and this RESULT's 'the plan was not mutated' line is inaccurate because the lead added laravel.md to paths.

## 2026-11-17T22:46 [lead]

status review -> done
