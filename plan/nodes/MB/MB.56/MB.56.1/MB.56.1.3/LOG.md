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
