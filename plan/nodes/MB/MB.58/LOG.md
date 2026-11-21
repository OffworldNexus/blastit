# Log MB.58

## 2026-11-20T23:56 [conductor]

created (task) Fix Alpine libgcc in run_packaging.py/install.sh and stage-1 child reaping in rs-test-php-image

## 2026-11-20T23:56 [lead]

status ready -> active

## 2026-11-20T23:56 [lead]

attempt 1/3 started

## 2026-11-20T23:56 [lead]

base ab50fe0c6a44b31c6cf2c69712fdce54bde3046b

## 2026-11-21T00:06 [lead]

REVIEW: review-arch + review-ux = 3 genuine fixes (dup waitpid vs blastit_sys::signal::reap; alpine leg never executes either new install.sh branch; --user hint gated on libc not apk + root told to sudo)

## 2026-11-21T00:06 [unknown]

edited fields

## 2026-11-21T00:17 [lead]

REVIEW: PASS (review-arch + review-ux, round 2). reap() single owner in blastit-sys with Reaped.exhausted; real musl archive leg PASS incl. root ensure_libgcc + --user 'run: apk add libgcc'; fast-check/docs-check/test_install green

## 2026-11-21T00:17 [lead]

status active -> done

## 2026-11-21T00:17 [lead]

commit 425bd05

## 2026-11-21T00:17 [lead]

commit 2398428 (amended: JOURNAL + LOG)
