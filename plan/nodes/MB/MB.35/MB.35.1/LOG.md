# Log MB.35.1

## 2026-11-17T13:17 [planner]

created (task) Verify pg_query on Windows MSVC and both Apple targets, plus the Zed download/Zip/which facts, with citations

## 2026-11-17T13:17 [planner]

edited fields

## 2026-11-17T13:18 [planner]

edited fields

## 2026-11-18T05:03 [lead]

status ready -> active

## 2026-11-18T05:03 [lead]

attempt 1/3 started

## 2026-11-18T05:03 [lead]

base d5e43e9

## 2026-11-18T05:11 [lead]

status active -> review

## 2026-11-18T05:11 [lead]

owner done: GO on all five targets (windows-latest proven at job level in pg_query.rs CI 36765631562; macOS buildable but unproven upstream = the one leg MB.35.5 must prove); Zed Zip EXTRACTS, make_file_executable is a no-op off Unix, which() honours PATHEXT; gh release uploads raw bytes and the browser URL 302s to release-assets.githubusercontent.com; Zed download_file gate is a USER setting so MB.33 must declare NO capability.

## 2026-11-18T05:23 [lead]

status review -> review

## 2026-11-18T05:23 [lead]

post-review: 8/8 MINORs applied (verdict wording now "five planned, Windows + Linux x86_64 proven, both Apple legs unproven until MB.35.5"; which/PATHEXT, platform_matrix, extension_settings and libclang-order citations corrected; gh caveats quoted; prior-art prose folded). 352 lines, stated plainly.

## 2026-11-18T05:51 [lead]

status review -> done

## 2026-11-18T05:51 [lead]

committed 8dc0fab
