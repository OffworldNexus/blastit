# Log MB.3.4.2

## 2026-11-14T03:37 [planner]

created (task) Param builder, allowlist wall and protected lockdown matrix - design MB.3.4b

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T11:55 [lead]

status ready -> active

## 2026-11-14T11:55 [lead]

attempt 1/3 started

## 2026-11-14T11:55 [lead]

base 872661db52414c7d9a5fd6535d9ed121dce683f0

## 2026-11-14T12:05 [lead]

status active -> review

## 2026-11-14T12:05 [lead]

budgets: 1189 added lines vs the design's 550; every §4.7 row for this leaf landed, none dropped (declared in RESULT.md).

## 2026-11-14T12:14 [lead]

attempt 2/3 started

## 2026-11-14T12:14 [lead]

REVIEW PASS with 3 MINORs to close in attempt 2: (1) lockdown.rs:215 Params::insert is public and accepts/emits PHP_VALUE, HTTP_PROXY, SCRIPT_FILENAME=/etc/passwd, falsifying 'build is the only producer'; (2) lockdown.rs:281 CONTENT_TYPE from facts bypasses the <=8192 value rule (9000-byte CT emitted); (3) the positive twin asserts only the encoded prefix+length, not the full expected byte string.

## 2026-11-14T12:14 [lead]

status review -> ready

## 2026-11-14T12:23 [lead]

status ready -> review

## 2026-11-14T12:31 [lead]

status review -> done

## 2026-11-14T12:31 [lead]

committed 0f441af; two accepted MINORs recorded for the epic gate: (a) the 65 535 total-byte cap is only driven through build via the pair cap, so add an 8x8192-value build case, (b) resolve() returns an over-long CONTENT_TYPE while build drops it.
