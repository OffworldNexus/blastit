# Log MB.3.28

## 2026-11-14T19:29 [planner]

created (task) Interim fail-closed refusal: [route.php] in an unwired build exits 1 and never falls through to WSGI

## 2026-11-14T19:29 [lead]

created (owner 2026-11-14). Interim fail-closed guard between MB.3.13.1 and MB.3.13.3: [route.php] currently parses and then Ctl::Spawn kills the container (no-python) or forks a WSGI view (python). Adds stage0::php_backend_unavailable in the MB.3.11/.16 idiom (boot exits 1, check refuses too), protected critical_php_backend.rs asserting the sentence AND the absence of the dev banner/view line, and flips no_python_build.rs's php-only case. MB.3.13.2 and .3 now depend on this node, so the refusal cannot be skipped before the wiring lands. .3 deletes it and rewrites the suite to a 200-through-view.chan proof (critical_guard --node MB.3.13.3).

## 2026-11-14T20:21 [lead]

status ready -> active

## 2026-11-14T20:21 [lead]

attempt 1/3 started

## 2026-11-14T20:21 [lead]

base a04ad51; .1 committed 341bf72 (loader+runtime+fork, 3 review passes, 22 defects). This guard is the interim safety: php is accepted at boot but unserved.

## 2026-11-14T20:30 [lead]

edited fields

## 2026-11-14T20:30 [lead]

PATH REPAIR by lead: added crates/blastit/src/check.rs. The guard is wired on both boot paths (run/run_dev) but blastit check calls static_app_refusal directly (check.rs:144), so check still proceeds to a php route; apply php_backend_unavailable there with check's exit 2, and cover it in the protected suite.

## 2026-11-14T20:43 [lead]

status active -> done
