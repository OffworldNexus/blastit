# Log MB.36.11

## 2026-11-18T18:05 [test-curator]

created (task) Guard the artifacts: the wheel's DT_NEEDED set (R12) and the deleted transport trait names (R3)

## 2026-11-18T18:54 [lead]

status proposed -> active

## 2026-11-18T18:54 [lead]

attempt 1/3 started

## 2026-11-18T18:54 [lead]

attempt 1/3 started; base 6f722ea

## 2026-11-18T19:07 [lead]

status active -> review

## 2026-11-18T19:20 [lead]

edited fields

## 2026-11-18T19:20 [lead]

REVIEW FAIL (round 1): MAJOR - the R3 exemption's authority is fabricated (design §4.2.3 does not authorise moving RequestIo/RequestEnd/flush_response; §4.2.1 says they are deleted; the cited ADR-0016 title does not exist). Paths widened with docs/design/view-crates-architecture.md so the leaf can correct §4.2.1/§4.2.3/§7 to the truth. MINORs: DT_STRSZ bounds the offset not the string end (a crafted ELF certifies clean); three refusal branches have no fixture.

## 2026-11-18T19:20 [lead]

status review -> active

## 2026-11-18T19:20 [lead]

attempt 2/3 started

## 2026-11-18T19:54 [lead]

status active -> done
