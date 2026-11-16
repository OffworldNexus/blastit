# Result MB.21.3.1

## Summary

Landed the bounded `Blastfile` lexer and the one reader every entry point will use. `blastfile/`
now holds `mod.rs` (the module tree, the three §4.5 budgets and `read`), `tokens.rs` (§5.1's closed
token set, §5.2's `Lexed { tokens, trivia }` with `Token.leading` naming the trivia before a token)
and `lexer.rs` (one byte-indexed pass: no `i += 1` anywhere, `col` in `char`s from 1, `line` from 1,
`MAX_TOKENS`/`MAX_TRIVIA` checked in the one `push`/`push_trivia`). `error.rs` gained the Blastfile
`ManifestError` variants with §18's one-line contract (`Position` = `line`/`col`, `NONE` renders no
prefix), `lib.rs` registers `pub mod blastfile` and re-exports `Position`, and
`scripts/check_manifest_purity.py` (+ its negative self-test) refuses `std::env` under
`crates/blastit-manifest/src/`; `make check` runs it as `manifest-purity`.

17 in-module tests cover every §5.1 token kind, every §5.3 row the lexer owns (and prove the two
parser rows are *lexed* for the parser), CRLF, a comment at EOF, `#` inside a raw string, `$DATABASE_URL`,
`$1`/`$?`, the token/trivia caps, a > 100 KiB one-pass input and a 1 MiB hostile corpus. The new
protected suite `tests/critical_blastfile_lexer.rs` covers R3a's `read` rows: the size cap from
metadata (a mode-`000` over-cap file is `TooLarge`, not `PermissionDenied`), and a FIFO that cannot
block or exceed the cap. `make check` exits 0.

**Read `## Deviations` first** — four measured conflicts with the brief/design, one of them a
sentence the design does not pin (human request `H-011`).

## Deviations (read before reviewing)

### D1. `'don''t'` is **two** tokens, not one — §5.1 and the accepted reference both forbid the escape

The dispatch's MUST-GET-RIGHT says "`'don''t'` style SQL stays one token". §5.1 says a `RawStr` is
"every byte verbatim up to the next `'`; no escapes", and the accepted page says the same in the
developer's words (`docs/reference/blastfile.md:276-277`: "A statement containing a `'` goes in a
`\"…\"` string … `'…'` has no escapes"). Making `''` an escape would invent a rule the design and the
DX contract both deny, and would break §6's `query '…'` production. The lexer therefore emits
`'don'` + `'t'`, and `raw_strings` pins that with the citation in a comment. A developer writes the
SQL inside `"…"`, where `'don''t'` stays one SQL literal and one Blastfile token — which is exactly
what the reference already tells them.

### D2. A control byte is `Unknown`, refused by the parser — §5.1 says so, §5.3 has no row for it

The NODE's `# Context` lists "a control byte" among the lexer's errors, but §5.3's table has no
control-byte sentence and §5.1 assigns "any other byte" to `Unknown`, "carried so the parser can
name it in hex". Inventing a lexer sentence was the only alternative, so the lexer emits
`Unknown(0x01)` (pinned by `control_bytes_are_unknown`) and `MB.21.3.2` refuses it with
`byte 0x1 is not Blastfile syntax` at its position. A **non-ASCII** byte *is* a lexer refusal,
because §5.3 gives that row its own longer sentence (`non_ascii_bytes`).

### D3. `read`'s non-regular-file and unreadable refusals have no sentence in the design (`H-011`)

§4.5 mandates that `read` refuse a non-regular file and §18 lists a position-less refusal family,
but the only pinned sentence in that row is the size one. Implemented as:

* `the file is not a regular file; Blastfile must be a plain file`
* `the file could not be read: <the operating system's sentence, escaped>`

Both are position-less and path-less (the caller prefixes the path exactly once, §18). `H-011` asks
the owner to confirm or re-word them before `MB.21.5.1` documents the reader.

### D4. The `≤ 700 changed lines` budget is exceeded: 9 files, 1 654 lines

Measured: 1 415 new lines (lexer 840 of which 449 are the in-module tests, `mod.rs` 99, `tokens.rs`
83, the protected suite 171, the two scripts 222) plus 239 changed lines in `error.rs`/`lib.rs`/
`Makefile`. Acceptance item 1 itself demands every §5.1 token, every §5.3 row, both caps, a > 100 KiB
input and a 1 MiB hostile corpus *as tests*, and item 4 demands two scripts with a real negative
self-test; those are ~840 lines before a line of production code. Nothing was padded: the 13 error
variants are §18's rows with their docs. A 700-line cap would have forced dropping coverage the same
node requires, so the coverage was kept and the overage is reported instead of hidden.

### D5. Two staging decisions worth a reviewer's eye

* `#![cfg_attr(not(test), allow(dead_code))]` in `blastfile/mod.rs`: the lexer and its types are
  consumed by `MB.21.3.2`'s parser and `MB.21.3.8`'s entry points, so in a non-test build today only
  `read` is reachable. The attribute is `not(test)`-scoped (a test build still lints dead code) and
  carries the comment naming the leaf that deletes it. No lint *level* was weakened.
* `MAX_SOURCE_BYTES`/`MAX_TOKENS`/`MAX_TRIVIA` stay `pub(crate)` per the dispatch; the public docs
  therefore say "1 MiB"/"262144" in words instead of linking a private item (a private intra-doc link
  is a `-D warnings` failure), and the integration test repeats the cap as a literal with a comment
  saying a change to the constant must fail that suite.

## Evidence

### 1. `cargo test -p blastit-manifest --locked --test critical_blastfile_lexer` (exit 0)

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_lexer
running 3 tests
test a_missing_file_and_a_directory_are_refused ... ok
test the_size_cap_is_checked_without_reading ... ok
test a_fifo_cannot_exceed_the_cap ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`the_size_cap_is_checked_without_reading` builds a `set_len(MAX_SOURCE_BYTES + 1)` regular file,
asserts the exact position-less sentence and `TooLarge { bytes: 1048577 }`, repeats it with mode
`000` (still `TooLarge`, never `Unreadable`), and reads a file of exactly the cap. `a_fifo_cannot_
exceed_the_cap` `mkfifo`s, calls `read` on a thread with a 10 s timeout (a regression to `open()`
fails instead of hanging), and asserts `/dev/zero` is refused the same way.

### 2. `cargo test -p blastit-manifest --locked` (exit 0)

```console
test result: ok. 75 passed; 0 failed   # lib (58 before: +17 lexer tests)
test result: ok. 3 passed; 0 failed    # critical_blastfile_lexer (new)
test result: ok. 17 passed; 0 failed   # critical_manifest_011
test result: ok. 23 passed; 0 failed   # critical_manifest_edit
test result: ok. 5 passed; 0 failed    # critical_manifest_edit_executor
test result: ok. 9 passed; 0 failed    # critical_php_manifest
test result: ok. 15 passed; 0 failed   # critical_static_manifest
test result: ok. 6 passed; 0 failed    # docs_fences
test result: ok. 19 passed; 0 failed   # edit_branches
test result: ok. 13 passed; 0 failed   # grants
test result: ok. 20 passed; 0 failed   # mutation_gaps
test result: ok. 39 passed; 0 failed   # parse
test result: ok. 11 passed; 0 failed   # routing
test result: ok. 8 passed; 0 failed    # uid
test result: ok. 29 passed; 0 failed   # validate
test result: ok. 6 passed; 0 failed    # doctests
```

No existing suite changed behaviour: every non-`blastfile` count above equals `MB.21.3.7`'s report,
and `quoted`'s output is byte-identical after being split into `clipped` + quotes.

### 3. `make rs-fmt` (exit 0) and `make rs-lint` (exit 0, `-D warnings`)

```console
$ make rs-fmt
cargo fmt --all -- --check
$ make rs-lint
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Checking blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
    Checking blastit-env v0.2.0 (…/crates/blastit-env)
    Checking blastit v0.2.0 (…/crates/blastit)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.86s
$ make rs-doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.27s
```

`clippy::unwrap_used`/`expect_used`/`panic`/`todo`/`indexing_slicing` are `deny`d at the top of
`blastfile/mod.rs` (R23) and apply to `lexer.rs`/`tokens.rs`; the only `allow` is inside
`#[cfg(test)] mod tests` and in the protected suite's header, exactly as the sibling suites do.

### 4. Acceptance item 1 — every token and every lexer refusal, at its exact `line:col`

```console
$ cargo test -p blastit-manifest --locked --lib -- blastfile::lexer
blastfile::lexer::tests::every_token_kind_is_lexed ... ok
blastfile::lexer::tests::word_rules ... ok
blastfile::lexer::tests::escaped_strings ... ok
blastfile::lexer::tests::raw_strings ... ok
blastfile::lexer::tests::env_names ... ok
blastfile::lexer::tests::carriage_returns ... ok
blastfile::lexer::tests::non_ascii_bytes ... ok
blastfile::lexer::tests::control_bytes_are_unknown ... ok
blastfile::lexer::tests::positions_count_chars ... ok
blastfile::lexer::tests::trivia_is_kept ... ok
blastfile::lexer::tests::comment_at_eof ... ok
blastfile::lexer::tests::empty_input_is_just_eof ... ok
blastfile::lexer::tests::the_parser_rows_are_lexed_for_the_parser ... ok
blastfile::lexer::tests::the_token_cap_trips_while_pushing ... ok
blastfile::lexer::tests::the_trivia_cap_trips_while_pushing ... ok
blastfile::lexer::tests::a_hundred_kibibytes_lex_in_one_pass ... ok
blastfile::lexer::tests::a_megabyte_of_hostile_input_is_refused_by_the_caps ... ok
```

| §5.1 token | Where |
|---|---|
| `Word` (`view`, `products`, `GET`, `2fa`, `a-b`, `a-1`, `x_y`) | `every_token_kind_is_lexed`, `word_rules` |
| `Str` (`\"`, `\\`, `\n`, `\t`, a literal newline) | `escaped_strings` |
| `RawStr` (`#` inside, a newline inside, `''` per D1) | `raw_strings` |
| `Env` (`$DATABASE_URL`, `$A1_B`, `$_X`) | `env_names` |
| `LBrace` `RBrace` `LParen` `RParen` `Comma` `Semi` | `every_token_kind_is_lexed` |
| `Newline` (`\n`, `\r\n` as one token) | `every_token_kind_is_lexed`, `carriage_returns` |
| `Equals`, `Dot` | `every_token_kind_is_lexed`, `the_parser_rows_are_lexed_for_the_parser` |
| `Unknown(byte)` | `every_token_kind_is_lexed` (`@`), `control_bytes_are_unknown` (`0x01`) |
| `Eof` | every test; `empty_input_is_just_eof`, `comment_at_eof` |

| §5.3 row | Owner | Test (exact `line:col`) |
|---|---|---|
| `body = "json"` | parser (§5.1) | `the_parser_rows_are_lexed_for_the_parser` — lexes `Word, Equals, Str, Eof` |
| `postgres "…"` | parser (§5.1) | same — lexes `Word, Str, Eof`, the literal never echoed anywhere |
| `$lower` | lexer | `env_names` — `1:1: `$lower` is not an environment variable name: …` |
| `$?`, `$`, `$1`, `$-`, `$é` | lexer | `env_names` — `1:1: `$` must start an environment variable name, …` |
| `"unterminated` | lexer | `escaped_strings` — `1:1:`/`2:1:`, `… on line 1`/`2`; a trailing `\` too |
| `'unterminated` | lexer | `raw_strings` — `1:1:`/`2:1:`, `… on line 1`/`2` |
| `"\q"` | lexer | `escaped_strings` — `1:2: `\q` is not an escape; the escapes are …` |
| a bare `\r` | lexer | `carriage_returns` — `1:2: carriage return without a newline …` |
| a non-ASCII byte | lexer | `non_ascii_bytes` — `1:1:`/`1:2: byte 0xc3 is not Blastfile syntax; …`; legal inside both strings |
| a `-` after no alphanumeric | lexer | `word_rules` — `1:1:`, `1:3:`, `1:5:` |

`positions_count_chars` proves `col` counts `char`s (`"é" $?` → the `$` is at `1:5`, not byte 6),
and `trivia_is_kept`/`comment_at_eof`/`a_hundred_kibibytes_lex_in_one_pass` rebuild the source from
the spans (tokens + their leading trivia) and assert byte equality — acceptance item 2.

Caps: `the_token_cap_trips_while_pushing` asserts `262145 tokens; Blastfile is limited to 262144 —
split the file` and that exactly `MAX_TOKENS` tokens still lex (`Eof` is exempt, documented in
`push`); `the_trivia_cap_trips_while_pushing` asserts the same for `262145 trivia runs`; the 1 MiB
test asserts `"a ".repeat(1 MiB)` is `TooManyTokens`, `" #x\n".repeat(1 MiB)` is `TooManyTrivia`, an
unterminated 1 MiB string is refused at `1:1`, and 1 MiB of spaces (or of `#`) is **one** trivia
node — the buffer never grows per byte.

### 5. `make check` (exit 0)

```console
$ make check
guard: OK (6 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 12 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… 133 × "test result: ok", 0 × "test result: FAILED"
No issues found                       # docs-check (zensical --strict)
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; …
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed
doc-tone: ok — 28 user-facing page(s), no banned phrase
uv run --no-project scripts/check_manifest_purity.py
manifest-purity: ok — 11 source file(s), no std::env
uv run --no-project scripts/test_manifest_purity.py
manifest-purity: ok — 11 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 11 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
make check exit=0
```

Run once, serially, on the final tree. `ruff check` and `ruff format --check` pass on both scripts
(`uvx --offline`, EXE001 fixed by making them executable like `scripts/platform_matrix.py`).

## Files

* `crates/blastit-manifest/src/blastfile/mod.rs` (new) — module docs, `#![forbid(unsafe_code)]` +
  R23's module-level `#![deny(…)]`, the three §4.5 budgets, `read` (regular file → size → `take(MAX+1)`).
* `crates/blastit-manifest/src/blastfile/tokens.rs` (new) — `TokenKind`, `Token { kind, span, leading }`,
  `TriviaKind`, `Trivia`, `Lexed`.
* `crates/blastit-manifest/src/blastfile/lexer.rs` (new) — `lex` + `Lexer` (`bump`/`advance`/`trivia`/
  `one`/`carriage_return`/`word`/`env`/`quoted`, six `*_run` scans), the caps, and 17 tests.
* `crates/blastit-manifest/src/error.rs` — `Position`, `prefix`, `clipped` (and `quoted` rebuilt on
  it), and 13 Blastfile variants: `Equals`, `QuotedStore`, `EnvName`, `EnvMarker`,
  `UnterminatedString`, `UnterminatedRawString`, `InvalidEscape`, `CarriageReturn`, `NonAsciiByte`,
  `BareHyphen`, `TooLarge`, `NotRegularFile`, `Unreadable`, `TooManyTokens`, `TooManyTrivia`.
* `crates/blastit-manifest/src/lib.rs` — `pub mod blastfile`, `Position` re-export, one doc paragraph.
* `crates/blastit-manifest/tests/critical_blastfile_lexer.rs` (new) — R3a's `read` rows.
* `scripts/check_manifest_purity.py`, `scripts/test_manifest_purity.py` (new) — the `std::env` guard
  and its four self-tests (dirty, clean, empty tree, the real tree).
* `Makefile` — the `manifest-purity` target and one dependency on `check:`.

## Follow-ups

1. **`H-011`** (opened): §4.5 mandates `read`'s non-regular-file and unreadable refusals but pins no
   sentence; confirm or re-word `the file is not a regular file; Blastfile must be a plain file` and
   `the file could not be read: …` before `MB.21.5.1` documents the reader (D3).
2. **`MB.21.3.2`'s `paths` lack `crates/blastit-manifest/src/error.rs`**, so §6.7's ~30 parser
   variants have no owner: this leaf added only §5.3's rows (including the two the parser raises,
   `Equals`/`QuotedStore`). The epic must widen `MB.21.3.2`'s paths or add a leaf, or the parser
   cannot produce §6.7's sentences.
3. **`MB.21.3.2` must re-export `lex`**: `mod.rs` deliberately has no `pub(crate) use lexer::lex;`
   yet (an unused `pub(crate)` re-export is an `unused_imports` failure under `-D warnings`), and the
   `#![cfg_attr(not(test), allow(dead_code))]` line in `mod.rs` is the one to delete when
   `Manifest::from_blastfile_str` lands (D5).
4. **`MB.21.3.8` extends `critical_blastfile_lexer.rs`** with the token-cap and hostile-corpus rows
   once the public parse entry point exists, and owns `blastfile_errors.rs`/`critical_blastfile_errors.rs`
   (R2, R26) — including the 121-`char` clip and the control byte in a path that `clipped` now serves.
5. **§18's `ManifestErrors` rendering is not what the type does yet.** §18 says the list "joins with
   newlines; each error is one line, so a boot's list is N lines, one per problem"; today
   `ManifestErrors`'s `Display` prints `manifest has N error(s)` and a `  - ` prefix per error, pinned
   by `validate.rs`'s two unit tests and `mutation_gaps.rs`/`parse.rs` (acceptance item 3 forbids
   changing them here). `read` returns one position-less `ManifestError` inside that wrapper, so a
   caller that renders `errors()[0]` gets §18's exact line; `MB.21.3.8` (R2/R26) owns the wrapper.
6. **Proposed `LESSONS.md` line** (outside `paths`, so recorded here): a `pub(crate) use` of a staged
   module's entry point fails `-D warnings` as an unused import before its consumer exists, and a
   `pub` doc that links a `pub(crate)` item fails `RUSTDOCFLAGS=-D warnings` as
   `private_intra_doc_links` — so a staged leaf either keeps its re-exports out until the consumer
   lands or spells the budget in words.
7. **Proposed `LESSONS.md` line:** `std::fs::metadata` is the FIFO defence, not `File::open`: an
   `open()` on a FIFO with no writer blocks forever, so a reader that checks `is_file()` *before*
   opening is the only shape a timeout-based test can prove.
