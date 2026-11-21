---
id: MB.22.2
title: "Design blastit lsp and the Zed extension (ADR-0013) with security/implementability/testability/consistency critics"
kind: task
status: done
owner: designer
priority: 2
depends_on: [MB.22.1]
paths:
  - "docs/design/blastfile-lsp-zed.md"
  - "docs/design/blastfile-lsp-zed.review-security.md"
  - "docs/design/blastfile-lsp-zed.review-implementability.md"
  - "docs/design/blastfile-lsp-zed.review-testability.md"
  - "docs/design/blastfile-lsp-zed.review-consistency.md"
  - "docs/decisions/ADR-0013-blastfile-lsp-and-zed.md"
  - docs/decisions/README.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T09:16"
updated: "2026-11-17T06:00"
---

# Goal

Turn the accepted editor DX into a technical design an implementer can build without
guessing, and record the durable choices in `ADR-0013`:

* `docs/design/blastfile-lsp-zed.md` — protocol, spans, formatting, hover, grammar, extension.
* `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` — the decisions (hand-rolled stdio
  JSON-RPC, full sync, in-repo grammar, isolated extension workspace, subcommand not a
  separate binary).
* Four fresh-context critics: `security`, `implementability`, `testability`, `consistency`,
  each writing `docs/design/blastfile-lsp-zed.review-<lens>.md` and ending `VERDICT: PASS`.
* The `ADR-0013` row in `docs/decisions/README.md`.

The design must pin, each in one short section with the code site it lands in and the test
tier that proves it:

1. **Module boundaries.** `crates/blastit/src/lsp.rs` owns framing, lifecycle and dispatch;
   `crates/blastit-manifest` owns `ManifestError::position()`, a `message()` that omits the
   `Blastfile:<line>:<col>: ` prefix, and positions for the non-fatal warnings;
   `crates/blastit/src/cli.rs` adds `Command::Lsp`, its usage and `from_argv` arm. State why
   the server is a subcommand of the existing binary (one artifact, one install, the
   Python-free standalone already ships it) and not a second crate/binary.
2. **The exact message set,** with JSON shapes: `initialize`, `initialized`,
   `workspace/didChangeConfiguration` (ignored), `textDocument/didOpen`, `didChange`,
   `didClose`, `textDocument/formatting`, `textDocument/hover`, `shutdown`, `exit`, and
   `$/cancelRequest`/`$/setTrace` (ignored). Ids are `serde_json::Value` echoed verbatim.
   Pin the capability document: `positionEncoding: "utf-16"`, `textDocumentSync:
   {"openClose": true, "change": 1, "save": false}`, `documentFormattingProvider: true`,
   `hoverProvider: true`, `serverInfo`. Pin JSON-RPC error codes and the rule that a
   malformed frame answers `-32700` with `id: null` and keeps serving.
3. **Bounds and I/O.** `Content-Length: N\r\n\r\n` with `N` = UTF-8 bytes; header block cap
   and body cap (name bytes), reject without allocating; read the body by length; stdout is
   framed only, logs and panics go to stderr; no `unwrap`/`panic!` on any input; no document
   text in logs. `#![deny(clippy::print_stdout)]` at the top of `lsp.rs`.
4. **Positions.** `Position` is 1-based in `char`s; LSP `line` is 0-based, `character` is
   **UTF-16** code units. Pin the formula `sum(utf16_len(c) for c in line[:col-1])`, the
   `line` split rule, `Position::NONE` → `(0,0)`, and the squiggle width (the rest of the
   `[A-Za-z0-9_-]` word, or one unit on punctuation). Say explicitly that Zed ignores the
   server's `positionEncoding` and always converts UTF-16, so a `char` count or byte count
   is a bug.
5. **Diagnostics.** For each `ManifestError`: `severity: 1`; the warning variants:
   `severity: 2`; `source: "blastit"`; **no** `code`; `message` = `message()` (the CLI
   sentence without the location prefix); `version` echoed when known; push replaces the
   whole set; `didClose` publishes `[]`. Enumerate the four boot warnings and the
   parse/validate/SQL error families, and name the `ManifestWarning` variants that must gain
   positions (unused `trait`, unrestricted `postgres`, unacknowledged `body raw`,
   `egress any`). A warning with no span maps to `Position::NONE` **only** when the CST truly
   has no site; say why per variant.
6. **Formatting.** `blastfile::fmt(text)`; equal output returns `[]`, a parse/format error
   returns `null` (never a JSON-RPC error), and otherwise one whole-document `TextEdit`
   whose `end` is `max_point_utf16` (line count of `'\n'`, UTF-16 length after the last
   `'\n'`). Decide CRLF deliberately: normalize to `\n` on save, or preserve the buffer's
   endings; record the choice and the test that proves it. State that Zed's `formatter:
   "auto"` reaches the primary language server, so declaring the capability is enough.
7. **Hover.** Word-under-cursor (`[A-Za-z0-9_-]+`, `$IDENT`) against a bounded static
   `&[(&str, &str)]` table copied from `docs/reference/blastfile.md`; unknown word → `null`;
   markdown contents. Definitions: only if a bounded `blastfile::symbols()` is in scope;
   otherwise say "deferred" with the exact follow-up API and location shape, not a vague
   "later".
8. **Extension anatomy.** `editors/zed/{extension.toml,Cargo.toml,src/lib.rs,LICENSE}` and
   `languages/blastfile/{config.toml,highlights.scm,brackets.scm,indents.scm,outline.scm}`;
   `extension.toml` keys (`id`, `name`, `version`, `schema_version = 1`, `authors`,
   `description` longer than `name`, `repository`), `[grammars.blastfile] {repository, rev,
   path}`, `[language_servers.blastit-lsp] {languages = ["Blastfile"], language_ids}`;
   `config.toml` `path_suffixes = ["Blastfile", "blastfile"]`, `line_comments`, `tab_size`,
   `brackets`; `Cargo.toml` `crate-type = ["cdylib"]`, `zed_extension_api = "0.7.0"`, empty
   `[workspace]`; `language_server_command` tries `blastit` then `uv run --quiet blastit lsp`,
   never shells out from the wasm, never needs a capability; `LICENSE` is a symlink inside
   the extension dir; generated `target/`, `extension.wasm`, `grammars/` are gitignored.
9. **Grammar.** The in-repo `editors/tree-sitter-blastfile` layout; `name: "blastfile"` must
   equal the `[grammars.*]` key and the exported `tree_sitter_blastfile`; newlines are not
   extras; two string nodes; contextual keywords; `_term`; `regex_raw_string`; committed
   `src/parser.c`, `src/grammar.json`, `src/node-types.json`; the pinned CLI version and the
   regeneration `git diff --exit-code` gate; corpus cases (`view view GET "/x"`, keyword as a
   name, newline after a trailing comma, multi-line raw string, `$IDENT`, `regex '…'`).
10. **Queries.** The 45-capture theme intersection and the documented right-to-left capture
    pair on `$IDENT`; capture by parent node, not literal where a word is contextual;
    `outline.scm` `@item` + `@name`; `indents.scm` `@indent`; `brackets.scm` `@open`/`@close`;
    no `injections.scm` for SQL in v1 (and why).
11. **Security and failure modes.** No capabilities, no network, no filesystem access beyond
    what Zed gives the server; a document is parsed in memory and never executed; the
    formatter refuses rather than guesses on broken input; a malformed message never kills
    the loop; the server never writes document text to stderr; oversized inputs are refused
    before allocation; `exit` code 0 only after `shutdown`.
12. **Testing matrix and release.** Which assertion lives in `crates/blastit/tests/
    critical_lsp.rs`, which in the grammar corpus/query run, which in a static
    `scripts/check_zed_extension.py`, and which needs a live Zed/manual pass. Include the
    Python console-script and standalone binary paths; the grammar `rev` pin procedure (a
    40-hex commit that contains `editors/tree-sitter-blastfile`, never a branch/HEAD/tag);
    and the registry publication preconditions (license inside the dir, version rules,
    submodule PR is human-only).

# Context

- Inputs: the accepted `MB.22.1` pages, `docs/research/2026-11-16-zed-blastfile-lsp.md`
  (every protocol/extension fact is verified there), `docs/design/blastfile-dsl.md` (caps,
  `fmt` idempotence, `Position` contract), `docs/reference/blastfile.md` (hover wording),
  `crates/blastit-manifest/src/error.rs`, `crates/blastit/src/cli.rs`.
- **ADR number:** `ADR-0011` and `ADR-0012` are taken; this is `ADR-0013`.
- Critics must be **fresh context** and write their own file. They may read only the repo.
- The design must stay implementable by a cheap model: numbered sections, one decision per
  section, tables for every mapping, explicit "refuse this, with this message" rows.
- No dependency may be added. If a critic asks for an LSP crate or an async runtime, refuse
  it and record why in the ADR (offline gate, MSRV 1.88, audit surface).
- **Language rules:** never mention the retired manifest filename; never describe a PHP
  alternative transport; the design and ADR are excluded from the public site but are still
  read by agents, so keep the same vocabulary discipline.

# Acceptance

```console
$ ls docs/design/blastfile-lsp-zed.md docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
$ grep -n "ADR-0013" docs/decisions/README.md
$ ls docs/design/blastfile-lsp-zed.review-{security,implementability,testability,consistency}.md
$ grep -rn "VERDICT" docs/design/blastfile-lsp-zed.review-*.md
$ uv run --no-project tools/plan.py validate
$ make docs-check
$ wc -l docs/design/blastfile-lsp-zed.md docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
```

1. Items 1–12 have a decision, a code site and a proving test tier; a reader can answer
   "what exactly does `didChange` carry?", "what is the column unit?", "what happens on a
   broken document when saving?" and "what is the grammar rev?" without asking a human.
2. The UTF-16 formula, the formatting `end` rule, the diagnostics severity/source/message
   contract, and the bounded-read limits are phrased as assertions a critical test can make.
3. The grammar and query sections are concrete enough to write `grammar.js` and every `.scm`
   without another research pass, including the four corpus traps named in item 9.
4. The extension section matches the verified manifest schema: no `name =` key under
   `[language_servers.*]`, `language_ids` declared, `LICENSE` inside the directory, empty
   `[workspace]`, `wasm32-wasip2`.
5. The security section covers no panic, no stdout pollution, no document text in logs,
   bounded allocation, no capabilities, and the exit-code rule.
6. All four critic files end `VERDICT: PASS`; every BLOCKER/MAJOR is fixed in the document or
   answered in `RESULT.md` with the section it changed.
7. `docs/decisions/README.md` lists `ADR-0013` with status `Accepted` and node `MB.22`;
   `make docs-check` is green (both trees are excluded from the public site).

# Notes

- Single-writer artefact: write the long design incrementally as
  `docs/design/blastfile-lsp-zed.md.part` and rename at the end; commit-ready state matters
  even though the lead commits.
- Copy the protocol facts from the research, but re-check the exact capability JSON against
  the spec section quoted there; do not "improve" the shapes.
- The one genuinely open decision is CRLF handling. Choose, justify, and give the test; do
  not leave it to the implementer.
- Definition support is optional scope. If you include it, require a bounded `symbols()`
  API in `blastit-manifest` and a test; if not, name the follow-up and the minimum
  `Location` shape so a later node is mechanical.
- Keep `ADR-0013` to one page: context, decision, consequences, alternatives rejected
  (`tower-lsp`, `lsp-server`, separate binary, separate grammar repo). The long detail lives
  in the design.
