# Blastfile for Zed

Zed language extension for `Blastfile`, Blastit's routing table and permission
model: syntax highlighting, brackets, an outline of the file's sections, and the
`blastit-lsp` language server.

## Install

**Blastfile** is not in Zed's registry yet: install Rust, `git clone
https://github.com/OffworldNexus/blastit`, then `zed: install dev extension` and pick
`editors/zed` (Zed builds it — and its `wasm32-wasip2` target — with `cargo`). Once
listed, `zed: extensions` → **Blastfile** is the whole step.

The extension finds the server itself, first match wins:

| It uses | What runs |
|---|---|
| `lsp.blastit-lsp.binary.path` | that `blastit-lsp` file — an unpacked archive, or a build of it |
| `blastit-lsp` on your `PATH` | `blastit-lsp` |
| `blastit` on your `PATH` | `blastit lsp` |
| none of these | its GitHub Release build for your platform, downloaded once |

Every row runs the same server: rows 1–3 use a file already on disk, and the last
fetches that same file once and reuses it. Never point `path` at a `blastit` binary
without `"arguments": ["lsp"]`: the bare binary is the production server and fails
closed. If none can run — no build for your OS and architecture, no network — Zed
says so in one line, and `binary.path` is the way past it. Unpacking an archive,
the Windows path and the `"arguments": ["lsp"]` shape are on the
[CLI page](../../docs/reference/cli.md#blastit-lsp).

`format_on_save` defaults to `off` for languages Zed does not special-case, so
add one line to your project's `.zed/settings.json` (this repository ships it):

```json
{ "languages": { "Blastfile": { "format_on_save": "on" } } }
```

Zed ≥ 0.205 is required (`zed_extension_api` 0.7.0); an older build refuses the
extension rather than mis-running it.

## The grammar is vendored in this repository

`editors/tree-sitter-blastfile` is the grammar, pinned by
`[grammars.blastfile]` in `extension.toml`. Zed never generates a parser: it
fetches the pinned commit and compiles the committed `src/parser.c` into
`grammars/blastfile.wasm`. The four committed generated files are
`src/parser.c`, `src/tree_sitter/parser.h`, `src/grammar.json` and
`src/node-types.json`.

## Regenerating (maintainers only)

Users need no `npm`, no Tree-sitter CLI and no `wasi-sdk`; this section is for
whoever edits `grammar.js`.

The generator is **Tree-sitter CLI 0.25.10**, pinned in
`editors/tree-sitter-blastfile/package.json` (`devDependencies`) and recorded
here so regeneration is reproducible:

```console
$ cd editors/tree-sitter-blastfile
$ tree-sitter --version          # tree-sitter 0.25.10
$ tree-sitter generate
$ tree-sitter test
$ git diff --exit-code -- src    # a non-empty diff means the tree is stale
```

Then the queries and the grammar wasm:

```console
$ cd editors/tree-sitter-blastfile
$ tree-sitter query ../zed/languages/blastfile/highlights.scm test/fixtures/complete.Blastfile
$ tree-sitter query ../zed/languages/blastfile/brackets.scm   test/fixtures/complete.Blastfile
$ tree-sitter query ../zed/languages/blastfile/indents.scm    test/fixtures/complete.Blastfile
$ tree-sitter query ../zed/languages/blastfile/outline.scm    test/fixtures/complete.Blastfile
$ clang -fPIC -shared -Os -Wl,--export=tree_sitter_blastfile \
      -Wl,--max-memory=4294967296 \
      -I editors/tree-sitter-blastfile/src \
      editors/tree-sitter-blastfile/src/parser.c -o /tmp/blastfile.wasm
```

That `clang` is the one from [wasi-sdk](https://github.com/WebAssembly/wasi-sdk)
(`wasm32-wasi`), which is what Zed's extension builder uses; a host `clang`
without the wasm target and a WASI sysroot fails at `<stdlib.h>`. Generate with
the default grammar ABI (14): 0.25.10 only writes ABI 15 when a
`tree-sitter.json` is present, and 14 is what the shipped runtimes load.

## Building the extension's Rust half

```console
$ rustup target add wasm32-wasip2      # once, on the build host
$ cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
```

`editors/zed/Cargo.toml` has an empty `[workspace]` table so the package stays
out of Blastit's root workspace, which is why `Cargo.lock` is committed here.
Zed writes `target/`, `extension.wasm` and `grammars/` into this directory when
it builds the extension; all three are gitignored.
