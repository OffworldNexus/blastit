# Log MB.51

## 2026-11-20T11:15 [conductor]

created (task) Fix final 3 PR #64 CI items (Requires-Python >=3.12 in check-long-description.py, setup-uv before chown in ci.yml & BLASTIT_REQUIRE_ROOT_TESTS in critical_handoff)

## 2026-11-20T11:16 [lead]

status ready -> active

## 2026-11-20T11:16 [lead]

attempt 1/3 started

## 2026-11-20T11:16 [lead]

base c0576557d1f8ff3078c49c43bfed35cc88e8a094

## 2026-11-20T11:25 [lead]

REVIEW FIX (7 genuine): 1) ci.yml pytest-plugin: /opt comment block orphaned above rust-toolchain -> reattach to its step. 2) ci.yml smoke (root) job comment budget 46 -> 53 (5+1+2+10+2+10+20+3) + name the index-stall risk. 3) ci.yml pytest-plugin: bound the 3 new steps (5/2/2) and rewrite the /opt chain as set -euo pipefail + sudo --non-interactive lines. 4) check-long-description.py: read requires-python ONCE in main(), config error -> exit 2 + actionable text (not N per-artifact failures under the description heading); UnicodeDecodeError -> catch ValueError; name the pyproject source in the mismatch. 5) new stdlib test in python/tests proving declared_requires_python follows pyproject + fails closed on 4 broken shapes (lazy readme_renderer import). 6) ci.yml root-runner rationale: say what BLASTIT_REQUIRE_ROOT_TESTS=1 does. 7) revert the unrelated reflow churn in check-long-description.py. REJECTED: check_workflows.py regex tripwire (invariants belong in types, not guard scripts), LESSONS.md (outside paths).

## 2026-11-20T11:33 [lead]

REVIEW FIX 2 (4 min): 1) split OSError ('cannot read <path>') from TypeError/ValueError ('not valid project metadata') in declared_requires_python. 2) path=None default so main()'s exit-2 config path is testable; add test_main_reports_a_broken_pyproject_once_and_exits_2. 3) move the 'only the interpreter tree becomes root's' comment inside run: above python_root= in pytest-plugin. 4) deferred readme_renderer import -> Failure with the uv invocation instead of ModuleNotFoundError traceback. REJECTED: pytest-plugin job-level timeout-minutes:45 (no measured healthy runtime; a new ceiling risks a fresh red job) -> follow-up.

## 2026-11-20T11:38 [lead]

status active -> done

## 2026-11-20T11:38 [lead]

commit 6a94156
