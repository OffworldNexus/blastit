# Log MB.21.2

## 2026-11-16T00:38 [planner]

created (task) Design the Blastfile lexer, parser, CST and cutover (ADR-0012) with security/implementability/testability/consistency critics

## 2026-11-16T00:40 [conductor]

Settled up front (not open): [route.php] workers is removed (scheduler owns parallelism; no workers directive in Blastfile), global top-level groups are removed (only explicit per-view 'use <trait>'), and pooler/vendor knobs are removed from the manifest.

## 2026-11-16T01:32 [lead]

status ready -> active

## 2026-11-16T01:32 [lead]

attempt 1/3 started

## 2026-11-16T01:32 [lead]

base c2d89e4de9e9bb22d9121e4df97ed9772acf2534

## 2026-11-16T02:12 [lead]

status active -> review

## 2026-11-16T02:14 [lead]

REVIEW FAIL (attempt 1): (1) BLOCKER §22 omits 22 of 47 round-4 BLOCKER/MAJOR rows (consistency M1,M2,M4-M13; implementability M2-M6,M9; testability B4,M2,M4,M7,M8); (2) BLOCKER §13.3:1078 '--manifest retires in favour of discovery' contradicts ADR-0002/CLI ref; (3) BLOCKER six stale §22 rows (to: 'view products body json', '[lints]', 'B1 3.1'); (4) MAJOR §9.1 dotfile provenance false + root '.' ; §9.4 url_path double slash on '/' mount; ADR-0012 d7 'this ADR' vs R13; §4.1 says 4.4 deletes parse.rs but 4.5 owns it and 4.5 paths too narrow; §20 P1/P3 in fmt tests vs pub(crate); R10 'stay refused exactly as today' vs new 400; §7.1 global groups not named as dropped; §16.2 README fence counts 1/0/0 vs five; §14/§15 cite nonexistent crates/blastit/src/fmt.rs

## 2026-11-16T02:14 [lead]

status review -> ready

## 2026-11-16T02:18 [lead]

attempt 2/3 started

## 2026-11-16T02:18 [lead]

status ready -> review

## 2026-11-16T02:25 [lead]

attempt 3/3 started

## 2026-11-16T02:25 [lead]

status review -> review

## 2026-11-16T02:30 [lead]

status review -> done
