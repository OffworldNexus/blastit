# Log MA.2.3.3

## 2026-11-12T13:12 [planner]

created (task) Leaf 10: unprivileged dev supervisor, execve barrier and dev --learn

## 2026-11-12T15:52 [lead]

edited fields

## 2026-11-12T15:52 [lead]

lead: added crates/blastit/src/cli.rs to paths — the cli.rs run_dev seam is explicitly delegated to this leaf

## 2026-11-12T15:52 [lead]

status ready -> active

## 2026-11-12T15:52 [lead]

attempt 1/3 started

## 2026-11-12T15:52 [lead]

base 15931c6

## 2026-11-12T15:57 [lead]

attempt 2/3 started

## 2026-11-12T15:57 [lead]

attempt 1 session ended without producing any file or RESULT (dead session); re-dispatching attempt 2

## 2026-11-12T16:16 [implementer]

attempt 2: dev supervisor, execve barrier, banner/expose guard, dev --learn and the protected critical_dev_and_fail_closed suite land green (7/7; make check exit 0)

## 2026-11-12T16:16 [lead]

status active -> review

## 2026-11-12T16:16 [lead]

attempt 2 done: dev supervisor + learn + critical suite (7 tests); make check exit 0; sent to review (base 15931c6)

## 2026-11-12T16:21 [lead]

REVIEW FAIL (attempt 2): MAJOR dev runtime root leaked on post-adopt fatal paths (port in use/route table/memory/app import _exit without cleanup; stage0 pipe_ok escapes fail_dev guard) + no critical twin; MINOR dev_learn.rs:522 compares proxy name to fallback route name; MINOR bare clippy allows without rationale; MINOR untested 'no port key in any event' and '--learn 413/malformed JSON'.

## 2026-11-12T16:21 [lead]

status review -> ready

## 2026-11-12T16:21 [lead]

edited fields

## 2026-11-12T16:21 [lead]

status ready -> active

## 2026-11-12T16:21 [lead]

attempt 3/4 started

## 2026-11-12T16:21 [lead]

base d291078

## 2026-11-12T16:41 [expert]

attempt 3: fixed R20 dev runtime root cleanup across every fatal exit in stage0/stage1/supervise, fixed dev_learn proxy-vs-route typo + unit test, grouped DevBoot/ViewSpawn args removing bare clippy allows, and added critical twins for occupied-port/bad-import cleanup, JSON no-port-key, and --learn 413/400 enforcement (9/9 critical; make check exit 0)

## 2026-11-12T16:45 [lead]

status active -> review

## 2026-11-12T16:45 [lead]

attempt 3 done: root cleanup on all fatal paths (DevBoot/ViewSpawn structs), dev_learn route typo, port-key and --learn 413/400 assertions; sent to review

## 2026-11-12T16:52 [lead]

REVIEW FAIL (attempt 3): MAJOR dev --learn mis-attributes statements across live views (push_routed on spawn / pop on exit over dbproxy's global LIFO stack; reproduced route a's SELECT granted to b) — fix belongs in dbproxy hooks::observe using listener route; MINOR stage1 must stamp privilege when adopting a dev root.

## 2026-11-12T16:52 [lead]

edited fields

## 2026-11-12T16:52 [lead]

status review -> ready

## 2026-11-12T16:52 [lead]

status ready -> active

## 2026-11-12T16:52 [lead]

attempt 4/4 started

## 2026-11-12T16:52 [lead]

lead: extended paths with crates/blastit-dbproxy/{src,tests}/** — route attribution fix lives in hooks::observe

## 2026-11-12T17:28 [expert]

attempt 4: attribute per-route learning observations from listener.spec.route in dbproxy hooks::observe (reserving ctl stack for '*' wildcard listeners and dropping spawn/exit push_routed/pop in supervise.rs), stamp privilege=unprivileged-dev + build on stage1 preload boot when adopting a dev root, and add multi-view attribution critical twin (10/10 critical_dev_and_fail_closed, 9/9 critical_learn_and_redaction, make check exit 0)

## 2026-11-12T17:28 [lead]

status active -> review

## 2026-11-12T17:28 [lead]

attempt 4 done: dbproxy attributes from listener route (wildcard-only LIFO), stage1 privilege stamp, reproducing twin; sent to review

## 2026-11-12T17:37 [lead]

REVIEW FAIL (attempt 4): MAJOR dev_learn edits() skips observations whose route equals learn_route (first declared route) whenever an unrouted request exists, so a real route silently loses its statements (reproduced: a lost 111/333). Needs a reserved fallback identity + critical twin.

## 2026-11-12T17:37 [lead]

status review -> ready

## 2026-11-12T17:37 [lead]

edited fields

## 2026-11-12T17:37 [lead]

status ready -> active

## 2026-11-12T17:37 [lead]

attempt 5/5 started

## 2026-11-12T18:18 [expert]

attempt 5: dedicate reserved slot-0 fallback identity (_learn_fallback) under dev --learn in stage0/stage1/dev_learn so the first declared route never collides with the unrouted fallback or loses its SQL grants; added failing-then-green critical twin (11/11 critical_dev_and_fail_closed, 9/9 critical_learn_and_redaction, 11/11 critical_config_and_presets, make check exit 0)

## 2026-11-12T18:21 [lead]

status active -> review

## 2026-11-12T18:21 [lead]

attempt 5 done: reserved _learn_fallback view slot 0 + critical twin; sent to review

## 2026-11-12T18:30 [lead]

status review -> done

## 2026-11-12T18:30 [lead]

committed 0ea35c0
