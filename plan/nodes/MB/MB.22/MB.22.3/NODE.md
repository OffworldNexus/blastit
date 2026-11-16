---
id: MB.22.3
title: "Add structured manifest spans and implement blastit lsp (stdio JSON-RPC) with protected framing/UTF-16/diagnostics/formatting tests"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.22.2]
paths:
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/tests/**"
  - crates/blastit/src/lsp.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/src/main.rs
  - crates/blastit/tests/critical_lsp.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T09:16"
updated: "2026-11-16T09:16"
---

# Goal

Add the structured spans the editor needs and implement `blastit lsp` as a stdio JSON-RPC
2.0 server, with the protected tests that make the protocol impossible to silently regress.

Two halves, one node because the first exists only to serve the second:

1. **Structured spans in `blastit-manifest`.** Add `ManifestError::position() -> Position`
   (one exhaustive `match` beside the enum, so a new variant cannot be forgotten), a
   `message()` that renders the same sentence `Display` does **without** the
   `Blastfile:<line>:<col>: ` prefix, and positions for the non-fatal `ManifestWarning`
   variants (`egress any`, unrestricted `postgres`, unacknowledged `body raw`, unused
   `trait`) wherever the CST has a site. `Position::NONE` only for whole-file refusals,
   with a one-line justification per variant in the code.
2. **`blastit lsp`.** New `crates/blastit/src/lsp.rs`, `Command::Lsp` in
   `crates/blastit/src/cli.rs` + usage line, module registration, and dispatch. Hand-rolled
   `serde_json` + `std::io`, blocking, single-threaded. Full sync
   (`TextDocumentSyncKind::Full = 1`), push diagnostics, whole-document formatting,
   keyword hover. **No new dependency**; `Cargo.toml`/`Cargo.lock` must not change.

Behaviour pinned by `MB.22.2`'s design (read it; this brief only summarises):

- `Content-Length: N\r\n\r\n`, `N` in UTF-8 **bytes**; header and body caps enforced before
  allocation; malformed JSON answers `-32700, id: null` and keeps serving; unknown method
  `-32601`; request before `initialize` `-32002`; notifications are never answered; ids are
  echoed verbatim as `serde_json::Value`; `shutdown` → `null`, `exit` code 0 only after it,
  otherwise 1.
- `initialize` capabilities exactly as the design pins (`positionEncoding: "utf-16"`,
  `textDocumentSync: {"openClose": true, "change": 1, "save": false}`,
  `documentFormattingProvider: true`, `hoverProvider: true`, `serverInfo`).
- `didOpen` stores the text; `didChange` takes the first full `contentChanges[].text`;
  `didClose` drops it and publishes `[]`.
- Diagnostics: `source: "blastit"`, severity 1 for errors and 2 for warnings, message =
  `message()`, `version` echoed, no `code`. Ranges convert 1-based `char` columns to 0-based
  **UTF-16** units by the design's formula; a point/word range, `end` exclusive.
- `textDocument/formatting`: `blastfile::fmt`; equal → `[]`; error → `null`; otherwise one
  edit ending at `max_point_utf16`.
- Hover: bounded static word → markdown table; unknown → `null`.
- stdout carries framed messages only; logs and panics go to stderr; no `unwrap`/`panic!` on
  input; document text never appears in a log. Put `#![deny(clippy::print_stdout)]` at the
  top of `lsp.rs`.

# Context

- `docs/design/blastfile-lsp-zed.md` and `ADR-0013` are the accepted design; the research is
  `docs/research/2026-11-16-zed-blastfile-lsp.md` (§7 protocol, §5 captures, §4 grammar).
- Consumers: `crates/blastit/src/check.rs` and `stage0.rs` already call
  `Manifest::parse_blastfile` and receive warnings; keep their behaviour identical. Do not
  delete or weaken existing tests when adding positions.
- `serde_json = "1.0.154"` and `serde` are already dependencies of `crates/blastit`.
  `blastit-manifest` must not gain one.
- `python/blastit/__main__.py` routes any subcommand through `_blastit.main`, so `lsp` must
  work with no Python capability and no privileges: it must not read `BLASTIT_*`, open
  `/run`, call `Host::binary()` or touch the filesystem.
- `MB.21.4` owned `target/` until it closed. This node starts only after `MB.21` is done; do
  not run `cargo` if the lead has not started you.
- **Language rules:** never mention the retired manifest filename; never describe a PHP
  alternative transport.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked
$ cargo test -p blastit --test critical_lsp --locked
$ cargo test -p blastit --lib --locked
$ cargo clippy -p blastit -p blastit-manifest --all-targets --locked -- -D warnings
$ cargo fmt --all -- --check
$ printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run -q -p blastit -- lsp; test $? -eq 1
$ uv run --no-project tools/plan.py validate
```

`crates/blastit/tests/critical_lsp.rs` (new file, protected) proves, both directions where a
bound exists:

1. **Framing:** stdout starts with the exact `Content-Length: `, the declared length equals
   the UTF-8 body length (feed a non-ASCII diagnostic message), a body is read by length not
   newline, a frame over the body cap is refused without allocating, a header block over its
   cap is refused, and a malformed JSON frame answers `-32700` with `id: null` and the
   server still answers the next request.
2. **Lifecycle:** `initialize` returns the pinned capabilities; `shutdown` returns `null` and
   `exit` produces status 0; EOF without `shutdown` produces status 1; a request before
   `initialize` returns `-32002`; an unknown method returns `-32601`; a notification gets no
   response; numeric and string ids are echoed verbatim.
3. **Sync and diagnostics:** `didOpen` on a `Blastfile` with a syntax error publishes a
   diagnostic at the exact 0-based line and **UTF-16** column (fixture line `# ✓ ok` then
   `body =` proves the unit); `didChange` with a full replacement updates it; `didClose`
   publishes `[]`; each warning variant publishes severity 2 with `source: "blastit"`; a
   message equals the CLI sentence without the location prefix; a validation/SQL error is
   included; `version` is echoed.
4. **Formatting:** canonical input → `[]`; non-canonical input → exactly one edit whose
   `end` is `max_point_utf16` and whose `newText` equals `blastfile::fmt`; broken input →
   `null` (not a JSON-RPC error); the CRLF case behaves as the design decided.
5. **Hover:** a directive word returns markdown; an unknown word returns `null`; the range is
   inside the line.
6. **Hygiene:** stdout contains only framed messages for a session that includes a document
   with a `query '…'`; stderr does not contain that SQL text; a truncated frame does not
   panic.
7. **Span exhaustiveness:** the single `match` in `position()` has no wildcard arm, so a new
   variant cannot compile without a decision. A test asserts a representative syntax error's
   exact site, a validation/SQL error's site, and a whole-file refusal's `NONE`; a second
   test asserts every `ManifestWarning` variant maps to a CST site or to the documented
   `NONE` for that variant.

# Notes

- The add-then-consume order matters: land `position()`/`message()` first, make
  `blastit-manifest` green, then write the server against them.
- Use a helper for `Range` construction and one for UTF-16 conversion; test the helper
  directly with `"# ✓ ok"`, `"é"` and an emoji in a comment, not only through a whole
  document.
- Never parse a `Display` string to recover a position or to strip a prefix; that is the bug
  `position()`/`message()` exist to prevent.
- Keep `lsp.rs` free of business logic: it reads a message, calls the manifest/`fmt` API and
  writes a reply. Hover text is data, not a second grammar.
- A crash is terminal in Zed (no automatic restart). Catch only what can be recovered from;
  never continue in an inconsistent state after a short read.
