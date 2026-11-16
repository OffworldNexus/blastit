---
id: MB.21.3.1
title: "Bounded Blastfile lexer: tokens, $IDENT, raw and escaped strings, trivia, line:col errors"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.7]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/blastfile/mod.rs"
  - "crates/blastit-manifest/src/blastfile/tokens.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_lexer.rs"
  - "crates/blastit-manifest/tests/blastfile_errors.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_errors.rs"
  - scripts/check_manifest_purity.py
  - scripts/test_manifest_purity.py
  - Makefile
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:41"
---

# Goal

Create the `blastfile/` module tree in `crates/blastit-manifest` and its **bounded lexer**: one pass,
byte-indexed, emitting every token and every piece of trivia with a `line:col` position, so the parser
and the CST can each consume the same stream.

# Context

- Grammar and token rules: `docs/design/blastfile-dsl.md` §5 (tokens, trivia, the budget table) and
  §4.1 (the module tree). Tokens: keywords, bare identifiers, `$IDENT` (`[A-Z_][A-Z0-9_]*`), escaped `"…"`, raw `'…'`, decimal integers, `{`, `}`,
  `,`, `;`, newline, EOF; `#` to end of line is trivia.
- Trivia (comments, spaces, blank lines) is **kept**, not dropped: `MB.21.3.4`'s printer must
  reproduce an untouched file byte for byte. Emit trivia as tokens or as spans with the exact text —
  the design decides, and this leaf implements it as written.
- Errors are `file:line:col` with one sentence: unterminated escaped string, unterminated raw string,
  invalid escape (`\x`/`\u` forms the grammar does not allow), `$` not followed by `[A-Z_]`, a control
  byte, an integer that overflows, a lone `\r`. A literal where `$VAR` is required is **not** a lexer
  error — the parser refuses it (both must say `line:col`).
- Boundedness: iterate with `text.char_indices()` (never an index that a mutant can freeze), cap the
  input the caller may hand the lexer, and assert on a 1 MiB hostile input that the lexer finishes in
  one pass without growing a buffer beyond the input.
- Tests: new protected suite `crates/blastit-manifest/tests/critical_blastfile_lexer.rs` — every token
  kind, CRLF and comment-at-EOF, `#` inside a raw string stays literal, `'don''t'` style SQL stays one
  token, `$DATABASE_URL` one token, `$1` and `$?` are not `$IDENT`, plus the whole deny list above
  asserting the position, not just the message.
- `scripts/check_manifest_purity.py` (with its `scripts/test_manifest_purity.py` self-test) refuses
  `std::env` under `crates/blastit-manifest/src/` (§20 R20/R24); wire it into `make check` with one
  line, which is why `Makefile` is in this leaf's `paths`. It must be green when you report.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_lexer
$ cargo test -p blastit-manifest --locked
$ make rs-fmt
$ make rs-lint
$ make check
```

1. Every token and every refusal in the design's §1 table is exercised by a test that asserts the
   exact `line:col`; the suite includes at least one > 100 KiB input that lexes in one pass.
2. Comments and blank lines survive the token stream (a test prints them back unchanged).
3. `make check` exit 0; no existing manifest test changed behaviour.
4. Files: `crates/blastit-manifest/src/blastfile/{mod,lexer,tokens}.rs`, the `lib.rs` module
   registration, the one new protected suite, `scripts/check_manifest_purity.py` with its self-test, and
   the single `Makefile` line that runs it (≤ 8 files, ≤ 600 changed lines).

# Notes

- A `\r\n` file is valid input: `\r` belongs to the newline token, not to the previous line's content.
- Do not resolve `$VAR` here or anywhere later: the lexer emits a name; the environment is the
  builder's business.
