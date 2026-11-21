# Log MB.36.2

## 2026-11-17T14:07 [planner]

created (task) Extract crates/blastit-view-static: openat2 root resolution, MIME/ETag/immutable/fallback responder, and the three protected static suites, with no behaviour change

## 2026-11-17T14:08 [planner]

edited fields

## 2026-11-17T14:08 [planner]

edited fields

## 2026-11-17T15:57 [planner]

edited fields

## 2026-11-18T12:14 [lead]

status ready -> active

## 2026-11-18T12:14 [lead]

attempt 1/3 started

## 2026-11-18T12:14 [lead]

attempt 1/3 started; base 4aa39d9

## 2026-11-18T12:34 [lead]

edited fields

## 2026-11-18T12:34 [lead]

lead note for MB.36.6: crates/blastit/src/stage1.rs:1444 assigns `static_max_bytes: blastit_scheduler::static_asset::STATIC_MAX_FILE_BYTES` unconditionally, and the scheduler re-exports `static_asset` only under feature = "static", so `cargo check -p blastit --no-default-features` no longer compiles (HEAD did). Nothing builds that configuration yet (blastit-py keeps default features on, so php+static+python). MB.36.6 must make the feature-off build compile for real — without duplicating the 64 MiB ceiling — and must wire `make rs-no-default` (which it creates) into `make check`, or R13's corner is a claim, not a check.

## 2026-11-18T12:44 [lead]

status active -> review

## 2026-11-18T12:55 [lead]

edited fields

## 2026-11-18T12:55 [lead]

reviewer VERDICT PASS (4 MINORs). Lead reopens for MINOR 2: build-standalone.sh:112 and publish-pypi.yml:250 build `-p blastit --no-default-features --features php`, so this leaf would leave the release artifact path uncompilable until MB.36.6 — too long. Fixing stage1.rs:1444 here instead; paths widened with crates/blastit/src/stage1.rs.

## 2026-11-18T12:55 [lead]

status review -> active

## 2026-11-18T12:55 [lead]

attempt 2/3 started

## 2026-11-18T13:08 [lead]

MB.36.6 MUST ALSO FIX BEHAVIOUR, NOT ONLY COMPILE: `scripts/build-standalone.sh` (5 sites) and `publish-pypi.yml:250` build the archive as `--no-default-features --features php`, i.e. without `static`, so until that spelling gains `static` the shipped standalone archive compiles but REFUSES a manifest with a mount (SchedulerError::StaticNotCompiled) — a silent capability loss vs 4aa39d9. MB.36.6 acceptance must run the built archive against a real mount and answer 200, not merely `make standalone` exit 0.

## 2026-11-18T13:08 [lead]

status active -> review

## 2026-11-18T13:21 [lead]

status review -> done
