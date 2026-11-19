# Log MB.42

## 2026-11-19T14:30 [conductor]

created (epic) Universal libpython-free blastit binary, .deb/.rpm + install.sh, py3-none wheels, Composer wrapper, local CLI handoff & zero-decision hero DX

## 2026-11-19T17:17 [unknown]

MB.42.1 done -> ADR-0017 Accepted + design/universal-distribution.md (141 lines). Next: MB.42.2 universal binary + handoff, then MB.42.5 PHP 8.6, then MB.42.3 packaging, then MB.42.4 docs.

## 2026-11-19T22:52 [lead]

created MB.42.7 from conductor diagnostic: blastit-sys seccomp::tests forks inside multi-threaded libtest, child deadlocked on futex_wait_queue (allocator mutex held at fork) and inherited fd::tests dup_* pipe write-end; hung make fast-check ~10min. Must land before the MB.42.2 gate because its make check runs rs-test.

## 2026-11-19T23:26 [lead]

MB.42.9 created: handoff treats ambient $VIRTUAL_ENV as 'explicit' so an unsafe active venv exits 1 - reproduced on this repo (parent dir 0775) where every blastit dev|check|fmt|lsp refuses; make test-critical needed BLASTIT_NO_LOCAL_HANDOFF=1 to pass. Must fix before MB.42.2 gate; also needs a design \)4 amendment ($VIRTUAL_ENV is ambient, not explicit).
