---
id: MB.21.3.1
title: "Bounded Blastfile lexer: tokens, $IDENT, raw and escaped strings, trivia, line:col errors"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.3.7]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/blastfile/mod.rs"
  - "crates/blastit-manifest/src/blastfile/tokens.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_lexer.rs"
  - scripts/check_manifest_purity.py
  - scripts/test_manifest_purity.py
  - Makefile
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T03:56"
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
- Errors are `file:line:col` with one sentence (§5.3's table, verbatim): unterminated escaped string,
  unterminated raw string, invalid escape (`\x`/`\u` forms the grammar does not allow), `$` not
  followed by `[A-Z_]`, a non-ASCII byte outside a string, a lone `\r`, `=` outside a statement, a `-`
  that follows no letter or digit. A control byte is `Unknown`, not a lexer refusal: §5.1 hands it to
  the parser, which names it in hex. A literal where `$VAR` is required is **not** a lexer error
  either — the parser refuses it (both must say `line:col`).
- Boundedness: iterate with `text.char_indices()` (never an index that a mutant can freeze), cap the
  input the caller may hand the lexer, and assert on a 1 MiB hostile input that the lexer finishes in
  one pass without growing a buffer beyond the input.
- Tests: new protected suite `crates/blastit-manifest/tests/critical_blastfile_lexer.rs` — every token
  kind, CRLF and comment-at-EOF, `#` inside a raw string stays literal, `$DATABASE_URL` one token,
  `$1` and `$?` are not `$IDENT`, plus the `read`-cap rows. Note §5.1's rule the briefs once got wrong:
  `'…'` has **no** escapes, so `'don''t'` is two tokens and a statement containing a quote is written
  in `"…"` (which is what `docs/reference/blastfile.md` already tells the developer); a control byte is
  `Unknown`, refused by the parser in `MB.21.3.2` with `byte 0x<hex> is not Blastfile syntax`, not by
  the lexer.
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

1. Every token and every refusal in §5.1/§5.3 is exercised by an **in-module** test asserting the exact
   `line:col` (that is the only place these private items are reachable), including a > 100 KiB input
   that lexes in one pass, a 1 MiB hostile input, and the `MAX_TOKENS`/`MAX_TRIVIA` caps.
   `tests/critical_blastfile_lexer.rs` is created here for the one part an integration test can reach —
   `blastfile::read`'s R3a rows: a regular file of `MAX_SOURCE_BYTES + 1` bytes refused from its size
   without reading it (mode `000` gives `TooLarge`, not `PermissionDenied`), and a FIFO that cannot
   exceed the cap. `MB.21.3.8` extends that suite with the token-cap and hostile-corpus rows once the
   public entry point exists, and owns `blastfile_errors.rs`/`critical_blastfile_errors.rs` with the
   same rule `MB.21.3.2` sets for `critical_blastfile_parser.rs`.
2. Comments and blank lines survive the token stream (a test prints them back unchanged).
3. `make check` exit 0; no existing manifest test changed behaviour.
4. Files: `crates/blastit-manifest/src/blastfile/{mod,lexer,tokens}.rs`, the `lib.rs` module
   registration, the `error.rs` Blastfile variants (§18), `tests/critical_blastfile_lexer.rs` (the
   `read` caps), `scripts/check_manifest_purity.py` with its self-test, and the single `Makefile` line
   that runs it (≤ 9 files, ≤ 700 changed lines).

# Notes

- A `\r\n` file is valid input: `\r` belongs to the newline token, not to the previous line's content.
- Do not resolve `$VAR` here or anywhere later: the lexer emits a name; the environment is the
  builder's business.
