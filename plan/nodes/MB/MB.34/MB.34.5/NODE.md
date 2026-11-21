---
id: MB.34.5
title: "blastit lsp: advertise codeAction, ship the Parameterize SQL Quick Fix and $?/... hover, and grow critical_lsp without losing a case"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.34.4]
paths: [crates/blastit/src/lsp/mod.rs, crates/blastit/src/lsp/hover.rs, crates/blastit/tests/critical_lsp.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T05:02"
---

# Goal

Add the two protocol methods a developer uses on a broken or pasted `query`, and nothing else:
one Quick Fix, one hover, one new capability.

1. **`textDocument/codeAction`.** `capabilities()` (`crates/blastit/src/lsp/mod.rs:743`) gains
   `"codeActionProvider": true` and no other key. The handler returns **at most one**
   `CodeAction`:
   - offered when the request's `range` intersects a `query` string of the tracked document
     whose current diagnostics include `LiteralQuery`, **or** whose text contains a numbered
     placeholder `$1…$n` (a paste from `psql`/logs);
   - `title: "Parameterize SQL for Blastfile ($?)"`, `kind: "quickfix"`;
   - `edit: { "changes": { "<document uri>": [ one TextEdit ] } }` replacing the **whole string
     interior** (from the first character after the opening quote through the character before
     the closing quote) with the rewritten statement — `$?` for each value,
     `IN ($?, ...)` for a list, `(row), ...` for repeated rows — using the same expansion rules
     as `MB.34.3`, implemented on the **manifest side** so the editor and the CLI cannot drift;
   - the rewritten text must itself pass `blastit check` (a test asserts the edit, applies it,
     re-parses, and expects no query diagnostic);
   - no action for `KNOWN_STATIC_SETUP_SQL`, for a redacted `…`, for an unknown document, or
     for a range outside every `query`;
   - `context.diagnostics` is never trusted: the server re-derives from its own tracked text.
2. **Hover on `$?` and `...`.** `hover_at` (whatever the current hover entry point is) keeps
   the word table for every word, and, when the position is inside a `query` string, resolves
   the token under the cursor:
   - `$?` → one paragraph: one bind placeholder per value; `IN ($?, ...)` binds any number of
     values; the wire sees `$1, $2, …`;
   - `...` → one paragraph: only legal as the variadic marker after an `IN` list or a `VALUES`
     row; it means "and more of the same", not SQL;
   - hover on anything else inside the string is `null` (no second grammar in hover).
3. **`critical_lsp.rs` grows; it does not lose a case.** Add: the capability document contains
   `codeActionProvider`; `codeAction` on a literal-bearing query returns exactly one action with
   the exact title/kind and a single edit whose applied result is clean; `codeAction` on a
   clean query returns `[]`; `codeAction` on `SET TIME ZONE 'UTC'` returns `[]`; hover on `$?`
   and `...` returns the two paragraphs; hover on `$?` outside a `query` returns `null`; a
   malformed `codeAction` params body answers the design's error code rather than panicking.

# Context

- **Protocol rules that already apply.** Ids echo verbatim; a notification is never answered;
  an unadvertised capability's request gets `null`; the frame/UTF-16 rules are unchanged
  (`docs/design/blastfile-lsp-zed.md` §2–§4). `codeAction` is a request with an `id`, so it must
  answer or return `null` — never a notification-shaped reply.
- **Position units.** The LSP range is 0-based UTF-16; the string interior is tracked in
  `char`s. `mod.rs` already has `Lines`/`char_index`/`max_point_utf16`; reuse them and pin a
  case with a non-BMP character before a query so a byte/char/UTF-16 mix-up is red.
- **Where the rewrite lives.** `MB.34.3`'s validator is `blastit-sqlpolicy`; the *rewrite* the
  Quick Fix needs (literals → `$?`, `$n` → `$?`, list/row collapse) should be one function in
  `blastit-sqlpolicy` (or in the manifest's query module) called by both. Decide with
  `MB.34.2`; do not reimplement it here as a regex over the string.
- **The document's text is authoritative.** `didOpen`/`didChange` already keep the text and the
  version; a `codeAction` for a stale version must not be computed against a different text.
  If the design requires a version check, return `[]` rather than an edit.
- **Protected file.** `crates/blastit/tests/critical_lsp.rs` is protected; it is listed in
  `paths` for additive cases only. Do not weaken, rename or reorder an existing assertion, and
  do not touch the child/process-arm assertions.
- **Do not** touch `editors/**`, the grammar, `crates/blastit-manifest/**` (except the shared
  rewrite function if `MB.34.2` puts it there — then list the path and say why), or the manifest
  sentence.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit --lib lsp
$ cargo test -p blastit --test critical_lsp
$ cargo clippy -p blastit --all-targets -- -D warnings
```

plus, in `RESULT.md`, a recorded live Zed session (not gated) over a `Blastfile` with

```text
query "SELECT id FROM products WHERE email = $1"
```

(a `"…"` string, so the literal/`$1` paste is the real spelling): the lightbulb title, the edit
applied, and the squiggle gone — with the resolution step that supplied the server (a
`blastit lsp` from `PATH`, the workspace build, or the extension's download).

`codeAction`'s new cases must each fail on a plausible defect: dropping `codeActionProvider` →
the capability test is red; returning an empty array for a literal query → the exact-title test
is red; editing only the literal instead of the interior → the re-parse test is red; trusting
`context.diagnostics` → the clean-query test (no diagnostics, `$1` present) is red.

# Notes

- **Do not add `workspace/applyEdit`.** Returning a `TextEdit` in the response is enough for
  Zed and needs no capability negotiation; a server-side apply is a second authority over the
  buffer.
- **No new message surfaces.** Do not add `codeAction/resolve`, `textDocument/hover` revisions,
  or `diagnostic.code` (design §5.2 keeps diagnostics code-free). If a Quick Fix needs a code,
  use the message and range, not a new field.
- The hover paragraphs are **user-facing sentences**: write them once in `hover.rs` and have
  `MB.34.1`'s page carry the same words; a mismatch is a review finding.
- Keep the handler ≤ 1 screen: `uri` → tracked text/version → range → string token → rewrite.
