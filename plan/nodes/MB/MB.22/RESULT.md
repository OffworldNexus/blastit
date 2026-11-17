# Result MB.22

## Summary

Six children, six commits at HEAD `aedb09b`: the editor DX pages (`8f538ae`), the accepted
design + `ADR-0013` (`7c2e9ca`), `blastit lsp` as a hand-rolled stdio JSON-RPC server with no
new dependency (`7f0b1b6`), `editors/zed` and the in-repo `tree-sitter-blastfile` grammar
(`053b56e`), the grammar rev pin + `zed-check` guard (`d08d04c`), and the audit (`aedb09b`).
Every `# Acceptance` command is recorded below; every child is `done`. One red remains in
`make check` — `docs-test` fence 1 of `docs/reference/cli.md` (`blastit dev --no-reload`),
outside this epic's paths and owned by `MB.23.5` — and one acceptance item is unautomated by
nature: the live Zed desktop pass. No new dependency, no root `Cargo.toml`/`Cargo.lock` byte
changed, root `target/` untouched.

## Evidence

### The nine `# Acceptance` commands

- `uv run --no-project tools/plan.py validate` → `plan OK`.
- `uv run --no-project tools/plan.py tree --under MB.22` → six children, all `done`:

  ```
  ● MB.22 [epic/done] ⇠MB.21.5.2 Ship blastit lsp and a ready-to-install Zed extension …
    ● MB.22.1 [task/done] @doc-writer …    ● MB.22.2 [task/done] @designer …
    ● MB.22.3 [task/done] @implementer …   ● MB.22.4 [task/done] @implementer …
    ● MB.22.5 [task/done] @implementer …   ● MB.22.6 [task/done] @test-curator …
    ~ MB.22.7 … MB.22.12 [task/superseded]   # the audit's six gaps, re-homed to MB.26.9–MB.26.14
  ```

- `make -k check` → **rc 2, exactly one failing target**, and it is not this epic's:

  ```
  FAIL  docs/reference/cli.md  fence 1  dev-flags  2.2s  blastit dev exited before ready; stdout:
  stderr:
  blastit dev: unknown argument `--no-reload`
  48 fences: 30 executed, 18 accounted, 1 failed · 25.9s
  make: *** [Makefile:342: docs-test] Error 1
  ```

  47 `PASS` fence lines; every other prerequisite green — `guard`, `rs-fmt`, `rs-lint`,
  `rs-doc`, `rs-test`, `test-critical`, `py-lint`, `docs-unit`, `docs-check`, `zed-check`,
  `seam-check`, `platform-matrix-check`, `workflow-check` (`365 checks over 6 workflows`),
  `packaging-gates-test` (`49 cases`), `doc-tone`, `manifest-purity`, `plan-validate`. The
  page was committed by `MB.23.1` and documents a flag `cli.rs` does not accept yet; the flag
  and the page are `MB.23.5`'s, outside this epic's `paths`.
- `make docs-check` → `No issues found` (`zensical build --strict`).
- `UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q`
  → `35 passed in 0.52s`.
- `uv run --no-project tools/guard.py` → `guard: OK (20 staged/untracked file(s) checked)`.
- `cargo metadata --manifest-path editors/zed/Cargo.toml --no-deps --format-version 1`
  → `"workspace_root":"…/.worktrees/release-0.2.0/editors/zed"` — the extension is its own
  workspace, so the root lockfile is not consulted.
- `rev=$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1)` →
  `053b56e1770f81a5856e17a0b8b68004d9e67069`; `git cat-file -e "$rev:editors/tree-sitter-blastfile/src/parser.c"`
  → OK (and `MB.22.5` also proved `editors/zed/extension.toml` at that rev, a `--depth 1`
  fetch and a `git archive` of the committed grammar directory).
- `printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | target/debug/blastit lsp`
  → the capability document, then exit `1` on EOF without `shutdown`:

  ```
  Content-Length: 247

  {"id":1,"jsonrpc":"2.0","result":{"capabilities":{"documentFormattingProvider":true,"hoverProvider":true,"positionEncoding":"utf-16","textDocumentSync":{"change":1,"openClose":true,"save":false}},"serverInfo":{"name":"blastit","version":"0.2.0"}}}EXIT=1
  ```

  The same session through `uv run blastit lsp` (console script, staged `PYTHONPATH`) is
  byte-identical in every asserted field in `MB.22.5`'s `RESULT.md`.

### The six `# Acceptance` criteria

1. **Every child `RESULT.md` exists, every acceptance command recorded as run.** Six of six
   (`MB.22.1`–`MB.22.6`), each with its command/output tails; `MB.22.3` and `MB.22.5` also
   record the one `make check` red and why it is not theirs.
2. **Live Zed pass — NOT evidenced; the epic's only unautomated item.** No desktop pass was
   performed: `MB.22.5` (follow-up 5) and the audit record it as human-only. Per the owner's
   instruction no separate human request is opened; it rides the single final notification
   when `0.2.0` is green on the release PR. Everything the pass would look at is proved
   mechanically instead: the 15 corpus cases, the four `.scm` query runs, the 42-property
   guard, and the live LSP transcripts (colours, outline and squiggles are the grammar + the
   diagnostic frames above; format-on-save is the committed `.zed/settings.json` plus the
   `docs/local.md` fence the guard checks).
3. **`blastit lsp` from both distribution paths, `shutdown` → `0`, no document text on
   stderr.** `python/tests/critical/test_lsp_stdio.py` (protected) drives one session through
   the standalone binary (`BLASTIT_LSP_COMMAND`) and through `sys.executable -m blastit lsp`;
   both transcripts: `shutdown -> null`, `exit status -> 0; stderr -> ''`, `EOF before
   shutdown -> 1`, and no `s3cr3t`/`COPY`/`PROGRAM` on stderr for the SQL fixture.
   `make test-critical` → 53 Python tests OK, 42 Rust suites `test result: ok`.
4. **`make check` + `tools/guard.py`; root `Cargo.toml`/`Cargo.lock`/`target/` untouched.**
   `guard: OK`; `git diff --stat 7062ded -- Cargo.toml Cargo.lock` → empty (byte-identical);
   `editors/zed/target/` is gitignored by `editors/zed/.gitignore:1:/target` and outside the
   root `target/`, whose mtime the wasm build does not move. The one `make check` red is
   `MB.23.5`'s `--no-reload` (above), not a regression here.
5. **Docs shipped, both DX critics pass, no retired filename and no alternative transport.**
   `docs/design/blastfile-editor.review-dx.md` and `…review-ai-ergonomics.md` both end
   `VERDICT: PASS`; the design's four lenses (`security`, `implementability`, `testability`,
   `consistency`) each end `VERDICT: PASS` after their rounds. `grep -rniE 'blastit\.toml'`
   over every page this epic shipped → exit 1 (no match); no page describes an alternative
   transport.
6. **`docs/testing/AUDIT-MB.22.md` exists, every open gap filed.** Tier PASS, teeth PASS,
   mutation PASS (92.0 % killed on the three in-scope manifest files). Six gaps filed as
   `MB.26.9`–`MB.26.14`; nothing open remains inside `MB.22`'s subtree.

## Files

| Area | Path |
|---|---|
| User pages | `docs/local.md` (`## Editors`, 3 steps), `docs/reference/cli.md` (`blastit lsp` row + section), `editors/zed/README.md` |
| Design | `docs/design/blastfile-lsp-zed.md`, `docs/design/blastfile-lsp-zed.review-{security,implementability,testability,consistency}.md`, `docs/design/blastfile-editor.review-{dx,ai-ergonomics}.md` |
| ADR | `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`, `docs/decisions/README.md` (one row) |
| `crates/blastit` | `src/lsp/mod.rs`, `src/lsp/hover.rs`, `src/cli.rs`, `src/lib.rs`, `tests/critical_lsp.rs` (protected, 42 tests) |
| `crates/blastit-manifest` | `src/error.rs` (`message()`, exhaustive `position()`), `src/blastfile/mod.rs` (`Checked`, `check()`), `src/blastfile/sites.rs` (new), `tests/warning_sites.rs` (new), `tests/docs_fences.rs` (census pins) |
| `editors/zed/**` | `extension.toml` (rev `053b56e…`), `Cargo.toml`/`Cargo.lock`, `src/lib.rs`, `README.md`, `LICENSE`, `.gitignore`, `languages/blastfile/{config.toml,highlights.scm,brackets.scm,indents.scm,outline.scm}` |
| `editors/tree-sitter-blastfile/**` | `grammar.js`, `package.json` (`tree-sitter-cli` 0.25.10), `.gitignore`, `src/{parser.c,grammar.json,node-types.json,tree_sitter/parser.h}`, `test/corpus/*.txt` (9 files, 15 cases), `test/fixtures/*.Blastfile` (8) |
| Editor config | `.zed/settings.json` (`format_on_save: on`) |
| Guard | `scripts/check_zed_extension.py` (42 properties), `scripts/test_check_zed_extension.py` (19 self-tests) |
| Build | `Makefile` (`zed-check` target, on the `check:` line) |
| Python | `python/tests/critical/test_lsp_stdio.py` (protected) |
| Audit | `docs/testing/AUDIT-MB.22.md` |

## Follow-ups

- **The live Zed desktop pass is the only unautomated acceptance item.** `zed: install dev
  extension` on `editors/zed`, a squiggle on `body = json` in the same words `blastit check`
  prints, and format-on-save after the documented `format_on_save` line. Per the owner's
  instruction, **no separate human request is opened**: it rides the single final
  notification when `0.2.0` is green on the release PR.
- **`MB.23.5` owns `blastit dev --no-reload`**, the one `make check` red (`docs-test` fence 1
  of `docs/reference/cli.md`). `make check` cannot be green until it lands.
- **`MB.26.9`** — gate `grammar.js` against the committed parser (`generate` + `git diff`):
  `tree-sitter test` never reads `grammar.js`.
- **`MB.26.10`** — make the `blastit lsp` rows in `docs/reference/cli.md` true: exit `1` also
  for an unframeable stream, `2` for a bad argument, and the editor's
  `--frozen --no-sync --quiet` form.
- **`MB.26.11`** — prove `blastit lsp` exits `1` when the process cannot drop its file
  capabilities (S10's seventh session, rootless e2e).
- **`MB.26.12`** — give `critical_guard` a per-node base; the release-branch merge base
  reports every earlier node's edits as violations.
- **`MB.26.13`** — negative twins for the five security-bearing `zed-check` properties (no
  capabilities, no process, argv, no scanner, no externals).
- **`MB.26.14`** — locate an endpoint-level `body raw` warning; `sites.rs::raw_body`'s
  endpoint chain is unproven and would draw at `(0,0)`.
- **Registry publication** is a human/`conductor` step: the by-SHA grammar fetch only works
  once `053b56e…` is reachable on the pushed `release/0.2.0` branch, and the extension is not
  listed until then (so step 1's first half is the dev install).
- **Owed to `docs/knowledge/LESSONS.md`** (outside every child's `paths`): the three
  `MB.22.3` notes (`#![deny]` in a non-inline module does not cover its integration test;
  `clippy::indexing_slicing` forbids `value["key"]`; `/proc/<pid>/task/<tid>/children` needs
  `CONFIG_PROC_CHILDREN`) and `MB.22.5`'s gitignored-build-artefact-inside-a-grep note.
- **Design text still to correct** (`MB.22.4`, `MB.22.5` follow-ups): §9.2's `_view_header`
  and missing dotted-name rule, §10.2's incomplete `highlights.scm` listing, §12.3's
  `grep -rn` vs the gitignored wasm build and "committed directory" wording, and the
  `npx` acceptance line this host cannot run (no `npm`; the pinned 0.25.10 binary was used).
