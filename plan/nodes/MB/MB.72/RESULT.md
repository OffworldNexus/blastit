# MB.72 — Bump GitHub Actions off Node 20

Target majors pre-verified from each upstream `action.yml` (`runs.using`); applied repo-wide with `sed -i`
per file, only the `@vX` token changed. 9 files, 102 lines, every changed line a `uses:` line.

| action | old | new | GitHub evidence |
|---|---|---|---|
| `actions/checkout` | `@v4` | `@v5` | `v5.0.0` "Use node 24"; `runs.using: node24` |
| `actions/upload-artifact` | `@v4` | `@v6` | `v6.0.0` "now runs on Node.js 24"; v4/v5 are node20 |
| `actions/download-artifact` | `@v4` | `@v7` | `v7.0.0` node24; v6 still node20 |
| `actions/setup-python` | `@v5` | `@v6` | `v6` `runs.using: node24` |
| `actions/configure-pages` | `@v5` | `@v6` | `v6.0.0` "upgrade to node 24" |
| `actions/deploy-pages` | `@v4` | `@v5` | `v5.0.0` "Update Node.js version to 24.x" |
| `actions/upload-pages-artifact` | `@v3` | `@v5` | v3/v4 composite pins node20 `upload-artifact@v4`; `v5` pins `upload-artifact@v7.0.0` (node24) |

Also bumped `.github/actions/build-wheel/action.yml` (line 116) — no stragglers. Skipped (already Node 24
or not a node runtime): `astral-sh/setup-uv@v10.3.2`, `dtolnay/rust-toolchain@*` (composite),
`Swatinem/rust-cache@v2`, `pypa/gh-action-pypi-publish@release/v1` (composite + Docker),
`uses: ./.github/actions/build-wheel` (local).

**No call-site adaptation required:** `upload-artifact@v6` keeps `name`/`path`/`if-no-files-found`/
`retention-days`/`compression-level`/`overwrite`/`include-hidden-files`; `download-artifact@v7` keeps
`name`/`path`/`pattern`/`merge-multiple` (still defaulting `false`); `setup-python@v6` is an input superset
(`python-version`, `cache`, `cache-dependency-path`, `update-environment` unchanged); `checkout@v5`,
`configure-pages@v6`, `deploy-pages@v5` keep identical input names; `upload-pages-artifact@v5` adds
`include-hidden-files` default `false`, matching the dotfile exclusion already in effect. Triggers,
permissions, matrices, artifact names/patterns and indentation untouched; no changed line had an inline
comment (`grep -rn "uses:.*#" .github/` → none).

## Evidence

`grep -rhoE "uses: [^ ]+" .github/ | sort | uniq -c`
```
     49 uses: actions/checkout@v5          22 uses: actions/download-artifact@v7
     19 uses: actions/upload-artifact@v6    9 uses: actions/setup-python@v6
      1 uses: actions/configure-pages@v6    1 uses: actions/deploy-pages@v5
      1 uses: actions/upload-pages-artifact@v5
     39 uses: astral-sh/setup-uv@v10.3.2    1 uses: dtolnay/rust-toolchain@1.88.0
     17 uses: dtolnay/rust-toolchain@stable 1 uses: every  <- comment prose, not a pin
      2 uses: ./.github/actions/build-wheel  2 uses: pypa/gh-action-pypi-publish@release/v1
     16 uses: Swatinem/rust-cache@v2
```
One version per action; zero `checkout@v4`, `upload/download-artifact@v4`, `setup-python@v5`,
`configure-pages@v5`, `deploy-pages@v4`, `upload-pages-artifact@v3`; `grep -rn "uses:" .github/ | wc -l`
→ before `180`, after `180`.

`make workflow-syntax` → `workflow-script-syntax: ok (5 shell word(s) in 8 workflow(s))`.
`make fast-check` → `install-test: all cases passed (fixture on http://127.0.0.1:8745, ...)`; guard,
workflow-syntax, rs-fmt/lint/doc/test, py-lint, plan-validate, universal-smoke all green.

One unrelated flake on the first `make fast-check` run:
`e2e_check_fmt::the_installed_bundle_carries_the_gate_and_the_venv_wins` panicked with `ExecutableFileBusy`
("Text file busy", an ETXTBSY race on the freshly installed binary); it passed on re-run
(`24 passed; 0 failed`) and the full target then passed, with no code change made for it.

## Runtime audit: nothing left on Node 20

Every `uses:` that remains after the bump is either node24, a composite, a Docker action, or local —
none targets Node 20: `astral-sh/setup-uv@v10.3.2` (`runs.using: node24`), `Swatinem/rust-cache@v2`
(`node24`), `dtolnay/rust-toolchain@stable`/`@1.88.0` (composite shell steps, no node runtime),
`pypa/gh-action-pypi-publish@release/v1` (composite + Docker; internally pins `setup-python@v6.2.0`),
`uses: ./.github/actions/build-wheel` (local composite), and the comment line `uses: every` (prose, not a pin).

Composite internals we do not own: `upload-pages-artifact@v5` pins `actions/upload-artifact@v7.0.0`
(node24), so the pages flow executes two upload-artifact majors (`@v6` for our steps, `@v7` inside the
composite). Deliberate — `upload-artifact@v7` (`archive: false` direct uploads ignore `name` and reject
multi-file globs) and `download-artifact@v8` (`digest-mismatch` defaults to `error`, no auto-unzip) both
carry call-site audits this node excludes, and neither removes an annotation. Deferred to a separate node.

## Evidence attribution

`make fast-check` ran in the shared worktree while MB.73's `e2e/run_packaging.py` rewrite was uncommitted,
so its `install-test` output is attributable to MB.73's code. MB.72's own attributable gates are
`make workflow-syntax` (green, above) and `uv run --no-project tools/plan.py validate`; re-run
`make fast-check` after MB.73 lands.

## Final gate re-run (docs only)

`uv run --no-project tools/plan.py validate` → exit `0`
```
plan OK
```

`make docs-check` → exit `0` (`gen_language_blocks.py --check` then `uvx --offline zensical build --strict`)
```
No issues found
Build finished in 0.37s
```
