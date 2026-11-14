# Log MB.3.28

## 2026-11-14T19:29 [planner]

created (task) Interim fail-closed refusal: [route.php] in an unwired build exits 1 and never falls through to WSGI

## 2026-11-14T19:29 [lead]

created (owner 2026-11-14). Interim fail-closed guard between MB.3.13.1 and MB.3.13.3: [route.php] currently parses and then Ctl::Spawn kills the container (no-python) or forks a WSGI view (python). Adds stage0::php_backend_unavailable in the MB.3.11/.16 idiom (boot exits 1, check refuses too), protected critical_php_backend.rs asserting the sentence AND the absence of the dev banner/view line, and flips no_python_build.rs's php-only case. MB.3.13.2 and .3 now depend on this node, so the refusal cannot be skipped before the wiring lands. .3 deletes it and rewrites the suite to a 200-through-view.chan proof (critical_guard --node MB.3.13.3).
