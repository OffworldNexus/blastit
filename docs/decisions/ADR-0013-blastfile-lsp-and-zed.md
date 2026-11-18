# ADR-0013: `blastit lsp` and the Zed Blastfile extension

- **Status:** Accepted; amended 2026-11-18 by `MB.34.2` (see the amendment at the end).
  Decision 4 said a warning's position is "computed by one CST walk … not by a field on
  `ManifestWarning` and not by a span field on `Manifest`"; a `query`'s inner offset is the
  one place where that recomputation is impossible, so the model carries a site (`QuerySite`,
  `blastfile-dsl.md` §13.4.5) — a named exception to that clause, not to the warning rule.
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

## Amendment 2026-11-18 (`MB.34.2`): spans inside a `query`, one protocol method, and the SQL grammar boundary

**Status:** Proposed (accepted with `MB.34.2`'s review). **Node:** MB.34.2.
**Design:** `docs/design/blastfile-lsp-zed.md` §0.2, §4.6, §7.6, §7A, §9.2a, §10.2, §13;
`docs/design/blastfile-dsl.md` §13.4–§13.5, §18.1. **Accepted DX:** `docs/reference/blastfile.md`
`query` section, `docs/local.md` `## Editors`, `docs/ci.md`, `docs/troubleshooting.md`
(MB.34.1).

The accepted pages gained one promise the 2026-11-17 decision did not cover: a `query` is
validated as the PostgreSQL statement it claims to be, the diagnostic sits on the SQL token
*inside* the string, a Quick Fix turns a pasted statement into `$?` form, hover explains
`$?`/`...`, and the editor colours the SQL. Three additions follow; **decisions 1–3 and
5–7 above stand unchanged**, and so does every refusal they record.

1. **A `query` refusal now carries a site, and the site is stored, not re-derived.** The
   validator answers with an offset into the **raw decoded** statement, and
   `blastfile::lower` records, per deduped query entry, the origin `Position` (the first
   character *inside* the opening quote), the interior's source byte range, and the
   decoded→source map for the `"…"` spelling's four escapes (`QuerySite`,
   `blastfile-dsl.md` §13.4.5). This is the exception decision 4's predecessor clause
   anticipated: `sites.rs` computes a warning's position from the CST on every call, which
   works because a warning names a *token*; a statement's offset cannot be recomputed
   without the source spelling, so the model carries the site once, beside the text it
   describes. `RedactedQueryPlaceholder` and `LiteralQuery` gain the same `at`, and
   `prefix(Position::NONE) == ""` keeps every programmatic-grant rendering byte-identical
   (`critical_manifest_011.rs` compiles with one added field and no changed assertion).
2. **Exactly one protocol method is added: `textDocument/codeAction`.** `codeActionProvider:
   true` (boolean, no `codeActionKinds`), one `quickfix` — `Parameterize SQL for Blastfile
   ($?)` — whose single `TextEdit` replaces the whole string interior, `context.diagnostics`
   never trusted, no `workspace/applyEdit`, no `codeAction/resolve`, no new error code. Hover
   grows two tokens (`$?`, `...`) inside a `query` only. Everything else stays a non-goal:
   `definition`, `documentSymbol`, completion, pull diagnostics, incremental sync,
   semantic tokens, `injections.scm`.
3. **The grammar gains one node and one rule of provenance.** `sql_string` colours the
   interior of a `query`'s `'…'` value with seven child node types, ordinary rules only —
   no external C scanner, no second grammar, no second Zed extension, and
   `editors/zed/languages/blastfile/highlights.scm` is its only consumer. Two mechanisms are
   part of the decision because the alternatives are silent breakage: `query` leaves the
   shared `keyword` rule (with it in, the longest-match lexer always prefers the one-token
   `raw_string` and `sql_string` is unreachable), and every interior token's lexical
   precedence is above the extras' (or a `#` in the literal is lexed as a `#…` comment and
   eats the closing quote). The node's language is exactly `raw_string`'s `'[^']*'`, so the
   colourist stays strictly looser than the parser.

**Consequences.** One new public shape (`TemplateError`, `QuerySite`, `query_sites()`, two
small `blastit-sqlpolicy` functions), one new error variant, one new RPC, nine grammar
rules (`query_decl`, `sql_string` and its seven child node types) and four highlight rules
with no new capture name. `blastit-manifest` still does not depend on `pg_query`
(`crates/blastit-manifest/Cargo.toml` stays empty of it); the parse lives in
`blastit-sqlpolicy`, which `crates/blastit` already depends on, so the editor adds no
dependency either. (The server today lives in `crates/blastit/src/lsp/{mod,hover}.rs`;
decision 1's `lsp.rs` is the 2026-11-17 plan that `MB.22.3` split — the accepted text stays
as the record.) `pg_query 6.2.1` does **not** expose `cursorpos` (the C struct has it at
`libpg_query/pg_query.h:15`; the Rust wrapper reads only `message`,
`src/query.rs:35`), so a `Syntax` refusal's offset is pinned to the **first non-trivia code
character** (spaces, newlines and comments skipped) — precise for the leading-token case, honest elsewhere, and re-pointable without a
contract change if the crate ever exposes the field. The message is never rendered: it quotes the offending lexeme, which can be a
value (security S1).

**Alternatives rejected.** *Deriving the inner `line:col` in `lsp.rs` from the document text
and a second scan* — a second reader of the string spellings, which is what decision 4
forbids. *A `workspace/applyEdit`/server-side apply* — a second authority over the buffer for
no capability Zed lacks. *An external scanner or an `injections.scm` over a bundled SQL
grammar* — a C build step or a second grammar repository to keep in step, for colour only.
*Refusing `#`-bearing or two-statement `query` text in the grammar* — the grammar must never
be stricter than the parser; the LSP owns the red.

**References.** `crates/blastit-sqlpolicy/src/template.rs`,
`crates/blastit-manifest/src/{blastfile/lower,validate,error,model}.rs`,
`crates/blastit/src/lsp/{mod,hover}.rs`, `editors/tree-sitter-blastfile/grammar.js`,
`editors/zed/languages/blastfile/highlights.scm` (`MB.34.3`–`MB.34.6`).
