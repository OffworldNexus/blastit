# Log MB.2.1

## 2026-11-13T15:11 [planner]

created (task) Write the lean static assets & SPA rewriting page (Local/CI/Deploy) and register it

## 2026-11-13T19:56 [lead]

status ready -> active

## 2026-11-13T19:56 [lead]

attempt 1/3 started

## 2026-11-13T19:56 [lead]

base 06013a6

## 2026-11-13T20:07 [doc-writer]

Wrote and registered docs/frameworks/static-and-spa.md (141 lines, 9 commands, 7 fences) + static-site fixture; dx and ai-ergonomics critics both PASS after two resolution rounds; docs-unit/docs-check/docs-test/docs_fences green. Follow-ups: ci.md/cli.md all-static reconciliation (MB.2.6 paths), static mode cannot parse curl (fences 3-4 are example-accounted).

## 2026-11-13T20:11 [lead]

status active -> review

## 2026-11-13T20:16 [lead]

REVIEW FAIL (attempt 1): 2 MAJOR + 4 MINOR. MAJOR1 inventory.toml:259-261 fence 7 registered 'static' with a false capability reason although the presets mode runs it today -> mode='presets'. MAJOR2 the all-static check contract (audit every root, run no pytest, exit 0, missing root 2) is attributed to MB.2.6 but no node owns it -> lead extends MB.2.6. MINOR3 RESULT Review Resolution omits dx B7/B9/B10. MINOR4 review-dx B1 quotes text no longer on disk -> re-quote page:97. MINOR5 page:90 says symlink escape 400 but MB.2.7 asserts 404 -> use 404. MINOR6 page:30 'npx vite build --outDir public' is prose, unregistered -> fence it with an example reason or scope it out.

## 2026-11-13T20:16 [lead]

status review -> ready

## 2026-11-13T20:18 [doc-writer]

Attempt 2 fixes: fence 8 -> presets (now executed); all-static check wording matches extended MB.2.6 (audit exists/dir/readable, no pytest, 0/2); symlink escape -> 404 per MB.2.7; Vite build moved into an example-registered fence; Review Resolution adds B7/B9/B10/B13 + gate re-review table; review-dx B1 re-quoted. Page 147 lines, 10 commands, 8 fences; docs-unit/docs-check/docs-test/docs_fences green, both reviewers PASS.

## 2026-11-13T20:18 [doc-writer]

Review artefacts after attempt 2: review-dx B1/B2 re-quoted against the final revision, review-ai-ergonomics A10 status line synced to the 404 symlink rule; no BLOCKER/MAJOR reopens, both files still end exactly 'VERDICT: PASS'.

## 2026-11-13T20:18 [lead]

attempt 2/3 started

## 2026-11-13T20:18 [lead]

status ready -> review

## 2026-11-13T20:22 [lead]

REVIEW PASS attempt 2 (5 MINOR follow-ups: critic artefact anchors stale after the Vite fence shifted indices; RESULT row should name dx B8; MB.2.6 NODE cites page:97-99 which is now the security paragraph; docs/reference/manifest.md:13 still says app yes* and no follow-up owns it -> MB.2.3).

## 2026-11-13T20:22 [lead]

status review -> done
