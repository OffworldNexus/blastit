# Log MB.3.64

## 2026-11-15T21:06 [lead]

created (task) Give the shim-swap boot test a deterministic seam instead of a 20-second race

## 2026-11-15T22:52 [lead]

status ready -> active

## 2026-11-15T22:52 [lead]

attempt 1/3 started

## 2026-11-15T22:52 [lead]

attempt 1/3 started; base fedd78e

## 2026-11-15T22:52 [lead]

edited fields

## 2026-11-15T22:53 [lead]

owner addition: prove the seam is ABSENT from the shipped binary (strings/grep -a the release standalone artifact for BLASTIT_TEST_SWAP_POINT -> 0 hits) AND PRESENT in the debug test binary (non-vacuity), as a wired check under scripts/** or the Makefile target; paths extended with scripts/** + Makefile. RESULT claim is 'the shipped artifact contains no such name, and this check fails if one appears' — never 'the seam cannot run in production'.

## 2026-11-15T23:09 [lead]

status active -> review

## 2026-11-15T23:18 [lead]

status review -> done
