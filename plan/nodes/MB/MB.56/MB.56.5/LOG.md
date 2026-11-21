# Log MB.56.5

## 2026-11-20T16:02 [conductor]

created (task) Run make coverage to delete uncovered dead Rust code, strip historical diary comments & pass full E2E gates

## 2026-11-20T17:09 [lead]

DEFERRED COMMENTS: crates/blastit/src/stage0/gates.rs:272 + crates/blastit-view-php/src/pinning.rs:268 still name the deleted scripts/check-no-test-seam.sh (now scripts/build-standalone.sh:161-183); e2e/venv-smoke/run.sh:238, crates/blastit-sqlpolicy/tests/critical_*.rs headers, crates/blastit-manifest/tests/docs_fences.rs:6 too. Also e2e ACL: e2e/Dockerfile:6-7, e2e/rootless/{Dockerfile:18,README.md:139,171}, e2e/venv-smoke/Blastfile:5, e2e/dx_replay.py:438, python/blastit/_check_pytest.py:4 stale _bin/blastit-dbproxy prose -> MB.56.4 owns e2e/**, MB.56.5 owns python/**

## 2026-11-20T17:28 [lead]

crates/blastit-view-static::StaticRoot::open_with is the only seam that can prove the openat2+Walk matrix for MB.56.3 - do NOT prune it. Also candidate dead (no in-crate user): blastit/src/{coverage.rs, mem_usage.rs::advisory, unified.rs renderer, py.rs::refusal_message, forking.rs::agrees}, blastit-view-php::is_serveable_entry

## 2026-11-20T17:40 [lead]

From MB.56.2.2 coverage sweep: blastit-sys/src/mem.rs (13 pub, 3 used outside), proc.rs thread_count/chdir only via stage0; god-files >500 LOC to split/trim while stripping comments: blastit-events/src/lib.rs 1813, json.rs 1402, blastit-lsp/src/lib.rs 1386, blastit-php-sapi/src/loader.rs 1075, blastit-sys/src/fanotify.rs 1031, blastit/src/supervise.rs 2767, worker(pre)-1611, discovery 1576

## 2026-11-20T17:40 [lead]

From MB.56.2.2 coverage sweep: blastit-sys/src/mem.rs (13 pub items, 3 referenced outside crate), blastit-sys/src/proc.rs thread_count/chdir only reachable via stage0 fork_exec/gates; god-files >500 LOC: blastit-events/src/lib.rs 1813, json.rs 1402, blastit-lsp/src/lib.rs 1386, blastit-php-sapi/src/loader.rs 1075, blastit-sys/src/fanotify.rs 1031, crates/blastit/src/supervise.rs 2767, blastit-scheduler worker 1611, discovery 1576

## 2026-11-20T18:01 [lead]

PRE-EXISTING DX BUG (found by MB.56.2.3 review): docs/reference/blastfile.md:662 promises '<file>:<line>:<col>:' but blastit check prints 'blastit check: manifest ./Blastfile: 2:10: ...' (space breaks editor error matchers); and :665 promises an unset $IDENT 'names the view and the fix' but the CLI prints 'environment rewrite has 1 error(s) - proxy "db" input "url": ... is not set' with no position/view/fix

## 2026-11-20T18:03 [lead]

tools/coverage_report.py does not expand FILES globs and reports 'no instrumented coverage' for manifest files even when profdata covers them; coverage for craws/blastit-manifest/src/blastfile/sites.rs is 0.0% only because it is reached through the LSP path (crates/blastit/tests/critical_lsp.rs) - keep it.

## 2026-11-20T18:22 [lead]

blastit-sqlpolicy: StatementClass::is_plain + Analysis::is_plain (analysis/types.rs:83,113) have NO production consumer -> delete. tools/coverage_report.py mis-reports multi-crate/multi-file FILES invocations ('no instrumented coverage') - fix or document. sqlpolicy coverage reachable-but-untested: analysis/mod.rs 26.9%, resolve/mod.rs 50%, classify.rs 16.3%, statements.rs 72% ; blastit-http server.rs 22.6% (accept/503), reason.rs 13.6%

## 2026-11-20T20:19 [lead]

crates/blastit/src/dev_reload.rs still carries #[cfg(any(test, debug_assertions))] TestClock/FakeWatcher/report_pair/send_batch whose last caller (critical_dev_reload) is gone -> prune the dead debug seams.

## 2026-11-20T21:07 [lead]

edited fields

## 2026-11-20T22:57 [lead]

edited fields

## 2026-11-20T23:17 [lead]

status ready -> done: dead symbols pruned; 295 citations removed; make check + all three Docker suites green
