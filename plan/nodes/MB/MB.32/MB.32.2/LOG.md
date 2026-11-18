# Log MB.32.2

## 2026-11-17T12:46 [planner]

created (task) Browser proof of the Blastfile highlighter: light and dark computed colours at AA, copy text and line anchors intact, instant navigation and plain fences (visual-checker)

## 2026-11-18T02:38 [lead]

status ready -> active

## 2026-11-18T02:38 [lead]

attempt 1/3 started

## 2026-11-18T02:38 [lead]

base 5f293985cc355b29dbac08421482b8ea84d1c677

## 2026-11-18T02:54 [lead]

status active -> review

## 2026-11-18T02:54 [lead]

owner done: 13 classes >=4.5:1 both schemes (light min 5.78, dark 7.08), non-Blastfile fences untouched, anchors/copy intact, instant nav + no-JS + 390px checked; 15 screenshots. Reviewer dispatched.

## 2026-11-18T03:32 [lead]

REVIEW FAIL (attempt 1): evidence integrity — 13-mobile-light.png is byte-identical to the dark capture (md5 de4740…), and the clientWidth==scrollWidth claim is contradicted by the same screenshot's clipped line; plus 5 MINOR (clipboard compared to textContent.trimEnd, missing paste screenshot, no EBNF before-count, screenshots that don't show what they claim, WCAG formula, no RESULT.md).

## 2026-11-18T03:32 [lead]

status review -> ready

## 2026-11-18T03:32 [lead]

attempt 2/3 started

## 2026-11-18T03:38 [lead]

status ready -> review

## 2026-11-18T03:38 [lead]

attempt 2 done: distinct light/dark mobile captures (differing md5), measured scroll metrics (code.scrollWidth 401 reachable to 11), 263/263-byte clipboard match + paste screenshot, EBNF 10/10/10 vs 14/14/14 span counts, formula fixed, RESULT.md added. Reviewer 2 dispatched.

## 2026-11-18T03:41 [lead]

REVIEW FAIL (attempt 2): 5 of 7 fixes verified (distinct md5s, pixel colours, span counts, formula, RESULT). Remaining: the anchor HUD claim has no HUD in the PNG; the clipboard byte count (263) hides a trailing-newline trim (fence body is 264 incl. \n); 07/08 labelled Light but are slate; md-code__content is not a real element; the 11px overflow attribution is wrong.

## 2026-11-18T03:41 [lead]

status review -> ready

## 2026-11-18T03:41 [lead]

attempt 3/3 started

## 2026-11-18T03:59 [lead]

REVIEW FAIL (attempt 3): 3/5 verified. Remaining: the 07 caption claims visible line-number anchors that the PNG does not show (the anchors are empty <a id> elements, DOM-only); the Django row reuses the Blastfile row's 390/401/11 px for a different fence whose real scroll is ~290px; one missing sentence on why trimEnd cannot hide injected characters. Bumping to 4 and handing over the reviewer's verbatim replacement text.

## 2026-11-18T03:59 [lead]

edited fields

## 2026-11-18T03:59 [lead]

status ready -> ready

## 2026-11-18T03:59 [lead]

attempt 4/4 started

## 2026-11-18T04:15 [lead]

reviewer 4: VERDICT PASS. Captions now match their pixels; Django row carries its own 390/710/320 (re-derived); trimEnd reason stated. Two MINOR precision nits recorded, not blocking.
