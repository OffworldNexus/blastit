# Log MB.56.1.3

## 2026-11-20T16:04 [planner]

created (task) Delete critical_guard.py, mutants-*.sh and all scripts/ meta-linters; fold newline-free ELF/zip assertions inline and rewire Makefile/GHA/AGENTS

## 2026-11-20T16:04 [planner]

edited fields

## 2026-11-20T16:04 [planner]

edited fields

## 2026-11-20T16:25 [lead]

edited fields

## 2026-11-20T16:36 [lead]

ALSO: stale blastit-audit/--internal-json prose in ARCHITECTURE.md:564,575 and Makefile:267 comment (left behind by MB.56.1.2.1)

## 2026-11-20T16:50 [lead]

ALSO: ARCHITECTURE.md:1449-1450 says the wheel file to chmod o+rx is _bin/blastit-dbproxy (now _bin/blastit); publish-pypi.yml:50 pull_request.paths still lists deleted blastit_build.py; pyproject.toml:15-20 claims a PEP517 build does what build-wheel.sh does (it skips the repack).

## 2026-11-20T16:54 [lead]

ALSO: ARCHITECTURE.md:183 still mentions blastit_build.py; docs/knowledge/LESSONS.md:7 too

## 2026-11-20T16:54 [lead]

status ready -> active

## 2026-11-20T16:54 [lead]

attempt 1/3 started

## 2026-11-20T16:54 [lead]

base 67139108ee74b59d94e397ab6c4d94e94c7bd00f

## 2026-11-20T17:08 [lead]

REVIEW FIX: (1) .opencode/skills/testing/SKILL.md:22 tier table still promises make docs-test; (2) AGENTS.md:31 false 'runnable-docs harness' in make check; (3) make mutants-rust dead target in .opencode/agents/lead.md:106, plan-system/SKILL.md:85, agents/strategist.md:47 -> cargo mutants -p; (4) agents/strategist.md:46 + skills/dx-first/SKILL.md:46 make docs-test; (5) .github/workflows/docs.yml:15 + docs/testing/README.md docs-test prose

## 2026-11-20T17:09 [lead]

REVIEW-ARCH FIX: (B) zed-check diff is red: editors/tree-sitter-blastfile/highlights.scm (39) vs editors/zed/.../highlights.scm (54) are different BY DESIGN -> replace diff with a Rust test asserting grammar captures are a SUBSET of Zed's + every node/capture declared in ../tree-sitter-blastfile/src/node-types.json + drive test/corpus|fixtures; (m) build-wheel.sh refuse any *.so member outside blastit/_bin/ and blastit.libs/; (m) build-musl-wheel.sh restore --require-shims fail-closed case. REJECTED: test_publish_gates.py retention oracle (publish/prune is not a TESTING_PLAN.md user story; owner-approved deletion). DEFERRED->MB.56.5: stale comments naming check-no-test-seam.sh in stage0/gates.rs:272, blastit-view-php/src/pinning.rs:268, e2e/venv-smoke/run.sh:238, sqlpolicy/manifest test headers

## 2026-11-20T17:12 [lead]

status active -> done
