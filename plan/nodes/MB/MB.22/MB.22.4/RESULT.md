# Result MB.22.4

`editors/zed` and `editors/tree-sitter-blastfile` exist with the committed tree the
node pins, the grammar generates byte-identically, the corpus proves the four traps and
`functions pg_catalog.unaccent`, and the four `.scm` files parse and capture against
`test/fixtures/complete.Blastfile`. Nothing is committed; the grammar `rev` is the
all-zero placeholder for `MB.22.5`.

## Generator (host fact: no `npm`/`npx` here)

The pinned generator is **Tree-sitter CLI `0.25.10`**, resolved from the GitHub release
asset and pinned exactly in `editors/tree-sitter-blastfile/package.json`
(`devDependencies.tree-sitter-cli = "0.25.10"`) and in `editors/zed/README.md`.

```console
$ curl -sSL -o /tmp/opencode/tsc.gz \
    https://github.com/tree-sitter/tree-sitter/releases/download/v0.25.10/tree-sitter-linux-x64.gz
$ gunzip -f /tmp/opencode/tsc.gz && mv /tmp/opencode/tsc /tmp/opencode/bin/tree-sitter
$ chmod +x /tmp/opencode/bin/tree-sitter
$ /tmp/opencode/bin/tree-sitter --version
tree-sitter 0.25.10 (da6fe9beb4f7f67beb75914ca8e0d48ae48d6406)
```

**Adaptation, recorded as a host fact:** every `npx --yes tree-sitter-cli@0.25 …` in the
acceptance block was run as the pinned `0.25.10` binary above, because this host has no
`npm`/`npx` (the `~/.npm/_npx` tree-sitter-cli has no native binary). Same version, same
commands, no weaker check. Consequently no `package-lock.json` could be produced here
(§9.4's `npm ci --ignore-scripts` step needs npm); see Follow-ups.

The wasm build uses Zed's own compiler, **wasi-sdk 25.0** (`clang 19.1.5-wasi-sdk`,
target `wasm32-unknown-wasi`), which is what `ExtensionBuilder` runs; the host `clang`
has no wasm target and no WASI sysroot and dies at `<stdlib.h>`. 0.25.10 writes grammar
ABI 14 unless a `tree-sitter.json` is present (it warns about that); ABI 14 is kept on
purpose, and it is what the shipped runtimes load.

## Grammar changes forced by the corpus (design §9.2 is a sketch)

| Change | Why |
|---|---|
| `view_decl: seq('view', name, optional(_methods), optional(_patterns), block)` — `_view_header` dropped | a `_view_header` of two optionals matches the empty string, which tree-sitter refuses to generate |
| `table_grant`'s keyword restricted to `select`/`insert`/`update`/`delete` (aliased to `keyword`), and `prec(1)` on `_table` | `select products` is a table grant while `egress any` stays a directive (`directive` still accepts `keyword name`); without the precedence the generator reports an unresolved `_value`/`_table` conflict |
| `dotted_name: seq(name, repeat1(seq('.', name)))` added to `_value` | **`functions pg_catalog.unaccent`**: a function name is `Word ('.' Word)*` (`docs/design/blastfile-dsl.md` §6.1, `parser.rs`/`syntax.rs`), and `blastit fmt` writes the qualified form, so the sketch's grammar emitted an `ERROR` node — a false red on a file `blastit` itself produced. `repeat1` not `repeat`: a bare name is `$.name` in the same choice, and `repeat` makes the rule match it too (generator: unresolved `_value`/`dotted_name` conflict) |
| `_values` separator is `optional(seq(',', optional(_terminator)))` | `body raw unvalidated` writes two values with no comma; a newline still ends the statement because a keyword lexes as a keyword there |
| `method: /[A-Z][A-Z0-9_-]*/` (was `/[A-Z]+/`) | round-2 review: `blastit` reads **any** word whose first byte is `A`–`Z` as a method (`parser.rs::is_method_word`), so `view v M-SEARCH "/*"` was an `ERROR` node — a false red. The design pins this regex (§9.2); corpus case `custom-method.txt` |

Everything else stays as the design pins it: newlines are **not** extras, every statement
ends at `\n`/`;`/the closing `}`, two string nodes plus `regex_raw_string`/`regex_body`,
and keywords stay legal as names (`word: $ => $.name`).

## Round-2 review fixes and the re-verification

Two MINOR findings, both fixed:

1. **`method` was stricter than the language.** `view v M-SEARCH "/*"` produced an
   `ERROR` node. The regex is now the design's `/[A-Z][A-Z0-9_-]*/`, and
   `test/corpus/custom-method.txt` proves both `view v M-SEARCH "/*"` and a list
   (`GET, M-SEARCH, PROPFIND`). No fixture carried a non-trivial method, so none was
   changed for this.
   *Residual, recorded rather than papered over:* `is_method_word` accepts any word that
   **starts** `A`–`Z`, so a mixed-case `GETx` would still be a method for `blastit` and an
   `ERROR` here. The design pins `[A-Z][A-Z0-9_-]*`, so the grammar follows the design;
   widening it to `/[A-Z][A-Za-z0-9_-]*/` is a design change, not a leaf decision.
2. **`dotted_name` had no capture.** `(dotted_name) @property` is now in
   `highlights.scm`; the capture name is inside the fifteen-name allowlist (`property` is
   already the capture for `(table)`/`(column)`), so the allowlist is unchanged and the
   capture set is still exactly those fifteen.

The `highlights.scm` capture proof now needs a qualified function in
`test/fixtures/complete.Blastfile`, which is otherwise `docs/reference/blastfile.md`'s
"A complete file" verbatim: it gained **one** directive,
`functions pg_catalog.unaccent`, in the `products` view's `postgres { }` grant. The
reference example itself stays byte-exact in `test/corpus/sections.txt`, so both are
proved; the fixture is one legal line longer than the page's.

```console
$ cd editors/tree-sitter-blastfile && tree-sitter generate        # pinned 0.25.10
rc=0
$ sha256sum -c …                                                  # re-generate is byte-identical
src/grammar.json: OK
src/node-types.json: OK
src/parser.c: OK
src/tree_sitter/parser.h: OK
$ git diff --exit-code -- editors/tree-sitter-blastfile/src
rc=0
$ tree-sitter test
Total parses: 15; successful parses: 15; failed parses: 0; success percentage: 100.00%
$ tree-sitter query …/highlights.scm test/fixtures/complete.Blastfile   → rc=0
$ tree-sitter query …/brackets.scm   test/fixtures/complete.Blastfile   → rc=0
$ tree-sitter query …/indents.scm    test/fixtures/complete.Blastfile   → rc=0
$ tree-sitter query …/outline.scm    test/fixtures/complete.Blastfile   → rc=0
$ tree-sitter query …/highlights.scm test/fixtures/complete.Blastfile | grep -c unaccent
1
  pattern: 23
    capture: 11 - property, start: (25, 18), end: (25, 37), text: `pg_catalog.unaccent`
$ for f in test/fixtures/*.Blastfile; do tree-sitter parse "$f" | grep -cE 'ERROR|MISSING'; done
0 0 0 0 0 0 0 0
$ /tmp/opencode/wasi-sdk-25.0-x86_64-linux/bin/clang -fPIC -shared -Os -Wl,--export=tree_sitter_blastfile …
… llvm-nm --defined-only /tmp/opencode/blastfile.wasm | grep tree_sitter_blastfile
000007de T tree_sitter_blastfile
$ uv run --no-project tools/guard.py
guard: OK (91 staged/untracked file(s) checked)
```

## Evidence

Every `# Acceptance` command, run in
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` with
`PATH=/tmp/opencode/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH`.

```console
$ cd editors/tree-sitter-blastfile && tree-sitter generate          # adapted: pinned 0.25.10
rc=0
$ cd editors/tree-sitter-blastfile && tree-sitter test              # adapted
Total parses: 15; successful parses: 15; failed parses: 0; success percentage: 100.00%
$ git diff --exit-code -- editors/tree-sitter-blastfile/src
rc=0
$ test -f editors/tree-sitter-blastfile/src/tree_sitter/parser.h
rc=0
$ cd editors/tree-sitter-blastfile && tree-sitter query ../zed/languages/blastfile/highlights.scm test/fixtures/complete.Blastfile
highlights rc=0   (100 matches)
$ … brackets.scm   → brackets rc=0   (14 matches)
$ … indents.scm    → indents  rc=0   (10 matches)
$ … outline.scm    → outline  rc=0   (14 matches)
$ clang -fPIC -shared -Os -Wl,--export=tree_sitter_blastfile …      # adapted: wasi-sdk 25.0
wasm-ld: warning: creating shared libraries, with -shared, is not yet stable
clang rc=0
-rwxr-xr-x 1 remy remy 23285 /tmp/opencode/blastfile.wasm
$ llvm-nm --defined-only /tmp/opencode/blastfile.wasm | grep tree_sitter_blastfile
000007de T tree_sitter_blastfile
$ cargo metadata --manifest-path editors/zed/Cargo.toml --no-deps --format-version 1 | grep '"workspace_root":".*/editors/zed"'
"workspace_root":"…/.worktrees/release-0.2.0/editors/zed"
$ rustup target add wasm32-wasip2 && cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
    Compiling blastit-zed v0.1.0 (…/editors/zed)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 28.60s     # from a removed editors/zed/target
-rw-r--r-- 2 remy remy 7436915 editors/zed/target/wasm32-wasip2/debug/blastit_zed.wasm
root target/ 2026-11-17 05:42:16.835959958 +0100                            # unchanged by the build
$ uv run --no-project python -c "import tomllib,pathlib; […]"        # extension.toml + config.toml
toml rc=0
$ uv run --no-project python -c "import json,pathlib; json.loads(…'.zed/settings.json'…)"
json rc=0
$ uv run --no-project tools/guard.py
guard: OK (91 staged/untracked file(s) checked)   # the count tracks this node's own untracked files
```

**Idempotency (acceptance item 1)** — the four generated files are byte-identical across
a second `generate`, and only those four are committed:

```console
$ sha256sum -c src-final-before.sha          # after re-running tree-sitter generate
src/grammar.json: OK
src/node-types.json: OK
src/parser.c: OK
src/tree_sitter/parser.h: OK
… (alloc.h and array.h are also stable, and editors/tree-sitter-blastfile/.gitignore
   keeps them out of the tree: parser.c includes only tree_sitter/parser.h)
```

**Capture proofs (acceptance item 3, design §10.4 (b)–(d)):**

```console
$ tree-sitter query …/highlights.scm test/fixtures/complete.Blastfile | grep -c variable.special
2
$ tree-sitter query …/highlights.scm test/fixtures/captures.Blastfile | grep -q 'constant.*unrestricted'   → ok
$ tree-sitter query …/highlights.scm test/fixtures/captures.Blastfile | grep -q 'function.*select'         → ok
$ test "$(tree-sitter query …/outline.scm test/fixtures/complete.Blastfile | grep -cE '^\s*capture: ([0-9]+ - )?item\b')" \
     -eq "$(grep -cE '^(app|entrypoint|postgres|trait|view|static)\b' test/fixtures/complete.Blastfile)"
items=8 sections=8
```

`(table_grant (keyword) @function)` is in `highlights.scm`: design §10.2's listing
omits it although §10.1 and §10.4 (Q3) require `select` to be a `function` capture — the
line was added, and `(mode (keyword)) @constant` / `(grant (keyword)) @constant` stay.

**The four traps + the three extras (acceptance item 2), `test/corpus/`:**

| File | Cases |
|---|---|
| `sections.txt` | the reference page's complete example, six sections |
| `contextual-keywords.txt` | `view view GET "/x"`; `trait session` + `view session` + `postgres view $DATABASE_URL` (keyword as name) |
| `trailing-comma-newline.txt` | `immutable "…",\n"…"`; `functions …,\n…` and a wrapped `select products(id,\n…)` |
| `multi-line-raw-string.txt` | `query 'SELECT …\nFROM …\nWHERE … $?'` |
| `env-and-query.txt` | `$ANALYTICS_URL`, `query "SET TIME ZONE 'UTC'"`, `$?` inside `'…'`, split-slot store |
| `regex-pattern.txt` | `view v regex '/v[0-9]+/items'` and a string pattern list |
| `one-line-block.txt` | `static "/" { root "public" }`, `POST "…" { body json }`, `postgres { select … }` |
| `qualified-function.txt` | **`functions pg_catalog.unaccent, st_distance` inside `postgres analytics { … }`** and the bare `functions unaccent` form |
| `custom-method.txt` | `view v M-SEARCH "/*"` and `view v GET, M-SEARCH, PROPFIND "/*"` (a method is any word starting `A`–`Z`) |

Fixture loop (design §9.5, "no false red"), all eight fixtures:

```console
$ for f in test/fixtures/*.Blastfile; do tree-sitter parse "$f" | grep -cE 'ERROR|MISSING'; done
0 0 0 0 0 0 0 0        # app, captures, complete, entrypoint, static, store, trait, view
```

`test/fixtures/complete.Blastfile` is `docs/reference/blastfile.md` §"A complete file"
plus the one `functions pg_catalog.unaccent` directive the round-2 capture proof needs
(its `property` capture is asserted below); the page's text itself is byte-exact in
`test/corpus/sections.txt`. `trait.Blastfile` also carries
`functions pg_catalog.unaccent, st_distance`, so the qualified form is proved by the
fixture loop as well as by the corpus.

## Files

| Path | Change |
|---|---|
| `editors/tree-sitter-blastfile/grammar.js` | new: design §9.2 + the four changes above |
| `editors/tree-sitter-blastfile/package.json` | new: `tree-sitter-cli` pinned to `0.25.10` |
| `editors/tree-sitter-blastfile/.gitignore` | new: `alloc.h`/`array.h` (unused by `parser.c`) |
| `editors/tree-sitter-blastfile/src/{parser.c,grammar.json,node-types.json,tree_sitter/parser.h}` | generated, committed-ready, idempotent |
| `editors/tree-sitter-blastfile/test/corpus/*.txt` | nine files, fifteen cases |
| `editors/tree-sitter-blastfile/test/fixtures/*.Blastfile` | eight files |
| `editors/zed/extension.toml` | new: id/name/version/schema/authors/description/repository, `[grammars.blastfile]` (`rev` = 40 zeros), `[language_servers.blastit-lsp]` + `language_ids` |
| `editors/zed/Cargo.toml`, `Cargo.lock` | new: `cdylib`, `zed_extension_api = "0.7.0"`, `publish = false`, empty `[workspace]` |
| `editors/zed/src/lib.rs` | new: `which("blastit")` → `blastit lsp`; else `which("uv")` → `uv run --frozen --no-sync --quiet blastit lsp`; one `shell_env()`; no process execution, no capability |
| `editors/zed/LICENSE` | new symlink → `../../LICENSE` (resolves to the repo root licence) |
| `editors/zed/.gitignore` | new: `/target`, `/extension.wasm`, `/grammars` |
| `editors/zed/README.md` | new: install, the two-line server resolution, the 0.25.10 pin, regeneration and the wasi-sdk clang command |
| `editors/zed/languages/blastfile/{config.toml,highlights.scm,brackets.scm,indents.scm,outline.scm}` | new: eight `config.toml` keys (`path_suffixes = ["Blastfile", "blastfile"]`, `line_comments = ["# "]`, `tab_size = 4`), fifteen captures, `@open`/`@close`, `@indent`, `@item`+`@name`; no `injections.scm` |
| `.zed/settings.json` | new: `{"languages":{"Blastfile":{"format_on_save":"on"}}}` |

Checks on the tree: `extension.toml`'s `description` is longer than `name`
(44 > 9 chars), `rev` matches `^[0-9a-f]{40}$` and is all zeros, the server table has no
`name` key, `language_ids` maps `Blastfile → blastfile`, `config.toml` has exactly eight
keys, `editors/zed/LICENSE` resolves to `<repo>/LICENSE`, and the root `Cargo.toml` /
`Cargo.lock` are byte-identical (`git diff --exit-code` → 0).

## The `rev` pin for `MB.22.5`

```toml
[grammars.blastfile]
rev = "0000000000000000000000000000000000000000"
path = "editors/tree-sitter-blastfile"
```

`MB.22.5` replaces it with the lead's commit that contains **both** `editors/zed` and
`editors/tree-sitter-blastfile` (design §12.3), then runs
`! grep -rn '0000000000000000000000000000000000000000' editors/zed`.

## Follow-ups

- `MB.22.5` owns `scripts/check_zed_extension.py` and its negative self-tests. It should
  treat the grammar directory's `.gitignore` (`alloc.h`, `array.h`) as intended, not as a
  missing generated file, and it should read the capture allowlist from
  `highlights.scm` only (the comments no longer name a dotted capture).
- `package-lock.json` for `editors/tree-sitter-blastfile` could not be produced on this
  host (no `npm`): §9.4's `npm ci --ignore-scripts` step needs one, so a CI/registry
  builder host with npm should commit it beside the exact `0.25.10` pin. The pin itself is
  exact and recorded, so regeneration is reproducible today.
- The design (`docs/design/blastfile-lsp-zed.md`) has three defects this node had to work
  around; the lead may want them fixed in the document: (a) §9.2's `_view_header` matches
  the empty string and cannot be generated; (b) §9.2 has no `.`/dotted-name rule although
  `fmt` writes `functions pg_catalog.unaccent` (§9.2's "no false red" rule is violated by
  its own sketch); (c) §10.2's "complete" `highlights.scm` omits
  `(table_grant (keyword) @function)`, which §10.1 and §10.4 (Q3) require.
- §12.8 hand-offs that touch this node's own text: the acceptance block still says
  `npx --yes tree-sitter-cli@0.25` (adapted here, justified above) and still names
  `tools/guard.py` as the `editors/**` checker; `scripts/check_zed_extension.py` is the
  checker `MB.22.5` adds. Both are plan-text changes the lead owns.
- `test/highlight/` was not added: the CLI looks for `queries/highlights.scm` inside the
  grammar directory, while this node keeps the queries in the extension directory (§10),
  so the four `tree-sitter query` runs plus the (b)–(d) greps are the capture proof.
- Live Zed behaviour (§12.6: colours, outline, a squiggle, format-on-save) is
  human-only and unverified here; `MB.22.5`'s `RESULT.md` records that pass.
