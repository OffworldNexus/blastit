---
id: MB.22
title: "Ship blastit lsp and a ready-to-install Zed extension for Blastfile (highlighting, diagnostics, formatting)"
kind: epic
status: ready
priority: 1
depends_on: [MB.21.5.2]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T09:16"
updated: "2026-11-17T00:47"
---

# Goal

Ship `blastit lsp` inside the `blastit` binary and a ready-to-install Zed extension in
`editors/zed/`, so a developer who opens `./Blastfile` gets colours, live errors and
format-on-save with **zero extra servers and zero project config beyond one JSON line**.
`blastit lsp` is a pure `serde_json` + `std::io` stdio JSON-RPC 2.0 server: no new dependency,
no async runtime, no LSP crate, no external process.

Four deliverables, all mandatory for `0.2.0`:

1. **Syntax highlighting.** `path_suffixes = ["Blastfile", "blastfile"]`, an in-repo
   `tree-sitter-blastfile` grammar with a **committed** `src/parser.c`, and `highlights.scm`
   using the 45-capture theme intersection (`@variable.special @variable` on `$IDENT`),
   plus `brackets.scm`, `indents.scm` and `outline.scm`.
2. **Static checking.** `textDocument/publishDiagnostics` on open/change/close
   (`TextDocumentSyncKind::Full = 1`) from structured `ManifestError::position()` spans,
   validation/SQL errors and boot warnings (`egress any`, `postgres unrestricted`,
   unacknowledged `body raw`, unused `trait`), with exact **UTF-16** columns.
3. **Formatting.** `textDocument/formatting` delegates to `blastfile::fmt`'s lossless CST
   printer; a committed `.zed/settings.json` plus one documented `format_on_save` line.
   Keyword hover ships in v1; go-to-definition only if a bounded `blastfile::symbols()`
   already exists.
4. **Workspace and guard hygiene.** `editors/zed/Cargo.toml` declares its own `[workspace]`
   (`zed_extension_api = "0.7.0"`, Apache-2.0, `wasm32-wasip2`), so the root offline Rust
   workspace and lockfile are untouched, and `tools/guard.py` passes on every committed file
   under `editors/**`.

# Context

- **The verified research is the source of truth:** `docs/research/2026-11-16-zed-blastfile-lsp.md`.
  Every trap in the briefs (byte-strict framing, UTF-16 positions, `format_on_save` default
  `off`, committed `parser.c`, `wasm32-wasip2`, `LICENSE` inside the extension dir, empty
  `[workspace]`, `path_suffixes` equality on the extensionless filename) is already checked
  there. Do not re-derive it, and do not contradict it without a new measurement.
- **Depends on `MB.21`.** The Blastfile DSL, parser, lossless CST, `blastit fmt` and the
  Blastfile pages must exist first. `MB.21.4` owns `target/` until it closes: no `MB.22`
  task may run `cargo` before then; the epic is blocked, not "mostly ready".
- **Existing code sites:** `crates/blastit-manifest/src/error.rs` (`Position`,
  `ManifestError`, `ManifestWarning`), `crates/blastit-manifest/src/blastfile/print.rs`
  (`pub fn fmt`), `crates/blastit/src/cli.rs` (`Command`), and the Python console script
  (`python/blastit/__main__.py`) which already routes `_blastit.main(["lsp"], …)`.
- **No new dependency:** `serde_json = "1.0.154"` and `serde` are already in
  `crates/blastit/Cargo.toml`. Root `Cargo.toml`/`Cargo.lock` must stay byte-identical.
- **Language rules for every page and brief here:** never mention the retired manifest
  filename; never describe a PHP alternative transport in any sentence; no kernel or
  grammar internals on a user page.
- **Out of scope:** completion, `textDocument/definition` (unless the bounded symbol API
  exists), SQL injection into `query '…'`, VS Code/Helix packaging beyond a 3-line client
  snippet, and the actual push/PR to `zed-industries/extensions` (human-only).

# Acceptance

```console
$ uv run --no-project tools/plan.py validate
$ uv run --no-project tools/plan.py tree --under MB.22
$ make check
$ make docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ uv run --no-project tools/guard.py
$ cargo metadata --manifest-path editors/zed/Cargo.toml --no-deps --format-version 1 | grep '"workspace_root":".*/editors/zed"'
$ git cat-file -e "$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1):editors/tree-sitter-blastfile/src/parser.c"
$ printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | target/debug/blastit lsp; test $? -eq 1  # EOF without shutdown exits 1
```

1. Every child `RESULT.md` exists and every `# Acceptance` command in it is recorded as run.
2. On Zed ≥ 0.205, `zed: install dev extension` on `editors/zed` yields colours, a squiggle
   on `body = json` with the same sentence `blastit check` prints, and format-on-save after
   the documented line. This one step needs a human/visual pass; if it cannot be automated,
   `MB.22.5` records the manual evidence and the lead opens the human request.
3. `blastit lsp` works from the standalone (Python-free) `target/debug/blastit` **and**
   through `uv run blastit lsp`, with `shutdown` → `exit 0` and no document text on stderr.
4. `make check` and `tools/guard.py` are green; root `Cargo.toml`, `Cargo.lock` and
   `target/` are untouched by this epic.
5. The docs are shipped and both DX critics pass; the new pages mention neither the retired
   manifest filename nor a PHP alternative transport.
6. `docs/testing/AUDIT-MB.22.md` exists, with every open gap filed as a follow-up node.

# Notes

- **Order:** `22.1` docs → `22.2` design → `22.3` server + `22.4` extension (disjoint paths,
  parallel-safe) → `22.5` pin/verify → `22.6` audit. Do not start `22.2` before `22.1` is
  accepted, and do not start any code before `22.2` is accepted.
- **The grammar `rev` pin is a lead commit:** `[grammars.blastfile].rev` must be a 40-hex
  commit that contains `editors/tree-sitter-blastfile`. Leaf workers never commit, so
  `MB.22.5` pins the SHA of the lead's `MB.22.4` commit. Never ship a branch name, `HEAD`,
  a tag or a placeholder rev.
- **`target/` discipline:** everything under `editors/zed/target/`, `extension.wasm` and
  `grammars/` is generated and gitignored. Only the grammar's `parser.c`, `grammar.json` and
  `node-types.json` are committed generated files.
- A crash in `blastit lsp` reads as "language server is not running" until the user restarts
  it (Zed does not auto-restart), so one malformed frame must never end the loop.
