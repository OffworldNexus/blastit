# Log MA.8

## 2026-11-13T11:21 [conductor]

created (task) Leaf 15: root-cause the two red Django check tests on the release CI

## 2026-11-13T11:21 [lead]

status ready -> active

## 2026-11-13T11:21 [lead]

attempt 1/3 started

## 2026-11-13T11:21 [lead]

base f97fd08d83ffcb1476f1db90ba8d5da2910adb63 (release/0.1.1, PR #62)

## 2026-11-13T11:35 [lead]

status active -> review

## 2026-11-13T11:39 [lead]

REVIEW FAIL (attempt 1): MAJOR python/tests/test_check.py:108 DJANGO_MANIFEST grants set_config('TimeZone') unconditionally -> on a UTC-spelling server (postgres:15-alpine) the same two tests now fail with dead_grant (route home, group connection, fix remove_query unsafe:true; base was green there). Must gate the grant on psycopg parameter_status('TimeZone') != 'UTC' and verify both postgres:18 and postgres:15-alpine. MINOR: :416/:578 assert messages still hide stdout findings JSON; RESULT.md plan/ claim false.

## 2026-11-13T11:39 [lead]

status review -> ready

## 2026-11-13T11:39 [lead]

status ready -> active

## 2026-11-13T11:39 [lead]

attempt 2/3 started

## 2026-11-13T11:48 [lead]

smoke (root) evidence (acceptance 6): NOT green and not a failure either — job 131823878765 is still in_progress in its 'sudo env ... cargo test -p blastit' step since 2026-11-13T10:12:02Z (35+ min; the same step took 5s on develop at 2c05139 under classic sudo with -E, and no run logs exist yet for the in-flight job). MA.7's sudo-rs step is therefore unproven: it got past the toolchain lookup and then hung. Out of MA.8's paths; reported up. Mitigation worth adding to ci.yml: timeout-minutes on the job (or --timeout on the step) so a hang fails loudly instead of pinning the PR.

## 2026-11-13T11:48 [lead]

status active -> done

## 2026-11-13T11:51 [lead]

committed d12317be3b2387ab07b476ed66f1636a11307754 on release/0.1.1 (python/tests/test_check.py, docs/knowledge/LESSONS.md, plan/)
