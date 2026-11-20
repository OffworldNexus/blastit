# RESULT — MB.56.1.1

Retired the legacy `0.1.0` Python surface and the `--internal-json` RPC bridge; the
published wheel is now a pure-stdlib `execvp` launcher for the shipped Rust binaries.

## What changed

**Deleted**

- `python/blastit/pytest_plugin.py` (2646), `fold.py` (1351), `logs.py` (528)
- `crates/blastit/src/cli/internal_json.rs` (224), `crates/blastit/tests/critical_internal_json.rs` (358)
- `python/tests/`: `runnable_docs/`, `project/`, `live_project/`, `packaged_project/`,
  `conftest.py`, `test_docs_assets.py`, `test_fold.py`, `test_pytest_plugin.py`,
  `test_plugin_helpers.py`, `test_logs.py`, `test_live_server.py`, `test_release_scripts.py`,
  `test_runnable_docs.py`, `test_wire_capture.py`, `test_project_manifest.py`,
  `test_edits_binding.py`, `test_check.py`, `critical/test_lsp_stdio.py`,
  `critical/test_fold_literals_and_atomic.py`, `critical/test_check_contract.py`

**Written**

- `python/blastit/_blastit.py` (150): `_bin/` discovery + mode repair + one `os.execvpe`;
  no NDJSON, no child, no thread, no fork guard
- `python/blastit/__main__.py` (33), `python/blastit/__init__.py` (13)
- `python/tests/test_launcher.py` (76): argv + exit status preserved, `pip`'s `0644` repaired,
  a package with no binary names `pip install --force-reinstall blastit` (no traceback)

**Edited**

- `pyproject.toml`: `[project.entry-points.pytest11]` and every `blastit-manifest`/`--internal-json`
  mention removed; `dependencies = []`, `build-backend = "blastit_build"` and the
  `_bin/blastit-dbproxy` include **unchanged** (slice `.2` owns them)
- `crates/blastit/src/cli.rs`, `cli/parse.rs`, `cli/handoff.rs`: `Command::InternalJson`,
  `parse_internal_json`, the `mod internal_json` and every dispatch/handoff arm removed
- `crates/blastit/tests/critical_handoff.rs`: the two `--internal-json` no-handoff vectors dropped
- `scripts/repack_universal_wheel.py`: `CONSOLE_SCRIPTS` is `blastit` + `blastit-lsp`
- `Makefile`: `docs-unit` now runs the package's offline suites; `docs-test` deleted with its
  retired runnable-fence harness (also out of `check`); `DOCS_PYTEST`/`DOCS_WHEEL_STAGE` dropped
- `python/tests/test_check_pytest_gate.py`: the retired-bridge comparison removed, gate coverage kept
- `docs/reference/cli.md`, `docs/reference/log-events.md`: the `blastit-manifest fold` section,
  the `--internal-json` paragraph and the fold log-format mention removed

## Evidence

```
$ wc -l python/blastit/*.py python/tests/*.py python/tests/critical/*.py
  150 python/blastit/_blastit.py
 1180 python/blastit/_check_pytest.py
   13 python/blastit/__init__.py
   33 python/blastit/__main__.py
  314 python/tests/test_check_pytest_gate.py
   76 python/tests/test_launcher.py
  109 python/tests/critical/test_stdlib_only.py

$ make docs-unit
Ran 15 tests in 0.053s
OK

$ make fast-check
fast-check exit=0
grep -cE "^(error|warning)" -> 0
All checks passed!
test result: ok. 16 passed; 0 failed …          # critical_handoff
install-test: all cases passed

$ make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.37s

$ rg -n "pytest11|internal-json|internal_json|blastit-manifest" python/ pyproject.toml \
      crates/blastit/src crates/blastit/tests scripts/repack_universal_wheel.py
crates/blastit/src/cli/parse.rs:115,117      # forwarded `--blastit-manifest` refusal (kept, see below)
crates/blastit/src/check.rs:16               # `-p no:blastit` comment about the 0.1.0 plugin
crates/blastit/tests/critical_config_and_presets.rs:472,474   # protected CLI matrix pins that refusal
crates/blastit/tests/critical_lsp.rs:2093, critical_raw_ingress.rs:36   # the `blastit-manifest` *crate* name
```

`python/blastit/`: 6 339 → 1 376 lines. `python/tests/`: 18 140 → 499 lines
(24 479 → 1 875, −92 %).

## Acceptance notes

- `pyproject.toml`, `python/`, `scripts/repack_universal_wheel.py` and `crates/blastit/src/cli*`
  hold zero retired-flag references.
- `crates/blastit/src/cli/parse.rs` keeps the forwarded-`--blastit-manifest` refusal: the protected
  matrix `critical_config_and_presets.rs:466-474` (outside this node's `paths`) pins that sentence,
  and it is still the right one — `blastit check` reads the manifest itself, so forwarding the word
  to `pytest` is a mistake. `crates/blastit/src/check.rs` and the two `crates/blastit/tests/`
  comments are outside `paths`.
- `_check_pytest.py` (1 180 lines) is kept as-is, per this slice's brief: it is the live
  `blastit check` gate `crates/blastit/src/check.rs:984` imports with `-p blastit._check_pytest`
  and never touched the RPC bridge. The node's "no module > 150 LOC" holds for the launcher
  modules (`_blastit.py` 150, `__main__.py` 33, `__init__.py` 13); splitting the gate is a
  separate slice.
- `python/tests/` = 499 lines (launcher + gate + the protected stdlib-only tier).

## Review fix (consolidated pass)

| Defect | Fix |
|---|---|
| `main_lsp` failed open into an `exec` loop, raw `ValueError` traceback | `__main__.main_lsp` now resolves the program first, copies the env with `setdefault(EXEC_GUARD, "1")`, and turns `(OSError, ValueError)` into `SystemExit("blastit-lsp: …")` naming `pip install --force-reinstall blastit` — the `main()` shape |
| no mirror oracle for the language server | `test_launcher.test_the_language_server_names_the_fix_instead_of_looping`: empty `_bin/`, exit `1`, reinstall sentence on stderr, no `Traceback` (§3 "project-pinned Python installs with safe CLI handoff") |
| orphan `crates/blastit/src/cli/internal_json/` still shipped | directory deleted (`args.rs` was already uncompiled) |
| dead `pytest_plugin.py` carve-out in the stdlib walk | removed from `critical/test_stdlib_only.py` (docstring and branch), so every module is held to the same rule |
| `main.rs` doc comment advertised the retired `fold`/RPC dispatch | now `blastit check` and `blastit fmt` |
| scripts still demanded `fold`, `logs`, `pytest_plugin` | `check-bundle-layout.sh` (`modules=` and the "four modules" prose) and `test_packages.sh` (3 paths dropped, total `16` → `13`) |
| `wheel-smoke.sh` hard-failed when `[pytest11]` was absent | block deleted; console scripts are `blastit blastit-lsp` (`design §6.1: blastit, blastit-lsp`) |
| `wheel-drive.sh` drove `blastit-manifest fold` and the NDJSON bridge | step deleted (and `manifest_bin`), steps renumbered `1..3` |
| `e2e/universal_binary.py` ran `python -m blastit.fold` | block + `FOLD_MANIFEST`/`FOLD_LOG` deleted; `bin_path()` probe now runs in `check_project`, return line says `check --json ok on CPython …` |
| retired `runnable_docs` in live tasks | `docs.yml` step is now the nav/orphan page check with no harness imports; `ci.yml` comments and the `--ignore=runnable_docs/fixtures` dropped; `e2e/dx_replay.py` row 5 writes its `shop`/`findings` projects from inline constants and serves them from an inline `PgStub` (no `python/tests/runnable_docs` import) |
| `pytest python/tests` needed `PYTHONPATH` | `python/tests/conftest.py` puts `python/` on `sys.path` |

`_check_pytest._hookimpl`'s docstring still named `blastit.pytest_plugin`; corrected.
`cli/parse.rs`'s `--blastit-manifest` refusal is unchanged on purpose (see above).

## Evidence — review fix

```
$ bash -n scripts/check-bundle-layout.sh scripts/test_packages.sh \
          scripts/wheel-smoke.sh scripts/wheel-drive.sh
bash -n: ok

$ uv run --no-project python -m pytest python/tests -q
......................                                                   [100%]
22 passed in 0.13s

$ uv run --no-project e2e/dx_replay.py run --binary target/debug/blastit \
      --run-root /tmp/opencode/mb5611/dxrun --out /tmp/opencode/mb5611/dxout \
      --rows 5 --python /tmp/opencode/mb5611/venv/bin/python
normalised row5-shop.stdout: 3 lines
captured rows 5 into /tmp/opencode/mb5611/dxout          # exit 0
$ cat /tmp/opencode/mb5611/dxout/row5-{shop,findings}.exit
0
1
$ sed -n '3,8p' /tmp/opencode/mb5611/dxout/row5-findings.stdout
blastit check: 3 requests, 2 statements, 3/4 views, 2 proxies
  view catalog: 1 statement not granted on proxy db
    SELECT 2
      fix: add it to view catalog postgres
  view search: 1 grant never used on proxy db
    SELECT 9

$ uvx --offline ruff check python tools && uvx --offline ruff format --check python tools
All checks passed!
13 files already formatted

$ # the rewritten docs.yml page check, run from the workflow's own heredoc
pages: 27 nav pages, 0 orphan pages
```

`make fast-check` and `make docs-check` re-run green after these edits.

## Follow-ups

- `docs/design/universal-distribution.md` §3 and `docs/decisions/ADR-0017` still describe the
  `--internal-json` dispatcher (agent-only docs, out of this node's `paths`).
- `crates/blastit/src/check.rs:16` and the `critical_config_and_presets.rs` flag case are the only
  remaining `pytest11`/`blastit-manifest` mentions in Rust; both need a `paths` that covers them.
