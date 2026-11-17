# Result MB.22.6

## Summary

Audited `blastit lsp` (framing, UTF-16 positions, diagnostics, formatting, hover, hygiene), the
manifest `position()`/`message()` and warning spans, the tree-sitter grammar and its queries, the Zed
guard, and the Python stdio route, at `d08d04c` on `release/0.2.0`. **Tier PASS** (103 cases: 64
critical, 39 useful, 0 fluff, 0 broken-oracle, 1 weak oracle). **Teeth PASS** — six of seven
deliberate regressions turned a named test red; the seventh (`grammar.js` `_terminator` removal) left
`tree-sitter test` green 15/15 because that command compiles the committed `src/parser.c`, and went
red only after `tree-sitter generate`. **Mutation PASS** — `blastit-manifest` 92.0 % killed
(`error.rs` 93.8 %, `blastfile/sites.rs` 87.5 %, `blastfile/mod.rs` 100 %), both survivors
classified. Six gaps filed, now `MB.26.9`–`MB.26.14`. No test, crate, script or SCM file was edited;
nothing is committed.

## Evidence

- `cargo test -p blastit --test critical_lsp --locked` → **42 passed, 0 failed, 0 ignored** (re-run
  after every regression was reverted).
- `cargo test -p blastit-manifest --locked` → **517 passed, 0 failed, 0 ignored** across 17 test
  binaries (incl. `warning_sites` 4, `sites.rs` 4, `error.rs` 5).
- `make zed-check` → **42 properties OK**, `test_check_zed_extension.py` **19 tests OK**.
- `make test-critical` → **exit 0: 42 Rust suites `test result: ok`, 53 Python tests OK**. The first
  run failed only `critical_php_embed::a_probe_that_hangs_is_killed_and_refuses_the_boot` (its own
  250 ms probe budget, at load 20); the quiet re-run is the green one.
- `BLASTIT_LSP_COMMAND="$PWD/target/debug/blastit lsp" uv run --no-project python -m unittest discover -s python/tests/critical -p 'test_lsp_stdio.py' -v` → **3 tests OK** (standalone binary); the console script under `test-critical`'s staged `PYTHONPATH` → **3 tests OK**.
- `cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 test` → **substituted** (no
  `npm`/`npx` on this host) with the pinned `tree-sitter 0.25.10 (da6fe9b…)`: **15/15 parses,
  failed 0, 100.00 %**.
- `make mutants-rust PKG=blastit-manifest` (`MUTANTS_TMP=/var/tmp/blastit-mutants-mb226`) → **1 445
  mutants found**, 15 outcomes in 20 min (14 caught, 0 missed, 0 timeout, 1 unviable), all
  `error.rs`; stopped for time and re-scoped with `-f` to the three in-scope files: 101 generated,
  **25 viable tested, 23 killed = 92.0 %**; survivors `error.rs:94:9` (equivalent, dead code) and
  `sites.rs::raw_body` endpoint chain (→ `MB.26.14`).
- `uv run --no-project tools/critical_guard.py --base "$(git merge-base HEAD origin/develop)" --node MB.22` → **exit 1, 9 violations**, every one of them an earlier node's edit (`MB.21.4.1`, `MB.21.4.3`, `MB.21.4.5.x`, `MB.21.4.7`, `MB.21.4.8`, `MB.21.4.10`, `MB.21.3.5`, `MB.1.15`, `MB.3.27`); `MB.22`'s own protected diff is **2 277 insertions, 0 deletions** (→ `MB.26.12`).
- `uv run --no-project tools/plan.py validate` → **plan OK** (with `MB.26.9`–`MB.26.14` present).
- Acceptance 1/4/5: `docs/testing/AUDIT-MB.22.md` names `d08d04c` and states **tier PASS / teeth PASS
  / mutation PASS**; the seven regressions are tabulated with the test that went red and their
  revert; `git status --short crates python scripts editors Makefile` is **empty**.

## Files

- `docs/testing/AUDIT-MB.22.md` (new; the only file this node wrote).

## Follow-ups

- `MB.26.9` — gate `grammar.js` against the committed parser (`generate` + `git diff --exit-code -- src`) and prove it with a negative self-test.
- `MB.26.10` — make the `blastit lsp` page's exit codes true (`1` also for an unframeable stream, `2` for a bad argument) and name the editor's `--frozen --no-sync --quiet` form.
- `MB.26.11` — prove `blastit lsp` exits `1` when the process cannot drop its file capabilities (S10's seventh session; rootless e2e).
- `MB.26.12` — give `critical_guard` a per-node base; the release-branch merge base reports every earlier node as a violation.
- `MB.26.13` — negative twins for the five security-bearing zed-guard properties (`lib: no capabilities`, `lib: no process`, `lib: argv`, `grammar: no scanner`, `grammar: no externals`).
- `MB.26.14` — locate an endpoint-level `body raw` warning; `sites.rs::raw_body`'s endpoint chain is unproven and would draw at `(0,0)`.
- Observation, not filed: `tree-sitter test` (even `--rebuild`) never reads `grammar.js` — it compiles `src/parser.c` — so the corpus is an oracle for the committed parser only; the `_terminator` removal stayed green until `tree-sitter generate`, and the regenerated `src/` then matched the restored `grammar.js` exactly.
- Host note: three of the four `cargo-mutants` launches were killed by the host (SIGTERM during the baseline build, SIGKILL at load 51 from a second project's suite), so §3 of the audit names the 11 hand-applied mutants one by one instead of extrapolating a ratio.
