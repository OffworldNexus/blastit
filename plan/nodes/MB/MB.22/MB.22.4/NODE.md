---
id: MB.22.4
title: "Ship editors/zed and the in-repo tree-sitter-blastfile grammar with corpus/query proofs"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.22.2]
paths: ["editors/zed/**", "editors/tree-sitter-blastfile/**", .zed/settings.json]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T09:16"
updated: "2026-11-17T06:03"
---

# Goal

Ship the Zed extension and the in-repo Tree-sitter grammar that make `./Blastfile` a
first-class Zed language, with the queries written against the verified theme-capture set.

Committed tree:

```
editors/zed/                         # the extension directory a developer points Zed at
  extension.toml                     # id/name/version/schema/description/repository,
                                     # [grammars.blastfile], [language_servers.blastit-lsp]
  Cargo.toml + Cargo.lock            # cdylib, zed_extension_api = "0.7.0", empty [workspace]
  src/lib.rs                         # ~50 lines: choose `blastit lsp`
  LICENSE -> ../../LICENSE           # symlink INSIDE the extension directory
  .gitignore                         # target/, extension.wasm, grammars/
  languages/blastfile/
    config.toml                      # name = "Blastfile", grammar = "blastfile",
                                     # path_suffixes = ["Blastfile", "blastfile"],
                                     # line_comments = ["# "], tab_size = 4, brackets
    highlights.scm brackets.scm indents.scm outline.scm
editors/tree-sitter-blastfile/       # the grammar repo, vendored in this repo
  grammar.js package.json
  src/parser.c src/tree_sitter/parser.h src/grammar.json src/node-types.json
                                     # COMMITTED generated files (parser.c includes the header)
  test/corpus/*.txt test/fixtures/*.Blastfile test/highlight/*
.zed/settings.json                   # dogfooding: format_on_save "on" for Blastfile
```

Hard requirements:

- `editors/zed/Cargo.toml` has an **empty `[workspace]` table**, `crate-type =
  ["cdylib"]`, `zed_extension_api = "0.7.0"`, `publish = false`. The root workspace and
  `Cargo.lock` stay byte-identical.
- `src/lib.rs` implements `zed::Extension::language_server_command`: if
  `worktree.which("blastit")`, run `<blastit> lsp`; else if `worktree.which("uv")`, run
  `uv run --quiet blastit lsp`; else return the one-line "install blastit with `uv add
  blastit`" error. No shell-out, no capability request, no `blastit --version` probe.
- `extension.toml`: `id = "blastfile"`, `name = "Blastfile"`, `version = "0.1.0"`,
  `schema_version = 1`, `authors`, `description` longer than `name`, `repository`;
  `[grammars.blastfile] { repository = <Blastit>, rev = "<pin>", path =
  "editors/tree-sitter-blastfile" }`; `[language_servers.blastit-lsp] { languages =
  ["Blastfile"], language_ids = { Blastfile = "blastfile" } }`. Do **not** add a `name` key
  under the server table.
- Grammar: `name: "blastfile"` (equal to the manifest key and the exported
  `tree_sitter_blastfile`); newlines are **not** extras; every statement ends in `_term`
  (`\n` or `;`); two string nodes (`string`, `raw_string`) plus `regex_raw_string`;
  `$IDENT` as `env_var`; contextual keywords allowed as names (`view view GET "/x"`).
- Queries: captures from the 45-theme intersection only. `$IDENT` uses
  `@variable.special @variable`; no bare `@function.method`, `@constant.builtin` or
  `@type.builtin`; capture by parent node, not literal text, where a word is contextual.
  `outline.scm` has `@item` + `@name`; `indents.scm` has `@indent`; `brackets.scm` has
  `@open`/`@close`. No `injections.scm`.
- The `rev` pin is the one field this node cannot finalize: leaf workers do not commit.
  Put the all-zero 40-hex SHA in `rev` and record it in `RESULT.md`; `MB.22.5` replaces it
  with the lead's commit that contains `editors/tree-sitter-blastfile` and proves
  `git cat-file -e`. Never use a branch name, tag or `HEAD`, even temporarily.

# Context

- The research is mandatory reading: `docs/research/2026-11-16-zed-blastfile-lsp.md` §1–§5
  (layout, manifest schema, build contract, grammar sketch, capture list). The design
  `docs/design/blastfile-lsp-zed.md` has the accepted decisions; this node implements them.
- The grammar's `src/parser.c` must be generated and committed: Zed never runs
  `tree-sitter generate`, it clones `[grammars.blastfile]` at `rev` and compiles
  `src/parser.c` with clang. `parser.c`, `src/tree_sitter/parser.h`, `grammar.json` and
  `node-types.json` are the generated files committed; nothing else under `src/` is.
- `npx --yes tree-sitter-cli@0.25 --version` is the generator. Record the exact resolved
  version in `editors/tree-sitter-blastfile/package.json` devDependencies and in the
  extension README so regeneration is reproducible.
- The `wasm32-wasip2` target is required for the Rust build (`rustup target add
  wasm32-wasip2`). It is a build-host prerequisite, not a user requirement; the docs never
  ask a user for it.
- **Language rules:** never mention the retired manifest filename; never describe a PHP
  alternative transport; no grammar or wasm internals on a user page (this node writes no
  user page).

# Acceptance

```console
$ cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 generate
$ cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 test
$ git diff --exit-code -- editors/tree-sitter-blastfile/src
$ test -f editors/tree-sitter-blastfile/src/tree_sitter/parser.h
$ cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 query ../zed/languages/blastfile/highlights.scm test/fixtures/complete.Blastfile
$ cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 query ../zed/languages/blastfile/brackets.scm test/fixtures/complete.Blastfile
$ cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 query ../zed/languages/blastfile/indents.scm test/fixtures/complete.Blastfile
$ cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 query ../zed/languages/blastfile/outline.scm test/fixtures/complete.Blastfile
$ clang -fPIC -shared -Os -Wl,--export=tree_sitter_blastfile -I editors/tree-sitter-blastfile/src editors/tree-sitter-blastfile/src/parser.c -o /tmp/opencode/blastfile.wasm
$ cargo metadata --manifest-path editors/zed/Cargo.toml --no-deps --format-version 1 | grep '"workspace_root":".*/editors/zed"'
$ rustup target add wasm32-wasip2 && cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
$ uv run --no-project python -c "import tomllib,pathlib; [tomllib.loads(pathlib.Path(p).read_text()) for p in ('editors/zed/extension.toml','editors/zed/languages/blastfile/config.toml')]"
$ uv run --no-project python -c "import json,pathlib; json.loads(pathlib.Path('.zed/settings.json').read_text())"
$ uv run --no-project tools/guard.py
```

1. `tree-sitter generate` is idempotent: re-running it leaves `src/` byte-identical
   (`git diff --exit-code`). `parser.c`, `src/tree_sitter/parser.h`, `grammar.json` and
   `node-types.json` are committed.
2. `tree-sitter test` passes a corpus that includes all four traps: `view view GET "/x"`, a
   keyword used as a name (`trait session`, `view session`), a newline after a trailing
   comma, and a multi-line raw `'…'`; plus `$IDENT`, `regex '…'` and a one-line block.
3. Each `.scm` parses against the grammar with no unknown node name; `highlights.scm` uses
   only the 45 intersection captures, with `@variable.special @variable` on `env_var`.
4. `clang` builds the grammar wasm with the exact command Zed uses (`--export=
   tree_sitter_blastfile`); the file is written to `/tmp/opencode`, never committed.
5. `cargo metadata` reports `workspace_root` = `editors/zed`; `cargo build --target
   wasm32-wasip2` produces `editors/zed/target/wasm32-wasip2/debug/blastit_zed.wasm`, and
   the root `target/` timestamp is unchanged.
6. `config.toml` selects the extensionless `Blastfile` by `path_suffixes` (assert the two
   strings literally) and `line_comments = ["# "]`.
7. `editors/zed/.gitignore` keeps `target/`, `extension.wasm` and `grammars/` out;
   `tools/guard.py` passes on every committed path under `editors/**`; `LICENSE` resolves
   to the repo root license.
8. `.zed/settings.json` contains the `format_on_save` line for `Blastfile` and is valid
   JSON.

# Notes

- Start with `docs/research/2026-11-16-zed-blastfile-lsp.md` §4's grammar sketch, then run
  the corpus; the sketch is unverified and will need precedence fixes. Keep the grammar
  small: no SQL grammar, no completion-only rules.
- `scanner.cc` is never compiled by Zed; use only `parser.c` (and a C `scanner.c` if truly
  needed). Do not add an external scanner unless the corpus demands it.
- Highlight test format: use `tree-sitter test` corpus for parse shapes; if the CLI supports
  `test/highlight/`, add capture assertions there. If it does not, keep the `query` runs as
  the capture proof and record the exact command in `RESULT.md`.
- The extension Rust must compile even when `blastit` is absent: the error string is the
  only runtime behaviour in that case, and it is what a user sees once.
- Do not run `cargo` in the root workspace, do not add the extension as a workspace member,
  and do not edit the root `Cargo.toml`/`Cargo.lock`.
