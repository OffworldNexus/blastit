# RESULT MB.34.3 — `blastit-sqlpolicy`: the `query` template engine

`status: done`. Branch `release/0.2.0`, base `5f293985cc355b29dbac08421482b8ea84d1c677`.
Attempt 1/3. Nothing was committed, staged, pushed or planned by this leaf.

## What shipped

| File | Change |
|---|---|
| `crates/blastit-sqlpolicy/src/template.rs` | **new** (1433 lines): the tokenizer over the shared `lexer`, the frame walk that deletes `, ...`, the `$?` numbering, the three caps, `check_template`, `parameterize`, `TemplateError`/`TemplateKind`, and 21 unit tests — including the one-statement property loop, which runs the crate-private pass |
| `crates/blastit-sqlpolicy/tests/critical_query_template.rs` | **new, protected** (789 lines, 24 tests): the rewrite table as a public oracle, every refusal with its offset, the UTF-8 offsets, the no-printing oracle, `parameterize` |
| `crates/blastit-sqlpolicy/src/literals.rs` | `+literal_offset(sql) -> Option<u32>` (the `LiteralQuery` site, §4.4) and its unit test |
| `crates/blastit-sqlpolicy/src/lib.rs` | `mod template;` and the re-exports |
| `crates/blastit-sqlpolicy/Cargo.toml` | **untouched** — no dependency was needed (`pg_query`, `serde_json` and `serde` were already declared) |

`normalize.rs` is **byte-for-byte unchanged**: `git diff crates/blastit-sqlpolicy/src/normalize.rs | wc -l` → `0`.

Public surface (the one function the design names, plus the types it returns):

```rust
pub fn check_template(sql: &str) -> Result<(), TemplateError>;
pub fn parameterize(sql: &str) -> Option<String>;              // §13.4.6
pub fn literal_offset(sql: &str) -> Option<u32>;               // literals.rs, §4.4
pub struct TemplateError { /* kind, offset */ }
impl TemplateError { pub fn kind(&self) -> TemplateKind; pub fn offset(&self) -> u32;
                     pub fn too_many_queries() -> Self; }
pub enum TemplateKind { Empty, MultipleStatements, StrayEllipsis, Syntax, TooLong,
                        TooManyParameters, TooManyQueries }
impl TemplateKind { pub fn sentence(&self) -> &'static str; }
pub const MAX_TEMPLATE_CHARS: usize = 65_536;
pub const MAX_TEMPLATE_PARAMS: u32 = 1_024;
pub const MAX_TEMPLATE_PARSES: usize = 4_096;
```

`TemplateError::too_many_queries()` exists because `MAX_TEMPLATE_PARSES` is a per-**file** count
that lives in `check_queries` (§13.4.3's `TooManyQueries` row), not in this crate: MB.34.4 holds
the counter and asks for the refusal. `TemplateKind::sentence()` is the one place the seven
sentences live, so `ManifestError::QueryTemplate { at, kind }` renders them without a second copy.

## The brief corrected from the design (three points the reviewer flagged, plus three I found)

`NODE.md` was written before the design landed; the design is the authority. Points 1–3 are the
reviewer's correction, points 4–6 are further places where the brief and §13.4 disagree and I
followed §13.4.

1. **`TemplateError::Syntax` carries no message** (NODE.md `# Goal` "pg_query's own message,
   clipped and escaped to one line" and `# Notes`' sanitizer). §13.4.3/§13.4.4/R33 forbid it:
   `pg_query`'s message is `syntax error at or near "<lexeme>"` and `<lexeme>` is statement text,
   which can be a value. The type has **no field** that could hold one, so the sentence cannot
   leak by construction; the protected suite asserts it on a statement holding `'a@b.com'` and on
   a bare `s3cr3t` token (`a_refusal_never_renders_the_statement`), and a temporary field was
   added to prove the test reds (below). If MB.34.4's brief still asks for a clipped message, the
   §18.1 row (`QueryTemplate { at, kind }`, rendering `{kind}`) is the authority.
2. **`Syntax`'s offset is the first character that is neither `is_space` nor inside a comment** —
   not "the start of the first `Code` region" (the lexer folds leading whitespace into `Code`) and
   not `pg_query`'s `cursorpos` (not exposed, pinned in §13.4.4). Implemented by taking the start
   of the first **token** the walk emits (whitespace and comments emit no token):
   `query '\nSELECT id FROM'` → `1`, `query 'SELCT id FROM t'` → `0`, `/* c */ SELCT` → `8`.
   A statement of nothing but trivia is `Empty` at `0`, decided before the rewrite.
3. **Rows 3–5 produce the PARSE form** (`IN ($1)`, one `(<row>)`), not the page's wire form
   (`IN ($1, $2, …, $n)`). There is no code path that produces the wire form.
4. **`parameterize` cannot call `check_learnable` on the input.** §13.4.6 says "`None` for any
   statement `check_learnable` refuses", but `check_learnable` refuses **every** `InlineLiteral` —
   which is exactly the Quick Fix's trigger — so a literal reading makes the Quick Fix dead code.
   The design's own fix (security S1) is "reuse the `learn.rs` policy on the **expanded/parsed**
   result", so `parameterize` expands its candidate and gates on
   `check_learnable(expanded)`: that call parses the text and requires exactly one statement (the
   `check_template` gate) **and** refuses `COPY … PROGRAM` and every
   `DANGEROUS_LEARN_FUNCTIONS` call. One parse, two gates. The protected suite proves
   `pg_read_file('…')`, `lo_import('…')`, `dblink('…')` and `COPY … PROGRAM 'sh'` get `None`, and
   that every `Some` result is accepted by `check_template` and is already normalised.
5. **§13.4.6's S3 table row `SET TIME ZONE 'Europe/Paris'` is empirically wrong.** It claims the
   rewrite is refused because "utility statements take no parameters"; `pg_query 6.2.1`'s grammar
   takes a variable parameter in `zone_value` — measured:
   `pg_query::parse("SET TIME ZONE $1")` → `Ok(1)`. So the gate lets it through and
   `parameterize("SET TIME ZONE 'Europe/Paris'")` is `Some("SET TIME ZONE $?")`. The test asserts
   the measured behaviour with a comment; inventing a second refusal list for it would be the
   "second allowlist" ADR-0003 and §17.5 forbid.
6. **The rewrite's proof lives with the pass.** `expand_template` is `pub(crate)` and the
   one-statement property loop runs in `template.rs`'s own tests, so no public name was added
   beyond §13.5 and no test carries a second expansion. (This was a `#[doc(hidden)] pub` seam in
   the first attempt; post-review MINOR 3 below made it crate-private.)

## Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
$ uv run --no-project tools/critical_guard.py --base 5f293985 --node MB.34.3
critical-guard: 5f293985cc355b29dbac08421482b8ea84d1c677..working tree, 27 changed file(s)
node MB.34.3: paths ['crates/blastit-sqlpolicy/src/template.rs', 'crates/blastit-sqlpolicy/src/lib.rs', 'crates/blastit-sqlpolicy/src/literals.rs', 'crates/blastit-sqlpolicy/tests/critical_query_template.rs', 'crates/blastit-sqlpolicy/Cargo.toml']
  no change to a protected critical test
```

```console
$ cargo test -p blastit-sqlpolicy
     Running unittests src/lib.rs (target/debug/build/blastit-sqlpolicy/…/out/blastit_sqlpolicy-…)
running 256 tests
test result: ok. 256 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.79s
     Running tests/critical_learn_policy.rs (…)
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/critical_lexer_boundaries.rs (…)
running 7 tests
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
     Running tests/critical_literals.rs (…)
running 25 tests
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests/critical_policy_surface.rs (…)
running 22 tests
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/critical_query_template.rs (…)
running 24 tests
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s
     Running tests/policy_module_edges.rs (…)
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
     Running tests/policy_module_tables.rs (…)
running 19 tests
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
   Doc-tests blastit_sqlpolicy
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

all doctests ran in 1.30s; merged doctests compilation took 1.29s
```

```console
$ cargo test -p blastit-sqlpolicy --test critical_query_template
     Running tests/critical_query_template.rs (…)
running 24 tests
test a_longer_run_of_dots_is_a_stray_marker_too ... ok
test a_qualified_in_or_values_is_not_the_keyword ... ok
test a_refusal_never_renders_the_statement ... ok
test a_stray_marker_is_refused_at_its_first_dot ... ok
test a_rewrite_never_swallows_a_separator_or_a_second_statement ... ok
test an_empty_query_is_refused_at_zero ... ok
test a_second_statement_is_refused_at_its_first_character ... ok
test a_parse_error_is_refused_at_the_first_non_trivia_code_character ... ok
test a_marker_inside_a_string_or_a_comment_is_not_a_marker ... ok
test an_unterminated_group_holds_no_legal_marker ... ok
test a_two_element_in_list_is_legal_and_a_second_placeholder_before_the_marker_is_not ... ok
test every_sentence_is_the_reference_pages_and_one_line ... ok
test the_first_stray_marker_in_the_text_is_the_one_reported ... ok
test the_literal_offset_is_the_whole_token_including_its_prefix ... ok
test offsets_are_counted_in_characters_and_the_byte_distance_is_asserted ... ok
test the_file_wide_refusal_is_constructible_and_says_the_pages_sentence ... ok
test every_quick_fix_result_passes_the_template_check ... ok
test every_rewrite_row_is_accepted_and_its_documented_expansion_parses ... ok
test every_accepted_fixture_is_accepted ... ok
test the_quick_fix_rewrite_is_canonical ... ok
test the_quick_fix_is_never_offered_where_it_would_widen_a_capability ... ok
test a_statement_over_the_cap_is_refused_not_grown ... ok
test the_quick_fix_is_offered_at_the_cap_and_refused_one_character_over_it ... ok
test the_1025th_placeholder_is_refused_at_its_own_position ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

```console
$ cargo clippy -p blastit-sqlpolicy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.10s
$ cargo fmt -p blastit-sqlpolicy -- --check        # no output, exit 0
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-sqlpolicy --no-deps
 Documenting blastit-sqlpolicy v0.2.0 (…)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.42s
   Generated …/target/doc/blastit_sqlpolicy/index.html
```

`cargo mutants` — see the section below.

## How the offsets and the rewrite are pinned

* **`Syntax`** — `pg_query::parse` runs on the expanded text; **every** `pg_query::Error` variant
  (`Parse`, `Scan`, `Split`, `Conversion`, `Decode`, `InvalidJson`, `InvalidPointer`) maps to the
  same `Syntax` refusal at the first token's offset, so no arm is left to `unwrap` (S13). A
  NUL-bearing statement (`SELECT\u{0}1`) is in `nothing_a_caller_can_write_makes_the_checks_panic`
  and is refused, not panicked on.
* **Rules 6–8 before any deletion**: the walk collects the `, ...` spans and the stray markers on
  the token stream, and only then is the output built, so a code `;` can never be swallowed by a
  rewrite. `IN ($?, ; ...)` is `StrayEllipsis` at the dots (asserted), not a silent second
  statement; `INSERT … VALUES ((SELECT 1; SELECT 2)), ...` is `MultipleStatements` at the second
  `SELECT`.
* **The deleted span** is `[comma.start, dots.end)`, so the whitespace **and the comment** between
  them go with it: `IN ($?, /* ; */ ...)` expands to `IN ($1)` (asserted).
* **`...` is exactly three dots**: a longer run is `StrayEllipsis` everywhere, and so is a marker
  in a function's argument list, a qualified `s.in (…)`, or a list holding two placeholders
  (`IN ($?, $?, ...)` → offset 38). `IN ($?, $?)` with no marker **passes** — the acceptance case.
* **Both bounds are characters**: `chars.len() > MAX_TEMPLATE_CHARS` (the `é`/`🦀` fixtures assert
  a 65 536-character, 262 105-byte input is accepted, so the 4×+8 byte *reservation* never grows),
  and `parameterize` uses `chars().take(MAX+1).count()`. There is **no** output-length refusal and
  **no** second length comparison anywhere: the only `len()` calls left are byte capacities
  (`String::with_capacity`) and statement counts. `MAX_TEMPLATE_PARAMS` counts `$?` **tokens**
  (`u32::checked_add`, refused at the 1025th at its own offset), so no byte/char unit question
  arises there.
* **The property** (`template.rs::tests`' `every_accepted_fixture_reaches_postgresql_as_exactly_one_statement`,
  over the same 29 fixtures the protected suite accepts): every rewrite row, a hand-written `$n`, a
  list of three, a repeated row list, the two protocol statements, `pg_read_file($?)`/`dblink($?)`,
  non-ASCII, and the mixed numbering case — each expanded through the crate-private
  `expand_template` (the pass `check_template` runs) and parsed with `pg_query`: exactly one
  statement each. The protected suite pins the same fixtures' *acceptance* and, for every rewritten
  row, that the text **as written** is not SQL — so an `Ok` there can only be the rewrite's doing.

## Guard proofs — every guard goes red

Script: `/tmp/opencode/mut/run.sh` — each mutant is applied by an exact literal replacement that
**fails if it does not apply**, then `cargo test -p blastit-sqlpolicy --lib --test
critical_query_template` runs, then the file is restored (`diff -q` against the backup, `RESTORED`).
Full log: `/tmp/opencode/mut/all.log`.

| # | Mutation | Red |
|---|---|---|
| M1 | cap counted in **bytes** (`chars.len()` → `sql.len()`) | `the_cap_is_counted_in_characters_not_bytes`, `a_statement_over_the_cap_is_refused_not_grown` |
| M2 | placeholder cap `>` → `>=` | `the_seven_refusals_carry_their_offsets` (`left: Some((TooManyParameters, 4099))`, `right: …4103`) |
| M3 | stray reported at the **last** dot (`min` → `max`) | unit `the_first_stray_marker_is_the_one_reported`; protected `the_first_stray_marker_in_the_text_is_the_one_reported` (`left: (StrayEllipsis, 28)`, `right: (StrayEllipsis, 7)`) |
| M4 | `Syntax` offset always `0` | `the_seven_refusals_carry_their_offsets` (`left: (Syntax, 0)`, `right: (Syntax, 1)`) |
| M5 | the `IN` keyword not required | unit `a_qualified_in_is_not_the_keyword` + `the_first_stray_marker…`; protected `a_qualified_in_or_values_is_not_the_keyword` (`left: (StrayEllipsis, 46)`, `right: … 33`) |
| M6 | second statement reported at the text's first character | unit `the_seven_refusals_carry_their_offsets`; protected `a_second_statement_is_refused_at_its_first_character` (`left: (MultipleStatements, 0)`, `right: … 10`), `a_rewrite_never_swallows_a_separator…` (`… 0` vs `… 37`) |
| M7 | a repeated `VALUES` row ends the row list | `every_expansion_row_is_the_tables_own_text`, `every_accepted_row_parses_to_exactly_one_statement` |
| M8 | the deleted span starts at the marker, not the comma | 3 tests (`left: "INSERT INTO t (a, b) VALUES ($1, $2), "`) |
| M9 | a longer dot run accepted as the marker | `a_longer_run_of_dots_is_a_stray_marker_too` |
| M10 | numbering drops its number (`$N` → `$`) | 7 tests (`left: "… WHERE c = $"`) |
| M11 | `is_space` no longer skipped | 5 tests (`left: Err(StrayEllipsis 34)` vs `Ok(())`, `Some((MultipleStatements, 3))` vs `Some((Empty, 0))`) |
| M12 | the Quick Fix skips `--learn`'s refusal set | `parameterize_offers_nothing_it_may_not` (`left: Some("SELECT pg_read_file($?)")`) |
| M13 | `parameterize` has no cap | unit `parameterize_offers_nothing_it_may_not`; protected `the_quick_fix_is_offered_at_the_cap_and_refused_one_character_over_it` (`left: Some("SELECT $?")`, `right: None`) |
| M14 | `literal_offset` points past the literal | `the_literal_offset_is_the_region_start_in_characters` (`left: Some(11)`, `right: Some(7)`) |
| M15 | `Empty` forgets the trailing separator | **equivalent** — the `stmts.len() == 0` arm reports the same `Empty` at `0` |
| M16 | no `VALUES` row list is ever opened | 3 tests |
| M17 | a comment becomes a token (first code character inside it) | `every_accepted_row_parses…`, `every_expansion_row…` |

The bytes-vs-chars guard, with `left`/`right` visible (M1), exactly as the reviewer asked:

```console
### M1 cap counted in bytes, not characters (chars.len -> sql.len)
---- template::tests::the_cap_is_counted_in_characters_not_bytes stdout ----
test result: FAILED. 252 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.85s
  left: Err(TemplateError { kind: TooLong, offset: 0 })
 right: Ok(())
test template::tests::the_cap_is_counted_in_characters_not_bytes ... FAILED
```

... and the same mutation reds the **protected** oracle on its own
(`cargo test -p blastit-sqlpolicy --test critical_query_template`):

```console
test a_statement_over_the_cap_is_refused_not_grown ... FAILED
---- a_statement_over_the_cap_is_refused_not_grown stdout ----
  left: Err(TemplateError { kind: TooLong, offset: 0 })
 right: Ok(())
test result: FAILED. 22 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

The **no-printing** guard cannot be reached by a one-line mutation because `TemplateError` has no
field to carry a message — that is the point of the design. To prove the test can fail, a
`message: Option<String>` field was added temporarily (populated from `pg_query`'s error in
`check_template`, rendered by `Display`), then reverted byte-for-byte (`diff -q` → `RESTORED`).
The red quotes the **value**, which is exactly why the sentence is fixed:

```console
$ cargo test -p blastit-sqlpolicy --test critical_query_template   # with the temporary field
test a_refusal_never_renders_the_statement ... FAILED
---- a_refusal_never_renders_the_statement stdout ----
thread 'a_refusal_never_renders_the_statement' (3798735) panicked at crates/blastit-sqlpolicy/tests/critical_query_template.rs:493:5:
  left: "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)` -- Invalid statement: unterminated quoted string at or near \"'a@b.com\""
 right: "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`"
test result: FAILED. 23 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

### A note on the shared tree

The reviewer's `make check` saw `1 failed / 252 passed` with `template.rs:387` holding
`sql.len()`: that was **my mutation window** — guard proof M1 was applied to the working tree for
the ~5 s of a test run, and `make check` sampled it. The delivered file has always had
`chars.len()`; that red was the intentional proof, not a defect and not a host flake. The guard
proofs now restore with a `diff -q` check (`RESTORED` in every block above). Delivered tree: 255
unit + 24 protected + 87 other integration + 12 doc tests green, clippy and rustdoc clean (see
the post-review section for the current counts and the two gates another leaf reds).

## `cargo mutants`

```console
$ TMPDIR="$HOME/.cache/blastit-mutants" cargo mutants -p blastit-sqlpolicy \
    --file crates/blastit-sqlpolicy/src/template.rs --no-shuffle --timeout 120
Found 134 mutants to test
ok       Unmutated baseline in 41s build + 6s test
…
134 mutants tested in 17m: 6 missed, 122 caught, 6 unviable
$ wc -l mutants.out/{caught,missed,timeout,unviable}.txt
  122 mutants.out/caught.txt
    6 mutants.out/missed.txt
    0 mutants.out/timeout.txt
    6 mutants.out/unviable.txt
  134 total
```

**122/128 killable mutants caught (95.3 %), 0 timeouts.** The six survivors are equivalent or
deliberately unreachable, each with the reason it cannot be observed — none is an untested path:

| Survivor | Why no test can kill it |
|---|---|
| `281:9 delete match arm 0 in check_template` | §13.4.2 requires the `stmts.len() == 0` arm to be asserted *although unreachable behind rules 3 and 4* (`is_empty` refuses first). Deleting it changes nothing a caller can see. |
| `409:5 is_empty -> false` | Then every empty text falls through to that same `0 => Empty` arm — same kind, same offset `0`. `is_empty` exists so `Empty` does not depend on `pg_query`'s empty-statement handling, not to change the answer. |
| `410:31 match guard last.kind == Kind::Semicolon -> false` | Same as above: with the guard never matching, `is_empty` is `false` and the `0` arm answers. |
| `423:72 < -> <= in second_statement` | At the last index `tokens.get(index + 1)` is `None`, so a trailing `;` yields `None` either way — a glance at the function shows the two comparisons cannot differ (the `<=` case has no token after the separator to point at). |
| `855:24 > -> < in code_token` (the `.5` number guard) | A leading-dot number as one token (`Other`) versus `.` + `5` (`Dot` + `Other`): no rule reads digits or the difference between those two item kinds when neither is a placeholder, a comma or a word, and the output copies characters verbatim, so both tokenisations rewrite identically. |
| `868:28 is_ident_start(*c) -> true in code_token` | Every code token becomes `Kind::Word`; the only readers of a word item compare the token's **text** (`in`, `values` via `slice_is_word`), and a number, operator or lone `$` never spells either. Quoted regions never reach `code_token`. |

The two mutants my first campaign missed for real were fixed rather than excused: the `Empty`
guard is now pinned by single-token statements (`BEGIN`, `COMMIT`, `ROLLBACK` — a one-word
statement is a statement, not an empty `query`), `slice_is_word`'s `&&` by a two-letter function
name (`fn ($?, ...)` is `StrayEllipsis`, not the `IN` keyword), `parameterize`'s cap by an at-cap
`Some` / one-over `None` pair, and `ItemKind`'s unused `Quoted`/`Semicolon` variants were deleted
so there is nothing to mutate.

## `make check`

Run **after the post-review fixes** (the acceptance command the reviewer asked for):

```console
$ make check
guard: OK (30 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 50 changed file(s)
make: *** [Makefile:184: rs-fmt] Error 1
$ cargo fmt --all -- --check | grep '^Diff in'
Diff in …/crates/blastit/src/lsp/hover.rs:296
Diff in …/crates/blastit/src/lsp/mod.rs:597
Diff in …/crates/blastit/src/lsp/mod.rs:642
Diff in …/crates/blastit/src/lsp/mod.rs:714
```

The formatter diff is entirely in `crates/blastit/src/lsp/**` — the **in-flight MB.34.5 work in
this shared worktree** (`git status` shows `M crates/blastit/src/lsp/hover.rs`, `M …/lsp/mod.rs`),
not in any file of this leaf. I did not run `cargo fmt --all` (that would edit another node's
uncommitted work). `cargo fmt -p blastit-sqlpolicy -- --check` exits `0`.

Every other target was then run individually, skipping only `rs-fmt` and `rs-test` (which fails on
the same other-leaf code, below):

```console
$ make guard critical-guard rs-lint rs-doc test-critical py-lint docs-unit docs-check \
       rs-no-python seam-check platform-matrix-check workflow-check \
       packaging-gates-test doc-tone manifest-purity zed-check plan-validate
guard: OK (30 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 50 changed file(s)
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.57s
… RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.76s
--------------------
Ran 25 tests in 4.751s
OK
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and exercised: 4/8 wheels, …
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)
doc-tone: ok — 30 user-facing page(s), no banned phrase
manifest-purity: ok — 17 source file(s), no std::env
manifest-purity self-test: ok — 82 assertions, 37 fixtures proved, 17 real source file(s)
zed-check: OK (47 properties checked)
OK
plan OK
$ echo $?
0
```

`make rs-test` (workspace tests) is the second red, and it too is another leaf's:

```console
test the_lsp_module_is_a_text_tool ... FAILED
thread 'the_lsp_module_is_a_text_tool' panicked at crates/blastit/tests/critical_lsp.rs:1477:13:
lsp must not name "fn parse"
test result: FAILED. 41 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.26s
make: *** [Makefile:193: rs-test] Error 101
```

`crates/blastit/src/lsp/mod.rs` is the file MB.34.5 is editing in this worktree, and its new call
names `fn parse`, which the protected LSP invariant forbids. **Neither red touches
`crates/blastit-sqlpolicy`; both reproduce from `git status` alone and neither is a flake.**
`cargo test -p blastit-sqlpolicy` (256 + 24 + 87 + 12 tests) and every gate above are green.

Earlier in this session `make check` also exited 1 at `docs-test` with
`zipfile.BadZipFile: Bad magic number for file header` while reading the wheel it had just built;
the wheel on disk verified clean (14 members, `testzip()` → `None`), `make docs-test` alone with a
fresh wheel exited 0 (48 fences: 30 executed, 18 accounted, 0 failed), and it never reproduced. The
log shows `Blocking waiting for file lock on package cache` immediately before it, i.e. a
concurrent build in this shared worktree. Recorded, not called a flake.

---

## Post-review MINORs (2026-11-18, second pass)

The reviewer's verdict was PASS with three code MINORs. All three are fixed, each proved red. No
assertion was dropped: what moved between tiers moved with an equivalent in its new home.

### MINOR 1 — a test that skipped on `None` (`tests/critical_query_template.rs`)

`every_quick_fix_result_passes_the_template_check` used `if let Some(new) = parameterize(sql)`, so a
regression that stopped offering the Quick Fix for `IN ($1, $2)` passed silently. It now **asserts**
`Some` and panics with the input:

```rust
        let new = match parameterize(sql) {
            Some(new) => new,
            None => panic!("parameterize refused {sql:?}, so the Quick Fix vanished"),
        };
        assert_eq!(check_template(&new), Ok(()), "for {sql:?} -> {new:?}");
```

**Proof it can fail.** Mutation: `parameterize`'s change test no longer sees a `$n`
(`!tokenize(…).any(Kind::Parameter)` → `true`), so a text with only `$1` gets `None`.

```console
== the FIXED test (asserts Some) under the mutation
test every_quick_fix_result_passes_the_template_check ... FAILED
test the_quick_fix_rewrite_is_canonical ... FAILED
parameterize refused "SELECT id FROM t WHERE id = $1", so the Quick Fix vanished
test result: FAILED. 22 passed; 2 failed; 0 ignored; 0 measured; … finished in 0.03s
== the review's if-let form, same mutation:
test every_quick_fix_result_passes_the_template_check ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.00s
```

Both runs are pasted verbatim from the two runs; the second is why the MINOR was real.

### MINOR 2 — the cap counted after the `Vec<char>` was built (`src/template.rs`)

`analyse` collected the whole statement before comparing its length, so an over-cap input allocated
~4× its bytes before being refused. It now counts **before** the collection, exactly as
`parameterize` already did:

```rust
    if sql
        .chars()
        .take(MAX_TEMPLATE_CHARS.saturating_add(1))
        .count()
        > MAX_TEMPLATE_CHARS
    {
        return Err(TemplateError::new(TemplateKind::TooLong, 0));
    }
    let chars: Vec<char> = sql.chars().collect();
```

The walk stops at the first character past the cap, so the refusal costs `MAX + 1` characters and
no allocation whatever the input's length.

**Proof it can fail.** Mutation: the pre-collect comparison `>` → `>=` in `analyse` only (the same
shape exists in `parameterize`, so the edit is scoped to the text after `fn analyse(`):

```console
### the pre-collect cap in analyse (> -> >=)
test template::tests::the_cap_is_counted_in_characters_not_bytes ... FAILED
  left: Err(TemplateError { kind: TooLong, offset: 0 })
 right: Ok(())
test result: FAILED. 255 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.75s
```

The *ordering* itself is a memory bound, not a behaviour, so no portable test observes it: the
evidence is the two lines above (count → collect) plus the identical shape already in
`parameterize`. The comparison is covered by the mutation above and by the protected
`a_statement_over_the_cap_is_refused_not_grown` (65 536 characters, 131 061 bytes, accepted).

### MINOR 3 — the seam widened the public surface (`src/lib.rs`, `src/template.rs`)

`#[doc(hidden)] pub use template::expand_template;` added a public name beyond §13.5. It is now
`pub(crate)`, the re-export is gone, and the one-statement property loop moved into
`template.rs`'s own `tests` (where the seam is reachable) over the same 29 fixtures the protected
suite accepts. The protected tier keeps a real oracle for the rewrite: every row is accepted, its
documented expansion is exactly one statement, and **the text as written is not SQL** wherever the
row is rewritten — so an `Ok` there can only come from the rewrite.

**Proof it can fail.** Mutation: the crate-private `expand_template` breaks the numbering
(`resume = i + 2` → `i + 1`, so `$1?` is emitted):

```console
### the crate-private seam breaks ($? keeps its '?')
test template::tests::every_accepted_fixture_reaches_postgresql_as_exactly_one_statement ... FAILED
test template::tests::every_expansion_row_is_the_tables_own_text ... FAILED
test template::tests::every_accepted_row_parses_to_exactly_one_statement ... FAILED
  left: Err(TemplateError { kind: Syntax, offset: 0 })
 right: Ok(())
```

… and the protected suite still reds on the same mutation for its own reason
(`every_rewrite_row_is_accepted_and_its_documented_expansion_parses`: the row stops being accepted).

`cargo fmt -p blastit-sqlpolicy -- --check` → 0, `cargo clippy -p blastit-sqlpolicy --all-targets
-- -D warnings` → clean, `RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-sqlpolicy --no-deps` →
clean, `plan.py validate` → plan OK.

`critical_guard.py --node MB.34.3` now reports two violations, and **neither file is mine**:
`crates/blastit-manifest/tests/critical_manifest_011.rs` (the `at` field MB.34.4 adds) and
`crates/blastit/tests/critical_lsp.rs` (MB.34.5's in-flight work). Both are unstaged working-tree
edits made by those leaves in this shared worktree; the guard is scoped to *my* node, so they
cannot be covered by my `paths`, and I neither edited nor staged them. Earlier in this session —
before those edits landed — the same command reported `no change to a protected critical test`,
and this leaf's only protected file (`tests/critical_query_template.rs`) is a **new addition**,
which the guard always allows.

### Re-run acceptance (after the three fixes)

```console
$ cargo test -p blastit-sqlpolicy
running 256 tests
test result: ok. 256 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.79s
     Running tests/critical_learn_policy.rs
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/critical_lexer_boundaries.rs
running 7 tests
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
     Running tests/critical_literals.rs
running 25 tests
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests/critical_policy_surface.rs
running 22 tests
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/critical_query_template.rs
running 24 tests
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
     Running tests/policy_module_edges.rs
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/policy_module_tables.rs
running 19 tests
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests blastit_sqlpolicy
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

```console
$ cargo test -p blastit-sqlpolicy --test critical_query_template
running 24 tests
test a_longer_run_of_dots_is_a_stray_marker_too ... ok
test a_refusal_never_renders_the_statement ... ok
test a_rewrite_never_swallows_a_separator_or_a_second_statement ... ok
test a_two_element_in_list_is_legal_and_a_second_placeholder_before_the_marker_is_not ... ok
test an_empty_query_is_refused_at_zero ... ok
test a_second_statement_is_refused_at_its_first_character ... ok
test a_stray_marker_is_refused_at_its_first_dot ... ok
test an_unterminated_group_holds_no_legal_marker ... ok
test a_marker_inside_a_string_or_a_comment_is_not_a_marker ... ok
test a_qualified_in_or_values_is_not_the_keyword ... ok
test every_sentence_is_the_reference_pages_and_one_line ... ok
test a_parse_error_is_refused_at_the_first_non_trivia_code_character ... ok
test the_file_wide_refusal_is_constructible_and_says_the_pages_sentence ... ok
test the_first_stray_marker_in_the_text_is_the_one_reported ... ok
test offsets_are_counted_in_characters_and_the_byte_distance_is_asserted ... ok
test the_literal_offset_is_the_whole_token_including_its_prefix ... ok
test every_accepted_fixture_is_accepted ... ok
test every_quick_fix_result_passes_the_template_check ... ok
test every_rewrite_row_is_accepted_and_its_documented_expansion_parses ... ok
test a_statement_over_the_cap_is_refused_not_grown ... ok
test the_quick_fix_rewrite_is_canonical ... ok
test the_quick_fix_is_never_offered_where_it_would_widen_a_capability ... ok
test the_1025th_placeholder_is_refused_at_its_own_position ... ok
test the_quick_fix_is_offered_at_the_cap_and_refused_one_character_over_it ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

`make check` and the individual gates: see the `## make check` section above (two reds, both in
`crates/blastit/src/lsp/**`, i.e. the other leaf's in-flight edits; every `blastit-sqlpolicy` gate
green).

### Still recorded as design-only notes (not my edits)

* §13.4.2's "4 × `MAX_TEMPLATE_CHARS` + 8" is a byte *reservation*, and R32 still phrases it as a
  refusal threshold; the code has no output-length refusal, which is what the section's "Length"
  paragraph says.
* §13.4.6's S3 table row for `SET TIME ZONE 'Europe/Paris'` contradicts the shipped grammar
  (`pg_query::parse("SET TIME ZONE $1")` → `Ok(1)`); the test asserts the measured behaviour and
  follow-up 3 records the choice.
* §20 R31 places "the one-statement property" in the critical tier; it is now a unit test
  (MINOR 3 above), and follow-up 4 records it.

## Follow-ups (out of this leaf's paths)

1. **MB.34.4 consumes this surface**: `error.kind()` + `error.offset()` for
   `ManifestError::QueryTemplate { at: site.position_at(offset), kind }`; `TemplateKind::sentence()`
   for its `Display`; `TemplateError::too_many_queries()` for the 4097th distinct entry, whose
   count lives in `check_queries` (`MAX_TEMPLATE_PARSES`); `literal_offset(raw)` for
   `LiteralQuery`'s `at`. The three sentences `QueryTemplate` renders are already the page's own.
2. **The epic's prose still says "PostgreSQL 18"** in `plan/nodes/MB/MB.34/NODE.md` and the
   `MB.34.4` brief; `pg_query 6.2.1` vendors **17.7** (`PG_VERSION_STR` at
   `…/pg_query-6.2.1/libpg_query/src/postgres/include/pg_config.h:639`), which §13.4.1 records.
   The epic owner's Follow-up 4 covers it.
3. **`parameterize` and `SET … $n`**: §13.4.6's S3 table claims a utility statement cannot take a
   parameter; the shipped grammar accepts it (`pg_query::parse("SET TIME ZONE $1")` → `Ok(1)`).
   The Quick Fix therefore offers `SET TIME ZONE $?`. If the owner wants that refused, it needs a
   *sentence* on the reference page first — a silent second list is what ADR-0003 forbids.
4. **§20 R31 says the critical tier holds "the one-statement property"; it now lives in
   `template.rs`'s unit tests**, because the rewritten text is only reachable through a
   `pub(crate)` seam and the reviewer's MINOR 3 (above) chose that over widening §4.4. The
   protected tier keeps a real oracle for the same rows: accepted, the documented expansion is one
   statement, and the text as written is *not* SQL wherever the row is rewritten. `docs/design/` is
   not in this leaf's paths, so the §20 row is noted here rather than edited.
5. **`normalize`'s `IN` rule and this one now share four constants and diverge on purpose**: the
   walk here deletes `, ...`, `normalize` rewrites the list. Both call `lexer::code_token_end`,
   `placeholder_end` and `is_space`, so a future lexer change reaches both; there is no second
   scanner to drift.
6. **`docs/knowledge/LESSONS.md` is outside this leaf's paths**, so two facts are recorded here for
   whoever owns it: (a) `pg_query 6.2.1`'s `parse` is the **raw** parser — it performs no parse
   analysis, so `SELECT $1`, `SELECT $1 FROM t` and `SET TIME ZONE $1` all return `Ok`, and a check
   built on it can never rely on type resolution or function resolution; (b) a mutation proof run
   as `sed -i 's#…#…#'` matches nothing when the pattern holds an unescaped `*` `(` `)` (BRE reads
   `(*` as a repetition) and the tree silently stays unmutated — replace with a literal
   string-replace that *asserts exactly one match*, or the "red" you keep is a green run.

