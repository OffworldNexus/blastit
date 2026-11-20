# Log MB.56.3.5

## 2026-11-20T18:45 [lead]

created (task) Finish consolidation: four suites + minimal common/, <=5,500 LOC, fix dead test legs

## 2026-11-20T18:50 [lead]

edited fields

## 2026-11-20T18:50 [lead]

GOAL UPDATED (from MB.56.3.1 review): also retire e2e/dx_replay.py (reads deleted dx-* fixtures) + amend docs/design/view-crates-architecture.md R14; fold the 4 spawn helpers into common/.

## 2026-11-20T19:50 [lead]

ALSO (from MB.56.3.2.2): Makefile PHP_SUITE_ARGS/rs-test-php*, e2e/php-sapi/*, .github/workflows/php-sapi.yml and scripts/platform_matrix.py still name the deleted critical_php_embed/critical_php_backend/critical_parallel_views suites - retarget to e2e_dev.

## 2026-11-20T20:56 [lead]

FROM MB.56.3.4: Makefile:203 still runs --test no_python_build -> e2e_prod_refusals; ci.yml root-test exclusion list names critical_dev_and_fail_closed (EXCLUDED_COUNT=2) -> update; make rs-no-default clippy is red for e2e_dev.rs (dead-code under zero features) and e2e_prod_refusals.rs shares it.

## 2026-11-20T21:05 [lead]

FROM MB.56.3.4: Makefile:203 + ci.yml root-test exclusion list now point at the consolidated suites - verify; e2e_prod_refusals is 2,2xx LOC (PHP stub/helper half is the split candidate if the 5,500 total is exceeded).
