# Log MB.40.2

## 2026-11-19T00:16 [conductor]

created (task) Fix the 4 live P0/High bugs: store-less check crash, table-grant $? check false-positive, --learn reload/probe bugs & coverage flush

## 2026-11-19T00:51 [lead]

status ready -> active

## 2026-11-19T00:51 [lead]

attempt 1/3 started

## 2026-11-19T00:51 [lead]

base 14b4047d71c784be607c556156aacbd2f858e98f

## 2026-11-19T01:05 [lead]

status active -> review

## 2026-11-19T01:10 [lead]

REVIEW FIX: (1) coverage.rs cfg(coverage) is never set by rustc -C instrument-coverage -> flush is dead code; gate on a real cargo feature/build.rs cfg and test the symbol is referenced. (2) restore_parameters leaves normalize '...' ellipsis -> IN ($?, ...) and VALUES ($?), ... still parse-fail => false missing_grant (Django __in/bulk_create); expand tail + allow/deny tests. (3) dev_learn fallback grants attribute probe SQL to the single legitimate view (probe paths filtered from views but not grants); refuse fallback grants when any unrouted path is unlearnable/static-covered + two-request test. (4) python/tests/test_check_pytest_gate.py double-close of adopted ctl fd -> 3/5 tests EBADF, CI would be red; fix teardown ownership. (5) note_self_write ordering cannot suppress the self-write; make it effective with a production-order test or delete the dead call. (6) stale plan_learn doc at audit_v1.rs:1481.

## 2026-11-19T01:25 [lead]

status review -> done

## 2026-11-19T01:25 [lead]

committed 88fd2cf
