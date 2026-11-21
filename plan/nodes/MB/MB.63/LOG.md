# Log MB.63

## 2026-11-21T15:20 [conductor]

created (task) Install sury PHP 8.6 from signature-verified hash-pinned payloads (no sury index)

## 2026-11-21T15:20 [lead]

status ready -> active

## 2026-11-21T15:20 [lead]

attempt 1/3 started

## 2026-11-21T15:20 [lead]

base d68793df4815d2ed08944d82bb062560af8c3dd5

## 2026-11-21T15:20 [lead]

ADDENDUM: acquisition (InRelease fetch + index variant select) wrapped in 3x bounded retry, sleep 10; bad signature / payload hash / apt install stay immediately fatal.

## 2026-11-21T15:37 [lead]

REVIEW FIX: (1) MB.62 retry comment + failure echo must not attribute sury's measured mismatches to the distro mirror, and must not say Debian (Ubuntu legs exist); (2) mapping awk derives the 7 names from PHP_MINOR instead of hardcoding 8.6 (one package list); (3) a variant absent from InRelease or 404 must fall through to the next variant with a message, not abort attempt 1; (4) no sleep 10 after the third attempt; (5) stale 'extra repository comes first' clause; (6) RESULT.md follow-ups for platform_matrix.py:325-333 and docs/design/universal-distribution.md:121 still describing sury as an apt source.

## 2026-11-21T15:44 [lead]

lead gate: e2e green on final bytes (EXIT=0, 3 probe legs ok); independent chain replay matched '.xz' (signed 6b5a7eae), plain+gz mismatch-skipped, 7/7 : OK; ruff/guard/plan validate clean

## 2026-11-21T15:44 [lead]

status active -> done

## 2026-11-21T15:44 [lead]

commit 2b82366a0249a0170798d73e6f38fea1342ed90f
