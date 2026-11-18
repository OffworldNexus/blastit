# AUDIT — MB.34 (one-statement `query` validation, exact inner spans, Quick Fix, SQL highlighting)

Epic gate audit for the nine leaves of `MB.34` on branch `release/0.2.0`, base `5f29398`,
HEAD `1117a10` (the MB.32 gate commit on top). Worktree `.worktrees/release-0.2.0`; every
command below ran there. Skills loaded: `testing`, `security-invariants`, `dx-first`,
`rust-standards`, `python-standards`.

**Scope.** `crates/blastit-sqlpolicy` (`template.rs`, `literals.rs`, protected
`tests/critical_query_template.rs`), `crates/blastit-manifest` (`error.rs`, `model.rs`,
`validate.rs`, `blastfile/lower.rs`, protected `tests/critical_query_sites.rs`, five `at:`
lines in `tests/critical_manifest_011.rs`, `tests/validate.rs`), `crates/blastit`
(`src/lsp/{mod,hover}.rs`, protected `tests/critical_lsp.rs`), and the editor chain
(`editors/tree-sitter-blastfile`, `editors/zed`, `scripts/check_zed_extension.py` +
`scripts/test_check_zed_extension.py`).

**Requirements audited.** `docs/design/blastfile-dsl.md` §13.4.1–§13.4.6, §18.1, §20
(R31–R36); `docs/design/blastfile-lsp-zed.md` §7.6, §7A.1–§7A.5, §9.2a, §10.2, §13.1–§13.3
(R60–R68); the user contract in `docs/reference/blastfile.md` (grants section), `docs/ci.md`,
`docs/local.md` step 15.

**Verdict at a glance: the epic's tests are honest.** No protected case was weakened (adds
only: +597 lines in `critical_lsp.rs`, +5 `at:` literals in `critical_manifest_011.rs`), every
new critical case carries a hand-computed oracle, the critical tier kills the position
arithmetic (§3), and `tools/critical_guard.py` passes for the epic as a whole. Six
requirements are proved only in an unprotected tier or only by the hand-recorded
`docs/testing/E2E-MB.34.md`; §6's eight nodes close them. Nothing in the epic is a broken
oracle.

---

## 1. Inventory and classification

`critical` = protected tier and deserves it; `useful` = keeps its tier; `fluff` = tautology
(delete); `misplaced` = right test, wrong tier; `broken-oracle` = a security test that never
checks the denied path (or an oracle that cannot fail).

### 1.1 `crates/blastit-sqlpolicy/tests/critical_query_template.rs` (new, 788 lines, 24 cases)

| Test | Class | Note |
|---|---|---|
| `every_rewrite_row_is_accepted_and_its_documented_expansion_parses` | critical | 26 rows of §13.4.2. The `written != expanded ⇒ parse(written).is_err()` pair is what makes `Ok` mean "the rewrite did it" — a no-rewrite implementation reds. The **exact** expansion text is pinned one tier down (`template.rs` `every_expansion_row_is_the_tables_own_text`, over `expand_template`); acceptable because numbering is a validation artefact (`normalize` folds `$n` → `$?`) and a non-deleted `, ...` cannot parse |
| `a_two_element_in_list_is_legal_and_a_second_placeholder_before_the_marker_is_not` | critical | the page's "a list with no `...` is legal at any length" plus rule 6 |
| `a_rewrite_never_swallows_a_separator_or_a_second_statement` | critical | S4: `;` between comma and marker, `;` inside a repeated `VALUES` row, `;` inside a quote |
| `a_marker_inside_a_string_or_a_comment_is_not_a_marker` | critical | data vs marker, both directions |
| `a_qualified_in_or_values_is_not_the_keyword` | critical | `s.in (…)`, `fn (…, ...)`, `s.values …` — the token is the whole word |
| `the_first_stray_marker_in_the_text_is_the_one_reported` | critical | first-stray determinism |
| `a_longer_run_of_dots_is_a_stray_marker_too` | critical | rule 6's "longer run" |
| `an_unterminated_group_holds_no_legal_marker` | critical | an unbalanced `(` cannot legalise `...` |
| `an_empty_query_is_refused_at_zero` | critical | 8 spellings, `kind` **and** offset |
| `a_second_statement_is_refused_at_its_first_character` | critical | offset 10 / newline case / leading `;` |
| `a_stray_marker_is_refused_at_its_first_dot` | critical | offsets 15, 0, and a quoted decoy |
| `a_parse_error_is_refused_at_the_first_non_trivia_code_character` | critical | the 0/0/1 pin of §13.4.4 plus comment-skipping |
| `a_statement_over_the_cap_is_refused_not_grown` | critical | 65 536 chars **of `é`** (over the cap in bytes, at it in chars) accepted; +1 refused — kills a byte-count cap |
| `the_1025th_placeholder_is_refused_at_its_own_position` | critical | `MAX_TEMPLATE_PARAMS` boundary, and the two chars at the offset are `$?` |
| `the_file_wide_refusal_is_constructible_and_says_the_pages_sentence` | useful | half a constructor assertion, but it pins the sentence and `MAX_TEMPLATE_PARSES` |
| `every_sentence_is_the_reference_pages_and_one_line` | critical | the seven sentences, byte for byte, one line each |
| `a_refusal_never_renders_the_statement` | critical | S1/S2: an unterminated literal is where PostgreSQL quotes the **value**; checked on `Display` and `Debug` |
| `offsets_are_counted_in_characters_and_the_byte_distance_is_asserted` | critical | `café`: offset 15, byte 16 |
| `every_accepted_fixture_is_accepted` | useful | a 29-shape accept-list; the one-statement property for these lives in the unit tier |
| `the_quick_fix_rewrite_is_canonical` | critical | literal → `$?`, `$n` → `$?`, `IN`/`VALUES` collapse, idempotence |
| `the_quick_fix_is_never_offered_where_it_would_widen_a_capability` | critical | `pg_read_file`, `lo_import`, `dblink`, `COPY … PROGRAM`, a typo, two statements, `…`, the two exempt statements, over-cap |
| `the_quick_fix_is_offered_at_the_cap_and_refused_one_character_over_it` | critical | both sides of the boundary |
| `every_quick_fix_result_passes_the_template_check` | critical | `Some` asserted (never an `if let`), the result re-checks, and `normalize(new) == new` |
| `the_literal_offset_is_the_whole_token_including_its_prefix` | critical | all seven literal spellings (`E'`, `B'`, `X'`, `N'`, `U&'`, `$$`, `'`) point at the quote `redact_literals` replaces |

No fluff, no broken oracle. The suite asserts the denied path wherever it asserts the allowed
path (`pg_read_file`/`COPY … PROGRAM` refused; `SELECT id FROM` refused; `IN ($?, $?, ...)`
refused).

### 1.2 `crates/blastit-manifest/tests/critical_query_sites.rs` (new, 261 lines, 4 cases)

| Test | Class | Note |
|---|---|---|
| `the_origin_is_the_first_character_inside_the_quote` | critical | 9:16 for `query ''` (the **closing** quote) and for `SELCT`; four field assertions on the site; `Position::NONE` unreachable for a parsed document |
| `the_escape_map_survives_a_multiline_double_quoted_query` | critical | `\"` costs two columns, a raw newline moves the line, a `\n` **escape** does not; (7,21) and (5,44) are hand-computed, not read back |
| `a_duplicate_query_gets_one_site_and_one_sentence` | critical | R35: one `queries()`, one `query_sites()`, one diagnostic at the first site, the line 6 spelling silent |
| `a_programmatic_grant_keeps_none` | critical | `PgGrant::new` → one site, `NONE`, and the rendered error has no `0:0` prefix |

### 1.3 `crates/blastit/tests/critical_lsp.rs` (+597 lines, 10 new cases; 52 in the file)

| Test | Class | Note |
|---|---|---|
| `code_action_provider_is_advertised_and_alone` (C1) | critical | exact key list; no `codeActionKinds`, no `resolveProvider` |
| `a_literal_query_gets_one_quick_fix` (C2) | critical | one action, exact title/kind, one uri, one edit over the **whole** interior, applied → `check` clean, then `[]` |
| `both_spellings_are_parameterized` (C3) | critical | `"…"` literal and `'…'` `$1`/`$2`; both ranges exact; both re-check clean |
| `nothing_is_offered_where_nothing_can_change` (C4) | critical | clean, `SET TIME ZONE`, `…` |
| `context_diagnostics_is_never_trusted` (C5) | critical | empty/fabricated/`only`/absent all give the same one action |
| `a_range_off_every_query_offers_nothing` (C6) | critical | comment, `body none`, `regex`, closing quote, unknown uri — and one real action |
| `the_edit_range_is_utf16_across_a_surrogate_pair` (C7) | critical | start 15 / end 70; `😀` above the query; a `char` count gives 69, a byte count 72 |
| `code_action_params_are_validated` (C8) | critical | `-32602` ×3, the session survives, the valid request still answers |
| `hover_explains_the_placeholder` (H7) | critical | `$`, `?`, next char; range 46–48; the closing quote is outside |
| `hover_explains_the_variadic_marker` (H8) | critical | three dots, +3 is `null`, `$?` in a comment and in `regex` are `null`, `$DATABASE_URL` still answers |

Both hovers compare against `docs/reference/blastfile.md`'s blockquotes (`page_paragraph`), so
the reply **is** the page: a page edit the editor does not follow reds here. `encode_interior`'s
`\`/`"` escapes and the §7A.4 order are unit-tested
(`lsp/mod.rs::the_interior_is_re_encoded_for_its_spelling`).

### 1.4 Grammar and guard (standard tier, `make zed-check` inside `make check`)

| Artefact | Class | Note |
|---|---|---|
| `test/corpus/query-sql.txt` (22 cases), `query-hash.txt` (4), `query-spelling.txt` (4) | useful | the `sql_*` child nodes are named in the expectations, so a reverted mechanism would red `tree-sitter test` — but **no gate runs `tree-sitter test`** (§5.3) |
| `scripts/test_check_zed_extension.py` (26 cases) | useful | each builds a faithful `editors/` copy in a temp git repo, doctors one property, and requires the exact `FAIL` set; the baseline must be green. Covers a new capture name, a missing `sql_*` capture, `query` back in `keyword`, a stale `node-types.json`, a committed wasm, the `rev` shape, the `settings.json` switch and the docs fence |
| `scripts/check_zed_extension.py`'s fixture loop | useful | `tree-sitter parse` over ≥23 in-repo `Blastfile`s, zero `ERROR`/`MISSING`; **skips** (with a note, not a silent `ok`) when the pin is absent, unless `--require-parse` |

Tier is right: highlighting is DX, not security; the `sql_string` grammar is read-only and
cannot affect validation, so standard tier plus `make check` is the correct home.

---

## 2. Requirement map (claim → the test that proves it)

| Claim | Where | Proved by | Status |
|---|---|---|---|
| the input is the **raw decoded** text, not `normalize`'s output | §13.4.1 | `validate.rs::a_defect_on_the_third_line_of_a_query_follows_the_newline` (7,15), `::an_escaped_double_quoted_query_points_at_the_sql_token` (5,49) | proved (standard tier) |
| a parse, not a policy: `pg_read_file($?)`, `dblink($?)` are **legal** here | §13.4.1 | `critical_query_template.rs` `REWRITE_ROWS` rows 28–29 + `every_accepted_fixture_is_accepted` | proved |
| every `pg_query::Error` variant is `Syntax`; no arm unwraps | §13.4.1 (S13) | `template.rs::nothing_a_caller_can_write_makes_the_checks_panic` | proved (unit tier) |
| one parse per distinct entry; the 4 097th is `TooManyQueries` at its own site | §13.4.2 | `validate.rs::a_file_past_the_parse_cap_is_refused_at_the_entry_past_it` (4101,16) | **unprotected** → gap G5 |
| rows 1–5 expand as the table says | §13.4.2 | `critical_query_template.rs` (the observable side) + `template.rs::every_expansion_row_is_the_tables_own_text` (the text) | proved |
| rules 6–8 are decided before any deletion | §13.4.2 (S4) | `a_rewrite_never_swallows_a_separator_or_a_second_statement`, `an_unterminated_group_holds_no_legal_marker` | proved |
| seven refusal sentences, verbatim, one line | §13.4.3 | `every_sentence_is_the_reference_pages_and_one_line` | proved |
| every refusal carries a site; `NONE` is never reached for a parsed document | §13.4.3/§18.1 | `critical_query_sites` (Empty, Syntax, StrayEllipsis, MultipleStatements, `…`, literal) + `critical_manifest_011`'s five fixtures | proved |
| `pg_query`'s message is never rendered, stored or logged | §13.4.4 (S1/S2) | `a_refusal_never_renders_the_statement` (`Display` **and** `Debug`) | proved |
| `Syntax`'s offset is the first **non-trivia code** character (0, 0, 1) | §13.4.4 | `a_parse_error_is_refused_at_the_first_non_trivia_code_character` | proved |
| the origin is `token.start + 1`; `query ''` points at the closing quote | §13.4.5 | `the_origin_is_the_first_character_inside_the_quote` | proved |
| a `"…"` escape costs two source columns; a raw newline moves the line; a `\n` escape does not | §13.4.5 | `the_escape_map_survives_a_multiline_double_quoted_query` | proved |
| chars, not bytes, on both sides (`café`) | §13.4.5 | `offsets_are_counted_in_characters_and_the_byte_distance_is_asserted` | proved |
| dedupe keeps the first site, and the duplicate gets nothing | §13.4.5 (R35) | `a_duplicate_query_gets_one_site_and_one_sentence` | proved |
| `Debug` redacts `raw` | §13.4.5 (S11) | `validate.rs::a_query_site_never_renders_the_statement_it_carries` | **unprotected** → gap G2 |
| `PgGrant`'s `PartialEq` ignores the sites | §13.4.5 | `critical_blastfile_parity.rs` (expected grants built with `PgGrant::new`, compared with `==`) | proved by construction |
| a non-zero `UidLayout` base shifts no line or column | §13.4.5 | — | **untested** → gap G3 |
| `parameterize`: literal/`$n`/`IN`/`VALUES`, exemption, `…`, idempotence, cap, own-output gate | §13.4.6 (R36) | `the_quick_fix_rewrite_is_canonical`, `the_quick_fix_is_never_offered_where_it_would_widen_a_capability`, `the_quick_fix_is_offered_at_the_cap…` | proved |
| `parameterize` is one implementation, called by the editor | §13.4.6 (R66) | `the_lsp_module_is_a_text_tool` (no `fn lex/parse/lower`) | partially; nothing greps for a copied rewrite (minor, §5.4) |
| `Lines::offset`/`point_at` are the byte image of §4.2's units | R60 | `lsp/mod.rs::p7_the_offset_is_bytes_and_the_point_is_utf16` + C7 | proved |
| hover `$?`/`...` only inside a `query` interior; `$IDENT` untouched | §7.6 (R61) | H7, H8 | proved |
| `$1…$n` is not a token this hover explains | §7.6 | `lsp/mod.rs::the_two_sql_tokens_are_source_spans` | proved (unit tier) |
| `codeAction` advertised as `codeActionProvider: true`, nothing else | §7A (R62) | C1 | proved |
| at most one `quickfix`; one uri; one `TextEdit` over the whole interior; no `diagnostics`/`isPreferred`/`command` | §7A.3 | C2, C3 | proved |
| the edit is re-derived from the tracked text; `context` never read | §7A.1, §13.3 (S8) | C5 | proved |
| unknown uri → `[]`, not an error | §7A.1 | C6 | proved |
| an **inverted** range → `[]` | §7A.2 | — | **untested** → gap G4 |
| the edit re-encodes the `"…"` spelling, `\` first | §7A.4 | `lsp/mod.rs::the_interior_is_re_encoded_for_its_spelling` (`\`/`"` only) | partially → gap G4 |
| the applied edit re-checks clean and is a real change | §7A.3 | C2, C3, C7 | proved |
| malformed params → `-32602`, never a panic | §7A.5 (C8) | C8 | proved |
| one parse per edit (`codeAction` shares the publish parse) | §13.3 (S8) | `query_sites()` over `OnceCell` (`document.sites`) | proved by code shape, not a test (acceptable) |
| the page's `9:16`, `11:26`, `13:31` are literals a test asserts | §20 R34 gate column, T3–T10 | only `9:16` (protected) and column `26` on line 5 | **gap G3** |
| the LSP publishes a `QueryTemplate` squiggle at the CLI's line:col | epic acceptance | `docs/testing/E2E-MB.34.md` (recorded by hand) | **no gated test** → gap G1 |
| `check --json` on a refused `query`: exit 2, empty stdout, `line:col` on stderr | `docs/ci.md` (MB.34.1) | E2E-MB.34 §2 (recorded by hand) | **no gated test** → gap G6 |
| `sql_string`/`sql_keyword`/`sql_parameter` trees cannot rot | §9.2a, R63/R65 | the corpus files exist; `make zed-check` checks *capture* names and (optionally) `parse` | **no gate runs `tree-sitter test`** → gap G7 |

---

## 3. Mutation testing

### 3.1 `blastit-sqlpolicy` (read from `MB.34.3`, not re-run)

`plan/nodes/MB/MB.34/MB.34.3/RESULT.md` records the campaign against `src/template.rs`
(`cargo mutants -p blastit-sqlpolicy --file …/template.rs --no-shuffle --timeout 120`):
**134 mutants, 17 min, 122 caught, 6 missed, 6 unviable, 0 timeout — 122/128 killable =
95.3 %**, far above the 75 % bar. This audit read that result instead of repeating a
17-minute run, as the brief instructed. The six survivors are each argued equivalent or
unreachable with a line-level reason (the `0 => Empty` arm, `is_empty`'s two mutants,
`second_statement`'s `<` at the last index, a leading-dot number's token kind,
`is_ident_start → true`); none is an untested path, and two real misses from that campaign were
fixed with tests rather than excused.

### 3.2 `blastit-manifest` — the position logic (this audit's run)

`cargo-mutants` 27.1.0 is installed. Two campaigns were attempted through the repo harness:

1. `make mutants-rust PKG=blastit-manifest MUTANTS_TMP=/tmp/opencode/tc-manifest-muttmp`
   lists **1 505 mutants**. Measured throughput on this host: the unmutated baseline is 50 s
   build + 13 s test, and the first mutant took 2 min 3 s because every mutation relinks all
   ~30 integration-test binaries of the crate. At `-j 2` a 1 505-mutant campaign is ~10 h —
   longer than any audit window, and `AUDIT-MB.21` already records OOM deaths on 300–400
   mutant campaigns. It was stopped; `git status --porcelain -- crates` is empty (no source
   file left mutated) and the scratch copies were removed (4.5 GB → 2.9 MB).
2. The scoped campaign below was used instead: the **new/changed functions only**
   (`--re 'position_at|query_sites|with_query_sites|check_queries|escape_indices|unsited'` →
   23 mutants), with the **protected tier** as the test selection so the number answers "does
   the protected suite kill the position logic?" rather than "does a crate mostly about other
   features get killed?".

```console
$ TMPDIR=/tmp/opencode/tc-manifest-muttmp MUTANTS_JOBS=2 MUTANTS_TIMEOUT=120 \
    scripts/mutants-run.sh -p blastit-manifest -o /tmp/opencode/tc-manifest-muttmp/results-protected \
    --re 'position_at|query_sites|with_query_sites|check_queries|escape_indices|unsited' \
    --cargo-test-arg=--test --cargo-test-arg=critical_query_sites \
    --cargo-test-arg=--test --cargo-test-arg=critical_manifest_011
Found 23 mutants to test
ok       Unmutated baseline in 56s build + 0s test
MISSED   crates/blastit-manifest/src/model.rs:245:19: replace < with > in QuerySite::position_at
23 mutants tested in 9m: 1 missed, 20 caught, 2 unviable
$ wc -l results-protected/mutants.out/{caught,missed,timeout,unviable}.txt
  20 caught · 1 missed · 0 timeout · 2 unviable
```

**20/21 killable = 95.2 % against the protected tier alone**, 0 timeouts, well above the
75 % bar. The 20 caught include every arithmetic mutant of `position_at` (`+=` toward `-=` and
`*=`, `==` toward `!=`, the `<` boundary as `<=` and `==`), both `escape_indices` maps and its
two operators, `check_queries -> ()`, `query_sites -> &[]`, `with_query_sites -> Default`, and
`position_at -> Default` — i.e. the origin/escape/dedupe oracles and the one-sentence chain are
real. One `parser.rs` mutant outside MB.34's diff was caught too.

| Survivor | Verdict |
|---|---|
| `model.rs:245:19 replace < with > in QuerySite::position_at` | **Equivalent in production, misplaced in tier.** The arm is `if walked < offset { return self.at }`, the past-the-end fallback. From a document no caller can produce `offset > raw.chars().count()`, so no editor behaviour changes; the rule exists so a buggy caller gets the token's own origin instead of an invented column (§13.4.5). Its only oracle is `validate.rs::a_site_points_at_the_origin_when_the_offset_is_past_the_text` (`position_at(7) == at(5,16)` for `raw = "SELECT"`; the mutant answers `(5,22)`) — so the **standard** tier kills it and the protected tier does not. Fix the placement (gap G8); do not rewrite the code. |
| `model.rs:204:9 unsited -> Self with Default::default()` (unviable) | Does not compile: `QuerySite` has no `Default`. Excluded from the denominator, correctly. |
| `model.rs:377:9 query_sites -> Vec::leak(vec![Default::default()])` (unviable) | Same reason. The sibling `Vec::leak(Vec::new())` mutant **is** viable and was caught. |

The epic therefore clears the mutation bar on both crates (sqlpolicy 95.3 % from `MB.34.3`,
manifest position logic 95.2 % protected / 100 % across both tiers), and the single survivor is
a tier-placement finding rather than a missing assertion.

---

## 4. Tier verdict

The epic's own rule (a position/redaction rule is critical; a formatting helper is not) is
applied correctly, with two placement exceptions:

* **Right:** the template engine, every refusal kind and its offset, the Quick Fix's rewrite
  and every "do not offer it" case, the origin/escape/dedupe oracles, the capability document,
  the `codeAction` reply, both SQL hovers and the UTF-16 edit range are all `critical`. Each is
  a contract a developer or an editor observes, and each can regress silently.
* **Right:** the grammar corpus, the highlight captures and the Zed guard's self-tests are
  `standard` (DX surface, no security consequence).
* **Misplaced (small):** the `QuerySite` `Debug` redaction — a *secret-leak* rule (S11) —
  lives only in `tests/validate.rs` (gap G2), and the `TooManyQueries` **enforcement** (a DoS
  bound) lives only in `tests/validate.rs` (gap G5). Both are one-tier upgrades of tests that
  already exist, not new coverage.
* **Not fluff:** the two "weakest" cases (`every_accepted_fixture_is_accepted`,
  `the_file_wide_refusal_is_constructible_and_says_the_pages_sentence`) each pin something a
  mutation can break (an accept-list entry, a sentence, a constant). No test in the four files
  can be deleted without losing a stated claim.

---

## 5. No weakening, and the guard

### 5.1 Diff of the protected tier (`git diff 5f29398..HEAD -- 'crates/*/tests/critical_*.rs'`)

```console
$ git diff --numstat 5f29398..HEAD -- 'crates/*/tests/critical_*.rs'
5	0	crates/blastit-manifest/tests/critical_manifest_011.rs
261	0	crates/blastit-manifest/tests/critical_query_sites.rs
788	0	crates/blastit-sqlpolicy/tests/critical_query_template.rs
597	0	crates/blastit/tests/critical_lsp.rs
```

**Additions only — 0 deleted lines in all four files.** `critical_manifest_011.rs`'s five
hunks are the `at:` field on five fixtures that are parsed from source, so each `at` is a real
`Position` (5,34 / 5,72 / 5,84 / 4,23 / 5,23) and `Position::NONE` — which would make the
assertion satisfiable by a wrong answer — appears in none of them. `critical_lsp.rs` is one
hunk at `@@ -1404,6 +1404,603 @@`: 603 added, 0 removed; the ten pre-existing messages and
the hygiene block are untouched.

`tests/validate.rs` (unprotected) also has exactly one deleted line — an import list — and no
removed assertion.

### 5.2 `tools/critical_guard.py`

```console
$ uv run --no-project tools/critical_guard.py --base 5f29398 --report
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  A  crates/blastit-manifest/tests/critical_query_sites.rs  [added (always allowed)]
  A  crates/blastit-sqlpolicy/tests/critical_query_template.rs  [added (always allowed)]
  M  crates/blastit/tests/critical_lsp.rs  [VIOLATION: not covered by any node path]
  report only: 2 breaking change(s) would fail an enforced run

$ uv run --no-project tools/critical_guard.py --base 5f29398 \
    --node MB.34.4 --node MB.34.5 --node MB.34.3 --node MB.34.6
critical-guard: OK: all 4 protected change(s) are authorized by node(s) MB.34.4, MB.34.5, MB.34.3, MB.34.6
EXIT=0
```

The two `M` rows are authorized by `MB.34.4` (`critical_manifest_011.rs` is in its `paths`) and
`MB.34.5` (`crates/blastit/tests/critical_lsp.rs` is in its `paths`). The whole branch passes
when the epic's nodes are named together — the form a release PR's reviewer runs.

### 5.3 What no gate runs

`grep -rn "tree-sitter test" Makefile .github/workflows scripts/` finds nothing: the 34 corpus
expectations (30 of them in the three new `query-*` files, which name `sql_*` children 58 times)
are exercised only by hand in `MB.34.6`'s `RESULT.md`. `make zed-check` runs `tree-sitter parse` (no `ERROR`/`MISSING`)
over the fixture corpus and skips — with a note — when the pinned binary is absent. So a
tokenisation regression that keeps the text parseable but changes the `sql_*` children is
invisible to every gate (gap G7).

### 5.4 Minor observations (no node)

* §13.3's caching claim ("one parse per edit") is true by construction (`OnceCell` on
  `Document`) and untested; a test would need a parse counter the crate does not expose.
* R66's "no copied rewrite in `lsp/`" has no grep test (`the_lsp_module_is_a_text_tool` bans
  `fn lex/parse/lower`, not a second `parameterize`). The current code calls the crate's
  function; the absence of a guard is a small drift risk, not a defect.
* `encode_interior`'s newline/tab arms are unreachable through `parameterize` today
  (`normalize` collapses whitespace), so the missing assertion is defensive only (gap G4 keeps
  the reachable half: the `\"` case through the handler).

---

## 6. Gaps — ready to file (one `plan.py new` task each, under the epic)

All `--kind task --owner implementer`, filed under `MB.34.7` as **MB.34.7.2–.9** in this
order (G1 → `MB.34.7.9`; the 05:46 `lead` reconciliation superseded both its own and this
audit's first copy of G1, so G1 was re-filed):
G1 `.9`, G2 `.2`, G3 `.3`, G4 `.4`, G5 `.5`, G6 `.6`, G7 `.7`, G8 `.8`.

### G1 — `critical_lsp.rs`: the published `QueryTemplate` squiggle is the CLI's `line:col`

* **Why:** the epic's headline DX promise ("the squiggle starts at the `SELCT`, not at
  `(0,0)`") is proved today only by the hand-recorded `docs/testing/E2E-MB.34.md`; the model
  side is protected, the LSP `publishDiagnostics` range for `QueryTemplate`/`LiteralQuery` is
  not.
* **Paths:** `crates/blastit/tests/critical_lsp.rs`
* **Acceptance:** a test in the protected file opens the four-defect fixture (`SELCT` typo,
  multi-line missing `FROM`, inline literal in the `"…"` spelling, redacted `…`) and asserts
  each `publishDiagnostics` `range.start`/`range.end` equals the CLI's `line:col` mapped to
  0-based UTF-16 — (4,15)–(4,20); (5,15)–(5,21); (7,53)–(7,54); (8,53)–(8,54) — that the
  `message` equals `ManifestError::message()` with no `line:col` prefix, that the CLI text and
  the LSP range agree on line **and** column, and that no diagnostic sits at `(0,0)`;
  `cargo test -p blastit --test critical_lsp` passes and `uv run --no-project
  tools/critical_guard.py --base <commit> --node <this node>` is green.

### G2 — protect the `QuerySite` `Debug` redaction

* **Why:** §13.4.5 (S11) is a leak rule ("never log a statement's value"), and its only test
  (`validate.rs::a_query_site_never_renders_the_statement_it_carries`) is unprotected.
* **Paths:** `crates/blastit-manifest/tests/critical_query_sites.rs`
* **Acceptance:** a protected case builds `query "SELECT 's3cr3t' FROM t"`, asserts
  `query_sites()[0].raw` holds the value, and that `format!("{:?}", site)`,
  `format!("{:?}", grant.query_sites())` **and** `format!("{:?}", grant)` contain no `s3cr3t`
  and do contain `SELECT … FROM t`; `cargo test -p blastit-manifest` green.

### G3 — the reference page's three site literals, and a non-zero base

* **Why:** §20's R34 gate column and the accepted T3–T10 resolution require the page's own
  `9:16`, `11:26`, `13:31` to be asserted literals. Today only `9:16` exists (protected) and
  `:26` appears on line 5, so a page edit can drift from the truth with every test green.
  §13.4.5's "`base` shifts no line or column" is untested.
* **Paths:** `crates/blastit-manifest/tests/validate.rs`, `docs/reference/blastfile.md`
* **Acceptance:** one fixture with the page's entries on lines **9, 11 and 13** at 8-space
  indent asserts `Position::at(9,16)` (`query ''`), `at(11,26)` (`SELECT 1; SELECT 2`) and
  `at(13,31)` (`SELECT id FROM ...`); the same document parsed with
  `Manifest::parse_blastfile(source, 20_000)` yields the same three positions; the page's table
  still says `9:16`/`11:26`/`13:31`; `cargo test -p blastit-manifest` green.

### G4 — the `codeAction` edge branches: inverted range, and the escaped `"…"` interior end to end

* **Why:** §7A.2's inverted-range `[]` and §7A.4's re-encoding are untested through the
  handler; only `encode_interior`'s `\`/`"` arms are unit-tested, and the newline/tab arms
  have no test at all.
* **Paths:** `crates/blastit/src/lsp/mod.rs`, `crates/blastit/tests/critical_lsp.rs`
* **Acceptance:** a `codeAction` whose `start` is after its `end` (same uri, same line)
  returns `[]` and **not** `-32602`; a `query "SELECT \"o\".id FROM t WHERE email = 'a@b.com'"`
  document returns exactly one action whose `newText` is
  `SELECT \"o\".id FROM t WHERE email = $?`, whose applied text re-checks clean, and whose
  `range` is the whole interior; unit tests cover `encode_interior` for a literal newline and
  a tab; `cargo test -p blastit --lib lsp --test critical_lsp` green.

### G5 — protect the file-wide parse cap's enforcement

* **Why:** `MAX_TEMPLATE_PARSES` is a DoS bound (a 16 MiB file, tens of thousands of `query`
  lines); the constant is protected but its enforcement (the 4 097th distinct entry refused at
  its own `at`) lives only in `tests/validate.rs`.
* **Paths:** `crates/blastit-manifest/tests/critical_query_sites.rs`
* **Acceptance:** a protected case writes 4 097 distinct `query` lines and asserts exactly one
  `ManifestError::QueryTemplate { at: Position::at(4101, 16), kind: TooManyQueries }`, that the
  first 4 096 are not refused, and that the message is the page's sentence with no `line:col`
  prefix; `cargo test -p blastit-manifest` green.

### G6 — `blastit check --json` on a refused `query`: exit 2, empty stdout, `line:col` on stderr

* **Why:** `docs/ci.md`'s new paragraph is a CI contract ("an empty stdout is a refusal, never
  a clean audit; no `kind`, no `fix.action`"); it is proved only by the recorded E2E
  transcript. A CI script that treats empty stdout as clean would silently pass a broken
  manifest.
* **Paths:** `python/tests/test_check.py`, `crates/blastit/tests/critical_static_cli.rs`
* **Acceptance:** a `Blastfile` holding `query 'SELCT id FROM t'` run through `blastit check
  --json` exits `2`, writes zero bytes to stdout, and writes exactly one stderr line whose
  prefix is the fixture's `line:col` and whose text is the page's `Syntax` sentence; the same
  fixture through plain `check` exits `2` with the same line; the assertion fails if the
  refusal is turned into a JSON finding.

### G7 — run the tree-sitter corpus expectations in a gate

* **Why:** three new corpus files encode §9.2a's trees, and no gate runs `tree-sitter test`;
  `make zed-check`'s loop only checks that fixtures parse without `ERROR`/`MISSING`. A
  regression that keeps `sql_string` but changes which children it holds passes every gate.
* **Paths:** `scripts/check_zed_extension.py`, `scripts/test_check_zed_extension.py`
* **Acceptance:** `make zed-check` runs `tree-sitter test` for
  `editors/tree-sitter-blastfile` when the pinned generator is present and fails on a
  mismatch; the guard's `--require-parse` mode also requires the corpus run; a self-test
  doctors one expectation in a copied corpus file and requires the exact `FAIL` line; the
  guard's skip path still prints a note (never a silent `ok`) on a host without the pin.

### G8 — protect `position_at`'s past-the-end fallback

* **Why:** it is the one mutant the protected tier misses (§3.2). The rule is §13.4.5's "a
  mapping that cannot be computed is a refusal at the token": an offset past `raw` falls back
  to the site's own origin instead of inventing a column. Its only oracle today is
  `validate.rs::a_site_points_at_the_origin_when_the_offset_is_past_the_text`.
* **Paths:** `crates/blastit-manifest/tests/critical_query_sites.rs`
* **Acceptance:** a protected case builds a `QuerySite` (fields are public) with
  `at: Position::at(5,16)`, `raw: "SELECT"` and asserts `position_at(0) == at(5,16)`,
  `position_at(6) == at(5,22)` (the end is a real offset) and `position_at(7) ==
  position_at(u32::MAX) == at(5,16)`; a `Position::NONE` site answers `NONE` for every offset.
  Re-running the §3.2 campaign must report `0 missed`, 21/21 killable.

---

## 7. Overall verdict

**PASS with follow-ups.** The epic's tests are honest: the protected tier covers every
security- and DX-bearing claim the design makes for the span contract, the template engine and
the editor surfaces; the oracles are hand-computed positions, not values read back from the
code; the denied/adversarial path is asserted wherever the allowed path is; no protected case
was weakened; and the position arithmetic survives mutation testing (§3.2). The remaining
`blastit-manifest` weak spots are *placement*, not absence (G2, G5). The gaps are the **last
mile**: the published squiggle (G1), the page's own numbers (G3), the CI refusal contract (G6)
and the corpus gate (G7) rest on a hand-recorded transcript or on no gate at all, and G8 is
the one assertion the protected tier does not yet carry. None of them
contradicts a shipped sentence, and all seven are cheap to close.
