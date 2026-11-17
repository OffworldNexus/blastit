# Log MB.24

## 2026-11-17T01:23 [planner]

created (epic) Parallel PHP workers: several view processes per route under the scheduler's max_workers (matching Python's on-demand threads)

## 2026-11-17T06:29 [conductor]

Mandatory 0.2.0 companion: execute MB.26.8 right after MB.24.4 (or fold its one-slot 8-concurrent-GETs x 20 regression test into MB.24.3/MB.24.4 and prove MB.26.8) so neither multi-worker nor 1-slot queued PHP dispatch can ever answer 502 view_died on retirement!

## 2026-11-17T10:18 [lead]

gate passed: all four children done; plan validate OK; blastit-scheduler + critical_parallel_views green; make check exit 2 only on the MB.23.5-owned --no-reload fence (169 targets ok); make e2e exit 0 (263 ok). RESULT.md written.

## 2026-11-17T10:18 [lead]

status ready -> done
