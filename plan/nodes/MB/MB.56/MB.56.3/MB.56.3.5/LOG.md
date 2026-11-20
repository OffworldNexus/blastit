# Log MB.56.3.5

## 2026-11-20T18:45 [lead]

created (task) Finish consolidation: four suites + minimal common/, <=5,500 LOC, fix dead test legs

## 2026-11-20T18:50 [lead]

edited fields

## 2026-11-20T18:50 [lead]

GOAL UPDATED (from MB.56.3.1 review): also retire e2e/dx_replay.py (reads deleted dx-* fixtures) + amend docs/design/view-crates-architecture.md R14; fold the 4 spawn helpers into common/.

## 2026-11-20T19:50 [lead]

ALSO (from MB.56.3.2.2): Makefile PHP_SUITE_ARGS/rs-test-php*, e2e/php-sapi/*, .github/workflows/php-sapi.yml and scripts/platform_matrix.py still name the deleted critical_php_embed/critical_php_backend/critical_parallel_views suites - retarget to e2e_dev.
