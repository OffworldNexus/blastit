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
