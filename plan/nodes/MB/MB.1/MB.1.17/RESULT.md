# RESULT MB.1.17 — Regression-test the release hygiene trio in `make check`

## Summary

`docs/testing/AUDIT-MB1.md` `G6` (`AUDIT-MB.1.md` `F8`): the three release-hygiene fixes of `MB.1.7`
were proved once by hand and had no test, so a later edit could re-break them silently. They are now
four oracles in `make check`'s `docs-unit`/`docs-test`, all offline, no Docker, no build:

1. **`python/tests/test_docs_assets.py`** (new, 4 tests) — every `docs/brand/*.svg` must parse as XML
   with an SVG root and a size; `logo.svg` must draw an `@media (prefers-color-scheme: light)` cutout
   for its own `#head-cutout` path; and the **computed** WCAG ratio of both cutouts against the site's
   `--blastit-background` token (`#f4efe3` light, `#121820` dark) must be >= 3.0:1. The ratios are
   computed from the hex values in the file, not read from the asset's comment, so a hand-edited fill
   fails with its own number.
2. **`python/tests/test_release_scripts.py`** (new, 13 tests) — `CARGO_NET_OFFLINE=` is `true`/`false`
   everywhere in `Makefile`/`scripts/**`/`.github/**`/`e2e/**`, naming `file:line`; `blastit-events`
   has no `serde` dependency (parsed with `tomllib`, with `serde_json` asserted present so a wrong
   table cannot pass); every `dist/blastit-*.whl` `METADATA` long description has no relative target
   (Markdown link, `src`/`href`, and `srcset` candidates) and does carry the
   `github.com/OffworldNexus/blastit/blob/` form; `build-wheel.sh` rewrites the README for the duration
   of the build and restores it on the failing **and** the successful path, removing its backup; and
   `dev-version.sh --check`.
3. **`scripts/dev-version.sh --check`** (new read-only mode) — the intra-workspace requirement scan is
   now one function shared by the `--check` mode and the post-rewrite proof, so the two cannot drift.
   `--check` rewrites nothing, reads no `Cargo.lock` and runs no `cargo metadata`; it exits 1 naming
   `file:line` for a lagging requirement (or one with no version at all) and 0 otherwise. The rewrite
   path is otherwise unchanged and is now itself tested on a synthetic tree.
4. **`Makefile`** — `docs-unit` runs both new files; `docs-test` runs the wheel long-description test
   by node id straight after `scripts/build-wheel.sh` with `BLASTIT_REQUIRE_WHEEL=1`, so a missing or
   broken wheel fails the target instead of skipping.

Because `build-wheel.sh` rewrites the *working tree's* `README.md` in place, the restore tests use a
README fixture owned by the test rather than a copy of the live file (see Follow-ups: a concurrent
`make e2e` had the real one rewritten mid-read during the first draft of this node).

## Revert proofs (one line each)

Every one was checked by mutating a **scratch copy** (`/tmp/opencode/mb117-*`) or a planted file, then
reverting; none of the mutations touches this repository.

| Behaviour | Mutation applied | Result |
|---|---|---|
| README rewritten for the build | deleted `"${link_python[@]}" scripts/readme-absolute-links.py` from the copy's `build-wheel.sh` | `test_the_readme_is_rewritten_while_the_wheel_is_built` FAILED: "maturin saw the README without absolute links" |
| same mutation, at `make` level | `make -o docs-unit docs-test` in the copy | exit 2, names `.github/brand/header-dark.svg, ARCHITECTURE.md, LICENSE` |
| README restored on exit | deleted `trap restore_readme EXIT` | both restore tests FAILED: "the README is still rewritten after a failed build" |
| wheel description absolute | (same rewrite deletion) | `test_built_wheel_long_description_has_absolute_links` FAILED naming the relative targets |
| `CARGO_NET_OFFLINE` strict bool | planted `CARGO_NET_OFFLINE=1` in a scratch `Makefile` | FAILED: "`Makefile:2: CARGO_NET_OFFLINE=1`" |
| `blastit-events` has no `serde` | planted `serde = { version = "1.0.219" }` in a scratch manifest | FAILED: "re-gained `serde` under [dependencies]" |
| `dev-version.sh --check` | ran the pre-node script (`git show 9c523e2:…`) | 3 FAILED ("the build number must be digits, not `--check`") |
| SVG light-scheme contrast | removed the `@media` rule / set the fill `#f0ece0` | 3 FAILED / 1 FAILED "1.03:1 on #f4efe3; needs 3.0:1" |
| every brand SVG parses | replaced `&lt;img&gt;` with `<img>` in `logo.svg`'s `<style>` | FAILED: `ParseError: mismatched tag` |
| `docs-test` enforces the wheel check | dropped the wiring line from a scratch `Makefile` | FAILED: "nothing enforces this check" |

## Acceptance

### 1. Asset test (green, then three mutations)

The acceptance command omits `--with pytest`; `--no-project` has no pytest in its environment here, so
the form below is the one the repo's own `docs-unit` uses (pinned `DOCS_PYTEST=9.2.0`).

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
....                                                                     [100%]
4 passed in 0.03s
```

`/tmp/opencode/mb117-svg` — a copy of `python/tests/test_docs_assets.py` + `docs/brand`; the test
resolves the brand directory from its own path, so the copy is self-contained:

```console
$ cd /tmp/opencode/mb117-svg && uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
....                                                                     [100%]
4 passed in 0.01s
```

Mutation A — the media rule removed (the paper cutout is all that is left):

```console
$ perl -0pi -e 's/\n    \@media \(prefers-color-scheme: light\) \{\n      #head-cutout \{ fill: #161a20; \}\n    \}\n//' docs/brand/logo.svg
$ grep -c prefers-color-scheme docs/brand/logo.svg
0
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
E       AssertionError: logo.svg has no `@media (prefers-color-scheme: light)` rule: the header loads the file as an <img>, so page CSS cannot redraw the cutout
FAILED python/tests/test_docs_assets.py::test_the_logo_draws_a_light_scheme_cutout
FAILED python/tests/test_docs_assets.py::test_the_light_cutout_reaches_non_text_contrast
FAILED python/tests/test_docs_assets.py::test_the_paper_cutout_stays_readable_on_the_dark_header
3 failed, 1 passed in 0.03s
```

Mutation B — the rule kept, the cutout painted a near-header colour (proves the ratio is computed):

```console
$ sed -i 's/#head-cutout { fill: #161a20; }/#head-cutout { fill: #f0ece0; }/' docs/brand/logo.svg
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
E       AssertionError: the light-scheme cutout #f0ece0 is 1.03:1 on #f4efe3; WCAG non-text contrast needs 3.0:1
FAILED python/tests/test_docs_assets.py::test_the_light_cutout_reaches_non_text_contrast
1 failed, 3 passed in 0.03s
```

Mutation C — the exact defect `MB.1.7` shipped: a literal `<img>` in the `<style>` comment:

```console
$ sed -i 's/as an &lt;img&gt;/as an <img>/I' docs/brand/logo.svg
$ grep -n "as an" docs/brand/logo.svg
4:    /* The site loads this file as an <img>, so page CSS cannot reach into it and
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
E                   xml.etree.ElementTree.ParseError: mismatched tag: line 13, column 4
FAILED python/tests/test_docs_assets.py::test_every_brand_svg_is_well_formed_xml
FAILED python/tests/test_docs_assets.py::test_the_logo_draws_a_light_scheme_cutout
```

(The first `sed` attempt with a lowercase pattern silently matched nothing and the suite stayed green —
the probe above re-ran case-insensitively and shows `grep` confirming the mutation was applied.)

Restored each time: `uv run ... -m pytest python/tests/test_docs_assets.py -q` -> `4 passed`.

### 2. Wheel description test (`make docs-test`), failing and green

`/tmp/opencode/mb117-wheel/copy` — the working tree rsynced without `.git`/`target`/`dist`, with
`cargo`/`install` stubs on `PATH` and a `MATURIN` stand-in that packages the current `README.md` as a
wheel's `METADATA` (stored, plus the `blastit/_bin/blastit-dbproxy` path the script greps for). The
one change to the copy besides the stub `PATH` is that the final recipe line (`runnable_docs/run.py`)
is dropped: those fences execute a built `blastit` extension the copy does not have. `make -o
docs-unit` skips the dependency target so the failure under test is the metadata step, not the unit
suite (which also notices a removed rewrite — see 3).

Rewrite removed from `scripts/build-wheel.sh`:

```console
$ sed -i '/"\${link_python\[@\]}" scripts\/readme-absolute-links.py/d' copy/scripts/build-wheel.sh
$ cd copy && PATH="…/stubs:$PATH" MATURIN=…/stubs/maturin make -o docs-unit docs-test
E       AssertionError: PyPI renders the long description with no repository around it, so these targets 404:
E         blastit-0.0.0-cp313-cp313-linux_x86_64.whl: no absolute repository link, so the README rewrite did not run before the build
E         blastit-0.0.0-cp313-cp313-linux_x86_64.whl: relative targets .github/brand/header-dark.svg, .github/brand/header-light.svg, .github/brand/header-light.svg, ARCHITECTURE.md, LICENSE, LICENSE
FAILED python/tests/test_release_scripts.py::test_built_wheel_long_description_has_absolute_links
make: *** [Makefile:155: docs-test] Error 1
EXIT=2
```

Rewrite restored (nothing else changed):

```console
$ cp ../../blastit/scripts/build-wheel.sh scripts/build-wheel.sh && rm -rf dist
$ PATH="…/stubs:$PATH" MATURIN=…/stubs/maturin make -o docs-unit docs-test
README.md: .github/brand/header-light.svg → https://raw.githubusercontent.com/OffworldNexus/blastit/develop/.github/brand/header-light.svg
README.md: ARCHITECTURE.md → https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md
README.md: LICENSE → https://github.com/OffworldNexus/blastit/blob/develop/LICENSE
UV_OFFLINE=1 BLASTIT_REQUIRE_WHEEL=1 uv run --no-project --with pytest==9.2.0 python -m pytest -q \
	python/tests/test_release_scripts.py::test_built_wheel_long_description_has_absolute_links
.                                                                        [100%]
1 passed in 0.02s
EXIT=0
$ cmp README.md /home/remy/offworld-nexus/blastit/README.md && echo "copy README identical to the repo's after the build"
copy README identical to the repo's after the build
```

### 3. `build-wheel.sh` restore

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_release_scripts.py -q
.............                                                            [100%]
13 passed in 0.27s
```

The restore test runs the real script in a throwaway tree with `MATURIN=/bin/false`, `cargo`/`install`
stubbed (the recipe's `cargo build --release` is not what this node tests), `TMPDIR` set to the tree,
and asserts the README byte-identical afterwards plus no `blastit-README.*` backup left. A second test
points `MATURIN` at a probe that snapshots the README and exits 1, so it proves both halves — the build
saw `https://github.com/OffworldNexus/blastit/blob/develop/…` and the tree got the original back. A
third runs the success path (a stub maturin writes a wheel carrying the dbproxy path) and asserts the
same restore.

Revert (trap deleted, in `/tmp/opencode/mb117-wheel/copy`):

```console
$ sed -i '/^trap restore_readme EXIT$/d' scripts/build-wheel.sh
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_release_scripts.py -q -k "restores_the_readme or successful_build"
E       AssertionError: the README is still rewritten after a failed build: `make check` would end on a dirty tree
E         README.md: ARCHITECTURE.md#82-configuration-reference → https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md#82-configuration-reference
E         README.md: LICENSE → https://github.com/OffworldNexus/blastit/blob/develop/LICENSE
FAILED python/tests/test_release_scripts.py::test_a_failing_maturin_restores_the_readme_and_leaves_no_temp_file
FAILED python/tests/test_release_scripts.py::test_a_successful_build_still_restores_the_readme
2 failed, 11 deselected in 0.42s
```

Revert (rewrite call deleted, same copy — its `docs-unit` runs the same file):

```console
$ make docs-unit
E       AssertionError: maturin saw the README without absolute links: the rewrite did not run before the build
FAILED python/tests/test_release_scripts.py::test_the_readme_is_rewritten_while_the_wheel_is_built
1 failed, 62 passed, 57 subtests passed in 0.26s
make: *** [Makefile:147: docs-unit] Error 1
EXIT=2
```

The wheel test would rather skip than pass on a missing artefact, so the skip branch checks the wiring
itself (`/tmp/opencode/mb117-skip`, a tree with no `dist/`):

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_release_scripts.py -q -k built_wheel   # Makefile wires it
1 skipped, 12 deselected in 0.05s
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_release_scripts.py -q -k built_wheel   # Makefile line dropped
E           AssertionError: nothing enforces this check: `make docs-test` must run test_built_wheel_long_description_has_absolute_links with BLASTIT_REQUIRE_WHEEL=1 after scripts/build-wheel.sh
FAILED python/tests/test_release_scripts.py::test_built_wheel_long_description_has_absolute_links
1 failed, 12 deselected in 0.04s
```

### 4. `dev-version.sh --check`

New usage line (`--check`), then on the real tree (read-only):

```console
$ scripts/dev-version.sh --check
every intra-workspace requirement names `0.1.2`
EXIT=0
```

Synthetic trees (`python/tests/test_release_scripts.py`), lagging requirement `0.1.0`:

```console
$ bash scripts/dev-version.sh --check
intra-workspace requirements not naming `0.2.0`:
crates/alpha/Cargo.toml:6: beta = { version = "0.1.0", path = "../beta" }
a requirement naming a version other than `0.2.0` (or none) does not accept this workspace's version, so no build of it can resolve
EXIT=1
```

A requirement with no version at all (`beta = { path = "../beta" }`) exits 1 naming the same
`crates/alpha/Cargo.toml:6:`; a consistent tree exits 0 **while a `cargo` stub on `PATH` exits 1 and
writes a sentinel**, and with no `Cargo.lock` — so `--check` provably runs no `cargo metadata` and
needs no lock. The tests also assert the manifest's bytes are unchanged after `--check`.

Revert (the pre-node script, `git show 9c523e2:scripts/dev-version.sh`, against the same tests):

```console
$ cd /tmp/opencode/mb117-devver && uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_release_scripts.py -q -k "check or rewrite"
E       AssertionError: a consistent tree must pass: the build number must be digits, not `--check`
FAILED python/tests/test_release_scripts.py::test_check_names_a_lagging_requirement_and_changes_nothing
FAILED python/tests/test_release_scripts.py::test_check_accepts_a_consistent_tree_without_cargo_or_a_lock
FAILED python/tests/test_release_scripts.py::test_check_flags_a_requirement_with_no_version_at_all
3 failed, 2 passed, 8 deselected in 0.17s
```

The two that still pass are the rewrite-path test (that behaviour was deliberately kept) and the
synthetic-wheel long-description check, which does not touch `dev-version.sh`.

### 5. Spelling / dependency invariants

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_release_scripts.py -q -k "offline or serde"
3 passed, 10 deselected
```

Revert, on `/tmp/opencode/mb117-scan` (the real test file, a `Makefile` with `CARGO_NET_OFFLINE=1`
and a `serde` line planted in `crates/blastit-events/Cargo.toml`):

```console
E       AssertionError: CARGO_NET_OFFLINE must be `true` or `false`, never cargo's `1`: maturin rejects it (`docs/knowledge/LESSONS.md`).
E         Makefile:2: CARGO_NET_OFFLINE=1
E           AssertionError: crates/blastit-events/Cargo.toml re-gained `serde` under [dependencies]: nothing in the crate uses `serde::` or its derives, and only Cargo.lock notices the extra edge
FAILED python/tests/test_release_scripts.py::test_no_cargo_net_offline_is_spelled_one
FAILED python/tests/test_release_scripts.py::test_blastit_events_does_not_depend_on_serde
2 failed, 1 passed, 10 deselected in 0.09s
```

Both scanners are also unit-tested on synthetic text (`CARGO_NET_OFFLINE=1` -> one offender,
`true`/`false`/comment -> none; `](LICENSE)`, `srcset="a.svg 1x, b.svg 2x"` -> offenders, absolute
URLs -> none), so a gutted detector fails even with a clean tree.

### 6. The gate

```console
$ make docs-unit
63 passed, 57 subtests passed in 0.67s
EXIT=0

$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.24s
EXIT=0

$ make docs-test
… (see the tail below)
EXIT=0
```

`make docs-test` tail (the build, the wheel assertion, then the fence gate; `EXIT=0`):

```
$ UV_OFFLINE=1 make docs-test
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py \
	python/tests/test_docs_assets.py \
	python/tests/test_release_scripts.py -q
63 passed, 57 subtests passed in 0.70s
scripts/build-wheel.sh
    Finished `release` profile [optimized] target(s) in 0.14s
README.md: ARCHITECTURE.md → https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md
README.md: LICENSE → https://github.com/OffworldNexus/blastit/blob/develop/LICENSE
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.2-cp314-cp314-manylinux_2_39_x86_64.whl
UV_OFFLINE=1 BLASTIT_REQUIRE_WHEEL=1 uv run --no-project --with pytest==9.2.0 python -m pytest -q \
	python/tests/test_release_scripts.py::test_built_wheel_long_description_has_absolute_links
1 passed in 0.03s
uv run --no-project python python/tests/runnable_docs/run.py
harness: wheel blastit-0.1.2-cp314-cp314-manylinux_2_39_x86_64.whl sha256 814deca7bddd · installed package verified against the built wheel
PASS  README.md  fence 1  install  0.8s  installed blastit-0.1.2-…whl and verified it against the built wheel
… (38 fences, all PASS)
38 fences: 26 executed, 12 accounted, 0 failed · 35.0s
EXIT=0
```

After the run `git status --short README.md` is empty — the trap restored the working tree.

`critical_guard.py` / `plan.py validate`:

```console
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.17
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [VIOLATION: not covered by any node path]
  A  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs  [added (always allowed)]
  M  crates/blastit/tests/critical_check_engine.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 2 protected critical test(s) changed or deleted without a covering path in node(s) MB.1.17.
GUARD_EXIT=1

$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.17 --node MB.1.12 --node MB.1.13 --node MB.1.15
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_edit.rs]
  A  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs  [added (always allowed)]
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.1.17, MB.1.12, MB.1.13, MB.1.14, MB.1.15
GUARD_EXIT=0

$ uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

The two "violations" are the sibling nodes in flight in this working tree (`MB.1.12`
`critical_policy_surface.rs`, `MB.1.13` `critical_check_engine.rs`, `MB.1.15`
`critical_manifest_edit.rs`), not this node: `MB.1.17` touches no `critical_*` file (its own diff is
`python/tests/test_docs_assets.py`, `python/tests/test_release_scripts.py`, `scripts/dev-version.sh`,
`Makefile`). The second run is the same guard with the nodes that own those paths, which the audit
(`AUDIT-MB1.md`, "Protected-critical mapping") prescribes for a gate over several leaves.

`make check` itself was not run (the lead's gate step, and a `make e2e` was in flight); its docs legs
are above. No Docker was run.

## Lint

```console
$ uvx --offline ruff check python/tests/ && uvx --offline ruff format --check python/tests/test_release_scripts.py python/tests/test_docs_assets.py
All checks passed!
2 files already formatted
```

`bash -n scripts/dev-version.sh` -> `syntax ok`; `docs/brand/logo.svg` is unchanged (no edit was
needed: its shipped light-scheme cutout passes the new test).

## Files

- `python/tests/test_docs_assets.py` — new (4 tests).
- `python/tests/test_release_scripts.py` — new (12 tests).
- `scripts/dev-version.sh` — `--check` mode; the requirement scan factored into one function used by
  both paths; no behaviour change to the rewrite.
- `Makefile` — `docs-unit` includes both files; `docs-test` runs the wheel metadata test after the
  build with `BLASTIT_REQUIRE_WHEEL=1`.

## Follow-ups

1. **A test may not copy the live `README.md`.** `scripts/build-wheel.sh` rewrites the working tree's
   `README.md` in place for the whole build, so a test that copies it races any concurrent
   `make docs-test`/`make e2e`: the first draft of this node read a fully rewritten README and failed
   on "no relative link to rewrite". The restore tests now use a fixture README; the real file is
   covered by the wheel-metadata check. Worth one line in `docs/knowledge/LESSONS.md` — left out
   because the file is not in this node's `paths`.
2. **The scheduler early return is not covered here.** The dispatch brief also asked for
   `Scheduler::plan_request_body`'s `Ok(BodyPlan::AllowEmptyOnly)` fast path when `policy.is_none()`.
   That is already pinned by `crates/blastit-scheduler/tests/head_allocs.rs`
   (`a_policy_less_head_is_planned_without_copying_its_headers`, audit `C9` = PASS), and this node's
   `paths` hold no Rust file, so no new test was added. If a direct oracle is still wanted, it needs a
   node owning `crates/blastit-scheduler/tests/**`.
3. **The residual from `MB.1.7` remains.** The asset-level `prefers-color-scheme` rule answers the OS
   light preference; the site's light **toggle** on a dark OS still draws the paper cutout, because an
   `<img>` cannot be reached by page CSS. The new tests pin what ships, not the toggle; closing it
   needs the logo inlined in `brand.css`/a page override, outside this node.
4. `dist/` is generated: the wheel-metadata test asserts every wheel present, so a stale wheel left by
   an old, broken build fails `docs-unit` before `docs-test` rebuilds it. Acceptable (the artefact is
   the claim); mention it if a CI cache ever holds `dist/`.
