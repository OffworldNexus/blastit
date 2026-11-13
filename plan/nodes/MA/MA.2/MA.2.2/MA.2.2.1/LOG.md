# Log MA.2.2.1

## 2026-11-12T13:12 [planner]

created (task) Leaf 4: manifest 0.1.1 schema, reserved-key guard and boot warnings

## 2026-11-12T13:58 [lead]

status ready -> active

## 2026-11-12T13:58 [lead]

attempt 1/3 started

## 2026-11-12T13:58 [lead]

base 62339f9114fd316484764d5169da84d2079a9611

## 2026-11-12T14:06 [lead]

edited fields

## 2026-11-12T14:06 [lead]

edited fields

## 2026-11-12T14:09 [lead]

status active -> review

## 2026-11-12T14:10 [lead]

REVIEW FAIL: parse.rs:138 walk_table applies the reserved-key check to table NAMES, so documented manifests are refused: [proxy.app], [group.app.db], [proxy.db.output] app=..., [route.db.tables] groups={select=[...]} all abort with the misleading '<key> after a [table] is not top-level'. Restrict the check to field tables and add the three allowed twins to critical_manifest_011.

## 2026-11-12T14:10 [lead]

status review -> ready

## 2026-11-12T14:10 [lead]

status ready -> active

## 2026-11-12T14:10 [lead]

attempt 2/3 started

## 2026-11-12T14:19 [lead]

status active -> done
