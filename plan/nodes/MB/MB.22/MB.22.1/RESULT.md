# Result MB.22.1

Editor DX written first: the `## Editors` section in `docs/local.md` (20 added lines,
section shape, not a new page) and the `blastit lsp` entry in `docs/reference/cli.md`
(14 added lines). Both goldfish critics end `VERDICT: PASS`.

## Files

| Path | Change |
|---|---|
| `docs/local.md` | new `## Editors` between `## A denial` and `## Before you deploy`: 3 steps, the one-line `format_on_save` JSON, one sentence each for other editors, toolchains, CI and deployments |
| `docs/reference/cli.md` | `blastit lsp` row in the top table and a `## blastit lsp` section between `check` and the production binary |
| `docs/design/blastfile-editor.review-dx.md` | dx critic review, post-fix re-review, `VERDICT: PASS` |
| `docs/design/blastfile-editor.review-ai-ergonomics.md` | ai-ergonomics critic review, post-fix re-review, `VERDICT: PASS` |
| `crates/blastit-manifest/tests/docs_fences.rs` | census pins only: `docs/local.md` `Counts::new(1, 6)` → `Counts::new(1, 7)`, `TOTAL` `Counts::new(12, 31)` → `Counts::new(12, 32)` |

The census moved by exactly one illustration because `docs/local.md` gained one fence, the
`json` block showing the `format_on_save` line. It is neither a document (`text title="Blastfile"`
is the only document opening) nor a statement list (its body is JSON, not keyword-led), so it
counts as an illustration: 6 → 7 on that page, 31 → 32 across the thirteen pinned pages. No
document was added or dropped, so `TOTAL`'s document half stays 12, and every other `PAGES` row
is untouched.

No `docs/editors.md`: the section shape was chosen, so `zensical.toml` nav and
`python/tests/runnable_docs/inventory.toml` are untouched. The new `json` block is not a
`console` fence, so the console indexes of `docs/local.md` (1–6) and `docs/reference/cli.md`
(1–4) did not move (`make docs-unit`: 107 passed).

## What the pages promise

- Zed ≥ 0.205: `zed: extensions` → install **Blastfile**; not listed yet or in a Blastit
  checkout, `zed: install dev extension` on `editors/zed` (that local build needs `cargo`).
- Open `./Blastfile`: colours, bracket matching, outline, and a squiggle for each syntax
  error in the words `blastit check` prints, at the same line and column; the extension
  starts `blastit lsp` from `PATH` or through `uv run`.
- `{ "languages": { "Blastfile": { "format_on_save": "on" } } }` merged into
  `.zed/settings.json`; Zed's default is `off`; the repo commits that file; a mis-indented
  line that snaps back on save proves it.
- Other editors register the extensionless `Blastfile` and start `blastit lsp` on
  stdin/stdout; using it needs no `npm`, Tree-sitter CLI, `cargo` or `wasi-sdk`.
- CI uses `blastit check`; production never runs `blastit lsp`.

## Critic findings and resolutions

Round 1, both lenses `VERDICT: FAIL`.

| Finding | Lens | Resolution |
|---|---|---|
| MAJOR: step 3 named no destination and "this repository" pointed at Blastit's repo while the reader edits their own | dx 1, ai 1 | step 3 now says "Merge this into `.zed/settings.json`" and "Blastit's repo commits that file" |
| MAJOR: the one-key `format_on_save` block was the research's unverified case, so a save could silently do nothing | dx 2 | the block stays (node-mandated one-line form); the page now gives a positive check — "save a mis-indented line and it snaps back" — and the one-vs-two-key probe is a follow-up for MB.22.4 |
| MAJOR: two install paths in one step, neither executable unattended | ai 2 | step 1 is one fork: registry install, else "Not listed yet, or in a Blastit checkout: `zed: install dev extension` on `editors/zed` (needs `cargo`)" |
| MAJOR: the squiggle was quoted with the `Blastfile:3:9:` prefix, which the LSP message does not carry | ai 3, dx 5 | `local.md` says "in the words `blastit check` prints, at the same line and column"; `cli.md` says "the sentence `blastit check` prints after `Blastfile:<line>:<col>:`" and scopes grants/queries/coverage to `blastit check` |
| MAJOR: the "other editor" sentence lacked the spawn facts (argv, `uv` prerequisite, extensionless attachment) | ai 4 | `cli.md` now gives the no-argument spawn, `uv run blastit lsp` with `uv add blastit`, and the `Blastfile` file-name registration |
| MAJOR: `cargo` was denied in the same section that needs it for a dev install | ai 5 | step 1 states the dev-install `cargo` need; the last paragraph scopes the denial to *using* the extension |
| MINOR: no fallback for a reader outside a Blastit checkout before the registry PR | dx 3 | step 1's "Not listed yet, or in a Blastit checkout" branch; publication itself is a follow-up |
| MINOR: the toolchain clause was garbled and half wrong | dx 4 | rewritten as "using it needs no `npm`, Tree-sitter CLI, `cargo` or `wasi-sdk`" |
| MINOR: `blastit fmt` was named but has no page or table row | dx 6, ai 7 | the name is gone: "Formatting is the canonical layout" |
| MINOR: "`1` otherwise" reads as "a broken document exits 1" | ai 6 | "`1` only if the client disappears without one — never because a document is broken" |
| MINOR: no observable oracle for the format line | ai 8 | the mis-indented-line check in step 3 |
| MINOR: `(needs cargo)` contradicted "no `cargo`" | dx (round 2, listing) | the denial is scoped with "using it"; the dev-install need is in step 1 |
| NIT: palette verb, `npm` list length, neutral lead for non-Zed readers | ai 9, ai 10 | answered: the three steps are Zed-first by the node's shape; the toolchain list is required by the node |

Round 2, fresh sessions per lens.

| Finding | Lens | Resolution |
|---|---|---|
| MAJOR: "if a save changes nothing" cannot tell a dead formatter from an already-canonical file, and pushes a second key onto working setups | dx M1 | clause removed; the check is now deterministic ("save a mis-indented line and it snaps back"). dx confirmed `M1 — RESOLVED`, `VERDICT: PASS` |
| MINOR: the diagnostic promise was broader than the server | dx 2 | step 2 now says "a squiggle for each syntax error" |
| MINOR: no create/merge rule for `.zed/settings.json` | ai 2 | step 3 says "Merge this into", which covers both |
| MINOR: no executable step outside a Blastit checkout | ai 1 | accepted as a publication ordering item (follow-up), not a page defect: the branch names the checkout it needs |

Both MINOR wording fixes were adopted after the PASS verdicts; neither adds a line and both
are the critics' own replacement text, so the verdicts stand.

## Evidence

```
$ grep -rn "install dev extension\|format_on_save\|blastit lsp" docs/local.md docs/editors.md docs/reference/cli.md 2>/dev/null | head
docs/local.md:143:   Blastit checkout: `zed: install dev extension` on `editors/zed` (needs `cargo`).
docs/local.md:146:   The extension starts `blastit lsp` itself, from `PATH` or through `uv run`.
docs/local.md:152:{ "languages": { "Blastfile": { "format_on_save": "on" } } }
docs/local.md:156:[`blastit lsp`](reference/cli.md#blastit-lsp) on stdin/stdout — using it needs
docs/local.md:158:`blastit check`, and production never runs `blastit lsp`.
docs/reference/cli.md:7:| `blastit lsp` | your editor |
docs/reference/cli.md:67:## `blastit lsp`
docs/reference/cli.md:70:`blastit lsp`, or `uv run blastit lsp` when the CLI is not installed (that needs
```

```
$ ls docs/design/blastfile-editor.review-dx.md docs/design/blastfile-editor.review-ai-ergonomics.md
docs/design/blastfile-editor.review-ai-ergonomics.md
docs/design/blastfile-editor.review-dx.md
```

```
$ grep -n "VERDICT" docs/design/blastfile-editor.review-dx.md docs/design/blastfile-editor.review-ai-ergonomics.md
docs/design/blastfile-editor.review-dx.md:113:VERDICT: PASS
docs/design/blastfile-editor.review-ai-ergonomics.md:80:VERDICT: PASS
```

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.28s
```

```
$ make docs-unit
107 passed, 65 subtests passed in 1.61s
```

```
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 0.54s
```

```
$ uv run --no-project tools/plan.py validate
plan OK
```

```
$ uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
```

```
$ git diff --stat docs/local.md docs/reference/cli.md
 docs/local.md         | 20 +++++++++++++++++++
 docs/reference/cli.md | 14 ++++++++++++++
```

```
$ timeout 180 cargo test -p blastit-manifest --test docs_fences --locked
    Finished `test` profile [unoptimized + debuginfo] target(s) in 50.65s
     Running tests/docs_fences.rs
running 11 tests
...
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

No other page's count moved, checked against the exhaustive-scan rule rather than assumed:

```
$ git diff --name-only docs/
docs/local.md
docs/reference/cli.md
```

Only those two published pages changed (the third, `docs/research/INDEX.md`, is another node's
and sits in `RECORD_DIRS`, which §16.2 never scans). `docs/reference/cli.md` is unpinned and the
scan reports its illustration count by path: `unpinned: docs/reference/cli.md (4 illustration(s))`,
the same four `console` fences `inventory.toml` lists. Proven from the file itself: the diff adds
no line that opens a fence (0 matches), and its fence-marker count is 8 before and 8
after (`docs/local.md` 14 → 16, the one new fence). A drift on a pinned page fails
`verify_page`, a document on an unpinned page fails `unpinned_document_error`, and a
statement-list page must be pinned — the test is green, so neither happened.

## Final polish (round 3)

The reviewer's `VERDICT: PASS` stood. Three MINOR factual corrections, each from the
reviewer's own wording; no restructuring, no new lines elsewhere, no new critic round.

- `cli.md`, diagnostic prefix: "after `Blastfile:<line>:<col>:`" was false for a refusal
  with no position (a missing `root`, an unset `$IDENT`, an unacknowledged `body raw`). It
  now says "after the `Blastfile:` prefix, and a refusal with no position is already one
  sentence".
- `cli.md`, formatting: the fragment "Formatting is the canonical layout." now links the
  layout — `[canonical layout](blastfile.md#formatting)`. That anchor is
  `## Formatting` at `docs/reference/blastfile.md:553`; `docs/blastfile.md` has no such
  heading, so the sibling reference page is the real target (and is not edited here).
- `local.md` step 1: the checkout now names *how* the extension is installed while it is
  not listed, and building it there is "the only step here that needs `cargo`", so the
  later "using it needs no `npm`, Tree-sitter CLI, `cargo` or `wasi-sdk`" no longer reads as
  a contradiction. That is one line more in the step: the section is 21 added lines, one
  over the node's ~20-line preference, and no other line moved.

Re-verified after the three edits:

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.33s
```

```
$ make docs-unit
107 passed, 65 subtests passed in 30.89s
```

```
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 2.89s
```

```
$ timeout 180 cargo test -p blastit-manifest --test docs_fences --locked
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

Fence counts are unchanged by the fixes (`docs/local.md` 16, `docs/reference/cli.md` 8),
so the `docs_fences` pins and the console indexes still hold, and the new link resolves
(`zensical --strict` reports no missing anchor). The retired v1 manifest filename still
appears nowhere in the node's files:

```
$ grep -rn "blastit\.toml" docs/local.md docs/reference/cli.md zensical.toml python/tests/runnable_docs/inventory.toml crates/blastit-manifest/tests/docs_fences.rs
grep exit=1 (1 = no match anywhere); docs/editors.md does not exist
```

## Follow-ups

- **`.zed/settings.json` must land in MB.22.4.** The page says the repo commits it; nothing
  tracks it on this branch yet. Until it is committed, that sentence is a contract, not a
  fact.
- **Probe the one-key block in MB.22.4.** It is unverified whether a project
  `.zed/settings.json` routes `format_on_save` to the extension's server without
  `"formatter": "language_server"`. If the probe says no, the block in `docs/local.md` gains
  the second key in the same commit that lands the file.
- **Registry publication gates step 1's first half (MB.22.8).** Until the extension is
  listed, the only executable path is the dev install from a Blastit checkout.
- **`blastit lsp` must resolve its own server environment.** A GUI-launched editor does not
  inherit a shell `PATH`, so the extension should consult `Worktree::shell_env` before
  reporting "language server not found"; the docs cannot promise a PATH the desktop does not
  have.
- **`blastit fmt` has no `docs/reference/cli.md` entry** (pre-existing). The lsp section
  avoids the name rather than linking a page that does not document it.
- `docs/editors.md` was not created; if the section grows past ~25 lines it should move to
  its own page and be registered in `zensical.toml` nav and `inventory.toml`.
