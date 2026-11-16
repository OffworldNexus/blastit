# Log MB.21.4.4

## 2026-11-16T00:38 [planner]

created (task) Migrate Python package, Makefile and every e2e fixture to Blastfile and delete the TOML reader

## 2026-11-16T01:49 [planner]

edited fields

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T02:18 [planner]

edited fields

## 2026-11-16T08:13 [planner]

edited fields

## 2026-11-16T08:13 [planner]

narrowed to e2e/**, the python/tests fixtures MB.21.4.1 did not migrate, scripts/** and the Makefile, with make e2e exit 0 as the leaf gate. Context now records that make e2e is red between MB.21.4.1 and this leaf, that only the epic gate requires it, and that no leaf may weaken a gate (skip, xfail, shim) to cover the gap. The six runnable-docs fixtures and their harness are named as 4.1's, not to be re-touched.

## 2026-11-16T12:54 [planner]

edited fields

## 2026-11-16T12:54 [planner]

re-pointed depends_on MB.21.4.3 -> MB.21.4.7 (new Python-cutover leaf) and Context now says the package surface is 4.7's: the plugin default/help and the four Python suites' fixtures belong to 4.7, so 4.4 translates only what 4.7 leaves (e2e/, the three project fixture directories, scripts, Makefile). Reason: the Python cutover was split out of 4.3 as one session's worth of work; 4.4 must run after the Python tree reads Blastfile, or it would translate the same fixtures twice.

## 2026-11-16T14:43 [lead]

status ready -> active

## 2026-11-16T14:43 [lead]

attempt 1/3 started

## 2026-11-16T14:43 [lead]

base 621626c

## 2026-11-16T15:27 [lead]

reported done but left two red tests in python/tests/test_pytest_plugin.py (line 720 expects exit 1 where the plugin now exits 4 'does not parse'; line 627 connection_setup_ungranted 2 vs 1) — both run in CI's pytest-plugin job, so the release PR would be red. Sent back for a fix inside this leaf (python/tests/** is in paths) before review.

## 2026-11-16T15:44 [lead]

status active -> review

## 2026-11-16T15:53 [lead]

reviewer PASS; six MINORs. Fixing 1-4 before commit (fmt of python/tests/project/Blastfile, the retired queries = [] sentence in e2e/scenario.py:765, the 'the answer is 400, not 400' message, and static_assets.py's page-count check that now accepts any number), and recording 5-6 (the unreproduced /api 502 as an open risk, and the MB.22/research files as another session's).

## 2026-11-16T16:08 [lead]

HOST CONDITION at commit time: this machine is at 44/48 GiB used by another project (orange-poc procrastinate workers, PIDs 1493095-97/1666738-53), MemAvailable 4.3 GiB, so a_host_whose_page_cache_is_above_the_default_still_boots_and_serves and the_dev_memory_advisory_is_one_line_and_only_above_the_default fail because blastit dev correctly refuses above BLASTIT_MEM_HIGH_WATERMARK=85 — the test's own premise (the reclaim-aware reading is below it) does not hold on a loaded host. Neither test touches this leaf. make check was exit 0 twice on this exact tree (owner and reviewer) and make e2e exit 0; my two runs fail only those two host-memory cases. Epic gate to re-run both after the host frees.

## 2026-11-16T16:08 [lead]

status review -> done
