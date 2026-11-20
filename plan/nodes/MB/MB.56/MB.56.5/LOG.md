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
