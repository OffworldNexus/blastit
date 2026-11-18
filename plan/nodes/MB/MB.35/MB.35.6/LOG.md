# Log MB.35.6

## 2026-11-17T13:17 [planner]

created (task) Enforce blastit-lsp portability in make check and prove the documented commands end to end

## 2026-11-18T07:06 [lead]

status ready -> active

## 2026-11-18T07:06 [lead]

attempt 1/3 started

## 2026-11-18T07:06 [lead]

base ef93363

## 2026-11-18T07:26 [lead]

status active -> review

## 2026-11-18T07:26 [lead]

owner done: check_lsp_portable.py (6 properties: manifest ceiling, source tokens, environment, cargo metadata closure, identity, wiring) + 15 mutation twins (47 assertions), make lsp-portable wired into make check; E2E-MB.35.md with three byte-identical transcripts, 0/1/2 exits, clean-dir archive run, and a scoped PASS with 4 legs NOT RUN.

## 2026-11-18T07:35 [lead]

reviewer: VERDICT PASS — four transcripts all 924f0d24 and diff-clean, archive layout + sha256sum + empty getcap + --help/--version exit 1, six properties, 47 assertions over 15 fixtures, wiring twin. Four MINORs: alias-spawn escapes the scanner, 3 twins missing, verdict wording vs Goal 4, a byte count. Fixing 1/2/3/4; 5 recorded.

## 2026-11-18T08:20 [lead]

REVIEW FAIL (round 2): MAJOR the brace/alias classification still lets four COMPILING escapes through (root alias, brace-item alias, std glob, fs brace alias) while the docstring and E2E claim coverage; MINOR #[path = "..."] mod compiles an unread file; MINOR the false coverage claim. Attempt 3 with the reviewer exact fixes.

## 2026-11-18T08:20 [lead]

status review -> ready

## 2026-11-18T08:20 [lead]

attempt 2/3 started

## 2026-11-18T08:54 [lead]

REVIEW FAIL (round 3): three MORE compiling escapes — std::os omitted from FORBIDDEN_MODULES (use std::os as o; o::unix::fs::symlink), and the split rule only matches a line ending exactly "usestd" (use std as\ns; and use std::\n*; are fmt-clean). The deny-list approach has an unbounded tail: 3 rounds, 3 new spelling classes. LEAD DECISION: invert the source property to an ALLOWLIST of the std items this crate may use (deny-by-default), so new spellings close instead of chase. max_attempts raised to 4 for that redesign, not for another deny-list round.

## 2026-11-18T08:54 [lead]

edited fields

## 2026-11-18T08:54 [lead]

status ready -> ready

## 2026-11-18T08:54 [lead]

attempt 3/4 started

## 2026-11-18T09:18 [lead]

edited fields

## 2026-11-18T09:18 [lead]

attempt 4/5 started
