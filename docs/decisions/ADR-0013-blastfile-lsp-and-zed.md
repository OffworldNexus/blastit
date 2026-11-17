# ADR-0013: `blastit lsp` and the Zed Blastfile extension

- **Status:** Accepted
- **Date:** 2026-11-17
- **Node:** MB.22
- **Owner:** designer
- **Analysis:** `docs/design/blastfile-lsp-zed.md` (the full design),
  `docs/local.md` `## Editors` + `docs/reference/cli.md` `## blastit lsp`
  (the accepted DX, MB.22.1), `docs/research/2026-11-16-zed-blastfile-lsp.md`
  (every protocol and extension fact).

## Context

A `Blastfile` is the one file every Blastit developer edits by hand, and until
0.2.x the editor showed it as plain text: a mistyped directive was found by
running `blastit check`, not by looking at the line. The accepted pages promise
the opposite — a squiggle in the words the CLI prints, at the same line and
column, canonical layout on save after one JSON line, and a hover that says what
a grant actually allows — for the extensionless `./Blastfile`, with no `npm`,
Tree-sitter CLI, `cargo` or `wasi-sdk` on the user's machine.

That promise needs three things: a language server, a grammar, and a Zed
extension to wire them up. The constraints are Blastit's: zero new
dependencies, MSRV 1.88, `CARGO_NET_OFFLINE`, one shipped artifact per
installation, and a manifest crate that already owns the one parse, the one
formatter and the one error vocabulary.

## Decision

1. **`blastit lsp` is a subcommand of the existing binary**, ~700 lines in
   `crates/blastit/src/lsp.rs`, hand-rolled `serde_json` + `std::io`, blocking,
   single-threaded. No async runtime on that path and no LSP crate. `serde_json`
   is already a dependency of `crates/blastit`, so the dependency count does not
   move. It reaches both installations through `cli::main` — the Python-free
   standalone binary and the wheel's console script — with no extra artifact,
   name or release step.
2. **The subset is the one Zed needs, and nothing more**: `initialize`/
   `initialized`, full sync (`change: 1`), `didOpen`/`didChange`/`didClose`,
   push `publishDiagnostics`, whole-document `textDocument/formatting`,
   `textDocument/hover`, `shutdown`/`exit`. `definition`, `documentSymbol`,
   completion, pull diagnostics and incremental sync are out of v1.
3. **Positions are converted, never assumed**: the manifest's `Position` is
   1-based in `char`s, LSP's is 0-based in **UTF-16 code units**, and Zed
   ignores whatever the server declares. The conversion is
   `sum(utf16_len(c) for c in line[:col-1])`, computed in one function over a
   line-offset table built once per publish.
4. **The editor never gets a second opinion.** A diagnostic's `message` is
   `ManifestError::message()` — the CLI's sentence without the location prefix —
   and a formatting reply is `blastfile::fmt`'s bytes, so the editor and the CLI
   can never disagree. Warnings reuse their existing `Display` text, and their
   positions are computed by one CST walk (`blastfile::sites`) that
   `blastfile::check` returns beside the warnings — not by a field on
   `ManifestWarning` and not by a span field on `Manifest`, because both are
   constructed in protected tests and in `model.rs`, which this node does not own.
5. **One extension and one grammar, both in this repository**:
   `editors/zed/` (with an empty `[workspace]` so the root workspace and lockfile
   stay untouched) and `editors/tree-sitter-blastfile/` (with generated
   `parser.c`, `grammar.json`, `node-types.json` and `src/tree_sitter/parser.h` committed, because Zed never
   runs `tree-sitter generate`). `[grammars.blastfile]` pins a 40-hex commit and
   `path = "editors/tree-sitter-blastfile"`.
6. **The extension resolves the server without executing repository code**:
   `Worktree::which("blastit")` then `Worktree::which("uv")` with
   `run --frozen --no-sync --quiet blastit lsp`, and `Worktree::shell_env()`
   handed to the child, because a GUI-launched Zed does not inherit a terminal
   `PATH`. `--no-sync` is not a detail: a bare `uv run` syncs the project, which
   runs the worktree's declared build backend with the developer's environment in
   `environ` — opening a cloned repository's `Blastfile` would execute that
   repository's build code. No `process:exec`, no capability prompt.
7. **Format-on-save is one documented line in `.zed/settings.json`**, committed
   in this repository, because `format_on_save` defaults to `off` and an
   extension cannot set a per-language default. CRLF buffers are normalised to
   LF on save — the formatter already writes LF and `blastit fmt` writes the same
   bytes.

## Consequences

- One release artifact, one name for the extension to find, one place to fix a
  protocol bug; the standalone binary and the wheel prove the same session.
- ~700 lines of protocol code and ~90 lines of grammar/extension wiring are
  ours to test, with no third-party audit surface and no async runtime to leak
  tasks.
- The manifest crate gains exactly one visibility change (`MAX_SOURCE_BYTES`), one
  new method (`ManifestError::message()`), one new module (`blastfile::sites`,
  ~120 lines) and one new entry point (`blastfile::check`); `model.rs` and
  `validate.rs` are untouched, so every protected test compiles unchanged.
- The binary's `Lsp` arm drops every file capability before it reads a byte, so a
  `setcap`'d deployment binary is still a safe editor server.
- A second editor (Helix, Neovim) costs a client config, not a second server;
  a VS Code client would be a later `editors/vscode` node.
- `definition` and semantic context inside strings wait for
  `blastfile::symbols()`, a bounded CST walk that is a follow-up node, not a
  rewrite.
- The grammar is a colourist, deliberately looser than the parser: it must never
  paint a red node on a file `blastit` accepts, and the LSP owns every red.

## Alternatives rejected

| Alternative | Why not |
|---|---|
| **`tower-lsp` (+ `tokio`)** | ~40 transitive crates and an async runtime for a single-threaded, strictly sequential protocol whose total traffic is one request at a time. It would also put the LSP on `tokio` while the rest of the binary's blocking model stays put, and it enlarges the offline/MSRV/audit surface for no capability we lack |
| **`lsp-server` + `lsp-types`** | Two crates whose value is re-deriving message shapes we need eight of; `lsp-types` would also become the second definition of `Position` next to the manifest's, which is exactly the conversion error this design exists to prevent |
| **A separate `blastit-lsp` binary or crate** | A second artifact to build, sign, publish and keep in step; a second name for the extension to resolve and for the docs to explain; and it would still link `blastit-manifest`, so the only thing it buys is distance from the code it serves. The subcommand also keeps the Python-free standalone promise for free |
| **A separate `tree-sitter-blastfile` repository** | Two rev pins to keep in sync (extension ↔ grammar), two release ceremonies, and a grammar that drifts from the parser it exists to colour. One repository with `path =` gives one commit that contains both halves, which is the pin `MB.22.5` proves |
| A grammar-only extension (no Rust) | Zed would colour the file and nothing else — no diagnostics, no formatting, no hover — contradicting the accepted pages |
| A shell-out from the extension to probe `blastit --version` | Gated by `process:exec` **and** a user-granted capability, turning a zero-config install into a permission prompt |

## References

- `docs/design/blastfile-lsp-zed.md` §0–§12 (the design this records).
- `docs/local.md` `## Editors`, `docs/reference/cli.md` `## blastit lsp` (MB.22.1).
- `docs/design/blastfile-dsl.md` (R2 `line:col` in `char`s, R6 `fmt` idempotence)
  and `ADR-0012` (the DSL).
- `crates/blastit/src/{lsp,cli}.rs`, `crates/blastit-manifest/src/error.rs`,
  `crates/blastit-manifest/src/blastfile/{mod,sites}.rs`, `editors/zed/**`,
  `editors/tree-sitter-blastfile/**` (`MB.22.3`–`MB.22.5`).
