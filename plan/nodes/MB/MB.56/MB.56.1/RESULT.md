# Gate: MB.56.1 — delete retired 0.1.0 code, `--internal-json`, `blastit-audit`, duplicate dbproxy binary, meta-linters

Base `51cc758` → `d6de59a`. Full `make check` run **once** here (the three sub-slices verified with `make fast-check`).

## `make check` (exit 0, zero warnings)

```
uv run --no-project e2e/universal_binary.py --binary /tmp/opencode/blastit-e2e/prefix/bin/blastit \
  --python 3.13 --python 3.14 --php 8.4 --static --handoff --check
one artifact, four runtimes ... 6 passed, 0 skipped, 0 failed
PASS check 3.12   check --json ok on CPython 3.12, no per-minor .so

cd editors/zed && cargo test --locked
running 11 tests
test tests::every_grammar_highlight_is_also_in_zeds_copy ... ok
test tests::every_query_names_a_declared_node_and_a_styled_capture ... ok
test tests::the_committed_corpus_and_fixtures_speak_the_grammars_vocabulary ... ok
test result: ok. 11 passed; 0 failed

uv run --no-project tools/plan.py validate
plan OK
```

## Net effect

| Deleted | Lines |
|---|---|
| `python/blastit/{pytest_plugin,fold,logs}.py` | 4 525 |
| `crates/blastit-audit` (whole crate) | 7 098 |
| `python/tests/` retired suites + fixtures | ~17 600 |
| meta-linters (`critical_guard.py`, `mutants-*`, `check_workflows`, `test_*_gates`, `check_zed_extension`, `check-no-test-seam`, `check_wheel_{shims,tag}`) | ~4 000 |
| `--internal-json` bridge + its critical test, `blastit_build.py`, `blastit-dbproxy` bin | ~1 200 |

Kept and moved: the live `check`/`dev --learn` grant-diff engine now lives in
`crates/blastit/src/check/` (3 987 prod LOC, largest file 418); one ELF per install
(`blastit --role=dbproxy`); `python/blastit/` is a 1 376-LOC stdlib `execvp` launcher.

Follow-ups recorded on `MB.56.3` (delete the temporary `audit_v1` shim +
`plan_learn_json`), `MB.56.4` (e2e prose), `MB.56.5` (stale comments naming deleted scripts).
