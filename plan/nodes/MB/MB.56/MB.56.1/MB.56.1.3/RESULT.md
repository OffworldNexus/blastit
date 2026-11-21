# MB.56.1.3 — delete the meta-linter bureaucracy and fold the useful artifact assertions inline

**STATUS: done**

## What changed

Deleted 13 files (11 scripts + the tool + the Rust artifact suite):

- `tools/critical_guard.py`
- `scripts/{mutants-run.sh,mutants-scratch.sh,check_workflows.py,check_workflows.py.lock,test_packaging_gates.py,test_publish_gates.py,check_zed_extension.py,test_check_zed_extension.py,check-no-test-seam.sh,check_wheel_shims.py,check_wheel_tag.py}`
- `crates/blastit/tests/critical_artifacts.rs` (2 652 lines)

The assertions that were genuinely about a shipped file now live **where the file is
produced**, so they run on every build instead of in a separate gate:

- `scripts/build-standalone.sh`: the debug-only boot seams (`BLASTIT_TEST_SWAP_POINT`,
  `BLASTIT_TEST_RELOAD_FAULT`, `BLASTIT_TEST_RELOAD_PAUSE`) are read out of the
  release binary inline (with a readability guard, so "unreadable" is never reported
  as "absent").
- `scripts/build-wheel.sh`: after the repack it reads the finished zip itself —
  `_bin/` allow-list, **every staged shim is in the archive**, each payload member is
  mode 0755, `*.dist-info/WHEEL` carries `Tag: py3-none-<platform>` **and**
  `Root-Is-Purelib: false`, the file name matches that tag — then measures every ELF
  (`check-portable-elf.sh`, `--max-glibc` for manylinux) and greps each one for the
  three boot seams. The `--print-glibc-floor` self-test hook is gone with its caller.
- `scripts/build-musl-wheel.sh`: its tag/shim re-checks collapse to
  `scripts/build-wheel.sh`'s inline read-back (and the `--print-build-wheel-args`
  self-test hook is gone).
- `scripts/wheel-smoke.sh`: the seam check is inline over the installed binary.

`scripts/platform_matrix.py` keeps one job: emitting the JSON matrix the workflows
consume (`--matrix`, `--count`, `--leg`), plus `--shim-floor`, `--summary` and a
structural `--check` (leg ids, declared families, shim names, `PROVEN` keys).
`STALE_CLAIMS`, the prose count-scanner (`CLAIM`/`PROVEN_CLAIM`/`PINNED`), the site
page walk (`_pages`/`site_exclusions`/`is_page`) and the staleness advisory are gone
(−404 lines), along with the `tomllib`/page machinery they needed.

Gates rewired:

- `Makefile`: dropped `critical-guard`, `platform-matrix-check`, `workflow-check`,
  `packaging-gates-test`, `seam-check` and the three `mutants-*` targets (and every
  `MUTANTS_*` variable) with their `.PHONY`/help entries.
  `fast-check = guard rs-fmt rs-lint rs-doc rs-test py-lint plan-validate universal-smoke (+ install-test)`;
  `check` = that plus `test-critical docs-unit docs-check rs-no-python rs-no-default e2e-universal zed-check`.
  `zed-check` now compiles and unit-tests `editors/zed` and diffs its committed
  `highlights.scm` against the grammar's copy — real fixtures, no Python guard.
- CI: the `guards` job is now a five-line `critical-tests` job that only runs
  `make critical-skip-check` (R42 survives), the `pytest-plugin` job is gone, and the
  lint job no longer runs the matrix/workflow/packaging scripts.
- `.github/actions/build-wheel`: the three post-build verification steps are gone;
  the action is a thin caller of the script that now proves the archive itself.
- `publish-pypi.yml`'s `verify` no longer runs `check_wheel_shims.py` (its read-back is
  in `build-wheel.sh`); comments in `nightly.yml`/`packages.yml`/`wheels.yml`/`live.yml`
  that named the deleted auditors are rewritten.

Docs/agents describe the surviving enforcement (types, crate boundaries, real E2E):

- `AGENTS.md` rule 3 is "Protected Critical Tests & Type-Boundary Enforcement"; the
  tools table no longer lists `critical_guard.py`.
- `.opencode/` testing/rust-standards/git-workflow skills and `lead`/`review-arch`
  agents list the new `fast-check`/`check` contents and no deleted script.
- `ARCHITECTURE.md`: dropped `blastit-audit` (the crate does not exist), `--internal-json`,
  every `blastit-manifest fold` paragraph (the subcommand does not exist) and
  `blastit_build.py`; the operator statement now reads that a child `execve`s
  `BLASTIT_EXEC`, or a wheel's `_bin/blastit`.
- `RELEASING.md`, `pyproject.toml` (PEP 517 ≠ `build-wheel.sh`: no repack, binary at
  `bin/blastit`, no `_bin/`, no `blastit-lsp`), `Makefile`'s `universal-smoke` comment,
  `docs/knowledge/LESSONS.md` (incl. the stale `blastit_build.py`) and
  `editors/tree-sitter-blastfile/highlights.scm` are corrected.

**Net: 40 files changed, 254 insertions(+), 12 552 deletions(-) → −12 298 lines**
(13 files deleted).

## Evidence

`rg -n "critical_guard|critical-guard|mutants-run|mutants-scratch|check_workflows|packaging-gates-test|seam-check|check_wheel_shims|check_wheel_tag|STALE_CLAIMS" Makefile tools/ scripts/ .github/ AGENTS.md RELEASING.md ARCHITECTURE.md .opencode/`

```
ACCEPTANCE_CLEAN   (no matches)
```

`make fast-check` — exit 0; 157 `test result: ok` suites, 2 944 tests passed:
```
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
FASTCHECK_EXIT=0
```

`make docs-check` — exit 0:
```
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.20s
DOCSCHECK_EXIT=0
```

`uv run --no-project scripts/platform_matrix.py --check` (structural, no prose):
```
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared; ...
```

`make check` was **not** run (epic gate, out of scope for a leaf slice).

## Second pass (review findings)

The doc/agent surface still prescribed commands this node retired. Fixed:

- `.opencode/skills/testing/SKILL.md` docs tier: `make docs-check` (`zensical build
  --strict`) + `make docs-unit`; the "valid CLI examples executed" claim (nothing
  enforces it now) is gone.
- `AGENTS.md` gate line: the real `make check` composition (`rs-test` + `test-critical`,
  `docs-unit`, `docs-check`, `rs-no-python`/`rs-no-default`, `e2e-universal`, `zed-check`,
  the standalone ELF matrix, `plan-validate`), not a "runnable-docs harness".
- `make mutants-rust` → `TMPDIR=$HOME/.cache/blastit-mutants cargo mutants -p <crate>`
  in `.opencode/agents/lead.md`, `.opencode/agents/strategist.md`,
  `.opencode/skills/plan-system/SKILL.md` (and its milestone-gate evidence list).
- `make docs-test` → `make docs-check` in `.opencode/agents/strategist.md`,
  `.opencode/skills/dx-first/SKILL.md`, `.opencode/agents/conductor.md`.
- `.github/workflows/docs.yml`: the comment claiming a "runnable-docs inventory" is
  rewritten to the nav/orphan assertion the step actually performs.
- `.github/workflows/ci.yml`: the `docs-gate` job called `make docs-test` (a target
  that no longer exists, with ~100 lines of wheel/shims/uv-cache scaffolding for the
  retired harness). It is now 15 lines: `make docs-check` + `make docs-unit`.
- `docs/testing/README.md`: the runnable-docs page is replaced by the current gate
  table (`fast-check`, `check`, `docs-check`, `docs-unit`, `e2e`), the story/real-
  socket/allowed+denied testing rules and the critical-suite + mutation policy.
- `scripts/build-wheel.sh`: the two comments that justified `--require-shims py` and
  the `--manylinux`-less path by `make docs-test` are rewritten; `docs/knowledge/LESSONS.md`
  loses the retired-harness framing from three uv-cache/maturin lessons, and `AGENTS.md`'s
  `python/` row no longer claims runnable-docs suites.

Evidence: `rg -n "docs-test|runnable_docs|mutants-rust|critical_guard" Makefile scripts
.github AGENTS.md ARCHITECTURE.md RELEASING.md pyproject.toml .opencode editors
docs/testing/README.md` → no matches; `make fast-check` and `make docs-check` both
exit 0 after these edits.

## Third pass (`review-arch`)

1. **`make zed-check` was red and could never pass.** It byte-diffed the grammar's
   `highlights.scm` (39 lines) against Zed's (54), which are different by design. The
   diff is gone; `editors/zed/src/lib.rs`'s `mod tests` now carries the grammar guard,
   compiled and run by `make zed-check` (`cd editors/zed && cargo test --locked`):
   - `every_grammar_highlight_is_also_in_zeds_copy`: every `(node, @capture)` pair and
     every capture the **grammar** copy states must also be in Zed's copy (subset, not
     equality), with a non-empty guard so the check cannot pass vacuously.
   - `every_query_names_a_declared_node_and_a_styled_capture`: every node named by the
     grammar query, Zed's `highlights.scm`, `brackets.scm`, `indents.scm` and
     `outline.scm` is declared in `editors/tree-sitter-blastfile/src/node-types.json`
     (a renamed node fails instead of silently dropping the capture); every capture in a
     *highlight* query is one of the fifteen names the bundled themes style.
   - `the_committed_corpus_and_fixtures_speak_the_grammars_vocabulary`: parses
     `test/corpus/*.txt` (banner + `---` expected trees) and asserts every node its
     expected tree names is declared, then asserts every leading word of the eight
     `test/fixtures/*.Blastfile` is a `grammar.js` literal or a word a committed corpus
     source uses. Real fixtures, no grep script.
   - The now-false comments in `Makefile` and `editors/tree-sitter-blastfile/highlights.scm`
     ("diffs the two files") say containment instead.
2. **`scripts/build-wheel.sh`** refuses any archive member ending `.so` that is outside
   `blastit/_bin/` or `blastit.libs/`: a stale retired `blastit/_blastit.cpython-31x-*.so`
   (the `sys.executable` fallback) can no longer ship while every other check stays green.
3. **`scripts/build-musl-wheel.sh`** validates `--require-shims` (`py|all`) before the
   `apk` guard and exits 2 otherwise, so `none` cannot flow into `assert_shims` and
   produce a musllinux wheel that installs on Alpine and cannot serve PHP.

Evidence:

```
$ make zed-check
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```
$ scripts/build-musl-wheel.sh 3.13 --require-shims none
error: --require-shims 'none' would ship a wheel whose blastit/_bin/ is unchecked
  pass `py` or `all`: this script always ships shims, so an unchecked wheel is a caller bug, not a wildcard
MUSL_EXIT=2
```

The wheel read-back run against a synthetic archive (well-formed payload + one stray
`blastit/_blastit.cpython-314-x86_64-linux-gnu.so`) reports exactly that member and
exits 1; `bash -n` is clean on both edited scripts; `make fast-check` exits 0.

## Follow-ups

- Out-of-scope stale comments still naming the deleted tools:
  `crates/blastit/src/stage0/gates.rs:272` and `crates/blastit-view-php/src/pinning.rs:268`
  (`scripts/check-no-test-seam.sh`), `crates/blastit-sqlpolicy/tests/critical_*.rs` and
  `crates/blastit-manifest/tests/docs_fences.rs` (`tools/critical_guard.py`),
  `e2e/venv-smoke/run.sh:238` (`check_wheel_shims.py`, `test_packaging_gates.py`), and
  `docs/decisions/ADR-0010` / `docs/design/*` (`make platform-matrix-check`,
  `--check` prose). All are prose only; no code depends on them.
- `plan/nodes/**` LOG/RESULT files from older slices still quote the retired
  `make mutants-rust` line; `.opencode/` now spells the command
  `TMPDIR=$HOME/.cache/blastit-mutants cargo mutants -p <crate>`.
