# Zed support for `Blastfile`: extension anatomy, Tree-sitter highlighting, and a zero-dependency `blastit lsp`

- Node: MB.22 (upcoming epic, DX polish after MB.21) — Author: researcher — Date: 2026-11-16

## TL;DR (<= 8 lines)

- A Zed language extension is `extension.toml` + `languages/blastfile/config.toml` + `*.scm`; Rust
  (`zed_extension_api = "0.7.0"`, `cdylib`, **`wasm32-wasip2`**) is needed **only** because we also ship
  a language server. Highlighting alone would need no Rust at all.
- Zed never runs `tree-sitter generate`: it clones `[grammars.blastfile].repository` at `rev`, honours an
  optional `path`, and clangs **`src/parser.c`** (+ `src/scanner.c`) into `grammars/blastfile.wasm`. The
  generated parser **must be committed**; `grammar.js`/`package.json` are never read.
- `path_suffixes = ["Blastfile", "blastfile"]` works for the extensionless filename, by exact-equality on
  the file name (`available_languages.rs`), not by extension. No globs, case-sensitive.
- Only 45 captures are styled by *every* bundled theme; `variable.special`, `constant.builtin`,
  `type.builtin` and `function.method` are **not** universal. Use the documented right-to-left fallback
  (`@variable.special @variable`) instead of relying on them.
- `blastit lsp` is ~600 lines of `serde_json` + `std::io`: full sync (`TextDocumentSyncKind::Full = 1`),
  push diagnostics, whole-document `TextEdit` from `blastfile::fmt`, optional hover. **No new
  dependency, no async runtime, no LSP crate.**
- LSP positions must be **UTF-16 code units**: Zed advertises only `general.positionEncodings =
  ["utf-16"]` **and ignores the server's `positionEncoding`** (`crates/lsp/src/lsp.rs`,
  `point_to_lsp` over `PointUtf16`), while `Blastfile`'s error `Position.col` counts **`char`s** — so a
  char→UTF-16 conversion is mandatory or squiggles drift on non-ASCII lines.
- DX gap to close in docs: `format_on_save` defaults to **`off`** for languages Zed does not special-case,
  so the extension cannot make `blastit fmt` run on save by itself — ship `.zed/settings.json` and one
  documented JSON line.

## Developer Experience Implications

**Out of the box, after `zed: install dev extension` (or a registry install) and zero project config:**

| Moment | What the developer sees | Why it works with no config |
|---|---|---|
| Opens `./Blastfile` | Colours, bracket matching, outline, `#` comment toggle | `languages/blastfile/config.toml` `path_suffixes = ["Blastfile", "blastfile"]` + `line_comments = ["# "]`; grammar `rev` is pinned by the extension |
| Types `body = json` | Red squiggle at the `=`, message *"`=` is not Blastfile syntax; write the value after the keyword: `body json`"* | `blastit lsp` publishes the manifest's own error text verbatim, so the editor and `blastit check` say the same sentence |
| Saves | Canonical layout (only once the user turns on format-on-save) | `textDocument/formatting` returns the whole document from `blastfile::fmt`; idempotent by construction (R6) |
| Hovers `unrestricted` | One-paragraph explanation + "warns at boot" | static docs table in `blastit lsp`, no grammar duplication |

**The minimum number of lines.** The extension resolves the server itself, so the developer configures
**nothing**: `blastit` on `PATH` → `blastit lsp`; otherwise `uv` on `PATH` → `uv run blastit lsp`
(the same `uv add blastit` install the docs already use). The only line we cannot ship from the
extension is formatting:

```json
{ "languages": { "Blastfile": { "format_on_save": "on" } } }
```

because Zed's `format_on_save` default is `off` and extensions cannot set per-language defaults
(`docs/src/reference/all-settings.md`: *"Default: `off` … Zed ships `"format_on_save": "on"` as a
per-language default for Astro, Dart, … Rust, …"*). Two zero-config mitigations for MB.22:
commit `.zed/settings.json` in the Blastit repo (contributors get it for free) and paste the one line in
`docs/local.md`'s editor section, not a whole page.

**Non-goals for DX.** No `npm`, no `tree-sitter` CLI, no `wasi-sdk`, no `cargo` on the *user's* machine:
those are needed only by whoever builds the extension (Zed installs `wasm32-wasip2`, downloads wasi-sdk
and needs system `cargo`). The grammar's `parser.c` is committed precisely so users never generate it.

## Technical Findings

### 1. Extension layout (what must be committed)

```
editors/zed/                        # the extension dir the developer points Zed at
  extension.toml                    # id, name, version, schema_version, authors, description, repository
  Cargo.toml                        # only because we ship an LSP (cdylib + [workspace] — see §3)
  src/lib.rs                        # Extension impl + register_extension!
  LICENSE                           # symlink -> ../../LICENSE (must be INSIDE the extension dir)
  README.md                         # optional
  languages/blastfile/
    config.toml                     # name, grammar, path_suffixes, line_comments, brackets, tab_size
    highlights.scm
    brackets.scm                    # optional
    indents.scm                     # optional, @indent required if present
    outline.scm                     # optional, @item + @name required if present
editors/tree-sitter-blastfile/      # the grammar repo (may live in this repo; see §3)
  grammar.js
  src/parser.c                      # COMMITTED, generated
  src/node-types.json               # COMMITTED, generated (queries are validated against it)
  src/grammar.json                  # COMMITTED, generated
  test/corpus/*.txt                 # `tree-sitter test`
```

Facts, from Zed source and docs:

- `languages/` is **discovered**, not declared: `ExtensionManifest::populate_defaults` walks
  `languages/*/` and registers every subdirectory that contains `config.toml`
  (`crates/extension/src/extension_manifest.rs`). The directory name is arbitrary; the language is
  identified by `config.toml`'s `name`.
- Every query file is optional and loaded with `.ok()`; a missing `highlights.scm` simply means no
  colours — the language still loads (`crates/language_core/src/queries.rs::QueryFile`,
  `crates/extension_host/src/extension_host.rs::load_plugin_queries`).
- The exact query filenames Zed reads are `highlights.scm`, `brackets.scm`, `outline.scm`,
  `indents.scm`, `injections.scm`, `overrides.scm`, `redactions.scm`, `runnables.scm`,
  `debugger.scm`, `textobjects.scm`. `embedding.scm` is **not** a Zed file (that is Helix/nvim).
- `extension.wasm`, `target/`, `grammars/<name>/` and `grammars/<name>.wasm` are written **into the
  extension directory** by Zed at install time (`extension_builder.rs`); gitignore them.
- Local dev install: `zed: extensions` → **Install Dev Extension** (action `zed: install dev
  extension`) → pick the directory containing `extension.toml`. Zed builds it in **debug** profile,
  so iteration is seconds. Logs: `zed: open log`, or `zed --foreground` to see `println!` from the wasm.

### 2. `extension.toml` schema (fields that matter, verified against `ExtensionManifest`)

```toml
id = "blastfile"                       # required; = extensions.toml key = submodule dir name
name = "Blastfile"                     # required
version = "0.1.0"                      # required; strict semver, must not decrease
schema_version = 1                     # required; exactly 1 (schema v1)
authors = ["Offworld Nexus"]
description = "Blastfile: Blastit's routing table and permission model"
repository = "https://github.com/OffworldNexus/blastit"

[grammars.blastfile]                   # key must be snake_case AND equal grammar.js `name`
repository = "https://github.com/OffworldNexus/blastit"
rev = "<40-hex commit on a branch>"     # `commit` is accepted as an alias; must be fetchable with --depth 1
path = "editors/tree-sitter-blastfile"  # optional subdirectory inside the grammar repo

[language_servers.blastit-lsp]
languages = ["Blastfile"]               # must equal languages/blastfile/config.toml `name` (case-sensitive)
[language_servers.blastit-lsp.language_ids]
"Blastfile" = "blastfile"               # languageId sent in didOpen
```

- `id`, `name`, `version`, `schema_version` have no serde default → hard parse errors when missing;
  `authors`/`description`/`repository` are `#[serde(default)]` but are required by the registry
  validator (`crates/extension_cli/src/main.rs::validate_extension_manifest`), and `description` must
  be **longer in chars than `name`**.
- `LanguageServerManifestEntry` today is `{ language?, languages, language_ids, code_action_kinds?,
  opt_in_languages }` — the `name = "…"` key shown in the docs is silently ignored. Use the table key
  as the server id.
- Declaring the language a second time via `languages = [...]` in `extension.toml` is not needed (v1
  schema auto-populates it).
- Published artifact version compatibility: `zed_extension_api = "0.7.0"` ↔ Zed `0.205.x`. Document
  "Zed ≥ 0.205"; older builds refuse the extension rather than mis-running it.

### 3. Grammar build contract and the WASM target (the two hard constraints)

`ExtensionBuilder::compile_extension` (`crates/extension/src/extension_builder.rs`):

- Rust: `cargo build --target wasm32-wasip2 --target-dir <ext>/target` in the extension dir, then reads
  `target/wasm32-wasip2/{debug,release}/<package_name with '-'→'_'>.wasm`, strips debug sections, writes
  `<ext>/extension.wasm`. `wasm32-wasip2` (component model) replaced `wasm32-wasip1` in May 2025; the
  docs state it and the code pins `const RUST_TARGET: &str = "wasm32-wasip2"`.
- Grammar: `git init` + `git remote add` + `git fetch --depth 1 origin <rev>` + `checkout <rev>` into
  `<ext>/grammars/<name>/`, then

  ```
  clang -fPIC -shared -Os -Wl,--export=tree_sitter_blastfile \
        -Wl,--max-memory=4294967296 -I <src> <src>/parser.c [<src>/scanner.c] \
        -o grammars/blastfile.wasm
  ```

  Consequences: **`src/parser.c` must be committed**; only `scanner.c` is compiled (**`scanner.cc` is
  not**); the `[grammars.X]` key must equal the exported `tree_sitter_<key>` symbol, i.e. `grammar.js`
  `name: "blastfile"`; `rev` must be reachable by a shallow fetch (a real commit on a branch, not an
  unreferenced orphan); `path` lets one repo host the grammar (our case).
- **`editors/zed/Cargo.toml` must contain an empty `[workspace]` table.** Blastit's root manifest is a
  workspace with `members = ["crates/*"]`; a package under `editors/` is neither a member nor excluded,
  and cargo refuses to build it (empirically verified, `/tmp/opencode/wstest`, 2026-11-16):

  ```
  current:   .../editors/zed/Cargo.toml
  workspace: .../Cargo.toml
  this may be fixable … add an empty `[workspace]` table to the package's manifest.
  ```

  With `[workspace]` added, `cargo metadata` resolves the package on its own and writes
  `editors/zed/target/`. This also keeps the root `Cargo.toml`/`Cargo.lock` untouched — important while
  `MB.21.4` owns `target/` and lock edits are serialized.

```toml
# editors/zed/Cargo.toml
[package]
name = "blastit-zed"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
zed_extension_api = "0.7.0"

[workspace]        # keep the extension out of Blastit's root workspace
```

```rust
// editors/zed/src/lib.rs (shape, ~50 lines)
use zed_extension_api as zed;

struct Blastit;

impl zed::Extension for Blastit {
    fn new() -> Self { Self }

    fn language_server_command(
        &mut self,
        _id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        if let Some(blastit) = worktree.which("blastit") {
            return Ok(zed::Command { command: blastit, args: vec!["lsp".into()], env: Default::default() });
        }
        if let Some(uv) = worktree.which("uv") {
            return Ok(zed::Command {
                command: uv,
                args: vec!["run".into(), "--quiet".into(), "blastit".into(), "lsp".into()],
                env: Default::default(),
            });
        }
        Err("blastit is not installed: run `uv add blastit` (or `uv tool install blastit`) in this project"
            .into())
    }
}

zed::register_extension!(Blastit);
```

- Only `Worktree::which` / `shell_env` are used. Do **not** shell out from the extension to probe
  `blastit --version`: that path is gated by `[capabilities]` `process:exec` **and** a user-granted
  capability (`crates/extension_host/src/capability_granter.rs::grant_exec`), which would turn a
  zero-config install into a permission prompt.
- The `Command` is spawned by Zed, not by the wasm, so the LSP needs no capability declaration.
- `profile.release`/LTO settings from the root workspace do not apply (empty `[workspace]`); Zed builds
  debug for dev extensions, release only in registry CI.

### 4. Grammar sketch (the node names the queries below assume)

`Blastfile` is newline-terminated and contextual-keyword-heavy, which drives three grammar decisions:

```js
// editors/tree-sitter-blastfile/grammar.js  (shape, not final)
module.exports = grammar({
  name: 'blastfile',
  extras: $ => [/[ \t\r]/, $.comment],        // NOT /\s/: a newline ends a statement
  word: $ => $.name,
  rules: {
    source_file: $ => repeat($._section),
    _section: $ => choice($.app_decl, $.entrypoint_decl, $.store_decl,
                          $.trait_decl, $.view_decl, $.static_decl),
    app_decl:        $ => seq('app', $.string, $._term),
    entrypoint_decl: $ => seq('entrypoint', $.language, $.string, optional($.block), $._term),
    language:        $ => choice('python', 'php'),
    store_decl:      $ => seq('postgres', optional($.name),
                              choice($.env_var, $.block), $._term),
    trait_decl:      $ => seq('trait', $.name, $.block),
    view_decl:       $ => seq('view', $.name, optional($._view_header),
                              choice($.block, repeat($._endpoint))),
    _view_header:    $ => seq(optional(seq($.method, repeat(seq(',', optional($._nl), $.method)))),
                              repeat(seq($.pattern, repeat(seq(',', optional($._nl), $.pattern))))),
    static_decl:     $ => seq('static', $.pattern, $.block),
    block:           $ => seq('{', repeat($._statement), '}', $._term),
    _statement:      $ => choice($.directive, $.endpoint, $.grant, $.mode, $.use_directive),
    directive:       $ => seq($.keyword, repeat1(seq($.value, repeat(seq(',', optional($._nl), $.value)))), $._term),
    grant:           $ => seq($.keyword, optional($.name), optional($.table_list), $._term),
    _term:           $ => choice('\n', ';'),   // `}` closes a block; the trailing newline is still one `_term`
    _nl:             $ => '\n',
    env_var:         $ => /\$[A-Z_][A-Z0-9_]*/,
    bind:            $ => '$?',
    name:            $ => /[a-z0-9][a-z0-9_-]*/,
    method:          $ => /[A-Z]+/,
    string:          $ => /"(\\.|[^"\\])*"/,
    raw_string:      $ => /'[^']*'/,            // multi-line, no escapes
    comment:         $ => /#[^\n]*/,
    placeholder:     $ => /\{[^}]*\}|\*/,
  },
});
```

- **Newlines are not extras.** Every statement rule ends with an explicit `_term`, and the four places
  where a newline must *not* terminate (inside `(...)`, inside either string form, right after `,`) are
  handled by `optional($._nl)` after commas. Treating `\n` as an extra would make `body` directives
  swallow the next section.
- **Contextual keywords work natively.** Tree-sitter's lexer is parse-state aware, so `view view GET
  "/x"` parses: in the name position only `$.name` is valid. Add a corpus test for exactly that, plus
  `trait session` / `view session`, because a keyword-reserving grammar would silently reject legal
  files that `blastit` accepts.
- **Two string nodes, not one.** `"…"` allows `\" \\ \n \t`; `'…'` is byte-verbatim and may span lines.
  Collapsing them into one rule would light up `regex '…'` as an escaped string and break
  `@string.regex`.
- Use `alias`/named rules for the repeated word class (`$.keyword`, `$.table`, `$.column`) so the
  queries can capture by node rather than by literal text.

### 5. Syntax highlighting: the capture list is narrower than the docs table suggests

The docs table (`zed.dev/docs/extensions/languages#syntax-highlighting`) lists 43 captures and
explicitly supports fallbacks: *"A single Tree-sitter pattern can specify multiple captures on the same
node to define fallback highlights. Zed resolves them right-to-left."*

Empirical check of the three bundled theme families (`assets/themes/one/one.json`, `ayu/*`,
`gruvbox/*`, 2026-11-16): the **intersection of the `syntax` style maps is 45 keys** and contains
`function` but **not** `function.builtin`/`function.method`, `constant` but **not** `constant.builtin`,
`type` but **not** `type.builtin`, and **not** `variable.special` (missing from Ayu; One and Gruvbox
have it). A capture with no theme style renders as plain text — capture names are never an error
(`highlights.scm` is loaded by `Query::new` in `crates/language_core/src/grammar.rs`), and dotted
suffixes fall back to a defined prefix on dot boundaries only (`syntax_theme.rs::highlight_id`).

So: prefer the intersection, and use the documented fallback for the two captures that carry real
meaning here:

```scheme
; --- sections and directives
["app" "entrypoint" "postgres" "trait" "view" "static"] @keyword
(directive (keyword) @keyword)           ; body, use, preload, root, index, immutable, fallback,
                                         ; egress, any, prefork, host, port, user, password,
                                         ; database, sslmode, ca_cert, regex
; --- grants vs modes
(grant (keyword) @function)              ; select, insert, update, delete, query
(mode (keyword) @constant)               ; transactions, functions, unrestricted
; --- literals
(env_var) @variable.special @variable    ; $DATABASE_URL — Ayu has no variable.special
(bind) @variable.parameter               ; $?
(string) @string
(string (escape) @string.escape)
(raw_string) @string.special             ; 'SELECT …' / 'SET TIME ZONE …'
(regex_raw_string) @string.regex         ; regex '/v[0-9]+/items'
(pattern) @string
(placeholder) @string.special.symbol     ; "{id:int}", "{id:uuid}", "*"
(method) @attribute                      ; GET, POST, PUT, PATCH, DELETE, OPTIONS
(language) @type                         ; python, php
(codec) @constant                        ; json, form, none, raw, unvalidated
; --- names
(view_decl name: (name) @function)
(trait_decl name: (name) @type)
(store_decl name: (name) @type)
(grant store: (name) @type)              ; postgres analytics { … }
(use_directive (name) @type)             ; use session, catalog
(grant (table) @property)
(grant (column) @property)
; --- punctuation and comments
["{" "}" "(" ")"] @punctuation.bracket
["," ";"] @punctuation.delimiter
(comment) @comment
```

Notes that keep the mapping honest:

- Distinguish section keywords from grant verbs by **parent node**, not by literal text, because
  keywords are contextual in `Blastfile`: a view may legally be named `view` (`view view GET "/x"`),
  and Tree-sitter resolves that through per-parse-state lexing, not by reserving words. Queries that
  match literals (`["view" "select"]`) would still work, but parent-node captures survive grammar
  refactors and let `transactions` be `@constant` while `select` is `@function`.
- `$IDENT` is the one construct whose "special variable" intent is real, and it is exactly the capture
  the default themes disagree about → the fallback pair is mandatory, not cosmetic.
- `regex '…'` deserves `@string.regex` (styled in all bundled themes) rather than `@string.special`.
- Do **not** attempt SQL injection (`injections.scm`) into `query '…'` strings in v1: Zed has no
  bundled PostgreSQL grammar to inject, and a missing injection language is a silently dead query file
  that reviewers must reason about. Revisit only if we ship `tree-sitter-sql` ourselves.
- `unrestricted` / `body raw` are dangerous but Tree-sitter cannot warn. Leave them plain (`@constant`)
  and let `blastit lsp` hover/diagnostics carry the warning text.
- `outline.scm` (`@item` + `@name` are the only required captures):
  `(view_decl) @item (view_decl name: (name) @name)` etc. for the six sections — ~8 lines, and
  `cmd-shift-o` becomes a route/ grant navigator. Cheap; do it.
- `brackets.scm` needs both `@open`/`@close`; skipping it loses rainbow brackets only (auto-close and
  newline-on-Enter come from `config.toml` `brackets`). `indents.scm` needs `@indent`; without it,
  Enter keeps the previous line's indent — with `tab_size = 4` and brace blocks, ship both.

### 6. `languages/blastfile/config.toml` and the extensionless filename

```toml
name = "Blastfile"
grammar = "blastfile"
path_suffixes = ["Blastfile", "blastfile"]
line_comments = ["# "]
tab_size = 4
hard_tabs = false
autoclose_before = ",;]}"
brackets = [
  { start = "{", end = "}", close = true, surround = true, newline = true },
  { start = "(", end = ")", close = true, surround = true, newline = false },
  { start = "\"", end = "\"", close = true, newline = false, not_in = ["string"] },
  { start = "'", end = "'", close = true, newline = false, not_in = ["string"] },
]
```

**Why `"Blastfile"` in `path_suffixes` works** — `crates/language/src/available_languages.rs`
(`find_for_file`), verbatim:

```rust
let extension = filename.and_then(|filename| filename.split('.').next_back());
let path_suffixes = [extension, filename] /* … */;
let ext = ".".to_string() + path_suffix;
let matched_suffix_len = path_suffixes
    .iter()
    .find(|(suffix, _)| suffix.ends_with(&ext) || *suffix == path_suffix.as_str())
    .map(|(suffix, _)| suffix.len());
```

Both the last dot-segment **and the whole file name** are candidates, and a suffix matches when the
candidate ends with `".<suffix>"` **or equals** it. So `"Blastfile"` matches the file `Blastfile` by
equality (score = length), and also matches `prod.Blastfile`; it does **not** match `prod-Blastfile`.
Matching is case-sensitive and glob-free (the docs say so, and only user `file_types` use `globset`);
`path_suffixes = ["Blastfile", "blastfile"]` covers a lowercase copy, and longest match wins across
languages, so extra suffixes are safe. Precedent in-tree: `bash/config.toml` ships `"bashrc"`,
`"PKGBUILD"`, `"APKBUILD"`; upstream `buffer_tests.rs` asserts `path_suffixes: ["Makefile"]` selects
`src/Makefile`.

`first_line_pattern` is **not** needed. It is a pure fallback in the `Undetermined` branch, evaluated
against the first **256 bytes** of the buffer (not just line 1) and only when file content is available
(`find_for_file(..., content: None, ...)` cannot match) — wrong tool for a fixed filename.

### 7. `blastit lsp`: the minimal LSP 3.17 subset Zed needs

**Framing.** `Content-Length: N\r\n\r\n` + exactly `N` bytes of UTF-8 JSON. The spec treats
headers as HTTP-semantic (case-insensitive names, optional `Content-Type`), but **Zed's client is
byte-strict**: it writes and parses the literal `"Content-Length: "` (capital `C`/`L`, exactly one
space) and never reads `Content-Type` (`crates/lsp/src/lsp.rs::CONTENT_LEN_HEADER`,
`crates/lsp/src/input_handler.rs::read_headers`). Emit exactly that line, and set `N` to the **UTF-8
byte** length of the body, not a char count. Read headers until the blank line, then read the body by
length — never by newline. Everything written to stdout must be framed; logs go to stderr. Bound the
reader: cap the header block (e.g. 8 KiB) and reject `Content-Length` above ~8 MiB rather than
allocating — Zed imposes no inbound limit of its own (`buffer.resize(message_len, 0)`).

**Lifecycle.** `initialize` (request) → `initialized` (notification) → … → `shutdown` (request,
`result: null`) → `exit` (notification). Exit code `0` iff `shutdown` was received, else `1`. Zed sends
`processId`, `clientInfo`, `rootUri` (and the deprecated `rootPath`), `workspaceFolders`,
`capabilities` (notably `general.positionEncodings = ["utf-16"]`), and `initializationOptions` — `None`
unless the extension supplies them (`crates/lsp/src/lsp.rs`). It does **not** send `locale`.
Ignore all of it. Reply:

```json
{"jsonrpc":"2.0","id":1,"result":{
  "capabilities":{
    "positionEncoding":"utf-16",
    "textDocumentSync":{"openClose":true,"change":1,"save":false},
    "documentFormattingProvider":true,
    "hoverProvider":true
  },
  "serverInfo":{"name":"blastit","version":"0.2.0"}
}}
```

`positionEncoding` may be omitted (default `utf-16`), but be explicit: Zed offers **only** `"utf-16"`
in `general.positionEncodings`, and the spec says a server that got no offer may only return `utf-16`.

Zed then sends `initialized` and, immediately after it, **one `workspace/didChangeConfiguration`**
(before it marks the server as running) — ignore both. On `shutdown` Zed waits 5 s for the `null`
result, sends `exit`, drains stdout, then kills the process, and it does **not** restart a server that
dies on its own: a crash reads as "language server is not running" until the user restarts it, so never
abort the loop on one bad message.

**Text sync.** `textDocument/didOpen` carries `textDocument{uri, languageId, version, text}`; Zed sends
it **unconditionally** on buffer registration (it ignores `openClose`), with `version: 0` and
`languageId` = the extension's `language_ids` value when set, otherwise the language name
**lowercased** — so declare `[language_servers.blastit-lsp.language_ids]` and don't guess.
`TextDocumentSyncKind::Full = 1` means `didChange` carries
`contentChanges: [{"text": "<whole document>"}]` with **no** `range` / `rangeLength`. That is the right
choice here: the source is capped at `MAX_SOURCE_BYTES = 1_048_576` (1 MiB) and
`MAX_TOKENS = 262_144`, the parser is one LL(1) pass, and incremental sync would force the server to
maintain a coordinate-mapped rope for zero benefit. `didClose` is likewise always sent → drop the
document and publish `[]` so Zed clears the squiggles.

**Omitting `textDocumentSync` is not "default Full":** Zed then sends no `didChange` at all
(`lsp_store.rs::text_document_sync_kind_for_buffer` returns `None` and the change handler skips the
buffer), which presents as "diagnostics never update". Declare `"textDocumentSync": 1` or
`{"openClose": true, "change": 1, "save": false}`.

**Diagnostics (push).** `textDocument/publishDiagnostics`
`{uri, version?, diagnostics:[{range, severity?, code?, source?, message, tags?, relatedInformation?}]}`;
severity `1`=Error…`4`=Hint, and **omitted severity is treated as Error** by Zed. Push needs no
capability flag (there is no `diagnosticProvider` boolean for push) and Zed registers the handler
unconditionally; each push **replaces** the whole set for that URI. Zed also speaks pull diagnostics,
but only for servers that advertise `diagnosticProvider` — do not, push is strictly simpler.

For each diagnostic: `source: "blastit"`; no `code` (the manifest's error enum has no stable key); and
`message` = the **verbatim** `Display` of `ManifestError` *without* the `Blastfile:line:col: ` prefix (the
range already carries the position) — the same sentence the CLI prints, which is the whole point of the
DX contract.

**Positions — the one place this can silently break.** LSP `character` is a **UTF-16 code-unit** offset
by default, and Zed does not negotiate anything else: it offers only `["utf-16"]` and **never reads the
server's `positionEncoding`** — it converts with `point_to_lsp(row, column)` over a `PointUtf16`
(`crates/language/src/language.rs`), so replying `"utf-8"` is not merely unnegotiated, it is ignored.
A `character` past the end of a line clamps to that line's length. `blastit-manifest`'s `Position` is
*"a 1-based `line` and a 1-based `col` counted in `char`s"* (`crates/blastit-manifest/src/error.rs`).
Conversion:

```
line_lsp  = position.line() - 1
char_col  = position.col() - 1                       # chars, 1-based → 0-based
character = sum(utf16_len(c) for c in line_text[:char_col])
```

where `line_text` is the document split on `'\n'` (the terminator is not part of the line; `Blastfile`
forbids lone `\r`). A byte offset would be wrong for any line containing `é`, `—`, or an emoji in a
comment or SQL string; a `char` offset is wrong only for characters outside the BMP, which is why the
formula above — not `col - 1` — is the rule to encode once and unit-test with `"# ✓ ok\nbody ="`.
`Position::NONE` (whole-file refusals, `line == 0`) maps to `(0,0)-(0,0)`.

Range width: start at the error position and extend to the end of the word under it (scan
`[A-Za-z0-9_-]`), or one `character` if the offender is punctuation (`=`), so the squiggle covers
something visible. `Range.end` is exclusive: a point range is `start == end`.

**Formatting.** `textDocument/formatting` with `options{tabSize, insertSpaces, trimTrailingWhitespace,
insertFinalNewline, trimFinalNewlines}` (Zed sends all five: 4 spaces, trim, final newline) → return
`TextEdit[] | null`. Its `formatter` setting defaults to `"auto"`, i.e. the primary language server
when Prettier is not allowed, so declaring `documentFormattingProvider` is all that is needed for
`editor: format`. `Blastfile` has one canonical layout, so:

- `blastfile::fmt(text)` (`crates/blastit-manifest/src/blastfile/print.rs`, already public) either
  succeeds or returns `ManifestErrors`; on error return `null` (diagnostics already explain it) — do
  **not** return an error code, or Zed surfaces "language server error" on every save of a broken file.
- If `fmt(text) == text`, return `[]` (no edits). A full-document `TextEdit` that changes nothing still
  marks the buffer dirty and fights `format_on_save`.
- Otherwise one edit:
  `{"range":{"start":{"line":0,"character":0},"end":{"line":<n>,"character":<m>}},"newText":"<canonical>"}`
  where `<n>` is the number of `'\n'` in the buffer and `<m>` is the UTF-16 length of the text after the
  last `'\n'` — i.e. `buffer.max_point_utf16()`. (Not `line_count - 1` on a file ending in a newline;
  not the last *non-empty* line, which leaves the final newline unreplaced.) This exact end is what Zed
  compares against to take its diff fast path; another end still applies via the generic edit path.

**Hover (do this in v1).** `textDocument/hover` → `{contents:{kind:"markdown", value:"…"}, range?}` or
`null` (Zed advertises `content_format: ["markdown"]`, and drops empty text).
No grammar or CST access needed: take the word under the cursor (`[A-Za-z0-9_-]+`, plus `$IDENT`) and
look it up in a `&[(&str, &str)]` table of directive semantics copied from `docs/reference/blastfile.md`
(`unrestricted`, `raw`, `raw unvalidated`, `transactions`, `functions`, `query`, codecs, `egress any`,
`prefork`, `immutable`, `fallback`, section keywords). This is the highest value per line of code in the
whole LSP: it puts the "what does this grant actually allow" answer where the developer is typing.

**Definition (defer).** `textDocument/definition` — Zed accepts `Location`, `Location[]` **and**
`LocationLink[]` (using `targetSelectionRange`); `Location{uri, range}` is the minimum — for `use
<trait>` → `trait <name>` and `postgres <store>` grants → `postgres <name> $URL` needs the *declaration*
span, and today the model exposes only `Manifest::route_lines()` (header line numbers). Implementing it
by re-scanning lines would duplicate grammar knowledge in the LSP and go wrong as soon as a name is
reused as a column. Gate it on a small `blastit_manifest::blastfile::symbols() ->
Vec<Symbol { kind, name, line, col }>` (one walk of the existing CST) and ship it in the follow-up.
Zed accepts a line-only range, so the first version can be `start == end == (line-1, 0)`.

**JSON-RPC details that bite.** Every message carries `"jsonrpc":"2.0"`; ids are `number | string` and
must be echoed **verbatim** → keep the id as `serde_json::Value`. Requests get exactly one `result` or
one `error`; notifications get nothing. Codes: `-32700` parse, `-32600` invalid request, `-32601`
method not found, `-32602` invalid params, `-32603` internal, `-32002` server-not-initialized (a request
before `initialize`), `-32800` request cancelled. A malformed frame answers `-32700` with `id: null` and
keeps serving. Any `$/…` **request** must error `-32601`; `$/cancelRequest`, `$/setTrace`,
`workspace/didChangeConfiguration` and `window/workDoneProgress/create` → tolerate/ignore (never reply to
a notification, never send `client/registerCapability` from the server). `blastit lsp` must never
`panic!`/`unwrap` on any JSON it reads, and must never echo document text into logs (a `Blastfile` can
hold a `query '…'`; a log line is still a log line).

**CLI surface.** Add `Command::Lsp` in `crates/blastit/src/cli.rs` (`from_argv` → `Some("lsp")`), a
`LSP_USAGE` block, and the usage line `blastit lsp   speak LSP on stdin/stdout (editors)`. It must not
touch privileges, `BLASTIT_*` config, `/run`, or `Host::binary()` — it is a pure text tool, so it also
works through the Python console script (`_blastit.main(["lsp"], …)`), which is exactly how `uv run
blastit lsp` reaches it. Keep the module free of `println!` (`#![deny(clippy::print_stdout)]` at the top
of `lsp.rs`) so no stray help text can corrupt the stream. Zed spawns the command with **cwd = the worktree
root**, stdin/stdout/stderr piped and `kill_on_drop`, which is exactly what the `uv run blastit lsp`
fallback needs.

### 8. Licensing and dependency check

| Component | License (verified) | Verdict |
|---|---|---|
| `zed_extension_api` 0.7.0 | **Apache-2.0** (docs.rs crate page + `crates/extension_api/Cargo.toml`) | allowed; same license as Blastit |
| Zed's bundled example extensions | Apache-2.0 (`LICENSE-APACHE` per extension dir) | safe reference |
| Registry accepted licenses | Apache-2.0, BSD-2/3, CC BY 4.0, GPLv3, LGPLv3, MIT, Unlicense, zlib | Apache-2.0 is accepted |
| `LICENSE` placement | must be **inside the extension directory**; a repo-root license "will not work" when `path =` is used; symlink allowed | `editors/zed/LICENSE -> ../../LICENSE` |
| `serde_json` 1.0.154 | MIT OR Apache-2.0, already a `crates/blastit` dependency | no new dependency |
| LSP crates (`tower-lsp`, `lsp-types`, `lsp-server`) | MIT OR Apache-2.0 | **not needed** |

No Rust/WASM GPL/AGPL enters shipped code: the extension's only dependency is Apache-2.0, and the
grammar is our own `grammar.js`/`parser.c` (no vendored third-party grammar).

## Comparison Table

### Where the extension lives

| Option | Distribution | Pros | Cons |
|---|---|---|---|
| **Submodule = `OffworldNexus/blastit`, `path = "editors/zed"`, grammar via same repo + `path`** | one registry entry | one version, one rev, no cross-repo drift; grammar reusable by Helix/Neovim users later | registry clones the whole Blastit repo (~heavy); `rev` bump touches the extension and grammar together |
| Separate `blastit-zed` + `tree-sitter-blastfile` repos | two repos | tiny clones, classic layout | two rev pins to keep in sync; more release ceremony for a v1 |
| Grammar-only extension, no Rust | registry | no cdylib, no cargo for us | **no LSP** — contradicts MB.22's goal |

### The LSP transport/runtime

| Option | Deps added | Footprint | Verdict |
|---|---|---|---|
| **Hand-rolled `serde_json` + `std::io`, blocking** | none (`serde_json` already there) | ~600 LOC, no runtime | **chosen**: a 1 MiB single-file DSL needs no concurrency, and every byte of the JSON-RPC is ours to test |
| `tower-lsp` (+ `tokio`) | ~40 crates, async runtime | fast to write | rejected: a blocking single-threaded loop is simpler, smaller, and cannot leak tasks |
| `lsp-server` + `lsp-types` | +2 crates, still sync | small | rejected: `lsp-types` re-derives every message shape we need 8 of; ours is 8 structs |
| `stdio` shell-out to an existing formatter | — | none | no: `blastit fmt` *is* the formatter, and `fmt` is already a library call |

### Editor reach

| Editor | Mechanism | Cost |
|---|---|---|
| Zed | this extension (grammar + LSP) | epic MB.22 |
| Helix / Neovim | same `tree-sitter-blastfile` grammar + `blastit lsp` | free if the grammar lives in a public repo and `lsp` is stdio |
| VS Code | needs a separate client package | out of scope (a later "editors/vscode" node) |

## Concrete Recommendations for Blastit

1. **Ship one extension directory, `editors/zed/`,** with `[workspace]` in its `Cargo.toml`, a
   `LICENSE` symlink, `zed_extension_api = "0.7.0"`, and a `language_server_command` that tries
   `blastit` then `uv run blastit lsp` (never shells out from the wasm).
2. **Keep the grammar in this repo** at `editors/tree-sitter-blastfile/`, `name: "blastfile"`, and pin
   `[grammars.blastfile] { repository = <blastit>, rev = <sha>, path = "editors/tree-sitter-blastfile" }`.
   Commit `src/parser.c`, `src/grammar.json`, `src/node-types.json`; gitignore
   `target/`, `extension.wasm`, `grammars/`.
3. **CI gate:** regenerate with a pinned `tree-sitter-cli` (0.25.x, ABI 14/15) and `git diff
   --exit-code` the generated files — the grammar equivalent of `blastit fmt --check` — plus
   `tree-sitter test` over `test/corpus/`, and parse every `.scm` with the CLI (a bad node name or
   capture is otherwise a silent no-op locally and a CI failure in the Zed registry).
4. **Write the highlighting, brackets, indents and outline queries against the 45-capture intersection**,
   with `@variable.special @variable` fallback for `$IDENT`; validate one file per section of
   `docs/reference/blastfile.md` (the complete-file example is the fixture).
5. **`blastit lsp` scope for MB.22:** lifecycle, full sync, publishDiagnostics, whole-document
   formatting, hover-over-keywords. Defer `definition`/`documentSymbol` until `blastfile::symbols()`
   exists; skip completion entirely (directive completion from a static list is a nice-to-have, not a
   DX blocker, and a bad completion list is worse than none).
6. **Add `ManifestError::position() -> Position`** to `blastit-manifest` (one exhaustive `match` beside
   the enum) so the LSP never parses `Display` strings and never misses a new variant.
7. **Docs:** one short "Editors" section in `docs/local.md` (≤ 15 lines): install the extension, the
   `format_on_save` line, and `blastit lsp` as the thing Zed runs (so a user with any other editor can
   wire it up in 3 lines). Commit `.zed/settings.json` with `format_on_save: "on"` for contributors.
8. **Publish** by PR to `zed-industries/extensions`: `git submodule add <blastit> extensions/blastfile`,
   `[blastfile] { submodule = "extensions/blastfile", path = "editors/zed", version = "<ext version>" }`,
   `pnpm sort-extensions`; the checked-out commit must be on a branch.

## Unverified / Open Questions

- **Grammar sketch not compiled here:** no `tree-sitter` CLI was available offline (the `npx
  tree-sitter-cli` binary download failed on this host, and `cargo install` was out of scope for the
  `MB.21.4`-owned `target/`), so §4 is a design sketch, not a generating grammar. Before writing
  queries, run `tree-sitter generate` + `tree-sitter test` on the corpus cases named there (`view view
  GET "/x"`, a newline after a trailing `,`, a multi-line `'…'`).
- **Perf claim:** the 1 MiB/`MAX_TOKENS` ceiling is a design budget, not a measured p99. MB.22 should
  record `criterion`-free timing (a `#[test]` that parses the reference's complete file 1 000×) before
  claiming "full sync per keystroke is free".
- **`format_on_save` + `formatter`:** verified default `off` and that `formatter: "language_server"` is
  the setting that routes to us; not verified whether a project `.zed/settings.json` can enable
  format-on-save for a language whose server comes from an extension *without* also naming
  `"formatter": "language_server"` (it likely defaults to the first formatter, i.e. us; probe it when
  implementing).
- **Grammar ABI:** Zed's runtime tracks tree-sitter `main` (rev `803c6eb…`, 2026-11) and loads the
  grammar as a plain module; the exact `LANGUAGE_VERSION` ceiling was not tested. Pin the CLI and
  verify with a dev install rather than trusting a version table.
- **Registry validation of `.scm`:** reported to parse every query in CI (`package-extensions`), not
  exercised here; treat a green local `tree-sitter parse` as necessary, not sufficient.
- **CRLF files:** Zed sends the buffer's actual line endings, so `didChange` text may hold `\r\n`. The
  formatter will emit `\n`, i.e. saving would normalise line endings — decide deliberately (accept it,
  or preserve the file's ending) and test a CRLF `Blastfile`.
- **No automatic restart** after a crash is inferred from `lsp_store.rs` (only explicit restart actions
  have tasks), not observed live; likewise whether a settings edit re-sends
  `workspace/didChangeConfiguration` or restarts the server.
- **`[language_servers.X].name`** is in the docs and in real extensions but absent from the current
  struct — cosmetic only; do not rely on it for the display name.
- Zed release number that first shipped `wasm32-wasip2` (PR merged 2025-05-26) was not pinned; the
  binding constraint for us is `zed_extension_api 0.7.0` ↔ Zed `0.205.x`.

## Sources

Primary (fetched 2026-11-16):

- <https://zed.dev/docs/extensions/developing-extensions> — layout, `extension.toml` basics, `cdylib`,
  `register_extension!`, `wasm32-wasip2`, wasi-sdk, Install Dev Extension, `--foreground`.
- <https://zed.dev/docs/extensions/languages> — `config.toml` keys, `[grammars.X]` `repository`/`rev`/
  `file://`, the capture table, fallback captures, `brackets.scm`/`outline.scm`/`indents.scm`, opt-in
  language servers, `language_ids`.
- <https://zed.dev/docs/extensions/publishing/prerequisites>,
  <https://zed.dev/docs/extensions/publishing/publishing-guide>,
  <https://zed.dev/docs/extensions/publishing/license-requirements> — submodule + `extensions.toml`
  `path`/`version`, one-extension PRs, license file inside the extension dir.
- <https://docs.rs/zed_extension_api/latest/zed_extension_api/> and `trait.Extension` /
  `struct.Worktree` — 0.7.0, Apache-2.0, `language_server_command(&LanguageServerId, &Worktree) ->
  Result<Command>`, `Worktree::which/shell_env/root_path`.
- <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/> —
  `#headerPart` (framing), `PositionEncodingKind` ("UTF-16 … the default and must always be supported"),
  `TextDocumentSyncKind` (None 0 / Full 1 / Incremental 2), `ServerCapabilities.positionEncoding`/
  `textDocumentSync`, `#textDocument_publishDiagnostics`, `#diagnosticSeverity`, `#textDocument_formatting`,
  `#textDocument_definition`/`#textDocument_hover`, `#errorCodes`, `#shutdown`/`#exit`, `Range.end`
  exclusive.
- LSP-types probe (`/tmp/opencode/lspprobe`, `zed-industries/lsp-types` rev `82d7fc6` — the fork Zed
  pins): a full-sync `TextDocumentContentChangeEvent` serializes to `{"text": …}` only (`range` is
  `skip_serializing_if`), and `FormattingOptions` serializes the five fields Zed sends.
- Zed source (paths as of `main`, 2026-11): `crates/extension/src/extension_manifest.rs`
  (`ExtensionManifest`, `GrammarManifestEntry{repository, rev, path}`,
  `LanguageServerManifestEntry{languages, language_ids, opt_in_languages}`),
  `crates/extension/src/extension_builder.rs` (`RUST_TARGET = "wasm32-wasip2"`, `compile_grammar`,
  `checkout_repo`, `compile_rust_extension`), `crates/language/src/available_languages.rs`
  (`find_for_file`, suffix matching), `crates/language_core/src/language_config.rs`
  (`LanguageMatcher`), `crates/language_core/src/queries.rs` (`QueryFile` names),
  `crates/syntax_theme/src/syntax_theme.rs` (`highlight_id` prefix fallback),
  `crates/extension_host/src/capability_granter.rs` (`process:exec` gating),
  `crates/lsp/src/lsp.rs` (`CONTENT_LEN_HEADER`, `position_encodings: Some(vec![UTF16])`,
  initialize params, `SERVER_SHUTDOWN_TIMEOUT`), `crates/lsp/src/input_handler.rs` (`read_headers`,
  `buffer.resize(message_len, 0)`), `crates/project/src/lsp_store.rs`
  (`text_document_sync_kind_for_buffer`, `build_full_change`, `max_point_utf16` formatting fast path,
  push-diagnostic handler), `assets/settings/default.json` (`format_on_save: "off"`, `formatter: "auto"`),
  `crates/grammars/src/bash/config.toml` (`bashrc`/`PKGBUILD` precedent),
  `crates/language/src/buffer_tests.rs` (`path_suffixes: ["Makefile"]`).
- Theme capture coverage: `assets/themes/one/one.json`, `assets/themes/ayu/ayu.json`,
  `assets/themes/gruvbox/gruvbox.json` (intersection computed locally, scripts in `/tmp/opencode/`).
- <https://zed.dev/docs/reference/all-settings#format-on-save> — `format_on_save` default `off`, the
  12 languages that ship `on` per-language.

Blastit-internal:

- `docs/reference/blastfile.md` (constructs, `fmt`, error wording), `docs/design/blastfile-dsl.md`
  (§4.5 caps, R2 `line:col` in `char`s, R6 fmt idempotence, R19 `fmt` exit codes),
  `crates/blastit-manifest/src/blastfile/mod.rs` (`MAX_SOURCE_BYTES = 1_048_576`,
  `MAX_TOKENS = 262_144`, `pub use print::fmt`), `crates/blastit-manifest/src/error.rs`
  (`Position`, `ManifestErrors`), `crates/blastit/src/cli.rs` (`Command::{Dev,Check,Fmt,Serve}`,
  `USAGE`, `run_fmt`), `python/blastit/__main__.py` (console-script → `_blastit.main`),
  `zensical.toml` (`/research/` excluded from the public site).
