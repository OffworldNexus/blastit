# Result MB.55

## Summary

Adds `make coverage` (`tools/coverage_report.py`, 100% stdlib) so `review-arch` can audit
changed source against the user-story tests: instrumented `cargo test` into `target/cov`
(never `target/debug`), rustup `llvm-profdata`/`llvm-cov`, per-file line % + region % and
the exact uncovered line ranges. Wires the three harness rules into `AGENTS.md`, the agent
prompts and the `dx-first`/`plan-system`/`testing` skills: a human gate on
`TESTING_PLAN.md` for new features, story-bound E2E-first tests, and a reviewer mandate
that turns uncovered code into "delete it" or "extend the E2E story".

## Evidence

`make coverage TESTS="-p blastit-lsp --tests" FILES="crates/blastit-lsp/src/lib.rs crates/blastit-lsp/src/hover.rs"`
(exit 0) — warm run ~1 s, `--locked` build, phase lines on stderr:

```
coverage: building instrumented test targets into target/cov (first run rebuilds the dependency graph)…
coverage: running 3 test executables…
coverage: 3 test executables + 1 bin object, 22 tests passed
  crates/blastit-lsp/src/lib.rs  lines 41.0%  regions 43.5%
  crates/blastit-lsp/src/hover.rs  lines 82.9%  regions 83.2%
  uncovered lines: 15-17, 21, 65-69, 71-72, 283
```

`hover.rs` is only non-zero because the spawned-server `bin` artefact is passed to both
`report` and `export`. Only `profile.test` artefacts are executed, every object must resolve
under `target/cov` (`target/debug` is never written), and `passed == 0` aborts.

`make coverage TESTS="-p blastit-lsp --tests" FILES="python/blastit/cli.py"` (exit 0):
prints `python: line coverage unavailable offline …` and skips the file instead of aborting;
any non-`.py` file with no instrumented row still raises.

`uvx --offline ruff check tools/coverage_report.py` -> `All checks passed!`;
`uvx --offline ruff format --check tools/coverage_report.py` -> `1 file already formatted`;
`uv run --no-project tools/guard.py` -> `guard: OK (4 staged/untracked file(s) checked)`;
`uv run --no-project tools/plan.py validate` -> `plan OK`.

## Files

- `tools/coverage_report.py` (new)
- `Makefile` (`coverage` target, `.PHONY`, header)
- `AGENTS.md` (rule 7, `8.6`, repository map)
- `.opencode/agents/{conductor,strategist,architect,planner,lead,writer,implementer,expert,review-arch,review-ux}.md`
- `.opencode/skills/{dx-first,plan-system,testing}/SKILL.md`
- `plan/nodes/MB/MB.55/RESULT.md`

## Follow-ups

- Python line coverage is unavailable offline (`uvx --offline coverage` is not cached and
  `python/tests` is pytest-only, so stdlib `trace` is impractical) → cache `coverage` in the
  offline toolset or add a `trace` branch later; until then `.py` files are audited against
  their story + E2E test and skipped with a stderr note.
- Re-measure `crates/blastit-lsp` coverage now that non-test bin objects are included, then
  decide delete-vs-extend-E2E per remaining uncovered branch (a `0.0%` row can also mean an
  `env_clear()` spawn hides `.profraw`).
