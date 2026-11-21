# RESULT MB.21.4.5.3

STATUS: done

## Summary

- `tests/common/mod.rs` keeps every exported name (`texts`, `text_map`, `parse_errors`,
  `validate_errors`, `validate_warnings`, `section_6`, `fixture`, `corpus`, `CORPUS_SIZE`,
  `fixture_directory`) with its body re-pointed at `Manifest::from_blastfile_str`.
  `SECTION_6` is now the same application written as a `Blastfile` (four views, one store,
  traits `session` + `catalog`, `prefork`, `egress any`) and `SECTION_5_1` the two stores of
  §5.1; `section_5_1()` was kept because `mutation_gaps` reads it.
- All eight suites now read `Blastfile` documents or the model's `#[doc(hidden)]` builder; no
  file in `crates/blastit-manifest/tests/` mentions the retired reader or its file name.
- A top-level `groups` list has no spelling (§7.1), so the three global-group assertions assert
  the Blastfile subject instead: a written `use session` is honoured (`grants.rs`,
  `mutation_gaps.rs`), an omitted one inherits nothing (`grants.rs`), an unknown one is
  `UnknownTrait` (`validate.rs`, `grants.rs`), and `global_groups()` is empty for every
  document (`grants.rs`, `mutation_gaps.rs`). The validator's `Location::Global` arm, which no
  document can reach, is kept with the model builder and says why in its doc comment.
- `docs_fences` no longer executes the retired opening. Every per-page count is still pinned
  (the retired column included), `THE_RETIRED_OPENING_IS_GONE` is still `false`, and the
  retired fences are counted and named but never parsed. What they stopped proving moved in
  the same change (below), and every message that spells the opening composes it from two
  literals (`concat!(r#"```toml title=""#, "blastit", ".toml", "\"")`).

## Review response (goldfish PASS, two MINORs)

| Finding | Fix |
|---|---|
| 1 [MINOR] `assert_eq!(replaced.len(), actual[2])` compared two results of the same collector over the same constant — a tautology; only `retired == TOTAL[2]` bit | The per-page cross-check is now an **independent scan** of the page's own text (`document.lines().filter(\|line\| line.trim() == RETIRED_OPENING).count()`): `actual[2] == named` pins the table against the page, and `retired_documents(...).len() == named` pins the collector against the same raw scan. Verified from the shell: the ten pages carry exactly 2 `text title="Blastfile"` lines and 8 retired opening lines. |
| 2 [MINOR] `route_count() > 0 \|\| !mounts().is_empty()` absorbed the shape, so a document that degraded from views to a mount still passed | The shape is now a value (`Shape::{Application, StaticSite, Empty}` via `shape_of`) and is **pinned per page** by name: `docs/local.md` and `docs/blastfile.md` must be `Shape::Application` (views > 0, mounts empty); an unpinned page panics instead of passing, and a page that later documents an all-static site adds its own `Shape::StaticSite` arm. |

### The four one-line names (acceptance 3)

- **Canary's replacement**: `docs_fences.rs::an_invalid_regex_is_refused` is now a `Blastfile`
  `view products GET regex '/products/(' { body none }`, refused as
  `ManifestError::PatternRegex` at the pattern's quote (a document can never reach
  `InvalidRegex`: the parser compiles a `regex` before it is wrapped, §8.1).
- **Static-site shape's new home**: `tests/static_mounts.rs::the_documented_static_site_is_one_mount`
  (+ its canary `a_drifted_static_site_is_not_the_documented_shape`), pinning prefix `/`,
  `root "public"`, `index "index.html"`, `fallback Some("index.html")`,
  `immutable ["/assets/*"]`, zero views, no app, `can_boot_without_app()`.
- **Deleted Django fragment test**: `docs_fences.rs::the_django_static_fragment_pins_its_documented_depth`
  is deleted (a `[route.static]` fragment has no spelling here), with the reason in
  `docs_fences.rs`'s module doc; `MB.21.5.1` rewrites the page as a `static` mount and
  `tests/static_mounts.rs::the_django_page_documents_one_glob_per_depth` already pins that
  mount's `root "staticfiles"` and its six-level `immutable` list.
- **Two error-variant assertions that changed subject**: `validate.rs::a_refusal_comes_back_before_validation_runs`
  (was `parse_errors_come_back_before_validation`, constructed `UnsupportedVersion`) now
  asserts the lexer-produced `MethodCannotReachView` for `CONNECT`, same "first refusal wins,
  validation never runs" claim; and `mutation_gaps.rs::the_error_list_counts_iterates_and_renders`
  (constructed `MissingVersion`) now uses `UnknownKeyword` + `InvalidName` and keeps the
  one-line-per-problem / no-header / iterate-by-ref-and-value assertions.

## Acceptance evidence

### 1a. `grep -rnE 'from_toml_str|toml::' crates/blastit-manifest/tests/`

```console
$ grep -rnE 'from_toml_str|toml::' crates/blastit-manifest/tests/ ; echo "exit=$?"
exit=1
```

### 1b. `grep -rnE 'blastit[.]toml' crates/blastit-manifest/tests/`

```console
$ grep -rnE 'blastit[.]toml' crates/blastit-manifest/tests/ ; echo "exit=$?"
exit=1
```

Both greps are over the **whole** test directory (every suite, not only this leaf's).

### 1c. `cargo test -p blastit-manifest`

```console
$ cargo test -p blastit-manifest > /tmp/opencode/mb21453-test3.log 2>&1 ; echo "exit=$?"
exit=0
$ grep -cE '^test result: ok' /tmp/opencode/mb21453-test3.log
32
$ grep -E '^test result:' /tmp/opencode/mb21453-test3.log | awk '{s+=$4} END {print "total passed:", s}'
total passed: 514
$ grep -E 'FAILED|failures:' /tmp/opencode/mb21453-test3.log | head
(no output)
```

The suites this leaf owns:

```console
$ cargo test -p blastit-manifest --test validate --test mutation_gaps --test routing \
      --test grants --test uid --test docs_fences --test static_mounts
test result: ok. 31 passed; 0 failed; ... (validate)
test result: ok. 22 passed; 0 failed; ... (mutation_gaps)
test result: ok. 11 passed; 0 failed; ... (routing)
test result: ok. 13 passed; 0 failed; ... (grants)
test result: ok.  8 passed; 0 failed; ... (uid)
test result: ok.  6 passed; 0 failed; ... (docs_fences)
test result: ok.  6 passed; 0 failed; ... (static_mounts)
```

`cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings` finishes clean and
`cargo fmt --all -- --check` prints nothing (exit 0).

### 1d. `make docs-unit`

```console
$ make docs-unit
96 passed, 65 subtests passed in 1.74s
make-exit=0
```

### 2. `docs_fences`: the per-page table, still pinned, `THE_RETIRED_OPENING_IS_GONE == false`

```console
$ cargo test -p blastit-manifest --test docs_fences
running 6 tests
test a_page_whose_count_drifted_is_named ... ok
test a_document_that_does_not_parse_names_its_page_position ... ok
test an_unclosed_fence_is_refused ... ok
test both_document_openings_are_collected_and_a_fragment_is_not ... ok
test an_invalid_regex_is_refused ... ok
test every_documented_document_parses_and_validates ... ok
test result: ok. 6 passed; 0 failed; 0 ignored
```

The pinned table (counts are `text title="Blastfile"` + `blastfile` + retired opening; the
last column is the retired one and is `0` only where a page already carries a `Blastfile`
document):

```text
("README.md",                         [0, 0, 1])
("docs/index.md",                     [0, 0, 1])
("docs/getting-started.md",           [0, 0, 1])
("docs/local.md",                     [1, 0, 0])
("docs/frameworks/index.md",          [0, 0, 1])
("docs/frameworks/django.md",         [0, 0, 1])
("docs/frameworks/flask.md",          [0, 0, 1])
("docs/frameworks/php.md",            [0, 0, 1])
("docs/frameworks/static-and-spa.md", [0, 0, 1])
("docs/blastfile.md",                 [1, 0, 0])
TOTAL: [2, 0, 8]

const THE_RETIRED_OPENING_IS_GONE: bool = false;   // line 64, flipped by MB.21.5.2
```

`every_documented_document_parses_and_validates` executes only the two `Blastfile` openers and
pins **two independent facts per page**: its document's `Shape::Application` (views > 0 and no
mount, matched by page name) and its retired count against a raw line scan of the page's own
text (not the fence collector), with `retired_documents(page).len()` cross-checked against the
same scan and `Σ retired == TOTAL[2] == 8`. `a_page_whose_count_drifted_is_named` keeps the
strict-flip assertion green with `retired_gone = true`.

Independent shell cross-check of the same numbers (ten pages, `grep -cx`; `retired` is the
page's `toml title=` opening, written here without the file's name so this record carries no
contiguous copy of it — `docs_fences`'s own constant composes it the same way):

```console
$ for f in README.md docs/index.md docs/getting-started.md docs/local.md \
      docs/frameworks/index.md docs/frameworks/django.md docs/frameworks/flask.md \
      docs/frameworks/php.md docs/frameworks/static-and-spa.md docs/blastfile.md; do
      printf "%-36s text=%s retired=%s\n" "$f" \
        "$(grep -cx '```text title="Blastfile"' "$f")" \
        "$(grep -cx '```toml title=.*' "$f")"; done
README.md                            text=0 retired=1
docs/index.md                        text=0 retired=1
docs/getting-started.md              text=0 retired=1
docs/local.md                        text=1 retired=0
docs/frameworks/index.md             text=0 retired=1
docs/frameworks/django.md            text=0 retired=1
docs/frameworks/flask.md             text=0 retired=1
docs/frameworks/php.md               text=0 retired=1
docs/frameworks/static-and-spa.md    text=0 retired=1
docs/blastfile.md                    text=1 retired=0
```

### 3. No page under `docs/` was edited

```console
$ git status --porcelain docs/ ; echo "exit=$?"
exit=0
```

### Sized, before → after

| File | Lines | `#[test]` |
|---|---|---|
| `common/mod.rs` | 172 → 176 | (no tests) |
| `validate.rs` | 781 → 1090 | 30 → 31 |
| `mutation_gaps.rs` | 714 → 771 | 21 → 22 |
| `routing.rs` | 259 → 274 | 11 → 11 |
| `grants.rs` | 261 → 280 | 13 → 13 |
| `uid.rs` | 127 → 128 | 8 → 8 |
| `docs_fences.rs` | 539 → 491 | 9 → 6 |
| `static_mounts.rs` | 112 → 251 | 3 → 6 |

## Re-homed, not dropped

`docs_fences` (the three tests the retired opening made unparseable):

| Deleted test | Now proved by |
|---|---|
| `the_static_page_manifest_has_its_documented_shape` | `static_mounts.rs::the_documented_static_site_is_one_mount` |
| `a_drifted_static_block_does_not_satisfy_the_documented_shape` | `static_mounts.rs::a_drifted_static_site_is_not_the_documented_shape` (five mutations: root, index, fallback, immutable, prefix) |
| `the_django_static_fragment_pins_its_documented_depth` | `static_mounts.rs::the_django_page_documents_one_glob_per_depth` (deleted, not translated: the fragment cannot be parsed; `MB.21.5.1` rewrites the page) |

`mutation_gaps` (table-walk refusals that no longer exist):

| Deleted test | Now proved by |
|---|---|
| `a_declared_proxy_name_is_a_grant_only_when_it_holds_a_table` | `a_reserved_word_is_a_name_inside_a_grant` (a store named `app`; `select groups, app` names two tables; a grant on `app`) |
| `a_reserved_key_inside_a_table_grant_row_is_a_misplaced_top_level_key` | `a_reserved_word_is_a_name_inside_a_grant` (same accepted twin) |
| `an_unknown_grant_key_holding_a_table_is_reported_as_the_key_it_is` | `a_keyword_is_only_a_directive_where_the_grammar_has_one` (`tables` in a grant block is `NotADirectiveHere`; `postgres unrestricted` is `StoreNameReserved`; the seven clauses accepted) |

Assertions whose *subject* moved earlier, disclosed rather than weakened:

- `routing.rs`: a document's `regex` is compiled by the parser (§8.1), so the four table-level
  refusals (`InvalidRegex` for `x)|(.*`, for `[`, the 100 KiB bomb, 200-deep nesting) are
  asserted where a user meets them — `PatternRegex` / `PatternTooLong` at the quote, with the
  same messages ("unopened group", "size limit", "nested parentheses", "at most 4096 bytes").
  `every_broken_pattern_is_reported` became `a_broken_pattern_is_refused_before_a_manifest_exists`:
  the parser refuses the first and yields no manifest (R21), so nothing can report the second.
  `methods_are_filtered_and_head_is_not_implied_by_get` became
  `methods_are_filtered_and_head_is_implied_by_get`: a `GET` view answers `HEAD` itself, which
  is the documented behaviour (§8.4).
- `validate.rs::every_validation_problem_is_collected` keeps its "all problems, not the first"
  claim with three findings that survive to `validate` (a trait's `unrestricted`, a redacted
  `query`, a duplicate mount). The four subjects the retired format also contributed (unknown
  group, invalid name, invalid regex, invalid method, unknown proxy) are refused at parse or
  lower now and are asserted in their own tests (`an_unknown_use_is_refused_at_the_view_that_writes_it`,
  `route_names_must_match_the_documented_pattern`, `invalid_regexes_are_reported_with_their_rule`,
  `methods_must_be_uppercase_http_tokens`, `grants_on_unknown_stores_are_reported_with_their_place`).

## Builder disclosures (`#[doc(hidden)]`, each with the reason in its doc comment)

Seven rules a `Blastfile` cannot express are still exercised through `Route::new` /
`Group::new` / `Manifest::from_parts`, so the validator's live arms do not become dead letters:

- `Location::Global` `UnknownGroup` (`a_global_group_reference_is_still_reported_by_validate`)
  — `lower` always leaves `global_groups` empty.
- `DuplicateRouteName` (`duplicate_view_names_are_refused_by_the_parser`) — the parser refuses
  a repeated `view` name first; the validator compares sandboxes.
- `InvalidRouteName` (`route_names_must_match_the_documented_pattern`) — a view name is a `Word`.
- `EmptyPath` / `EmptyPattern` (`a_route_without_usable_patterns_is_reported`) — a view needs an
  endpoint and a pattern starts with `/`.
- `InvalidRegex` (`invalid_regexes_are_reported_with_their_rule`, `mutation_gaps.rs::an_invalid_regex_reports_the_engines_last_line`)
  — a document's regex is compiled by the parser.
- `InvalidMethod` / `EmptyMethods` (`methods_must_be_uppercase_http_tokens`, `methods_must_not_be_an_empty_list`)
  — a header takes `Method` tokens and its list is non-empty by grammar.
- `UnrestrictedWithQueries` / `UnrestrictedWithContent` (`an_unrestricted_grant_takes_no_queries`,
  `an_unrestricted_grant_takes_no_tables`) — `postgres unrestricted` is a whole block that takes
  no directive.

## Follow-ups (out of scope)

- `tests/grants.rs`, `tests/uid.rs` and `tests/routing.rs` now assert `manifest.mounts()` /
  `manifest.php()` / `manifest.effective_grants()` only; the per-route `Route::static_policy()` /
  `Route::php()` pair is still read by no test here and is `MB.21.4.5.6`'s to delete with the
  reader.
- `ManifestError::MissingKey`, `UnknownKey`, `NotTopLevel` and `WrongType` remain in `error.rs`
  with no producer or test after this leaf; they are `MB.21.4.5.6`'s (or a later cleanup's) to
  delete, not this leaf's — `error.rs` is outside its paths.
- `Location::Group` and `Location::Trait` render identically ("trait \"session\""); only `Trait`
  is produced by a `Blastfile`. If 5.6 unifies them, `validate.rs::locations_read_well_in_errors`
  and `grants_on_unknown_stores_are_reported_with_their_place` need a one-token update.
- A schema-qualified or otherwise non-`Word` table name (`audit.users`, `pg_catalog.pg_class`)
  cannot be written in a grant (§13.1); `validate.rs` keeps the rule on the model and says so.
- Sibling work in this shared worktree (`crates/blastit-manifest/src/routing.rs`,
  `crates/blastit-scheduler/**`, `crates/blastit-env/**`) is in flight; the crate built and its
  suite was green at every run recorded above, and none of those files was touched here.
