# Result MB.21.3.2

## Summary

Landed the LL(1) parser and the one tree. `blastfile/syntax.rs` is the §14.1 arena — `Cst`,
`Element::{Node,Token,Trivia}`, §6.4's 23 `NodeKind`s, the typed accessors that return node ids, and
`print_lossless` — and `blastfile/parser.rs` is `pub(crate) fn parse(&str) -> Result<Cst, ManifestErrors>`:
§6.1's grammar in §6.5's ten functions plus the three leaf helpers, one token of lookahead, §6.3's
three-way `view` decision and its two reserved words, §6.6's newline/`;` table, and §4.5's
`MAX_BLOCK_DEPTH`/`MAX_LIST_ITEMS` in the depth counter and every list loop. `error.rs` gained the
parser's variants on `Position`, §6.7's sentences verbatim.

39 in-module tests (32 in `parser.rs`, 7 in `syntax.rs`): 16 production tests (each asserting the node kinds it built), the §6.7 refusal
matrix with the exact message **and** `line:col` (plus six individually named rows), both budgets, a
256 KiB document, a hostile `{{{{…`, and P1 over the whole grammar. No lowering, no `Manifest`, no
public API, no new dependency.

Attempt 2 closes the review's findings: §6.3's `` `get` is not a method `` sentence is what a lower-case
word in a view block now gets (D12, with §6.7 row 3 keeping the keyword case), the duplicate rows are
proved with the first declaration on line 3 as well as line 1 for all five sections, the arena walk
asserts exact counts, the evidence arithmetic is corrected, and the two lead rulings are recorded
(D10, D11).

## Deviations (read before reviewing)

### D1. One catch-all sentence, because §6.7 has no row for a token that is not a word (`H-012`)

§6.7 pins a sentence for every *word* in the wrong place, and §5.3/§5.1/§8.3 pin the `=`, the `.` and
an unknown byte. A total parser must also refuse a token that is not a word at all — `"x"` where a
section keyword belongs, `{` where a value belongs, `postgres` with no `$IDENT`/name/block, a newline
where `{` is required. §18 says the new variants are exactly the rows of §5.3/§6.7/§8.3/§11.3/§12/§9.1,
so no sentence covers that residue, and §6.7's slots must hold a name or a number (§18 S2), which a
string literal is not. Implemented as exactly one variant:

```text
<l>:<c>: expected <what the grammar allows>, found <token class | `word` | `$NAME` | punctuation>
```

`found` is a fixed description (a class name, a punctuation spelling, or a `Word`/`$IDENT` as written)
and a `Str`/`RawStr` is named by its class, so no value that could be a credential is echoed — pinned by
`a_refusal_never_echoes_a_value`. `H-012` asks the owner to confirm or re-word it and to add the row to
§6.7 before `MB.21.3.8`'s error suite (R2/R26) pins it.

### D2. The two `regex` sentences are both used, each where it fits

§6.1's `pattern → Str | 'regex' RawStr` makes an escaped string after `regex` a grammar violation. The
design pins two sentences for it: §8.3's `` `regex` takes a raw string: write `regex '/v[0-9]+/items'` ``
for `regex "…"`, and §6.3's reserved-word row `` `regex` must be followed by a raw string: `regex
'/v[0-9]+/items'` `` for any other token after `regex` (`regex foo`, `regex` then a newline). Both are
implemented, split on the token that follows, so each pinned sentence has a test and neither is dead.
Only the parser can see which quote was used, so `MB.21.3.3` keeps every other §8.3 row (they are about
the pattern's text, which `pattern.rs` receives). A bare `'…'` pattern is not in the grammar either and
is refused with the catch-all.

### D3. `use <name>` of an unknown trait is not a parse refusal

§11.2: "A trait may be referenced before it is written, so `use` is resolved **after** step 3 of §7.3 has
built the trait map" — a one-pass parser cannot know the trait set yet. The existing `UnknownGroup` with
the view's location is `MB.21.3.8`'s (`lower`/`validate`), as §11.3's table says. The CST keeps every
`Use` node and its names, so nothing is lost.

### D4. `postgres unrestricted` inside a `trait` parses

§11.3 says that shape is an error (`UnrestrictedGroup`, validate's), not a parse refusal, and §6.1's
`store_grant` allows it wherever a grant is allowed. The parser accepts it and `validate` refuses it, so
the two leaves cannot disagree. `egress any` and every other non-`postgres` directive inside a trait are
§6.7 rows 13/14 and are refused at parse time.

### D5. An empty `entrypoint python` block and a second `preload` are accepted

§6.1's `python_block → '{' 'preload' Str (',' Str)* '}'` requires exactly one `preload`, but §6.7 pins no
sentence for an empty block or for a second one, and §14.2's empty-block rule names only a trait. Rather
than invent two more sentences, the parser accepts both and `lower` sees the list as written (empty, or
concatenated). Recorded here rather than hidden; `H-012`'s answer may pin them.

### D6. A third block is unreachable through the grammar, so the counter is proved directly

The deepest reachable nesting is two: an endpoint block takes `body` only and a grant block takes grants,
so `view { GET "…" { postgres { … } } }` is §6.7's "a grant is per view" before any third `{`. The
counter is still §4.5's defence in depth and is proved by driving `open_block` three times in
`a_third_block_is_refused_at_its_brace`; the hostile shapes a developer can actually write (`{{{{…`,
`view v { view v { …`, `view v { postgres { postgres { …`) are refused by their own sentences without
recursion, and `a_wall_of_braces_is_refused_without_recursion` pins that.

### D7. The `≤ 600 changed lines` budget is exceeded: 3 320 lines

Measured: `error.rs` +415, `mod.rs` +20/−4, `syntax.rs` 762 (514 production, 248 tests), `parser.rs`
2 119 (1 304 production, 815 tests) — 2 253 production and 1 067 test lines. Acceptance item 1 demands 21
productions as tests, item 2 demands 32 §6.7 rows with exact `line:col`, and §18 demands one documented
variant per row; that is ~1 000 lines before the arena. Nothing was padded and nothing required was
dropped, so the overage is reported instead of hidden (as `MB.21.3.1`'s D4 did).

### D8. `#![cfg_attr(not(test), allow(dead_code))]` stays in `blastfile/mod.rs`

`parse`, `Cst`, the accessors and `print_lossless` have no non-test caller until
`Manifest::from_blastfile_str` lands in `MB.21.3.8`, so a non-test build lints them as dead code. The
attribute is `not(test)`-scoped (a test build still lints dead code) and its comment names the leaf that
deletes it. No lint level was weakened.

### D9. `line:col` is computed lazily, because the first draft was O(n²)

`position_of` counts newlines from byte 0, so calling it per section (to name a duplicate's first line)
made a 256 KiB file of ~6 700 views take **15.6 s** in a debug build. Every per-item site now stores the
byte offset and converts to a `Position` only on the error path; the same test runs in 0.16 s. The
`line_of`/`position_of` pair is the only O(offset) work left and every call is on a refusal path.

### D10. `BlockTooDeep` keeps §4.5's long sentence (lead ruling)

§4.5's budget table pins `` blocks nest two deep at most: a view's `postgres` block is the deepest ``;
§6.7 row 26 gives the short `blocks nest two deep at most`. §4.5 owns the budget rows, so the long
sentence is what a developer sees (and what `a_third_block_is_refused_at_its_brace` asserts). The
inconsistency between the two rows is recorded as an epic follow-up, not silently resolved.

### D11. `regex` is reserved in a pattern list only (lead ruling)

§6.3's prose says the two reserved words "cannot be a view, trait or store name", but its own table gives
each word one **Where**: `unrestricted` in `postgres <name>`, `regex` in a pattern list. The table wins:
`unrestricted` is refused only where a store name would appear, and `regex` is an ordinary view, trait or
store name everywhere else — pinned by `regex_is_reserved_in_a_pattern_list_only` (`view regex "/x" { }`
and `postgres regex $URL` both parse). §6.3's prose is the sentence an epic follow-up should correct.

### D12. §6.3's one bit decides which refusal a lower-case word gets (review finding 1)

Inside a view block the method-vs-directive decision is one bit: a `Word` starting `A`–`Z` is a `Method`
(§4.2, §6.3); everything else goes to the directive dispatch. §6.3 then pins `get "/x"` →
`` `get` is not a method: methods are upper-case, `GET` ``, while §6.7 row 3 pins `fallback` in a view →
`` `fallback` is not a directive here; … ``. Both cannot be the answer for every lower-case word, so the
dispatch asks one more question of the **same** token: is the word a keyword of the language anywhere
(`is_keyword`, §6.1's closed set plus `regex`)? A keyword (`fallback`, `query`, `root`, …) gets row 3
with the view's list, which now names the endpoint form too (`` … and `METHOD "pattern"` ``) so the fix
is visible; a word that is no keyword at all (`get`, `post`, `Fetch`) can only be a method attempt and
gets §6.3's sentence. `every_refusal_row` pins both (`fallback` and `get`), and the decision still reads
one token: the word's own text, never a second token.

## Evidence

### 1. `cargo test -p blastit-manifest --locked blastfile::parser` (exit 0)

```console
$ cargo test -p blastit-manifest --locked blastfile::parser
test blastfile::parser::tests::a_body_on_a_static_mount_is_refused ... ok
test blastfile::parser::tests::a_comma_first_is_refused_at_the_comma ... ok
test blastfile::parser::tests::a_duplicate_section_names_the_first_line ... ok
test blastfile::parser::tests::a_grant_names_a_store ... ok
test blastfile::parser::tests::a_list_holds_at_most_256_items ... ok
test blastfile::parser::tests::a_quarter_megabyte_parses ... ok
test blastfile::parser::tests::a_query_outside_a_grant_block_is_refused ... ok
test blastfile::parser::tests::a_refusal_never_echoes_a_value ... ok
test blastfile::parser::tests::a_semicolon_does_not_continue_a_statement ... ok
test blastfile::parser::tests::a_third_block_is_refused_at_its_brace ... ok
test blastfile::parser::tests::a_trailing_comma_is_refused_at_the_comma ... ok
test blastfile::parser::tests::a_wall_of_braces_is_refused_without_recursion ... ok
test blastfile::parser::tests::an_empty_file_is_just_the_root ... ok
test blastfile::parser::tests::an_unclosed_block_names_its_opening_line ... ok
test blastfile::parser::tests::app_parses ... ok
test blastfile::parser::tests::body_forms_parse ... ok
test blastfile::parser::tests::entrypoint_php_parses ... ok
test blastfile::parser::tests::entrypoint_python_parses ... ok
test blastfile::parser::tests::every_refusal_row ... ok
test blastfile::parser::tests::grant_block_parses ... ok
test blastfile::parser::tests::newlines_and_semicolons_continue ... ok
test blastfile::parser::tests::pattern_forms_parse ... ok
test blastfile::parser::tests::regex_is_reserved_in_a_pattern_list_only ... ok
test blastfile::parser::tests::static_parses ... ok
test blastfile::parser::tests::store_block_parses ... ok
test blastfile::parser::tests::store_named_parses ... ok
test blastfile::parser::tests::store_url_parses ... ok
test blastfile::parser::tests::the_size_cap_is_checked_before_lexing ... ok
test blastfile::parser::tests::trait_parses ... ok
test blastfile::parser::tests::use_egress_and_prefork_parse ... ok
test blastfile::parser::tests::view_block_parses ... ok
test blastfile::parser::tests::view_header_parses ... ok

test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 82 filtered out; finished in 0.08s
```

`every_refusal_row` is §6.7's table verbatim: 40 cases, each `(document, "<l>:<c>: <sentence>")`, and the
five rows the brief calls out (`a_comma_first…`, `a_trailing_comma…`, `an_unclosed_block…`,
`a_third_block…`, `a_duplicate_section…`) have their own tests with a second document each. The
`a_duplicate_section…` test now carries every section twice, once with the first declaration on line 1
and once on line 3, so the `first` slot and `line_of` are both proved. `every_element_is_visited_once`
asserts the walk's token, trivia and node counts **equal** the arena's own (`cst.tokens.len()`,
`cst.trivia.len()`, the `Node` elements), so a duplicated or dropped element fails even if the
concatenation happened to stay byte-equal. The `syntax.rs` module tests
(`p1_lossless`, `p1_lossless_over_every_form`, `every_element_is_visited_once`,
`the_root_is_the_file_node`, `node_text_spans_the_subtree`, `typed_accessors_walk_one_kind`,
`a_node_without_children_spans_nothing`) run under the same filter's parent path and are green in the
full run below.

### 2. `cargo test -p blastit-manifest --locked` (exit 0)

```console
$ cargo test -p blastit-manifest --locked
test result: ok. 114 passed; 0 failed   # lib (75 before: +39 blastfile parser/syntax tests)
test result: ok. 3 passed; 0 failed    # critical_blastfile_lexer
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

Every non-`blastfile` count equals `MB.21.3.1`'s report, so the TOML path is byte-for-byte unchanged
(acceptance item 4).

### 3. `make rs-fmt` (exit 0)

```console
$ make rs-fmt
cargo fmt --all -- --check
```

### 4. `make rs-lint` (exit 0, `-D warnings`) and `make rs-doc` (exit 0)

```console
$ make rs-lint
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Checking blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
    Checking blastit-env v0.2.0 (…/crates/blastit-env)
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Checking blastit v0.2.0 (…/crates/blastit)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.21s
$ make rs-doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
    Documenting blastit v0.2.0 (…/crates/blastit)
    Documenting blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.65s
   Generated …/target/doc/blastit/index.html and 13 other files
```

`clippy::unwrap_used`/`expect_used`/`panic`/`todo`/`indexing_slicing` are `deny`d at the top of
`blastfile/mod.rs` (R23) and apply to `syntax.rs`/`parser.rs`; the only `allow` is inside
`#[cfg(test)] mod tests`. No `i += 1`: the cursor is `std::slice::Iter`, every skip counts its prefix
with `take_while(…).count()`, and every `+1` is a `saturating_add` on a budget, never a loop step.
`rs-doc` is green too: the public module docs name the private items in code spans, not intra-doc links.

### 5. `make check` (exit 0)

```console
$ make check
guard: OK (3 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 10 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… 133 × "test result: ok", 0 × "test result: FAILED"
No issues found                       # docs-check (zensical --strict)
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; …
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed
doc-tone: ok — 28 user-facing page(s), no banned phrase
manifest-purity: ok — 13 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 13 real source file(s)
plan OK
exit=0
```

Run on the final tree (after the attempt-2 findings), once, serially.

### Acceptance item 1 — one test per production, asserting the node kinds

| §6.1 production | Test |
|---|---|
| `keyword [args] { … }` (the block shape) | `trait_parses`, `static_parses`, `store_block_parses`, `grant_block_parses` |
| `app` | `app_parses` |
| `entrypoint` (`python` + `preload`, `php`) | `entrypoint_python_parses`, `entrypoint_php_parses` |
| `store` (URL, named, block) | `store_url_parses`, `store_named_parses`, `store_block_parses` |
| `trait` | `trait_parses` |
| `view` header, both forms | `view_header_parses`, `pattern_forms_parse` |
| `view` block + `endpoint` + `endpoint_block` | `view_block_parses` |
| `grant` / `grant_block` / `table_grant` / `query` | `grant_block_parses` |
| `static` + `static_directive` | `static_parses` |
| `body` (all five forms) | `body_forms_parse` |
| `use`, `egress`, `prefork` | `use_egress_and_prefork_parse` |
| `store_grant` with a name and `unrestricted` | `a_grant_names_a_store` |
| `file` (empty) | `an_empty_file_is_just_the_root` |
| §6.6 newline/`;` | `newlines_and_semicolons_continue`, `a_semicolon_does_not_continue_a_statement` |
| §6.3 reserved words | `regex_is_reserved_in_a_pattern_list_only` (D11) |

### Acceptance item 2 — every refusal, message **and** `line:col`

`every_refusal_row` covers all 32 §6.7 rows (40 cases in all); the table also carries §5.3's `=` and
quoted-store rows, §5.1's unknown byte, §6.3's reserved `unrestricted` and `regex` rows, §8.3's
`regex "…"` row, and §4.5's list cap. The individual tests add a second document per called-out row. No
refusal defaults to a permissive value: an unclosed block, a stray `}`, a missing pattern, an empty store
block, a view with no endpoint, a third nesting, a second `body`/`postgres` and a duplicate name are all
errors.

### Acceptance item 3 — bounded recursion

`a_quarter_megabyte_parses` builds a 256 KiB file of ~6 700 views, parses it, asserts the view count and
round-trips it through `print_lossless`. `a_wall_of_braces_is_refused_without_recursion` feeds 100 000
`{`, 10 000 nested `postgres {` and 10 000 `view v { ` and asserts each is refused at its first
offending token. `a_list_holds_at_most_256_items` proves the six list loops (methods, patterns, columns,
preload, `immutable`, table grants) accept 256 and refuse 257. The depth counter is proved directly (D6).

### Acceptance item 5 — files and tests

`src/blastfile/{syntax,parser}.rs` are new, `mod.rs` registers them and carries the two new budgets, and
`error.rs` carries the variants; the tests are in-module only. No integration suite, no `lower.rs`, no
`model.rs`, no `Cargo.toml`/`Cargo.lock` change (zero new dependencies).

## Files

* `crates/blastit-manifest/src/blastfile/syntax.rs` (new) — the §14.1 arena (`Cst`, `Element`,
  `NodeKind`, `NodeId`/`TokenId`/`TriviaId`), the `Builder` that lays out one contiguous child run per
  node, the typed accessors (`views`/`traits`/`stores`/`statics`/`store_slots`/`endpoints`/`patterns`/
  `grants`/`words`/`strings`/`envs`/`node_text`/…), `print_lossless`, and 7 tests (P1 over the whole
  grammar, the arena invariants, the accessors).
* `crates/blastit-manifest/src/blastfile/parser.rs` (new) — `parse`, the §6.5 functions
  (`file`/`statement`/`store`/`trait_`/`view`/`view_item`/`grant`/`static_`/`pattern`/`pattern_list`)
  plus the leaves (`word`/`string`/`env`/`name`/`method`/`function`/`table_grant`), `separators`/
  `newlines`/`end_statement`, the refusal builders, and 32 tests.
* `crates/blastit-manifest/src/blastfile/mod.rs` — `mod parser; mod syntax;`, `MAX_BLOCK_DEPTH` (2) and
  `MAX_LIST_ITEMS` (256), and the module docs for the parse.
* `crates/blastit-manifest/src/error.rs` — 37 variants on `Position` with §6.7's sentences verbatim
  (plus §5.3's `=`, §5.1's byte, §6.3's reserved word, both `regex` rows, §4.5's list cap and D1's
  catch-all).
* `docs/knowledge/LESSONS.md` — one line (D9's O(n²) `line:col` helper).

## Follow-ups

1. **`H-012`** (opened): confirm or re-word the one catch-all sentence (`expected …, found …`) and add
   the row to §6.7 before `MB.21.3.8`'s error suite pins it (D1).
2. **`MB.21.3.3`**: the `regex "…"` row of §8.3 and the `regex` reserved-word row of §6.3 are both
   raised by the parser (D2), because only the parser sees the token after `regex`; 3.3 keeps the other
   §8.3 rows and should not re-implement either.
3. **`MB.21.3.4` (print)**: the arena's `NodeKind`s are exactly §6.4's list, and `Cst::children`/
   `child_nodes`/`of_kind` are the walk the printer needs. A node's `first..last` is its **direct**
   children, so a printer must recurse through `Element::Node` rather than concatenate a range.
4. **`MB.21.3.8` (lower)**: the accessors it needs are already here (`views`, `traits`, `stores`,
   `statics`, `app`, `entrypoint`, `store_slots`, `endpoints`, `patterns`, `grants`, `words`, `strings`,
   `envs`, `node_text`, `first_of_kind`, `of_kind`). `strings` returns the token text **with** its quotes
   and escapes, so `lower` owns unescaping; `envs` returns the name without `$`. If a new accessor is
   needed, `syntax.rs` is not in 3.8's paths — the epic owner must widen them. `parse` and `Cst` are
   reachable as `crate::blastfile::parser::parse` / `crate::blastfile::syntax::Cst`; `mod.rs` has no
   `pub(crate) use` for them because an unused re-export is an `unused_imports` failure under
   `-D warnings` (the same staging rule as `MB.21.3.1`'s Follow-up 3).
5. **`MB.21.3.8`**: an empty `entrypoint python` block and a second `preload` are accepted today (D5);
   `lower` should decide whether an empty preload list is legal, and `H-012`'s answer may add the two
   sentences.
6. **`MB.21.3.8`**: `use <name>` of an unknown trait is `UnknownGroup` at lower time (§11.2, D3), and
   `postgres unrestricted` inside a trait is `UnrestrictedGroup` at validate time (§11.3, D4).
7. **`MB.21.3.8`/`MB.21.5.2`**: the protected suite `tests/critical_blastfile_parser.rs` (R4's
   `depth_two_is_the_limit`, `a_wall_of_braces_is_refused_without_recursion`) lands with the public entry
   point; today the same properties are in-module tests because `parse` is `pub(crate)`.
8. **Proposed `LESSONS.md` line** (appended as D9): a `line:col` helper that scans from byte 0 turns a
   per-item call into O(n²) — store the offset and convert only on the error path.
9. **`docs/design/blastfile-dsl.md` §6.5** says the parser is "ten small functions … each returns
   `Result<NodeId, ManifestError>`"; the builder returns nothing (the arena is sealed in one pass), so
   the functions return `Result<(), ManifestError>` and `parse` returns the tree. Worth a sentence in
   §6.5 when the design is next touched.
