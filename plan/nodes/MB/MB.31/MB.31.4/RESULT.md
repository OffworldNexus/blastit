# Result MB.31.4 — attempt 1

STATUS: done (one release-time step left to the gate: the Zed grammar `rev` re-pin).
The two editor surfaces now teach the truth: the `blastit lsp` hover says what
`immutable` does (bare = every exact file, a pattern list with a `/*` that crosses `/`,
`regex '…'`), and the tree-sitter grammar parses `immutable regex '…'` as a
`regex_raw_string` instead of a `name` + `raw_string`. `src/parser.c`,
`src/grammar.json` and `src/node-types.json` were regenerated with the pinned
`tree-sitter 0.25.10`; nothing is hand-edited. No `Cargo.toml`/`Cargo.lock` byte changed,
nothing staged, nothing committed, no plan field mutated, no protected test touched.

## What changed (paths only)

| File | Change |
|---|---|
| `crates/blastit/src/lsp/hover.rs` | The `immutable` row (305 bytes, middle of the table, still sorted) is now: *"`immutable` adds `Cache-Control: public, max-age=31536000, immutable` to every exact file the mount serves; a pattern list caches the files a pattern matches — a trailing `/*` crosses `/` at any depth and `regex '…'` is the escape hatch — while an `index` or `fallback` answer is always `no-cache`."* The false stop-at-`/` staircase sentence is gone; no internals (`REGEX_SIZE_LIMIT`, two-phase compile) are named, and every key still appears in `docs/reference/blastfile.md` (the row's own H6 test). |
| `editors/tree-sitter-blastfile/grammar.js` | `_value` now admits `prec(1, $.regex_raw_string)` beside `$.string`/`$.raw_string`/`$.name`. `immutable` is a `directive`, so bare `immutable`, a comma list, and `immutable regex '…'` all parse with no new node and **no change** to the existing `(directive (keyword) …)` shape the Zed query file colours. `prec(1)` resolves the only overlap the addition creates (`regex` is also a `name`, so `name` + `raw_string` must lose to the longer rule) — the `table_grant` idiom, and `name` stays legal everywhere a keyword is. |
| `editors/tree-sitter-blastfile/src/{parser.c,grammar.json,node-types.json}` | **Regenerated**, not edited: `/tmp/opencode/bin/tree-sitter generate` (0.25.10, ABI 14 — no `tree-sitter.json`, so the CLI's ABI-15 warning is expected). `node-types.json` gains exactly one child type (`regex_raw_string` on `directive`); `grammar.json` one `PREC` node; `parser.c` is the state renumbering that follows. |
| `editors/tree-sitter-blastfile/test/corpus/sections.txt` | Three new cases (bare `immutable`; `immutable "/static/*", "/static/admin/*"`; `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'`) with their exact trees. The regex case asserts a `regex_raw_string`/`regex_body` node — the shape the capture needs — and parses with no `ERROR`/`MISSING`. |
| `editors/tree-sitter-blastfile/highlights.scm` | New, in the paths: the pattern captures the grammar package carries for consumers of `src/node-types.json` — `(string) @string`, `(string (escape) @string.escape)`, `(regex_raw_string) @string.regex`, `(raw_string) @string.special`, all inside Zed's fifteen-name set. Zed's installed copy (`editors/zed/languages/blastfile/highlights.scm`) already captures `(regex_raw_string) @string.regex`, so this is the same query for the other consumer, not a second dialect. |
| `docs/design/static-and-spa.md` | The false interim paragraph (§4.1, was 261-268) is replaced by the final truth (one matcher: `StaticRoot` holds the manifest's `StaticImmutable`, `static_response::cache_control` asks `matches(&file.url_path)`; equality is `(text, spelling)`). Also corrected: the §4.2 `ImmutableGlob` type block (the type is deleted), the §4.5 `immutable` vectors (now the one compiler's: crossing `/`, `/assets` miss, `assets-secret` denial, `{file}`, refused `*` positions, a regex row), R5's "historical `glob()` record" sentence, §5.2 row 35's "never globbed", §5.2's "configured glob", decision 7's "`*` stops at `/`" rationale, and the §8 row that still listed `ImmutableGlob`. |
| `editors/zed/extension.toml` | **Unchanged, deliberately.** The grammar lives in this repository, so the commit that carries the new `parser.c` cannot be known before it exists. The two-step (`MB.22.4` → `MB.22.5`) is a **release-time** step for the gate/conductor: commit A (grammar + corpus + hover + design doc + this file), then commit B re-pins `rev` to commit A's 40-hex hash. A guessed SHA is never acceptable, so none was written. The current rev still resolves: `git cat-file -e 053b56e…:editors/tree-sitter-blastfile/src/parser.c` and `…:editors/zed/extension.toml` both succeed. |

## Evidence — the node's `# Acceptance`, in full

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK

$ cargo test -p blastit --lib lsp
test lsp::hover::tests::hover_table_is_bounded_and_transcribed ... ok
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out

$ cargo test -p blastit lsp
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out
   (every other target: 0 passed, 0 failed — the filter selects the hover legs)

$ cargo test -p blastit --test critical_lsp
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.20s

$ make zed-check
zed-check: OK (42 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
Ran 19 tests in 1.992s
OK

$ /tmp/opencode/bin/tree-sitter --version
tree-sitter 0.25.10 (da6fe9beb4f7f67beb75914ca8e0d48ae48d6406)

$ cd editors/tree-sitter-blastfile && /tmp/opencode/bin/tree-sitter generate
Warning: No `tree-sitter.json` file found in your grammar, this file is required to generate
with ABI 15. Using ABI version 14 instead.

# `git diff --exit-code -- src` is the committed-tree gate; this tree is uncommitted, so the
# same property is proven directly — generate twice, hashes unchanged, and only the three
# generated files differ from 32dc8ca:
$ sha256sum src/parser.c src/grammar.json src/node-types.json > before && tree-sitter generate
src/parser.c: OK
src/grammar.json: OK
src/node-types.json: OK
GENERATE IS IDEMPOTENT
$ git status --short -- editors/tree-sitter-blastfile/src
 M editors/tree-sitter-blastfile/src/grammar.json
 M editors/tree-sitter-blastfile/src/node-types.json
 M editors/tree-sitter-blastfile/src/parser.c      # no src/tree_sitter/{alloc,array}.h in the diff

$ /tmp/opencode/bin/tree-sitter test
  sections:
     13. ✓ the reference page's complete example parses as six sections
     14. ✓ bare immutable caches every exact file the mount serves
     15. ✓ an immutable list is one pattern per item, the same spellings a view takes
     16. ✓ an immutable regex is its own node, not a name and a raw string
Total parses: 18; successful parses: 18; failed parses: 0; success percentage: 100.00%

# The three shapes, parsed for real (tree-sitter parse, from the grammar directory):
$ tree-sitter parse /tmp/opencode/immutable.Blastfile
      (directive [2, 4] - [2, 13] (keyword))                                     # bare
      (directive (keyword) (string) (string))                                    # list
      (directive (keyword) (regex_raw_string (regex_body)))                      # regex — no name+raw_string

# Never stricter: every committed fixture still parses with no ERROR/MISSING.
$ for f in test/fixtures/*.Blastfile; do tree-sitter parse "$f" | grep -cE '\(ERROR|\(MISSING'; done
0 0 0 0 0 0 0 0

$ REV=$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1)
$ test ${#REV} -eq 40 && git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"
$ git cat-file -e "$REV:editors/zed/extension.toml"
$ git rev-parse "$REV"
053b56e1770f81a5856e17a0b8b68004d9e67069        # resolves; re-pin is commit B, see above
```

Mandated extras:

```console
$ cargo test -p blastit-manifest --test docs_fences
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make docs-test
48 fences: 30 executed, 18 accounted, 0 failed · 34.7s

$ make check
MAKE CHECK EXIT=0          # grep -E 'FAILED|^FAIL' -> 0 lines; zed-check: OK (42 properties)

$ uv run --no-project tools/critical_guard.py --base 32dc8ca --node MB.31.4
critical-guard: 32dc8ca..working tree, 11 changed file(s)
node MB.31.4: paths [...]
  no change to a protected critical test

$ git diff --stat 32dc8ca -- Cargo.toml Cargo.lock
(empty — byte-identical)
```

There is no `make grammar-check` target; `make zed-check` **is** the grammar/editor guard
(`scripts/check_zed_extension.py` + its 19 negative self-tests), and the corpus/generator
gate is the `tree-sitter test` + `generate` pair above.

## The grep: nothing in the toolchain still teaches a `*` that stops at `/`

```console
$ grep -rn 'stops at `/`\|stops at /' crates editors --include='*.rs' --include='*.js' \
      --include='*.scm' --include='*.toml' --include='*.txt' | grep -v /target/
(no output — exit 1)

$ grep -rn 'stops at `/`' docs/reference docs/frameworks docs/deploy docs/local.md \
      docs/blastfile.md docs/security.md
(no output — exit 1: every live user page is clean)
```

The whole tree, classified (`docs/` only):

```console
$ grep -rn 'stops at `/`\|stops at /' docs --include='*.md'
docs/decisions/ADR-0007-…:37   # the original decision, with its ADR-0012 amendment note
                               #   ("…this metacharacter sentence … [is] history") — a record
docs/design/*.review-*.md     # frozen goldfish critiques — records, never rewritten
docs/design/learn-single-pass.md:806,829     # "a Decision Record" for MB.3.51
docs/design/frameworks-restructure.md:104,247 # MB.20's page blueprint (status: Draft)
docs/design/blastfile-lsp-zed.md:1068         # MB.22.2's design; its table transcribes the row
                                              #   this node just fixed in hover.rs
```

`crates/blastit-scheduler/tests/critical_static_resolve.rs:757` and its siblings say
"the retired stop-at-`/` … are gone" — they name the deleted matcher, not the rule, and
they are protected tests. The three design docs above are the *only* non-record survivors;
they are outside this node's `paths`, so the exact one-line fixes are reported below
rather than applied.

## Gotchas (for the next grammar touch)

- Adding a literal-led rule (`regex_raw_string`) to a choice that also holds `$._name`
  needs `prec(1, …)` at the **use site**; without it `tree-sitter generate` reports an
  unresolved conflict between `regex_raw_string` and `name` + `raw_string`.
- Keeping `immutable` inside `directive` (instead of a new `immutable_decl`) is what keeps
  the installed Zed query's `(directive (keyword) @keyword)` capture firing; a new node
  would have needed a query change in a file outside this node's paths.

## Follow-ups

1. **Release-time, the gate/conductor:** commit A carries this grammar change; commit B
   sets `[grammars.blastfile] rev` in `editors/zed/extension.toml` to commit A's hash.
   Until then Zed still shallow-fetches the previous grammar.
2. **Three out-of-path design docs keep the retired sentence** (one line each, ready to
   apply):
   - `docs/design/blastfile-lsp-zed.md:1068` → replace the row with the new
     `HOVER_TABLE` value verbatim (it is the accepted hover table's transcription).
   - `docs/design/learn-single-pass.md:806,829` → "`*` crosses `/` at any depth and must
     end the pattern; `regex '…'` is the escape hatch" (its `--learn` pattern element
     currently specs the deleted `**`/stop-at-`/` distinction).
   - `docs/design/frameworks-restructure.md:104,247` → the Django page's six
     `/static/*…` patterns are one `immutable "/static/*"` after `MB.31.1`.
3. `editors/zed/README.md` and `docs/local.md` contain no `immutable` prose (checked), so
   the release-prep prose sweep is only items 2a–2c.
