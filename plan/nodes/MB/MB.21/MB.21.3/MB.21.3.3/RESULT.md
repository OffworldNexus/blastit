# Result MB.21.3.3

Attempt 2 closes the attempt-1 blocker (the missing `error.rs`) and lands the rest of the leaf: the
ten `ManifestError` variants, the parser hook that compiles each pattern at its own `line:col` and
stores the result on the pattern's token, and the `Str`/`RawStr` decoders `lower` will call.

## Summary

* `blastfile/pattern.rs` is the one compiler from a `Blastfile` pattern to the fragment a
  `Route.path` entry holds, and the one method normalisation lowering applies before a `Route`
  exists.
  * `compile(pattern, at)` scans the pattern **once**, by `char_indices()`: `{` opens a parameter
    (read to the first `}`, split on `:`, name and type validated, the type's fixed fragment
    pushed), `*` only at the last byte after a `/` pushes `.*`, and every other run of literal bytes
    goes through `regex::escape`. The emitted fragment is §8.1's table character for character
    (pinned by `the_emitted_fragments`, not merely by matching the same paths).
  * `compile_regex(pattern, at)` keeps `regex '…'` verbatim and compiles it with the existing
    `routing::compile_pattern` — pre-compiled on its own, so an unbalanced `)` can never be rescued
    by the wrapper into a catch-all, then wrapped `^(?:…)$` with `REGEX_SIZE_LIMIT`/`REGEX_NEST_LIMIT`.
    A pattern the crate refuses is §8.3's row with the crate's own message: a clean refusal, never a
    panic.
  * Both spellings are capped at `MAX_PATTERN_BYTES` (256, bytes not chars) — §4.5's "Where
    enforced: `pattern.rs`".
  * Both return `Compiled { text, fragment }`: the value it was compiled from (decoded for `"…"`) and
    the fragment, so nothing downstream re-decodes or recompiles.
  * `normalise_methods(written)` implements §8.4's table exactly: nothing → `None`, `GET` →
    `["GET","HEAD"]`, `GET, HEAD` → the same, `POST` → `["POST"]`, `GET, POST` →
    `["GET","HEAD","POST"]` (written order, `HEAD` after its `GET`, duplicates deduplicated, `HEAD`
    alone kept), and `CONNECT`/`TRACE` refused with §8.4's sentence at the method's own position.
* `error.rs` gains §8.3's nine rows and §8.4's method row as `ManifestError` variants on `Position`:
  `PatternParameterUnclosed`, `PatternParameterUnnamed`, `PatternParameterName`,
  `PatternParameterType`, `PatternStarPosition`, `PatternParameterDuplicate`, `PatternNotAnchored`,
  `PatternTooLong`, `PatternRegex`, `MethodCannotReachView`. Each renders §18's one line —
  `<line>:<col>: <message>`, values escaped and cut at 120 `char`s, `Position::NONE` rendering bare.
* The **parser hook** (`parser.rs::pattern`) compiles every pattern as it is read, at the pattern
  token's own position, and stores the result in the tree (`Cst::pattern(pattern_node)`), so §8.3's
  rows and the 256-byte cap are parse refusals with a `line:col` (§8.1's "capped in the parser") and
  `lower` reads the fragment instead of compiling the text again.
* `tokens.rs` gains the decoders: `str_value` (§5.1's `\"`, `\\`, `\n`, `\t`, quotes stripped) and
  `raw_value` (quotes stripped, every byte verbatim). `strings()` on the tree still returns the
  source spelling, so P1 is untouched.
* `Route::methods`' doc no longer promises the opposite of the implication, and `routing.rs`'s
  "`HEAD` is never implied by `GET`" now says the implication happened at lower time and that
  `RoutingTable` keeps comparing exact strings.

Tests: 15 in `pattern.rs` (§8.1's table both directions — `/a.b` ≠ `/axb`, `{id:int}` rejects `12a`,
`{id:uuid}` rejects three near misses, `/assets/*` matches a nested file and not the prefix itself, a
bare `/x` ≠ `/xy` — the emitted fragments, every literal metacharacter, §8.3's matrix with message
**and** `line:col`, both caps including the byte-vs-char boundary, the regex row, §8.4's table,
`CONNECT`/`TRACE`, the variant identity of every row, the position-less rendering, and the
one-line/120-char rendering), 3 in `tokens.rs` (the four escapes, the raw form, and the two forms
disagreeing), and 4 in `parser.rs` (the compiled pattern on its token, the decoded pattern, §8.3's
rows as parse refusals at their own `line:col`, and a second pattern's own position plus both caps).

`make check` exits 0.

## Deviations (read before reviewing)

### D1. Attempt 1's blocker is closed by the plan edit, not by this leaf

`crates/blastit-manifest/src/error.rs` was missing from this node's `paths` (and from `3.8`'s); the
lead widened both and answered `H-013` in `LOG.md`. The temporary `pub(crate) Refusal` seam is gone:
every refusal is now the `ManifestError` variant of its own row, and `a_refusal_is_the_variant_of_its_row`
pins the variant, its position and the value the sentence quotes, so no caller has to parse a message
back into an error.

### D2. §8.3's anchoring row is applied to the quoted form only — `regex '…'` is verbatim

§8.3's `"x"` row shows the quoted spelling, and §8.1 says a `regex` is "the text verbatim, wrapped
`^(?:…)$`". Refusing a raw pattern that does not start with `/` would refuse `regex '^/x$'`, which is
a perfectly good double-anchored regex, and would contradict "verbatim". So `compile` refuses an
unanchored pattern and `compile_regex` does not. A raw pattern that can never match is the author's
own regex, and the crate already answers for its syntax.

### D3. Three consequences of §8.2's scanner that the design does not spell out

* A `}` with no `{` is **literal** text (`regex::escape` handles it): §8.2 makes only `{` special and
  §8.3 pins no sentence for a stray `}`. Inventing one was the only alternative, so it is literal.
* A literal `{` cannot be written in the quoted form at all: `{` always opens a parameter, so `/a{b`
  is §8.3's unclosed row. The raw form is the escape hatch (`regex '/a\{b'`). Same for a literal
  `*` anywhere but the trailing position (`/a*b` is the `*` row; `regex '/a\*b'` is the literal).
* A parameter's name is `[a-z][a-z0-9_]*` — narrower than §6.7's view name: no `-`, no leading digit
  — because §8.3's own sentence says so.

### D4. The quoted form is not re-compiled at parse time, and that is a deliberate trade

§8.1's "the fragment is compiled by the existing `routing::compile_pattern`: pre-compiled on its own,
then as `^(?:…)$`" is what `RoutingTable` does with every `Route.path` entry when it builds the table,
and it is what `compile_regex` does at parse time so §8.3's `regex` row can be a parse refusal. The
quoted form's fragment cannot fail to compile (§8.2 emits only `regex::escape` output and three fixed
fragments), so `compile` does not compile it a second time: doing so made `a_quarter_megabyte_parses`
(6 700 patterns) take 1.84 s instead of 0.20 s, and it would double the boot cost of every generated
file to prove an invariant the construction already gives. A regression in the escape table is still
caught — by `RoutingTable` at boot, naming the route and the rule — and by `the_emitted_fragments`,
which pins the fragment strings themselves.

### D5. `position_of` is now a binary search over a line index

The hook asks for a position once per pattern, and `position_of` scanned from byte 0, which is the
`O(n²)` trap `MB.21.3.2` recorded in `LESSONS.md` (it made that same 256 KiB test take 15.6 s). The
parser now builds `line_starts` in one pass and binary-searches it, so a position is `O(log n)` for
every caller, including the duplicate-name paths. `a_quarter_megabyte_parses` stays at 0.20 s. The
results are identical (`position_of`'s tests and every `line:col` row still pass).

### D6. Where the compiled pattern lives: a side table on `Cst`, keyed by the pattern's token

`Cst` gains `patterns: BTreeMap<TokenId, Compiled>` and `Cst::pattern(pattern_node)` finds the
pattern's string token and looks it up, so `lower` writes `cst.pattern(pattern)?.fragment()` and never
sees a token id. The source spelling is still the token's span (`token_text`), so P1 is untouched and
nothing is duplicated; `Compiled` carries the *decoded* value beside the fragment because that is the
one thing a reader could not otherwise recover without re-running the decoder.

### D7. The `≤ 450 changed lines` budget is exceeded: 8 files, ~1 320 lines

848 new lines in `pattern.rs` (318 production, 530 tests), and 468 changed across the other seven:
`parser.rs` 199 (the hook, the line index and 4 tests), `error.rs` 117 (ten documented variants),
`tokens.rs` 100 (58 production, 42 tests), `syntax.rs` 31, and 21 doc-comment lines in
`mod.rs`/`model.rs`/`routing.rs`. Acceptance item 1 demands §8.1's table in both directions, item 2
the method table, item 3 every §8.3 row with message **and** `line:col`; the three tables alone are
~300 lines of assertions before a line of production code, and §18 demands one documented variant per
row. Nothing was padded and nothing required was dropped (as `MB.21.3.1`'s D4 and `MB.21.3.2`'s D7
did).

### D8. `MAX_PATTERN_BYTES` lives in `pattern.rs`, not in `mod.rs` with the other budgets

§4.5's row names `pattern.rs` as the "Where enforced" site, and this is the one module that sees the
text of both spellings, so the constant is beside its check. The other five budgets stay in `mod.rs`.

## Evidence

### 1. `cargo test -p blastit-manifest --locked blastfile::pattern` (exit 0)

```console
$ cargo test -p blastit-manifest --locked blastfile::pattern
     Running unittests src/lib.rs
running 15 tests
test blastfile::pattern::tests::a_parameter_is_validated_before_it_is_emitted ... ok
test blastfile::pattern::tests::a_refusal_is_one_line ... ok
test blastfile::pattern::tests::a_refusal_is_the_variant_of_its_row ... ok
test blastfile::pattern::tests::a_positionless_refusal_renders_bare ... ok
test blastfile::pattern::tests::connect_and_trace_cannot_reach_a_view ... ok
test blastfile::pattern::tests::the_method_table ... ok
test blastfile::pattern::tests::the_parameter_ends_at_the_first_close_brace ... ok
test blastfile::pattern::tests::the_refusal_matrix ... ok
test blastfile::pattern::tests::a_stray_close_brace_is_literal ... ok
test blastfile::pattern::tests::a_compiled_pattern_keeps_its_text_and_its_fragment ... ok
test blastfile::pattern::tests::a_pattern_holds_at_most_256_bytes ... ok
test blastfile::pattern::tests::a_regex_the_crate_refuses_is_a_clean_refusal ... ok
test blastfile::pattern::tests::the_emitted_fragments ... ok
test blastfile::pattern::tests::literal_bytes_are_escaped ... ok
test blastfile::pattern::tests::the_table ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 121 filtered out; finished in 0.02s
```

### 2. `cargo test -p blastit-manifest --locked` (exit 0)

```console
$ cargo test -p blastit-manifest --locked
Running unittests src/lib.rs                 test result: ok. 136 passed; 0 failed   # 114 + 22 new
Running tests/critical_blastfile_lexer.rs     test result: ok. 3 passed; 0 failed
Running tests/critical_manifest_011.rs        test result: ok. 17 passed; 0 failed
Running tests/critical_manifest_edit.rs       test result: ok. 23 passed; 0 failed
Running tests/critical_manifest_edit_executor.rs  test result: ok. 5 passed; 0 failed
Running tests/critical_php_manifest.rs        test result: ok. 9 passed; 0 failed
Running tests/critical_static_manifest.rs     test result: ok. 15 passed; 0 failed
Running tests/docs_fences.rs                  test result: ok. 6 passed; 0 failed
Running tests/edit_branches.rs                test result: ok. 19 passed; 0 failed
Running tests/grants.rs                       test result: ok. 13 passed; 0 failed
Running tests/mutation_gaps.rs                test result: ok. 20 passed; 0 failed
Running tests/parse.rs                        test result: ok. 39 passed; 0 failed
Running tests/routing.rs                      test result: ok. 11 passed; 0 failed
Running tests/uid.rs                          test result: ok. 8 passed; 0 failed
Running tests/validate.rs                     test result: ok. 29 passed; 0 failed
doctests                                      test result: ok. 6 passed; 0 failed
```

Every non-`blastfile` count equals `MB.21.3.2`'s report, so the TOML path is byte-for-byte unchanged.

### 3. `make rs-fmt` (exit 0) and `make rs-lint` (exit 0, `-D warnings`)

```console
$ make rs-fmt
cargo fmt --all -- --check
$ make rs-lint
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Checking blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
    Checking blastit-env v0.2.0 (…/crates/blastit-env)
    Checking blastit-view v0.2.0 (…/crates/blastit-view)
    Checking blastit v0.2.0 (…/crates/blastit)
    Checking blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.67s
```

### 4. `make rs-doc` (exit 0, `RUSTDOCFLAGS=-D warnings`)

```console
$ make rs-doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --features python
    Documenting blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Documenting blastit v0.2.0 (…/crates/blastit)
    Documenting blastit-py v0.2.0 (…/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.63s
   Generated …/target/doc/blastit/index.html and 13 other files
```

The module docs name `routing::compile_pattern` in a code span, not an intra-doc link, because it is
`pub(crate)` (`MB.21.3.1`'s Follow-up 6).

### 5. `make check` (exit 0)

```console
$ make check
guard: OK (2 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 15 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… 133 × "test result: ok", 0 × "test result: FAILED"
No issues found                       # docs-check (zensical --strict)
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; …
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed
doc-tone: ok — 28 user-facing page(s), no banned phrase
manifest-purity: ok — 14 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 14 real source file(s)
plan OK
exit=0
```

Run once, serially, on the final tree.

### Acceptance item 1 — §8.1's table, both directions

`the_table` asserts each row matches what it says and nothing that only shares its characters:
`/products` (not `/product`, not `/products/1`), `/v1.0/x` (not `/v1x0/x`), `{id}` (not
`/products/`, not `/products/7/x`), `{id:int}` (not `12a`, not `a12`, not `4.2`), `{id:uuid}` (not an
eleven-digit last group, not the dashless form, not a 13-digit first group, not `not-a-uuid`),
`/assets/*` (matches `/assets/a/b.css` and `/assets/`, not `/assets` and not `/assets-secret/key.pem`),
`/*` (matches `/` and any path), `/x` (not `/xy`, not `/x/y`), `/a.b` (not `/axb`).
`the_emitted_fragments` pins the fragment strings themselves, so a change to the emission table fails
even if the matching happens to stay the same.

### Acceptance item 2 — the method normalisation as a tested unit

`the_method_table` covers §8.4's six rows plus `POST, GET`, `DELETE, GET`, `GET, GET`,
`GET, HEAD, GET, HEAD`, `PUT, PUT`, `HEAD` alone and `HEAD, GET`.
`connect_and_trace_cannot_reach_a_view` asserts both sentences at their own `line:col`, including
when `GET` precedes them. `MB.21.3.8` re-asserts the same property on the parsed `Manifest` through
the public entry point.

### Acceptance item 3 — every refusal with message and `line:col`

`the_refusal_matrix` is §8.3's table: unclosed `{`, `{}`, `{Id}`, `{id:str}`, `*` not last, `/assets*`,
the duplicate parameter, and a pattern not starting with `/` (both `"x"` and `""`), each asserted as
`3:7: <sentence>`. `a_parameter_is_validated_before_it_is_emitted` adds `{id:}`, `{1id}`, `{a-b}`,
`/x/{` and `{a{b}`. `a_pattern_holds_at_most_256_bytes` proves 256 bytes is legal and 257 is not, for
both spellings, and that the cap counts bytes (127 `é` after `/x` is legal, 128 is not).
`a_regex_the_crate_refuses_is_a_clean_refusal` proves look-around and an unbalanced `)` are clean
refusals naming the crate's message — never a panic — and that the raw form keeps `{2}` and `*`
meaning what the crate says. `a_refusal_is_one_line` proves a control byte is escaped and a long
parameter is cut at 120 characters with `…`; `a_positionless_refusal_renders_bare` proves §18's
`NONE` rule for four of the rows. Through `parse`, `a_bad_pattern_is_refused_where_it_is_written`
asserts the same rows at `1:8`/`1:14` of a real document, and
`a_bad_second_pattern_names_its_own_position` proves the second pattern of a header is refused at its
own column and that both caps are parse refusals.

### Acceptance item 4 — `make check` exit 0

Above.

### Acceptance item 5 — files and tests

`src/blastfile/pattern.rs` (new), `src/blastfile/tokens.rs` (the two decoders), `src/blastfile/parser.rs`
(the hook and the line index), `src/blastfile/syntax.rs` (the compiled-pattern table and its accessor),
`src/blastfile/mod.rs` (`mod pattern;` + docs), `src/error.rs` (the ten variants), `src/model.rs` and
`src/routing.rs` (doc comments). In-module tests only. No `lower.rs`, no new public item, no
`Cargo.toml`/`Cargo.lock` change.

## Files

* `crates/blastit-manifest/src/blastfile/pattern.rs` (new) — `MAX_PATTERN_BYTES`, the three fixed
  fragments, `Compiled`, `compile`, `compile_regex`, `normalise_methods`,
  `parameter_end`/`parameter`/`is_parameter_name`/`push_literal`/`check_cap`, and 15 tests.
* `crates/blastit-manifest/src/blastfile/tokens.rs` — `str_value`, `raw_value`, and 3 tests.
* `crates/blastit-manifest/src/blastfile/parser.rs` — the `pattern` hook (compile + store), the
  `patterns` map, `line_starts`, the binary-search `position_of`, and 4 tests.
* `crates/blastit-manifest/src/blastfile/syntax.rs` — `Cst.patterns`, `Cst::pattern`, `Builder::build`.
* `crates/blastit-manifest/src/blastfile/mod.rs` — `mod pattern;` and the module docs.
* `crates/blastit-manifest/src/error.rs` — the ten §8.3/§8.4 variants with §18's rendering.
* `crates/blastit-manifest/src/model.rs` — `Route::methods`' doc.
* `crates/blastit-manifest/src/routing.rs` — `RoutingTable::route`'s doc.

## Follow-ups

1. **`MB.21.3.8`** owns `tests/blastfile_patterns.rs` and `tests/critical_blastfile_patterns.rs`
   (§20 R9: the traversal corpus, a 257-byte pattern, an unbalanced `)`, `HEAD` in `methods`). The
   in-module tests here are the same properties; the suites re-assert them through
   `from_blastfile_str`. `lower` reads `cst.pattern(pattern)?.fragment()` for `Route.path`, calls
   `pattern::normalise_methods(&[(method, position)])` for the header's methods, and uses
   `tokens::str_value`/`raw_value` for every other string (`strings()` still returns the source
   spelling, quotes included).
2. **§8.4's "`RoutingTable::lookup` refuses `TRACE` and `CONNECT` for every view" is not
   implemented, and there is no `lookup` on `RoutingTable`** (measured: `RoutingTable::route` compares
   only the declared list, and the only `CONNECT` refusal in the tree is `blastit-http`'s `501`; a
   `TRACE` request reaches a method-less route today). A behaviour change is outside this leaf's
   `routing.rs` scope, which is doc comments only, so no doc comment claims it. The lead recorded it
   as an epic follow-up.
3. **§8.3 has no row for a stray `}` or a literal `{`/`*`** in the quoted form (D3). The behaviour is
   pinned by tests; a design sentence would make it a documented rule rather than an implementer's
   reading.
4. **`docs/design/blastfile-dsl.md` §8.1** says the cap is enforced "in the parser"; it is enforced in
   `pattern.rs`, which the parser calls. Worth a sentence when the design is next touched.
5. **Proposed `LESSONS.md` line** (outside `paths`, so recorded here): a per-item `line:col` is the
   `O(n²)` trap `MB.21.3.2` recorded, and the fix that survives a new eager caller is a `line_starts`
   index plus `partition_point`, not moving the call to the error path — the pattern hook needs a
   position for every pattern, so laziness was not available.
