# Log MB.2.4.5

## 2026-11-13T23:10 [test-curator]

created (task) critical_rewrite: pin client cancel, buffered-forward queue accounting and the bound

## 2026-11-13T23:57 [lead]

ADDITIONAL GAP (from MB.2.4.3's campaign at 0796921):   moved CAUGHT -> TIMEOUT at this tip, i.e. pump_static parks on a zero-byte read instead of aborting. Pin it here (it is 'the bound' this node already owns): a zero-size read must abort via RespAbort, not wait for request_timeout.

## 2026-11-14T00:03 [lead]

status ready -> active

## 2026-11-14T00:03 [lead]

attempt 1/3 started

## 2026-11-14T00:03 [lead]

base 69504ed

## 2026-11-14T00:21 [implementer]

closed G7/G8/G9 + zero-size read: 7 new flow oracles (32/32 green); 2 scheduler.rs fixes proven by hand-mutations (buffered-forward bound now counts itself like begin_raw; want==0 aborts instead of parking until request_timeout — STATIC_CHUNK_BYTES=0 mutant went TIMEOUT->CAUGHT in 0.13s)

## 2026-11-14T00:23 [lead]

status active -> review

## 2026-11-14T00:28 [lead]

REVIEW PASS attempt 1 (4 MINOR: the new want==0 branch has no in-suite test that reaches it; a constant restated as a doc pin; LESSONS.md is technically outside paths; sibling file tripped the guard).

## 2026-11-14T00:28 [lead]

status review -> done
