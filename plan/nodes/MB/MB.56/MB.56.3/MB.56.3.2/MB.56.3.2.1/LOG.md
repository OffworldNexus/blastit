# Log MB.56.3.2.1

## 2026-11-20T18:54 [lead]

created (task) e2e_dev.rs part 1: Python WSGI multi-view, host rules, venv activation

## 2026-11-20T18:54 [lead]

status ready -> active

## 2026-11-20T18:54 [lead]

attempt 1/3 started

## 2026-11-20T18:54 [lead]

base eef8793da590e19b43eb36ee579f143fce19d18b

## 2026-11-20T19:34 [lead]

REVIEW FIX: (B) stage1 pin/adopt descriptor-identity check lost its only witness (critical_python_shim.rs:731 deleted) -> restore as an E2E leg or a minimal focused pin test (maps to TESTING_PLAN 6 tampered descriptors); (M) RESULT false claims: py-sapi discovery has 0 tests -> add absolute BLASTIT_PYTHON leg + correct claims; (M) shared-libpython test panics where both minors ship -> export search list from blastit-py-sapi; (M) duplicate timing helpers vs critical_parallel_views; (m) free_port EADDRINUSE retry; (m) deny paths over both views

## 2026-11-20T19:34 [lead]

edited fields

## 2026-11-20T19:34 [lead]

REVIEW FIX 2 (ux): (F1) docs/reference/log-events.md:104 environment cell contradicts what the test proves; (F2) settings.md has no BLASTIT_PYTHON row + troubleshooting has no missing-libpython row with the 4 remedies; (F5) drop absolute PARALLEL_BUDGET=450ms (keep control margin + t0 span); (F4) after a denial assert the sibling view still serves (overlaps both-view loop)

## 2026-11-20T19:34 [lead]

REVIEW FIX 2 (ux): (F1) docs/reference/log-events.md:104 environment cell contradicts the test; (F2) settings.md lacks a BLASTIT_PYTHON row and troubleshooting lacks the missing-libpython row with the four remedies; (F5) drop the absolute PARALLEL_BUDGET=450ms assertion, keep the control margin and t0 span; (F4) after a denial assert the sibling view still serves its granted statement

## 2026-11-20T19:43 [lead]

status active -> done

## 2026-11-20T19:43 [lead]

commit ec0e19c
