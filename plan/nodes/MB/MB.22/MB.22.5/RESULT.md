# Result MB.22.5

The extension is installable and proven, not merely present:

1. `[grammars.blastfile].rev` is **`053b56e1770f81a5856e17a0b8b68004d9e67069`** (the
   lead's `MB.22.4` commit), proved to be a commit that contains both
   `editors/tree-sitter-blastfile` and `editors/zed`, and reproduced by a
   `--depth 1` fetch and by `git archive`.
2. `scripts/check_zed_extension.py` checks **42 properties** of
   `editors/**` one line each; `scripts/test_check_zed_extension.py` has **19
   cases**, each doctoring one property and requiring the guard to go red naming
   it. `make zed-check` runs both, and `check:` lists `zed-check` as a
   prerequisite (`grep -Eq '^check:.*zed-check' Makefile` is true).
3. `python/tests/critical/test_lsp_stdio.py` (protected) drives one real session
   through both distribution paths: the standalone binary
   (`BLASTIT_LSP_COMMAND`) and the console script (`sys.executable -m blastit lsp`
   under the `test-critical` staged `PYTHONPATH`). Both transcripts are below and
   are byte-identical in every asserted field.

Nothing is committed. No plan status was mutated; only
`plan.py --agent implementer log MB.22.5` was appended.

## Evidence

## The rev pin, in the order §12.3 gives it

```console
$ REV=$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1); test ${#REV} -eq 40
REV=053b56e1770f81a5856e17a0b8b68004d9e67069 len=40
$ git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"
parser.c-at-rev OK
$ git cat-file -e "$REV:editors/zed/extension.toml"          # §12.8's added half
extension.toml-at-rev OK
$ git rev-parse "$REV"; git cat-file -t "$REV"
053b56e1770f81a5856e17a0b8b68004d9e67069
commit
$ rm -rf /tmp/opencode/grammar-fetch && git init -q /tmp/opencode/grammar-fetch
$ git -C /tmp/opencode/grammar-fetch remote add origin "file://$PWD"
$ git -C /tmp/opencode/grammar-fetch fetch -q --depth 1 origin "$REV"   # no `||` fallback
$ git -C /tmp/opencode/grammar-fetch checkout -q FETCH_HEAD
$ test -f /tmp/opencode/grammar-fetch/editors/tree-sitter-blastfile/src/parser.c
shallow-fetch OK
$ git -C /tmp/opencode/grammar-fetch rev-parse FETCH_HEAD
053b56e1770f81a5856e17a0b8b68004d9e67069
```

`git archive` reproduces the **committed** grammar directory byte for byte:

```console
$ git archive "$REV" -- editors/tree-sitter-blastfile | tar -x -C /tmp/opencode/grammar-archive
$ diff -r --exclude=alloc.h --exclude=array.h \
    /tmp/opencode/grammar-archive/editors/tree-sitter-blastfile editors/tree-sitter-blastfile
archive matches the committed dir
$ git check-ignore -v editors/tree-sitter-blastfile/src/tree_sitter/alloc.h …
editors/tree-sitter-blastfile/.gitignore:5:/src/tree_sitter/alloc.h
editors/tree-sitter-blastfile/.gitignore:6:/src/tree_sitter/array.h
```

`alloc.h`/`array.h` are the two generator headers `MB.22.4` deliberately left
gitignored (only an external scanner includes them; this grammar has none), so
they are absent from the archive and from the commit by design.

The placeholder appears nowhere in the committed or source tree:

```console
$ # the accept-side proof: tracked files only, so the gitignored wasm build cannot count
$ git grep -n '0000000000000000000000000000000000000000' -- editors/zed ; echo rc=$?
git-grep-rc=1
$ # the literal `grep -rn` needs the gitignored build output set aside (see Deviation 1)
$ grep -rn '0000000000000000000000000000000000000000' editors/zed ; echo rc=$?
placeholder absent under editors/zed: rc=0
```

**Public-fetch precondition.** The `file://` fetch above proves the pin against
this local object store. Zed's registry fetch is a `--depth 1 <rev>` against the
public remote, and a by-SHA fetch only works once `053b56e…` is reachable on a
**pushed** branch. That push is `conductor`'s step for `release/0.2.0`; this node
records it as a precondition, not something it performs (it never pushes or tags).

There is no `npx --yes tree-sitter-cli@0.25 …` command anywhere in `MB.22.5`'s
acceptance or body, so nothing had to be adapted to the host's lack of
`npm`/`npx` here. `MB.22.4`'s pinned `0.25.10` release binary (still at
`/tmp/opencode/bin/tree-sitter`) was not needed by this node: the guard is
static, and the only grammar artefact it reads is the committed `src/`.

## Static guard: `make zed-check`

`scripts/check_zed_extension.py` is stdlib-only, takes `--root`, never reads the
network and never runs `tree-sitter`. It prints one line per property (42) and
fails 0 when they all hold:

```console
$ uv run --no-project scripts/check_zed_extension.py
ok   extension: keys
ok   extension: id
ok   extension: schema_version
… (42 `ok` lines)
zed-check: OK (42 properties checked)
$ wc -l < …/zedcheck.out
43
```

The properties, as the design pins them (§8.1–§8.4, §10.1–§10.3, §12.4):
`extension.toml`'s seven required keys and `schema_version = 1`; `id`/`name`;
strict-semver `version`; `description` longer in chars than `name`;
`[grammars.blastfile]` with the literal repository, a 40-lowercase-hex rev that is
not the placeholder, and `path = "editors/tree-sitter-blastfile"`; the server's
`languages = ["Blastfile"]`, `language_ids.Blastfile = "blastfile"`, and no `name`
key; `config.toml`'s `name`/`grammar`/`path_suffixes`/`line_comments` as literals
plus `tab_size = 4`/`hard_tabs = false`; the four generated files and the
`tree_sitter_blastfile` export; `grammar.js`'s `name` equal to the grammar key;
no `scanner.c`/`scanner.cc` and no `externals`; `package.json` carrying an exact
generator version; `highlights.scm`'s captures **exactly** the fifteen-name
allowlist (§10.1, `variable.special` included) and the
`(env_var) @variable.special @variable` pair; `indents.scm` `@indent`;
`brackets.scm` `@open`/`@close`; `outline.scm` `@item`/`@name`; the empty
`[workspace]` and `crate-type = ["cdylib"]`; `LICENSE` resolving to the
repository's license; no **committed** `extension.wasm`/`target/`/`grammars/`
and all three gitignored; `src/lib.rs`'s `which("blastit")` before
`which("uv")`, the six argv literals in order with `--no-sync`, exactly one
`shell_env()`, no `std::process`/`Command::new`, the `uv add blastit`/`uv sync`
error, and no `[capabilities]`; and the committed `.zed/settings.json` plus the
`docs/local.md` fence both turning `format_on_save` on.

The "committed build artefact" rule is a **git** fact (`git ls-files`), because
Zed writes `target/`, `extension.wasm` and `grammars/` into this directory at
install time and `MB.22.4`'s wasm build leaves `editors/zed/target/` behind; a
filesystem-only rule would fail every checkout that ever built the extension.

The self-tests build a faithful temp copy of `editors/`, `.zed/settings.json`,
`LICENSE` and `docs/local.md`, `git init` + `git add -A` it, doctor exactly one
property and require `rc=1` naming it:

```console
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py -v
test_a_missing_file_is_red_not_a_traceback ... ok
test_branch_shaped_rev_is_red ... ok
test_capture_outside_allowlist_is_red ... ok
test_committed_wasm_is_red ... ok
test_description_not_longer_than_name_is_red ... ok
test_docs_fence_off_is_red ... ok
test_license_escaping_the_repository_is_red ... ok
test_missing_indent_is_red ... ok
test_missing_item_is_red ... ok
test_missing_language_ids_is_red ... ok
test_missing_license_is_red ... ok
test_missing_workspace_is_red ... ok
test_path_suffixes_typo_is_red ... ok
test_settings_off_is_red ... ok
test_short_rev_is_red ... ok
test_swapped_which_order_is_red ... ok
test_the_repository_passes ... ok
test_two_shell_env_calls_is_red ... ok
test_unexported_grammar_is_red ... ok
----------------------------------------------------------------------
Ran 19 tests in 2.705s
OK
```

```console
$ make zed-check
uv run --no-project scripts/check_zed_extension.py
ok   extension: keys
…
zed-check: OK (42 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
...................
Ran 19 tests in 2.705s
OK
$ grep -Eq '^check:.*zed-check' Makefile && echo check-line-ok
check-line-ok
```

## Install build (§ acceptance 3)

```console
$ stat -c '%y' target
2026-11-17 06:36:50.771530372 +0100
$ cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
$ stat -c '%y' target
2026-11-17 06:36:50.771530372 +0100                     # unchanged
$ ls -l editors/zed/target/wasm32-wasip2/debug/blastit_zed.wasm
-rw-r--r-- 2 remy remy 7436915 … blastit_zed.wasm       # from editors/zed/target/
```

(The grammar wasm from the committed `parser.c` with Zed's wasi-sdk clang line is
`MB.22.4`'s proof, re-run there with `llvm-nm … | grep tree_sitter_blastfile`; nothing
in this node changed `src/`.)

## Live proof: both distribution paths over real pipes

`make rs-no-python` rebuilt and inspected the standalone artifact; the same
session driver then spoke to both commands over real pipes. Both transcripts are
**identical in every field**, including the exit statuses:

```console
$ make rs-no-python
…
clippy      -p blastit{,-view} (standalone)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
default build: 0 pyo3 dependencies (no libpython linked)
rc=0
```

```console
$ uv run --no-project python /tmp/opencode/lsp_transcript.py "$PWD/target/debug/blastit" lsp
$ /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/debug/blastit lsp
initialize -> {"capabilities": {"documentFormattingProvider": true, "hoverProvider": true, "positionEncoding": "utf-16", "textDocumentSync": {"change": 1, "openClose": true, "save": false}}, "serverInfo": {"name": "blastit", "version": "0.2.0"}}
didOpen broken -> [{"message": "`=` is not Blastfile syntax; write the value after the keyword: `body json`", "range": {"end": {"character": 8, "line": 2}, "start": {"character": 7, "line": 2}}, "severity": 1, "source": "blastit"}]
formatting broken -> null
didChange canonical -> diagnostics []
formatting canonical -> []
didChange misindented -> diagnostics []
formatting misindented -> [{"newText": "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n", "range": {"end": {"character": 0, "line": 5}, "start": {"character": 0, "line": 0}}}]
didChange SQL -> [{"message": "route \"v\" (index 0): queries[0] inlines a quoted literal (grant on proxy \"db\"); bind the value with `$?` instead of writing it in the statement", "range": {"end": {"character": 0, "line": 0}, "start": {"character": 0, "line": 0}}, "severity": 1, "source": "blastit"}]
shutdown -> null
exit status -> 0; stderr -> ''
EOF before shutdown -> 1
```

```console
$ # console-script route: staged PYTHONPATH from `make test-critical`
$ PYTHONPATH="$PWD/target/python-critical:$PWD/python" PYTHONHOME=… LD_LIBRARY_PATH=… \
    uv run --no-project python /tmp/opencode/lsp_transcript.py "$PYO3_BIN" -m blastit lsp
$ /home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/bin/python3.14 -m blastit lsp
initialize -> {"capabilities": {"documentFormattingProvider": true, "hoverProvider": true, "positionEncoding": "utf-16", "textDocumentSync": {"change": 1, "openClose": true, "save": false}}, "serverInfo": {"name": "blastit", "version": "0.2.0"}}
didOpen broken -> [{"message": "`=` is not Blastfile syntax; write the value after the keyword: `body json`", "range": {"end": {"character": 8, "line": 2}, "start": {"character": 7, "line": 2}}, "severity": 1, "source": "blastit"}]
formatting broken -> null
didChange canonical -> diagnostics []
formatting canonical -> []
didChange misindented -> diagnostics []
formatting misindented -> [{"newText": "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n", "range": {"end": {"character": 0, "line": 5}, "start": {"character": 0, "line": 0}}}]
didChange SQL -> [{"message": "route \"v\" (index 0): queries[0] inlines a quoted literal (grant on proxy \"db\"); bind the value with `$?` instead of writing it in the statement", "range": {"end": {"character": 0, "line": 0}, "start": {"character": 0, "line": 0}}, "severity": 1, "source": "blastit"}]
shutdown -> null
exit status -> 0; stderr -> ''
EOF before shutdown -> 1
```

The diagnostic's sentence is the CLI's own, verified against the same fixture:

```console
$ cd /tmp/opencode/checkproof && cat Blastfile     # "# ✓ ok\nview v \"/x\" {\n  body =\n}\n"
$ target/debug/blastit check
blastit check: manifest ./Blastfile: 3:8: `=` is not Blastfile syntax; write the value after the keyword: `body json`
check-rc=2
```

`3:8` is 1-based; the LSP's `line: 2, character: 7` is exactly `line - 1` /
`col - 1`, and the message is the CLI line minus its `Blastfile:3:8: ` prefix.

## Protected critical suite

```console
$ BLASTIT_LSP_COMMAND="$PWD/target/debug/blastit lsp" \
    uv run --no-project python -m unittest discover -s python/tests/critical -p 'test_lsp_stdio.py' -v
test_eof_before_shutdown_exits_1 ... ok
test_one_document_three_formatting_outcomes_and_a_clean_stderr ... ok
test_the_capability_document_is_the_accepted_one ... ok
----------------------------------------------------------------------
Ran 3 tests in 0.012s
OK
$ # console script, no BLASTIT_LSP_COMMAND, staged tree
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 PYTHONPATH="$PWD/target/python-critical:$PWD/python" … \
    uv run --no-project python -m unittest discover -s python/tests/critical -p 'test_lsp_stdio.py' -v
Ran 3 tests in 0.057s
OK
$ make test-critical
…
python -m unittest discover -s python/tests/critical
.....................................................
----------------------------------------------------------------------
Ran 53 tests in 10.572s
OK
rc=0
```

The suite asserts: the capability document (and the four absent capabilities);
exactly one diagnostic at `(2,7)-(2,8)` with severity 1, source `blastit` and the
exact sentence; formatting `null` on the broken buffer; `[]` diagnostics and `[]`
edits on the canonical buffer; one whole-document `TextEdit` ending at `(5,0)` for
the two-space buffer; one diagnostic for the `query "COPY s3cr3t …"` document and
**no `s3cr3t`/`COPY`/`PROGRAM` on stderr**; `shutdown`+`exit` = `0`; EOF before
`shutdown` = `1`. `BLASTIT_LSP_COMMAND` is shell-split, so the standalone binary
and the console script run the same assertions.

## The rest of `# Acceptance`

```console
$ uv run --no-project tools/guard.py
guard: OK (11 staged/untracked file(s) checked)
$ make docs-unit
107 passed, 65 subtests passed in 1.53s
$ make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.20s
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 0.65s
$ uv run --no-project tools/plan.py validate
plan OK
```

`make check` reaches `docs-test` and stops on **one** fence, exactly the failure
the node predicted:

```console
FAIL  docs/reference/cli.md  fence 1  dev-flags  0.2s  blastit dev exited before ready;
stderr: blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 37.0s
make: *** [Makefile:342: docs-test] Error 1
```

`grep FAIL /tmp/opencode/make-check.log` finds only that fence; the workspace
tests, `critical_lsp`, `test-critical` (53 tests) and `docs-unit` before it all
passed. The page and the `--no-reload` flag belong to `MB.23`, not this node
(§12.8's follow-up row). Because `docs-test` stops the chain, the targets after it
were run individually and are green: `rs-no-python`, `seam-check`,
`platform-matrix-check`, `workflow-check`, `packaging-gates-test`, `doc-tone`,
`manifest-purity`, `zed-check`, `plan-validate` (tails above; `platform-matrix`
reports the documented 4/8 wheels, 2/4 archives, 6/12 shims, 0 legs dropped).

## Review fixes (round 1)

- `scripts/check_zed_extension.py`: dropped the unused `#!/usr/bin/env python3` (EXE001 — `make zed-check` runs it as `python <path>`) and spelled `re.MULTILINE` / `re.DOTALL` (FURB167). `uv run --no-project --with ruff ruff check scripts/check_zed_extension.py scripts/test_check_zed_extension.py` is clean, and `make py-lint` is unaffected (66 files formatted, checks passed).
- `scripts/test_check_zed_extension.py`: the positive case now asserts **exactly 42** properties and the sorted property-name list (`EXPECTED_PROPERTIES`), so a deleted or renamed check fails; deleting `guard.expect("config: hard_tabs", …)` from the guard makes it fail with `41 != 42`, and the guard was restored byte-for-byte afterwards (sha256 verified).

```console
$ sed -i '/guard.expect("config: hard_tabs", data.get("hard_tabs"), False)/d' scripts/check_zed_extension.py
$ uv run --no-project python scripts/test_check_zed_extension.py -v -k test_the_repository_passes
FAIL: test_the_repository_passes
AssertionError: 41 != 42 : the guard checks exactly 42 properties
Ran 1 test in 0.127s
FAILED (failures=1)
$ cp /tmp/opencode/check_zed_extension.py.bak scripts/check_zed_extension.py   # sha256 restored=yes
$ make zed-check
Ran 19 tests in 2.573s
OK
```

## Files

| Path | Change |
|---|---|
| `editors/zed/extension.toml` | `rev` = `053b56e1770f81a5856e17a0b8b68004d9e67069` (was the all-zero placeholder) |
| `scripts/check_zed_extension.py` | new: 42-property static guard, stdlib, `--root`, no network/tree-sitter |
| `scripts/test_check_zed_extension.py` | new: 19 positive/negative self-tests (temp tree + `git add`) |
| `Makefile` | new `zed-check` target (guard + self-tests), `zed-check` in `.PHONY` and on the `check:` line |
| `python/tests/critical/test_lsp_stdio.py` | new protected suite: both distribution paths, three formatting outcomes, EOF exit `1`, stderr secrecy |

## Deviations / interpretations

1. **`! grep -rn '000…0' editors/zed` vs the acceptance's own build.** The
   literal command also scans the gitignored `editors/zed/target/` that
   `cargo build … --target wasm32-wasip2` (a required acceptance step) writes, and
   one binary there contains a run of 40 zero bytes. Run with the build directory
   temporarily renamed aside, the literal command passes (`rc=0`), and
   `git grep` (tracked files only) finds nothing. The intent — no placeholder in
   the shipped tree — holds; the plan's literal command cannot coexist with the
   build step.
2. **`LICENSE`:** §8.1 pins `editors/zed/LICENSE -> ../../LICENSE` and says
   `readlink -f` is `$PWD/LICENSE`, while NODE/§12.4 say "resolves inside the
   extension dir". The guard accepts the committed symlink: the entry lives inside
   `editors/zed/` and its target resolves to **the repository's** `LICENSE`; a
   target outside the repository is red (self-test
   `test_license_escaping_the_repository_is_red`).
3. **Committed vs present build artefacts:** the guard asks `git ls-files`, not
   the filesystem, because the same acceptance builds into
   `editors/zed/target/`. In a tree that is not a git checkout (the self-tests)
   existence counts as committed, so a doctored wasm still goes red.

## Follow-ups

1. `docs/design/blastfile-lsp-zed.md` §12.3 / `MB.22.5`'s acceptance: change the
   last command to `git grep -n '000…0' -- editors/zed` (or add
   `--exclude-dir=target`); the literal `grep -rn` fails on the gitignored wasm
   build the same block requires.
2. §12.3's "`git archive` reproduces the directory": say "the **committed**
   directory"; `alloc.h`/`array.h` are generated, gitignored and correctly absent
   (documented by `MB.22.4`).
3. `docs/reference/cli.md` fence 1 (`blastit dev --no-reload`) fails `make
   docs-test`: `MB.23` owns the flag and the page. `make check` cannot be green
   until it lands (this node is not the cause).
4. A `docs/knowledge/LESSONS.md` line is owed for Deviation 1 (a gitignored build
   artefact inside the grepped directory), but `docs/**` is outside this node's
   `paths`.
5. §12.6's live Zed desktop pass (dev-install the extension, open `./Blastfile`,
   see colours/brackets/outline, type `body =` and read the sentence, save a
   mis-indented line and watch it snap back, hover `unrestricted`, confirm Zed
   ≥ 0.205 loads the grammar wasm) is the one row no `make check` target owns; it
   needs a human at a desktop and was not performed here.
6. §12.7 registry preconditions, as they stood when this node ran: `LICENSE`
   inside the extension dir (symlink) — yes; `description` longer than `name` —
   yes; `version = "0.1.0"` strict semver — yes; `extensions.toml` entry, the
   submodule PR and the pushed branch — not done by this node (never push/tag/PR),
   and `conductor`'s push of `release/0.2.0` is what makes the by-SHA fetch
   public.
