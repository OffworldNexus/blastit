# Log MB.2.4.1

## 2026-11-13T20:52 [planner]

created (task) Resolve request paths beneath a static root (openat2, refusal matrix, TOCTOU) - design MB.2.4a

## 2026-11-13T21:40 [lead]

status ready -> active

## 2026-11-13T21:40 [lead]

attempt 1/3 started

## 2026-11-13T21:40 [lead]

base 40ba576

## 2026-11-13T21:47 [implementer]

implemented static_asset resolver (segments/classify/StaticRoot open+open_with+resolve+resolve_with, openat2+Walk, total errno table, ImmutableGlob, same_inode, STATIC_RESOLVE_FLAGS), SchedulerConfig manifest_dir+static_max_bytes, S/F/N/D fixture and 13-test critical matrix; all acceptance commands green

## 2026-11-13T21:48 [lead]

status active -> review

## 2026-11-13T21:56 [implementer]

attempt 2: review fixups — local segment guard in resolve_with (walk cannot escape via caller-supplied .., empty, embedded /), probe fd pinned across the read open, production head_segments entry point replaces the suite's decode/dotfile mirror, backend() accessor removed; new escape test both backends; all gates green

## 2026-11-13T21:56 [lead]

attempt 2/3 started

## 2026-11-13T21:56 [lead]

status review -> review

## 2026-11-13T22:01 [lead]

REVIEW PASS attempt 2 (2 MINOR: head_segments is a 5th doc(hidden) pub seam -> MB.2.2 should add it to design §4.2; plan/ files changed outside paths are plan.py writes, not hand edits).

## 2026-11-13T22:01 [lead]

status review -> done
