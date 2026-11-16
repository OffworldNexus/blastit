# Result MB.21.3.4

## Summary

`blastfile/print.rs` is the canonical printer (§14.2) and `blastfile::fmt` is its one entry point
(§4.4): `parse` → one walk of the arena → canonical text, with `MAX_SOURCE_BYTES` re-checked on the
output (R25). `blastit fmt [--check] [PATH]` is wired into `cli.rs` next to `dev` and `check`, with
`./Blastfile` discovery (`BLASTIT_MANIFEST` honoured), `--check` writing nothing at all, and the
`0`/`1`/`2` table of §14.4.

**Attempt 2 fixed the two blocking findings.**

1. **A list continued after a `,` is no longer left wrapped.** `split_lines` now honours §6.6: a
   `Newline` ends a statement only when the previous token is neither `,` nor `{` and no `(` is
   open, so `select a,` newline `b` is written `select a, b` for `functions`, `select`, `insert`,
   `update` and `delete` alike (and for columns wrapped inside a table grant's parentheses).
   `a_wrapped_grant_list_is_written_on_one_line` and `a_comment_inside_a_grant_list_moves_above_its_statement`
   pin it, and the critical suite's messy document now wraps a `select` list so the end-to-end path
   proves it too.
2. **The corpus is canonical.** `06-unrestricted.blastfile`, `07-body.blastfile` and
   `15-multi-endpoint.blastfile` were reformatted (layout only — see "The corpus" below), and
   `critical_blastfile_print.rs::the_corpus_is_canonical` asserts `fmt(source) == source` for all 15
   fixtures with `CORPUS_SIZE = 15`, so §21's "`fmt --check` clean on those fixtures" is now true and
   a future fixture that drifts fails here.

**P1–P3 hold over the whole corpus, not one file.** P2 (fixed point), P2b (ordered comment texts and
the token each comment precedes) and P3 (`Manifest::equivalent` plus equal warnings) loop every
fixture, and a hand-written document using every production is formatted from a deliberately messy
spelling (`;` separators, no indentation, blank lines inside blocks, one-line endpoint blocks, a
wrapped grant list) and asserted byte-equal to its canonical form.

`make check` exits 0: 161 suites / 3175 tests, 0 failures; `critical-guard` reports **no change to a
protected critical test**.

## The corpus

Three fixtures were reformatted in this leaf; the diff is layout only, and no token changed:

| Fixture | Change |
|---|---|
| `06-unrestricted.blastfile` | one blank line between the two top-level `postgres` sections |
| `07-body.blastfile` | five one-line endpoint blocks expanded to `{` newline `body …` newline `}` |
| `15-multi-endpoint.blastfile` | one one-line endpoint block expanded the same way |

`git diff crates/blastit-manifest/tests/fixtures/` is 19 insertions and 6 deletions across those
three files (`06` +1/−0, `07` +15/−5, `15` +3/−1), all of them whitespace or brace placement. The
parity content is unchanged, and
`cargo test -p blastit-manifest --locked --test critical_blastfile_parity` was re-run to prove it:
20 passed, including `route_lines_carry_the_view_header_line`, which derives the expected line from
the fixture source and so follows the reformat by construction. `blastit fmt --check` over all 15
fixtures exits 0 for each.

## Evidence

All commands were run serially from the worktree at `release/0.2.0` (base `1d0d17b`).

### 1. `cargo test -p blastit-manifest --locked --test critical_blastfile_print` — PASS

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_print
running 9 tests
test a_comment_inside_a_list_is_kept_above_its_statement ... ok
test a_query_with_a_quote ... ok
test p2_fixed_point_over_the_corpus ... ok
test p2_fixed_point_over_every_production ... ok
test p2b_comments_survive ... ok
test p3_over_every_production ... ok
test the_corpus_is_canonical ... ok
test p3_over_the_corpus ... ok
test fmt_refuses_a_file_it_would_grow_past_the_cap ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.47s
```

`p2_fixed_point_over_the_corpus`, `p3_over_the_corpus` and `the_corpus_is_canonical` each assert
`common::CORPUS_SIZE == 15` and `corpus().len() == CORPUS_SIZE` before the loop.

### 2. `cargo test -p blastit-manifest --locked` — PASS (32 suites, 515 tests)

```console
$ cargo test -p blastit-manifest --locked
… (32 `Running`/`Doc-tests` blocks)
$ grep -c '^test result: ok'  → 32
$ grep -c 'test result: FAILED' → 0
$ (sum of passed) → 515
```

### 3. `cargo test -p blastit-manifest --locked --test critical_blastfile_parity` — PASS

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_parity
running 20 tests
test a_view_cannot_carry_a_static_or_php_declaration ... ok
test route_lines_carry_the_view_header_line ... ok
test row_a_multi_endpoint_view ... ok
…
test the_corpus_is_the_fifteen_rows_and_every_one_lowers ... ok

test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

### 4. `make rs-fmt` — PASS

```console
$ make rs-fmt
cargo fmt --all -- --check
EXIT=0
```

### 5. `make rs-lint` — PASS

```console
$ make rs-lint
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s
EXIT=0
```

### 6. `make rs-doc` — PASS

```console
$ make rs-doc
… RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
 Documenting blastit v0.2.0 (…/crates/blastit)
 Documenting blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.15s
   Generated …/target/doc/blastit/index.html and 13 other files
EXIT=0
```

### 7. `make check` — PASS (exit 0)

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (5 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 13 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
…
     Running tests/critical_blastfile_fmt.rs
running 6 tests
… test result: ok. 6 passed; 0 failed; …
     Running tests/fmt_command.rs
running 12 tests
… test result: ok. 12 passed; 0 failed; …
     Running tests/blastfile_fmt.rs
running 7 tests
… test result: ok. 7 passed; 0 failed; …
     Running tests/critical_blastfile_parity.rs
running 20 tests
… test result: ok. 20 passed; 0 failed; …
     Running tests/critical_blastfile_print.rs
running 9 tests
test p2_fixed_point_over_the_corpus ... ok
test the_corpus_is_canonical ... ok
… test result: ok. 9 passed; 0 failed; …
96 passed, 65 subtests passed in 1.18s
49 fences: 28 executed, 21 accounted, 0 failed · 26.1s
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)
doc-tone: ok — 28 user-facing page(s), no banned phrase
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 16 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`grep -c "test result: ok"` = 161, `grep -c "test result: FAILED"` = 0, and the passed counts sum to
3175. The `blastit-view` frame-budget test did not flake on this run.

### 8. The §14.4 exit table, end to end

```console
$ printf 'app "shop"\nview products GET "/products" { body json ; prefork }\n' > Blastfile
$ blastit fmt --check ; echo $?
./Blastfile: not in canonical layout
1
$ blastit fmt ; echo $?
0
$ cat Blastfile
app "shop"

view products GET "/products" {
    body json
    prefork
}
$ blastit fmt --check ; echo $?
0
$ blastit fmt missing ; echo $?
./Blastfile: the file could not be read: No such file or directory (os error 2)
2
$ blastit fmt adir ; echo $?
adir: the file is not a regular file; Blastfile must be a plain file
2
$ BLASTIT_MANIFEST= blastit fmt ; echo $?
blastit fmt: BLASTIT_MANIFEST must not be empty: put ./Blastfile in the working directory or name a file
2
$ for f in crates/blastit-manifest/tests/fixtures/*.blastfile; do blastit fmt --check "$f"; done ; echo $?
0
```

### 9. `./target/debug/blastit fmt --check --help` — PASS (exit 0)

```console
$ ./target/debug/blastit fmt --check --help
usage: blastit fmt [--check] [PATH]

  --check       change nothing; name every file that is not in canonical
                layout and exit 1
  PATH          the file to format (default ./Blastfile, or BLASTIT_MANIFEST)
  -h, --help    print this help

`blastit fmt` rewrites the file in the canonical layout: one directive per line,
four spaces per level, one blank line between sections. Comments are kept, and
without --check the rewrite is atomic and keeps the file's mode.

Exit codes: 0 clean or rewritten, 1 --check found a file that is not canonical,
2 the file could not be read, parsed or written.
EXIT=0
```

## Deviations disclosed, each re-checked against the source

(a) **The P2b *owner* oracle lives in `print.rs`'s module tests, not in the critical suite.**
§20 names `critical_blastfile_print.rs::p2b_comments_survive` with `comment_texts`/`comment_owners`,
but an integration test is a separate crate and `Cst` is `pub(crate)`, so it cannot walk the tree.
The critical suite therefore asserts the ordered comment texts **and** the token each comment
precedes, from a test-local scanner that never touches the lexer (so the two cannot share a bug);
`print.rs::tests::p2b_comment_owners_survive` asserts the design's exact oracle — the kind of the
node each comment is a child of, before and after `fmt` — over the corpus and a commented document.

(b) **A comment that cannot stay on its statement's line is written above the statement.** What the
code does now, per list kind:

| Shape | What happens |
|---|---|
| a comment on its own line above a directive, a section or a grant | stays there, indented to the line it precedes |
| a comment trailing a directive, a block's `{` or a block's `}` | stays on that line, one space before `#` |
| a comment inside a view header list (`view v GET,` newline `# note` newline `POST "/x" {`) | the header is one line, so the comment moves to the line above the `view` |
| a comment inside a grant list (`select a,` newline `# note` newline `b`) | the list is one line, so the comment moves to the line above the `select` |
| a comment inside a table grant's parentheses (`select products(` newline `# note` newline `id)`) | same: above the `select` |
| a comment after the last section | its own line, with no blank line above it |

The move is the only choice that keeps the comment and stays a fixed point: dropping it would fail
P2b, and leaving it as a trailing comment would merge it with any existing one (`# a # b` is one
comment). `a_comment_inside_a_list_is_kept_above_its_statement`,
`a_comment_inside_a_grant_list_moves_above_its_statement` and `a_comment_that_has_no_line_of_its_own_moves_above_its_statement`
assert it, each with the second-pass fixed point.

(c) **A comment after the last section keeps its own line with no blank line above it, and one
between two sections takes the blank line above it.** §14.2 fixes "one blank line between top-level
sections" but says nothing about a comment at end of file; the two rules above are the only ones that
are idempotent (a comment emitted directly under the last `}` re-parses as a *trailing* comment of
that line and would move on the second pass). Asserted by `comments_survive_once` and
`p2b_comments_survive`.

(d) **`fmt` writes LF, so a CRLF file is rewritten.** The lexer treats `\r\n` as one `Newline`
token and the printer emits `\n`; §14.2's "the file ends: one trailing newline" has one spelling.
`a_crlf_file_is_written_with_lf` asserts it, and P3 holds (the model is unchanged).

(e) **`cli.rs` carries a small copy of `error::clipped` (`one_line`/`one_line_text`).**
`blastit-manifest`'s `escape_control`/`clipped` are `pub(crate)` and `error.rs` is not in this
node's `paths`, but §18 requires the path to be escaped and cut at 120 chars so a
`BLASTIT_MANIFEST` holding a newline cannot break the one-line invariant. Follow-up below.

(f) **`report_manifest_errors` decides the `<path>:<line>:<col>` vs `<path>: <message>` spelling by
reading `Display`'s documented prefix.** `ManifestError` exposes no position accessor and `error.rs`
is out of `paths`; §18's two spellings are exactly "starts with `<digits>:<digits>: `" and "does
not", so the caller reads the one rendering it has. Follow-up below.

(g) **`pattern.rs` carries a one-line comment change, and its import order was briefly rewritten.**
`cargo fmt --all -- --check` is part of `make check`; one `cargo fmt --all` run reordered
`use crate::error::{ManifestError, Position, regex_message}` to `{regex_message, ManifestError,
Position}`, which the next `--check` rejected, and a further `cargo fmt --all` restored the committed
order. The tree is left in the state `--check` accepts (verified three times, and by `make rs-fmt`),
and the only content change in `pattern.rs` is the stale `dead_code` comment — `pattern.rs` is inside
this node's `blastfile/**` glob, so both are in scope. The oscillation was not reproduced
deliberately; it is recorded because a reviewer will see the file in the diff.

(h) **`blastfile::fmt` does not re-parse its own output.** R25 requires the size re-check (done) and
§14.3 phrases the parse-back as an assertion, not as a runtime step; `output_reparses` and
`a_query_with_a_quote` assert it over the corpus and over the two string forms, and re-parsing inside
`fmt` would double the cost of every `blastit fmt` for a property the tests already pin.

(i) **The over-cap refusal reuses `ManifestError::TooLarge`, so its sentence names the file's length
rather than the formatted length.** §4.5's sentence is the reader's ("the file is N bytes"), and the
formatted text is not on disk; the number in the message is the formatted length. A dedicated
sentence is a follow-up below.

## Files

Added:

- `crates/blastit-manifest/src/blastfile/print.rs` (canonical layout + 13 module tests)
- `crates/blastit-manifest/tests/blastfile_fmt.rs` (7 tests: P2, P3, reparse, cap, refusal)
- `crates/blastit-manifest/tests/critical_blastfile_print.rs` (9 tests: P2, P2b, P3, the canonical
  corpus, every production)
- `crates/blastit/tests/fmt_command.rs` (12 tests: the §14.4 command table)
- `crates/blastit/tests/critical_blastfile_fmt.rs` (6 tests: the write contract)

Modified:

- `crates/blastit-manifest/src/blastfile/mod.rs` (+3: `mod print`, `pub use print::fmt`)
- `crates/blastit-manifest/src/blastfile/syntax.rs` (+53: `trivia_kind`/`trivia_text`/`trivia_span`,
  the P1-over-the-corpus module test, and the `dead_code` comments the printer's arrival made stale)
- `crates/blastit-manifest/src/blastfile/pattern.rs` (+1/−1: a stale `dead_code` comment; the import
  order is deviation (g))
- `crates/blastit/src/cli.rs` (+244: `Command::Fmt`, `FmtOptions`, `FMT_USAGE`, `parse_fmt`,
  `fmt_path`, `run_fmt`, `report_manifest_errors`, `one_line`, the two exit constants, the usage line)
- `crates/blastit-manifest/tests/fixtures/{06-unrestricted,07-body,15-multi-endpoint}.blastfile`
  (layout only; see "The corpus")

## Follow-ups

- **`ManifestError::position()`** (or a reachable `Position` on a variant) so the CLI stops reading
  `Display`'s prefix to choose between §18's two spellings (deviation f). `error.rs` is out of
  `paths`.
- **Export `error::clipped`/`escape_control`** so `cli.rs` stops carrying `one_line`/`one_line_text`
  (deviation e). `error.rs` is out of `paths`.
- **A dedicated over-cap sentence for the formatted length** (deviation i): §4.5's `TooLarge` text
  describes the file the reader saw, and `fmt`'s refusal is about text that is not on disk yet. A
  variant such as `FormattedTooLarge { bytes }` in `error.rs` would say it; `error.rs` is out of
  `paths`.
- **`fmt`'s discovery is local to `cli.rs`** (`fmt_path`), because `config::manifest_path` is private
  and `config.rs` is not in this node's `paths`. `MB.21.4.1` must move `fmt` onto the one discovery
  function when it rewrites `./Blastfile` discovery for every command; until then `blastit dev` and
  `blastit check` still look for the retired file name while `blastit fmt` looks for `./Blastfile`.
- **The missing-file sentence is still `read`'s** (`the file could not be read: No such file or
  directory (os error 2)`). §15.1 pins
  `./Blastfile: no such file or directory (put a Blastfile in the working directory, or set
  BLASTIT_MANIFEST)`; that sentence is `MB.21.4.1`'s discovery leaf, and `fmt` will inherit it once
  `blastfile::read` produces it.
- **`ManifestErrors`' `Display` is still the retired `manifest has N error(s)` + `\n  - ` join**
  (§18 wants a newline join). `fmt` renders `errors()` itself, one line per refusal, so a developer
  sees the contract; the `Display` fix belongs to a leaf that owns `error.rs` and the suites that pin
  its text (`mutation_gaps.rs`, `parse.rs`, `validate.rs`).
- **`--check` takes one path, so it prints at most one line.** §14.4's table says "per file"; the
  command's grammar is `[--check] [PATH]` (one file), so a multi-file `--check` would be a new flag
  and is not invented here.
- **`cargo fmt --all` reordered `pattern.rs`'s imports once on this host** (deviation g): two
  consecutive `cargo fmt --all` runs disagreed on that one line, and `--check` accepts only one of
  them. Worth a `LESSONS.md` line when someone owns the toolchain pin; the tree is left in the
  accepted state.
