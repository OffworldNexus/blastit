# Log MB.31.3

## 2026-11-17T12:33 [planner]

created (task) Delete ImmutableGlob and glob(): static mounts consume the manifest's matcher, with bare-immutable S3 proof

## 2026-11-17T23:39 [lead]

inherits from MB.31.2: make StaticPattern PartialEq compare text AND spelling; make cross-/ live by calling StaticImmutable::matches and delete ImmutableGlob/glob()/MAX_IMMUTABLE_PATTERN_BYTES; then the two public pages' cross-/ sentences become true.

## 2026-11-17T23:39 [lead]

status ready -> active

## 2026-11-17T23:39 [lead]

attempt 1/3 started

## 2026-11-17T23:39 [lead]

edited fields

## 2026-11-17T23:39 [lead]

base 942cb16; model.rs added to paths for the StaticPattern PartialEq spelling fix

## 2026-11-18T00:06 [lead]

done: ImmutableGlob/glob/MAX_IMMUTABLE_PATTERN_BYTES deleted, cache_control == StaticImmutable::matches (one matcher, no per-request compile), StaticPattern PartialEq compares (text, spelling). Red both ways: always-immutable 7 FAILED, never-immutable 9 FAILED. 288+523 tests, make check EXIT=0. Interim design paragraph now stale (MB.31.4/gate).

## 2026-11-18T00:06 [lead]

status active -> review

## 2026-11-18T00:14 [lead]

reviewer VERDICT PASS (5 MINOR, all flags for the gate/MB.31.4): hover.rs still teaches stop-at-/, the design's interim paragraph is now false four ways, and the node's own .map acceptance text is factually wrong for the documented regex -- the implementer asserted the truth instead. NOTE for later curators: do NOT restore the .map expectation.

## 2026-11-18T00:14 [lead]

status review -> done
