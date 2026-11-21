# Log MB.22.5

## 2026-11-16T09:16 [planner]

created (task) Pin the grammar rev, guard editors/**, and prove blastit lsp live in the standalone and console-script paths

## 2026-11-17T06:43 [lead]

status ready -> active

## 2026-11-17T06:43 [lead]

attempt 1/3 started

## 2026-11-17T06:43 [lead]

base 7f0b1b6; grammar rev target 053b56e

## 2026-11-17T06:54 [implementer]

Pinned the grammar rev to 053b56e, added scripts/check_zed_extension.py + 19 negative self-tests wired as make zed-check (a check: prerequisite), and added the protected python/tests/critical/test_lsp_stdio.py proving both lsp distribution paths; make zed-check, make test-critical and the standalone/console transcripts are green.

## 2026-11-17T06:54 [lead]

status active -> review

## 2026-11-17T07:00 [lead]

NODE.md item 6 reworded: scripts/check_zed_extension.py is the editors/** guard (design §12.8), guard.py only checks big/stray/binary files

## 2026-11-17T07:00 [lead]

NODE.md item 3: 45-theme renamed to the design's fifteen-name allowlist (design §12.8)

## 2026-11-17T07:02 [lead]

status review -> done

## 2026-11-17T07:02 [lead]

committed d08d04c
