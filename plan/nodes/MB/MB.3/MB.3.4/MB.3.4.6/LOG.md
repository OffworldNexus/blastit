# Log MB.3.4.6

## 2026-11-14T13:15 [test-curator]

created (task) Pin the last two unprotected branches in the fastcgi protected suites: head_end at MAX_HEAD_BYTES and a digit-leading header name

## 2026-11-14T13:16 [lead]

status ready -> active

## 2026-11-14T13:16 [lead]

attempt 1/3 started

## 2026-11-14T13:16 [lead]

base 2ad094b

## 2026-11-14T13:23 [lead]

attempt 1 report: 2 protected files, +25/-1, no src change, 44/12/30/1 green, mutants 3/331/18 unchanged, critical-guard OK for both files. make check fails ONLY at docs_fences (docs/frameworks/php.md still boots the old 'listen' grammar) -- the same shared blocker as MB.3.5.1, being fixed by MB.3.9.1. Review deferred until docs_fences is green so no attempt is burned on a sibling's gate; no code change is expected.

## 2026-11-14T13:23 [lead]

status active -> review

## 2026-11-14T13:56 [lead]

status review -> done

## 2026-11-14T13:56 [lead]

commit 5c4d8c6
