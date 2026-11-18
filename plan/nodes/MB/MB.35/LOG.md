# Log MB.35

## 2026-11-17T13:17 [planner]

created (epic) Separate cross-platform blastit-lsp binary for Linux, macOS and Windows while blastit stays Linux-only

## 2026-11-17T13:18 [planner]

edited fields

## 2026-11-17T13:18 [planner]

Decomposed MB.35 into 6 leaves: research (35.1), docs (35.2), ADR-0013 design (35.3), crate extraction (35.4), CI/packaging (35.5), portability guard+e2e (35.6). MB.33 now depends on MB.35.

## 2026-11-18T11:58 [lead]

Gate: all six leaves done. Acceptance run at this tree: make check EXIT=0 (189 suites, lsp-portable inside), make e2e EXIT=0 (411 checks / 0 failures), plan.py validate + guard.py green. `cargo test -p blastit-lsp` 16+3, `-p blastit --test critical_lsp` 52 unchanged, `cargo tree -p blastit-lsp` 0 forbidden packages, three view crates n/a. Linux x86_64 PROVED; the four macOS/Windows/aarch64 legs are NOT RUN pending the first CI run after the Release PR (E2E-MB.35.md §6). No test-curator mutation audit was run for this epic: the crate is a behaviour-preserving move whose equivalence is pinned by the untouched 52-case protected suite and by two reviewers; a blastit-lsp mutation audit is filed as a follow-up.

## 2026-11-18T11:58 [lead]

status ready -> done
