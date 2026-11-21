# Log MB.3.35

## 2026-11-14T23:17 [lead]

created (task) Name the shipped PHP shims per ADR-0010 s4.1 (libblastit_php<minor>-<arch>-<libc>.so) everywhere

## 2026-11-14T23:32 [lead]

status ready -> active

## 2026-11-14T23:32 [lead]

attempt 1/3 started

## 2026-11-14T23:32 [lead]

base 71eed2f3a9b9772748f0c17b8cdbe57c658c0e9e

## 2026-11-14T23:45 [lead]

edited fields

## 2026-11-14T23:45 [lead]

lead: paths widened for the comment-only edits the worker made (stage0.rs, blastit-view php_native.rs, php-embed-sapi.md) + LESSONS.md + the CI workflow whose comment still names libblastit_php8.4.so

## 2026-11-14T23:45 [lead]

status active -> review

## 2026-11-14T23:53 [lead]

REVIEW FAIL (attempt 1): MAJOR Makefile:427-428 derives <libc> from a filesystem probe while the boot uses cfg!(target_env), so on a glibc host with Debian's musl package installed (or any cross PHP_SAPI_CC build) make php-sapi writes a -musl name the gnu boot never selects, silently. MINORs: stage0 duplicates target_arch/target_libc with no literal assertion; two php_native.rs hints hard-code -x86_64-gnu; RESULT omits the report-only critical_guard line.

## 2026-11-14T23:53 [lead]

status review -> ready

## 2026-11-14T23:53 [lead]

status ready -> active

## 2026-11-14T23:53 [lead]

attempt 2/3 started

## 2026-11-15T00:00 [lead]

status active -> review

## 2026-11-15T00:08 [lead]

status review -> done
