# MA.3.9 RESULT — account for every documented snippet shape

- **Node:** `plan/nodes/MA/MA.3/MA.3.9/NODE.md` (attempt 1/3)
- **Status:** `done` (see `## Acceptance` for the last `make check`)
- **Paths touched:** `python/tests/runnable_docs/{fences.py,run.py,inventory.toml}`,
  `python/tests/test_runnable_docs.py`. Nothing else, no commits, no plan statuses.

## What changed

`fences.py` no longer recognises one spelling of a fence with a regex. `scan_page` walks the
Markdown once and returns every fenced block — three or more backticks **or** tildes, any
indentation (a list item or an admonition holds one), and an info string after the language
(` ```console title="…" `) — with the body dedented by the block's own indentation, so an
indented `console` block still yields its `$ ` commands. `console_fences` is a filter over those
blocks, so numbering in reading order and the `Fence`/`Command` API the harness already imports are
unchanged. The same pass returns the **shapes it cannot account for**, which `check_inventory` now
turns into a red gate (`run.scan`), instead of the five silent passes the audit reproduced:

| shape | before | now |
|---|---|---|
| `~~~console` | 0 fences found | 1 block, executed |
| ```` ````console ```` | 0 fences found | 1 block, executed |
| two/three-space indented fence | 0 fences found | 1 block, dedented, executed |
| ` ```console title="start it"` | 0 fences found | 1 block, executed |
| `$ ` prompt in a `bash` block | 0, invisible | **red**: "label the block `console`" |
| `$ ` prompt outside every block (a fence that closed early) | 0, invisible | **red**: "a `$ ` command outside any block" |
| `console` block with no closing marker | trailing commands dropped | **red**: "has no closing marker" |
| a fence marker left inside a `console` body (duplicated/truncated) | read as output | **red**: "a fence marker inside the console block" |
| `reason = " "` / `"deploy"` / `"TODO"` | accounts a fence | **red**: blank / too short / placeholder |

`public_pages` now answers the question the gate actually has: `README.md` plus every
`docs/**/*.md` that `zensical.toml`'s `exclude_docs` does not exclude (leading `/` anchors a rule,
a trailing `/` excludes a directory, a bare name matches at any depth, `*`/`?` are wildcards, `!`
re-includes). The nav is a menu; a page it omits is still built and served, so it must be
inventoried. `check_inventory` takes a `repo` root (default `REPO`, so the CI caller in
`.github/workflows/docs.yml` is unchanged) which is what lets the new tests run it over a synthetic
tree.

`python/tests/test_runnable_docs.py` (new, `unittest`, stdlib only, standard tier): 21 tests over
synthetic Markdown fed to the real parser and the real inventory check — no mocks of our own
harness. It covers all eight fence spellings, the five scan failures with their allowed twins,
orphan/excluded pages through a throwaway repo, the reason floor, `report_result` counts, a
`Fence.commands()` heredoc, and the repository's own inventory.

## Acceptance

### `make docs-test` (38 fences / 21 pages, no Docker)

```console
$ make docs-test
…
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl
harness: wheel blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 c99b73b22890 · per-run uv cache /tmp/opencode/brd-oe2bxfep/uv-cache · installed package verified against the built wheel
…
38 fences: 26 executed, 12 accounted, 0 failed · 22.0s
EXIT=0
```

**Concurrency caveat, and the pinned run that removes it.** This tree is shared with sibling nodes
in flight: `python/tests/runnable_docs/harness.py` (MA.3.10's path, not mine) was an uncommitted
work-in-progress throughout, and still growing (90 448 B at 02:09:23, 110 042 B at 02:09:51) after
my run — so that run executed their WIP harness, not `HEAD`'s. To remove the variable I copied
the tree to `/tmp/opencode/ma39/gate`, pinned `harness.py` (and `pgstub.py`) to `HEAD`, kept my four
files and symlinked `dist/` at the wheel `make docs-test` had just built, then ran the gate's second
step:

```console
$ uv run --no-project python python/tests/runnable_docs/run.py     # pinned HEAD harness
harness: wheel blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 c99b73b22890 · per-run uv cache /tmp/opencode/brd-vahztvf9/uv-cache · installed package verified against the built wheel
…
38 fences: 26 executed, 12 accounted, 0 failed · 19.2s
```

Both runs agree on the same 21 pages, the same 38 fences and the same 26/12 split, and the same
wheel. The inventory half is the part this node owns, and it is the part that decides what the
execution half is allowed to skip.

### `uv run --no-project python -m pytest python/tests/test_runnable_docs.py -q`

The literal command needs `pytest` on the interpreter it picks; this host's `.venv` has none, and
the gate's own pin is `pytest==9.2.0` (`harness.PYTEST_VERSION`):

```console
$ uv run --no-project python -m pytest python/tests/test_runnable_docs.py -q
/home/remy/offworld-nexus/blastit/.venv/bin/python: No module named pytest

$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_runnable_docs.py -q
.....................                                                    [100%]
21 passed, 19 subtests passed in 0.07s
```

### The tests are red on pre-fix code (the point of the exercise)

`git show HEAD:python/tests/runnable_docs/{fences,run}.py` into a copy of the tree, then the same
suite:

```console
$ cd /tmp/opencode/ma39/prefix-tree && uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_runnable_docs.py -q
FAILED python/tests/test_runnable_docs.py::FenceShapeTest::test_every_shape_yields_its_command     # every shape row: plain, tilde, four backticks, indented, info suffix
FAILED python/tests/test_runnable_docs.py::PageListTest::test_an_orphan_page_without_an_entry_is_red
FAILED python/tests/test_runnable_docs.py::PageListTest::test_published_pages_include_an_orphan_and_drop_the_excluded
FAILED python/tests/test_runnable_docs.py::SilentCommandTest::test_an_unclosed_block_is_red
… 33 failed, 7 passed in 0.29s
```

and the same rows measured directly against HEAD's parser (old → new):

```console
$ uv run --no-project python /tmp/opencode/ma39/prefix_probe.py
shape                       HEAD  now  row
tilde                          0    1  green (0 problem)
four backticks                 0    1  green (0 problem)
two-space indented             0    1  green (0 problem)
info suffix                    0    1  green (0 problem)
bash block with a prompt       0    0  red (1 problem)
page list HEAD: ['README.md', 'docs/index.md']
page list now:  ['README.md', 'docs/index.md', 'docs/orphan.md']
```

### HOME-scrubbed unit tests (the tests read no `HOME`)

```console
$ env -i HOME=/tmp/opencode/ma39/docs-home UV_CACHE_DIR="$(uv cache dir)" UV_OFFLINE=1 PATH="$PATH" \
    uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_runnable_docs.py -q
.....................                                                    [100%]
21 passed, 19 subtests passed in 0.06s
```

### `make check`

Run after the sibling nodes' work in this shared tree settled (the run before it was red only on
MA.3.10's in-flight `harness.py`). Full gate, `EXIT=0`:

```console
$ make check
uv run --no-project tools/guard.py
uv run --no-project tools/critical_guard.py --base HEAD 
PYTHONHOME=… PYO3_PYTHON=… cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked -- -D warnings
… cargo test --workspace --all-targets --locked        # test result: ok … (every crate)
… cargo test --workspace --doc --locked
… cargo doc --workspace --no-deps --locked
python -m unittest discover -s python/tests/critical
Ran 50 tests in 3.348s
OK
uvx --offline ruff check python tools
uvx --offline ruff format --check python tools
uvx --offline zensical build --strict
scripts/build-wheel.sh
…
38 fences: 26 executed, 12 accounted, 0 failed · 25.3s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

That run used the *current* `harness.py` (MA.3.10's stricter F2/F4 modes, the
"statically checked every command, flag and snippet" detail), so it also proves the stricter
scanner did not turn a single real page red: 38 fences, same 26/12 split, zero failures.

### The CI half of the same inventory check still passes

`.github/workflows/docs.yml` runs `check_inventory` plus its own nav-orphan sweep (MA.3.8's job).
Extracting that script and running it against the new `public_pages`:

```console
$ uv run --no-project python /tmp/opencode/ma39/ci_inventory.py
inventory: 21 pages / 38 fences accounted, 20 nav pages, 0 orphan pages
EXIT=0
```

## Follow-ups (out of scope here)

- `harness.py` still parses `toml` blocks with its own `^```toml` regexes (`_page_manifest`,
  `_static_toml`). An indented or tilde TOML block is now visible to the inventory but still
  invisible to those two helpers — F4/H14 (MA.3.10's path).
- `docs.yml`'s inline nav sweep is now redundant with `run.public_pages`; it can be dropped when
  that workflow is next touched (keep it if you want nav-membership enforced as a separate rule,
  since the inventory now accepts a published-but-unlisted page once it has an entry).
- `docs/archive/` and `docs/spikes/` are excluded from the site, so nothing in them is inventoried
  or scanned. That is deliberate (they are working material), but it does mean those two pages can
  drift.
- Host note for whoever runs the gate next: `/tmp` here is a 32 GiB tmpfs at 97 % full (~19 GiB of
  stale `*-target` dirs from earlier nodes), so a worktree build cannot go in `/tmp`; the pinned
  run above deliberately reused the existing `dist/` wheel instead of rebuilding.
