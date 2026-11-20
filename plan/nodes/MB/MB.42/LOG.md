# Log MB.42

## 2026-11-19T14:30 [conductor]

created (epic) Universal libpython-free blastit binary, .deb/.rpm + install.sh, py3-none wheels, Composer wrapper, local CLI handoff & zero-decision hero DX

## 2026-11-19T17:17 [unknown]

MB.42.1 done -> ADR-0017 Accepted + design/universal-distribution.md (141 lines). Next: MB.42.2 universal binary + handoff, then MB.42.5 PHP 8.6, then MB.42.3 packaging, then MB.42.4 docs.

## 2026-11-19T22:52 [lead]

created MB.42.7 from conductor diagnostic: blastit-sys seccomp::tests forks inside multi-threaded libtest, child deadlocked on futex_wait_queue (allocator mutex held at fork) and inherited fd::tests dup_* pipe write-end; hung make fast-check ~10min. Must land before the MB.42.2 gate because its make check runs rs-test.

## 2026-11-19T23:26 [lead]

MB.42.9 created: handoff treats ambient $VIRTUAL_ENV as 'explicit' so an unsafe active venv exits 1 - reproduced on this repo (parent dir 0775) where every blastit dev|check|fmt|lsp refuses; make test-critical needed BLASTIT_NO_LOCAL_HANDOFF=1 to pass. Must fix before MB.42.2 gate; also needs a design \)4 amendment ($VIRTUAL_ENV is ambient, not explicit).

## 2026-11-20T00:27 [lead]

MB.42.10 created (release-blocking): libc 0.2.190 declares execveat only in linux/gnu/mod.rs, so crates/blastit does not compile for *-unknown-linux-musl - killing both musl archives, both musllinux wheels, and the worker suites on every musl PHP leg (MB.42.5 had to set suites:False for 8.6 musl). libc::SYS_execveat IS declared for musl, so libc::syscall is the portable fix.

## 2026-11-20T05:52 [lead]

GATE EVIDENCE (host): make check exit 0 (incl. e2e-universal 6/6 + zed-check + plan); make rs-test-php-image pass (PR_SET_DUMPABLE witness, 2 roles denied EACCES to uid 1000); e2e/run.py exit 0 with 46/46 ok after moving E2E_PORT/SQLALCHEMY/STATIC to free ports (21080/21081/21095) - the default 18081/18443 are held by a FOREIGN container (n2f-vuln) which I did not stop; e2e/rootless/run.py --local CANNOT run in this sandbox: setcap -> 'unable to set CAP_SETFCAP effective capability: Operation not permitted'.

## 2026-11-20T06:01 [lead]

status ready -> active
