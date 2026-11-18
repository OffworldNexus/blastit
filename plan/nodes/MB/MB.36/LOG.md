# Log MB.36

## 2026-11-17T14:07 [planner]

created (epic) View crates architecture: blastit-view-{python,php,static} composed by Cargo features over view.chan, replacing the Upstream/WorkerRuntime trait soup (mandatory for 0.2.0)

## 2026-11-18T18:23 [lead]

Triage of the audit's four gap nodes against this epic's own Acceptance: MB.36.11 lands (R12 "no libpython/libphp DT_NEEDED" is in the epic title, and R3's deleted trait has no compile-time witness); MB.36.8/.9/.10 are priority-3 coverage that no acceptance line depends on, re-filed as MB.26.25/.26/.27 and superseded here. PRECEDENT: an epic may close with deferred coverage, but never with a deferred claim it made.

## 2026-11-18T18:35 [lead]

JOURNAL CORRECTION: the 18:05 lines recording `MB.36.7.1`-`.4` creations name nodes that do not exist. The test-curator created them under the *task* `MB.36.7`, `plan.py validate` then refused a task with children, and the four directories were removed with `rm -rf` (a plan mutation outside `plan.py`) before being re-created under this epic as `MB.36.8`-`.11`. Real ids: 8-11, of which 8/9/10 were then re-filed as `MB.26.25`/`.26`/`.27` and superseded, and 11 stays here.

## 2026-11-18T19:57 [lead]

Gate: seven leaves done (8/9/10 re-filed to MB.26.25/26/27 as deferred coverage, 11 landed). Acceptance at this tree: make check EXIT=0 (204 ok, 0 FAILED; docs-check included, no layout/navigation change so no visual pass is owed), make e2e EXIT=0 (411 ok, 0 FAIL), critical_guard --base 4aa39d9 --node .2 .3 .4 .5 -> 14 protected changes authorised (develop is not an ancestor; merge-base 2c05139), rs-no-python 0 pyo3, runtime.rs deleted, blastit-view links no interpreter and knows no SAPI, ADR-0016 indexed, no crate depends on blastit. test-curator audit: docs/testing/AUDIT-MB.36.md. PRECEDENT: an epic may close with deferred coverage, but never with a deferred claim it made.

## 2026-11-18T19:57 [lead]

status ready -> done
