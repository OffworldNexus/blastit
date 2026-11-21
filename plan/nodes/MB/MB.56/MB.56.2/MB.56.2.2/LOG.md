# Log MB.56.2.2

## 2026-11-20T17:15 [lead]

created (task) Delete non-§7 micro-tests in php-sapi, sys, env, ingress, py-shim, events, frame, py-sapi, lsp/src

## 2026-11-20T17:30 [lead]

edited fields

## 2026-11-20T17:30 [lead]

status ready -> active

## 2026-11-20T17:30 [lead]

attempt 1/3 started

## 2026-11-20T17:30 [lead]

base e13cf585512e43441c1ebbcdc6ce013430f6bda7

## 2026-11-20T17:40 [lead]

edited fields

## 2026-11-20T17:40 [lead]

REVIEW FIX: (a) inventory the two critical_file_sink cases (.env read never reported / per-request reset / exit() still reports; BLASTIT_PHP_ABI_VERSION+struct frozen, wrappers not replaced); (b) fix e2e/php-sapi/Dockerfile:203,210 (deleted --test critical_file_sink) + Makefile:774 count; (c) crates/blastit-sys/src/proc.rs:26 dangling testutil ref; (d) PY_SAPI_PROBE now a no-op: publish-pypi.yml:386, build-wheel.sh:437,443,491, build-standalone-in-image.sh:312,328, py_sapi_leg.sh:121-128, platform_matrix.py:417

## 2026-11-20T17:41 [lead]

REVIEW FIX 2: crates/blastit-ingress/tests/critical_ingress.rs is a TESTING_PLAN §7 'body policies' suite (JSON/form/none/raw refusals) and was deleted by mistake - blastit-http cannot reach codec transcoding (no dependency). RESTORE it; §7 owner stays blastit-ingress.

## 2026-11-20T17:45 [lead]

status active -> done

## 2026-11-20T17:45 [lead]

commit ba279d8
