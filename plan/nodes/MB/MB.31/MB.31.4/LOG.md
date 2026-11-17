# Log MB.31.4

## 2026-11-17T12:33 [planner]

created (task) Hover and tree-sitter teach bare immutable and regex: regenerate parser.c, then re-pin the Zed grammar rev

## 2026-11-17T12:50 [conductor]

Polish in editors/zed/src/lib.rs while touching editors/: check zed::settings::LspSettings::for_worktree('blastit-lsp', worktree) binary.path/arguments/env first before falling back to worktree.which('blastit') -> uv, so users can also point Zed directly at a local build path if not on PATH.

## 2026-11-18T00:14 [lead]

status ready -> active

## 2026-11-18T00:14 [lead]

attempt 1/3 started

## 2026-11-18T00:14 [lead]

base 32dc8ca; hover.rs:120-124 still teaches stop-at-/ and docs/design/static-and-spa.md:261-268's interim paragraph is false four ways -- both land here

## 2026-11-18T00:14 [lead]

edited fields

## 2026-11-18T00:30 [lead]

done: hover teaches the three shapes; grammar _value admits regex_raw_string so bare/list/regex all parse with all 8 fixtures still 0 ERROR; parser.c/grammar.json/node-types.json regenerated with pinned tree-sitter 0.25.10 ABI 14 and idempotent by sha256; 3 corpus cases (18/18); new highlights.scm; design interim paragraph, ImmutableGlob block and the stale vectors replaced. Zed rev re-pin deferred to release (no commit exists to point at; never guessed).

## 2026-11-18T00:30 [lead]

status active -> review

## 2026-11-18T00:37 [lead]

reviewer VERDICT PASS (3 MINOR, none blocking): the design doc's R5 sentence should be past tense, the NODE Goal should name the design-doc amendment, and zed-check does not run tree-sitter test (handed to MB.26.9's regeneration gate). Generator verified idempotent twice; the new regex corpus case fails on the base grammar, so it is a real guard.

## 2026-11-18T00:37 [lead]

status review -> done
