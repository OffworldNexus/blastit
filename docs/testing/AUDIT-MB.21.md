# AUDIT MB.21.5.3 — the Blastfile toolchain (lexer, parser, CST, printer, editor)

- **Date:** 2026-11-16
- **Node:** `MB.21.5.3` (test-curator). Report path per its `# Acceptance`: `docs/testing/AUDIT-MB.21.md`.
- **Base:** `6bfd1cc` on `release/0.2.0` (audited tree = working tree; no source or test file was edited)
- **Scope:** `crates/blastit-manifest/src/blastfile/{lexer,tokens,syntax,parser,pattern,print,write,mod}.rs`,
  `crates/blastit-manifest/src/edit.rs`, and their suites (`tests/blastfile_*.rs`,
  `tests/critical_blastfile_*.rs`, `tests/edit_branches.rs`, `tests/critical_manifest_edit{,_executor}.rs`,
  `tests/docs_fences.rs`, `tests/common/mod.rs`, and the `#[cfg(test)]` modules of the files above).
- **Requirements:** design `docs/design/blastfile-dsl.md` §3 (R1–R30), §20 (the tier table), §21
  (`MB.21.5.3`'s row), §22; skills `testing`, `security-invariants`, `rust-standards`.

## Verdict

**PASS WITH GAPS.** The parsing surface is the best-tested part of the crate: every §20 row has its
fast unprivileged test, the protected tier holds 106 cases across twelve files, and **every one of the
nine modules in scope clears the 75 % kill bar** (84.6–100 %, 92.0 % over 827 accounted mutants of 828; §2.5). No case in
scope is fluff and no protected suite lost an assertion.

Two findings matter more than a missing assertion because each is a *gate a one-line edit evades*: a
test that is absent costs a future bug, but a gate that reports green while the property is false costs
the property itself:

1. **R24's purity gate is evadable** — `scripts/check_manifest_purity.py` matches the literal
   `std::env`, so `use std::{env};` plus `env::var(..)` passes. Measured in §5.1, filed as `MB.21.5.5`.
2. **`blastfile::read` has no accepted-path test in this crate** — the only successful read is a file
   of *exactly* `MAX_SOURCE_BYTES`, so two surviving mutants of the read bound are invisible here
   (the real small-file reads live in `crates/blastit/tests/blastfile_discovery.rs`). Filed as `MB.21.5.6`.

Everything else is an ordinary follow-up: P4's message precision and its untested oracle, the design's
≥ 20 corpus floor that `MB.21.5.2` did not land, and one uid-0 vacuous test.

## Method

1. Read all nine `blastfile/` modules and `edit.rs` in full, and every in-scope suite; classified each
   case `critical` / `useful` / `fluff` / `misplaced` / `broken-oracle` against the `testing` skill.
2. Mapped R1–R30 to the concrete test names that carry them, asking of every policy-bearing row
   whether a *deny-side* assertion exists (a row whose test passes with the check removed is fluff).
3. `cargo-mutants` 27.1.0 over the nine in-scope source files, through the repo's own harness:

   ```console
   $ MUTANTS_TMP=$HOME/.cache/blastit-mutants/MB.21.5.3 \
     MUTANTS_JOBS=4 MUTANTS_TIMEOUT=90 MUTANTS_MEMORY_MAX=12G \
     scripts/mutants-run.sh -p blastit-manifest -o "$MUTANTS_TMP/results" \
       -f crates/blastit-manifest/src/blastfile/lexer.rs \
       -f crates/blastit-manifest/src/blastfile/tokens.rs \
       -f crates/blastit-manifest/src/blastfile/syntax.rs \
       -f crates/blastit-manifest/src/blastfile/parser.rs \
       -f crates/blastit-manifest/src/blastfile/pattern.rs \
       -f crates/blastit-manifest/src/blastfile/print.rs \
       -f crates/blastit-manifest/src/blastfile/write.rs \
       -f crates/blastit-manifest/src/blastfile/mod.rs \
       -f crates/blastit-manifest/src/edit.rs
   ```

   `cargo-mutants` 27.1.0 writes per-outcome files only (`LESSONS.md` `MB.1.1`), so every ratio is
   `caught / (caught + missed + timeout)` computed from `mutants.out/{caught,missed,timeout,unviable}.txt`;
   `unviable` mutants are excluded from the denominator and named in §2.
4. Re-measured R24's gate on a scratch tree rather than assuming the evasion.
5. Diffed every protected suite against the revision before its last edit, counting assertions.

## 1. Inventory and classification

### 1.1 Where the cases live

| Area | Unit cases | Standard integration | Protected (`critical`) |
|---|---|---|---|
| `lexer.rs` + `tokens.rs` | 17 + 3 | — | `critical_blastfile_lexer.rs` (5) |
| `syntax.rs` (CST, P1) | 8 | corpus via `common::corpus()` | — (P1 is a module property) |
| `parser.rs` | 36 | `blastfile_errors.rs` (10), `blastfile_patterns.rs` (13) | `critical_blastfile_parser.rs` (4), `critical_blastfile_errors.rs` (6) |
| `pattern.rs` | 15 | `blastfile_patterns.rs` (13) | `critical_blastfile_patterns.rs` (7) |
| `print.rs` (`fmt`) | 14 | `blastfile_fmt.rs` (7) | `critical_blastfile_print.rs` (9) |
| `write.rs` (editor) | 4 | `edit_branches.rs` (28) | `critical_manifest_edit.rs` (24), `critical_manifest_edit_executor.rs` (5) |
| `mod.rs` (`read`, entry points) | — | `blastfile_errors.rs` (10) | `critical_blastfile_lexer.rs` (5), `critical_blastfile_security.rs` (4) |
| `edit.rs` (`write_atomic`, `EditError`) | 1 runnable doctest | `edit_branches.rs` (write half) | `critical_manifest_edit.rs` (atomic-write half) |
| docs gates | — | `docs_fences.rs` (11) | — (docs tier by design, §20) |

208 in-scope cases read (93 unit, 9 standard-suite, 106 protected). **No case in scope is fluff:**
every security/policy case in the trace below asserts an allowed **and** a denied input, and the deny
side is checked rather than implied. Three cases are `misplaced` only in the sense that their tier is
lower than the property (`docs_fences` is unpinned by design, §20), and two are weak oracles (§4).

### 1.2 Requirement trace (design §20)

Legend: **✓** = the §20 row is carried by a named case; **✓d** = the row's deny side is asserted too;
**gap** = no test (or no meaningful oracle) — see §5.

| R | Fast test (names) | Protected test | Deny side | Verdict |
|---|---|---|---|---|
| R1 lexer, one pass, no `i += 1` | `lexer.rs::every_token_kind_is_lexed`, `word_rules`, `escaped_strings`, `raw_strings`, `env_names`, `carriage_returns`, `non_ascii_bytes`, `control_bytes_are_unknown`, `positions_count_chars`, `trivia_is_kept`, `comment_at_eof`, `empty_input_is_just_eof` | `critical_blastfile_lexer.rs` (size/FIFO/token cap/hostile corpus) | ✓d (`refusal(..)` per §5.3 row) | ✓ |
| R2 `line:col` everywhere | `blastfile_errors.rs::every_refusal_is_one_line`; `parser.rs::every_refusal_row` (46 rows) | `critical_blastfile_errors.rs::every_refusal_is_positioned_and_one_line` | ✓d | ✓ |
| R3a size cap before read | — (only the at-cap read) | `critical_blastfile_lexer.rs::the_size_cap_is_checked_without_reading`, `::a_fifo_cannot_exceed_the_cap`, `::a_missing_file_and_a_directory_are_refused` | partial: no accepted read, no mode-000 *under* the cap | **gap 1** |
| R3b token/trivia cap while pushing | `lexer.rs::the_token_cap_trips_while_pushing`, `::the_trivia_cap_trips_while_pushing` (lowered `Caps`) | `critical_blastfile_lexer.rs::the_token_cap_is_reachable_through_the_public_entry_point` | ✓d (at-cap lexes) | ✓ (production trivia boundary only through the hostile corpus) |
| R4 LL(1), depth ≤ 2, no recursion | `parser.rs::a_third_block_is_refused_at_its_brace`, `::a_wall_of_braces_is_refused_without_recursion`, `::a_list_holds_at_most_256_items` | `critical_blastfile_parser.rs::depth_two_is_the_limit`, `::a_wall_of_braces_is_refused_without_recursion`, `::a_half_megabyte_of_views_lowers` | ✓d | ✓ |
| R5 lossless CST | `syntax.rs::p1_lossless`, `::p1_lossless_over_every_form`, `::p1_lossless_over_the_corpus`, `::every_element_is_visited_once` | — (structural property) | n/a | ✓ |
| R6 `fmt` canonical, idempotent, comments kept | `blastfile_fmt.rs::p2_idempotent_over_the_corpus`; `print.rs::p2_canonical_fixed_point`, `::every_layout_rule`, `::comments_survive_once` | `critical_blastfile_print.rs::p2_fixed_point_over_the_corpus`, `::p2_fixed_point_over_every_production`, `::p2b_comments_survive`, `::the_corpus_is_canonical` | ✓d (a dropped comment fails P2b) | ✓ |
| R7 `fmt` preserves meaning | `blastfile_fmt.rs::p3_meaning_preserved_over_the_corpus` | `critical_blastfile_print.rs::p3_over_the_corpus`, `::p3_over_every_production` | ✓d | ✓ |
| R8 lowering parity | `critical_blastfile_parity.rs` (24 rows, expected models built in the test) | same file | ✓d (`assert_dropped_rows`, warnings) | ✓ |
| R9 patterns, `HEAD` | `pattern.rs::the_table`, `::the_emitted_fragments`, `::literal_bytes_are_escaped`, `::the_refusal_matrix`, `::the_method_table`; `blastfile_patterns.rs` | `critical_blastfile_patterns.rs` (7: traversal, sibling widening, 4 097-byte, unbalanced `)`) | ✓d | ✓ |
| R10 / R10a routing, static, precedence | scheduler crate (`critical_rewrite`, `critical_static_*`) — outside this node's files | those suites | ✓d | covered by `MB.21.4.2` |
| R11 `body` / `raw unvalidated` | `blastfile_body.rs` (8) | `critical_blastfile_body.rs` (6) | ✓d (`unvalidated` beside `json` refused, one spelling) | ✓ |
| R12 traits and `use` | `blastfile_traits.rs` (11) | `critical_manifest_011.rs`, `critical_blastfile_traits.rs` (7) | ✓d (unknown/duplicate/egress/unrestricted) | ✓ |
| R13 `entrypoint php`, `workers` | `blastfile_entrypoint.rs` (12) | `critical_php_manifest.rs`, `critical_blastfile_entrypoint.rs` (6) | ✓d (`workers` at every site, one entrypoint, PHP path rules) | ✓ |
| R14 grants | `blastfile_grants.rs` (14) | `critical_manifest_011.rs` | ✓d | ✓ |
| R15 surgical editor + P4 | `edit_branches.rs` (28) | `critical_manifest_edit.rs` (24), `critical_manifest_edit_executor.rs` (5) | ✓d (literal, redacted, unknown store, conflict, verification) | ✓ (gaps 4–5 are the two oracle holes) |
| R16 / R17 discovery, no detection | `crates/blastit/tests/blastfile_discovery.rs` | `critical_config_and_presets.rs` | ✓d | covered by `MB.21.4.1` |
| R18 fence census | `docs_fences.rs` (11, docs tier **not** protected — by §20) | — | ✓d (self-tests both directions) | ✓ |
| R19 `blastit fmt` CLI | `crates/blastit/tests/fmt_command.rs` | `critical_static_cli.rs` | ✓d | outside this node's files |
| R20 / R24 `$IDENT` is a name | `critical_blastfile_security.rs::parse_never_reads_env` | same | ✓d (`HOME` set, value absent; unset name carried as a name) | **gap 2** (gate evadable) |
| R21 no partial manifest | `blastfile_errors.rs::a_refused_document_yields_no_manifest` | `critical_blastfile_security.rs::an_error_never_yields_a_manifest` | ✓d (both entry points) | ✓ |
| R22 `check` findings | `crates/blastit/tests/critical_check_engine.rs` | same | ✓d | outside this node's files |
| R23 no `unsafe`/`unwrap`/indexing | `#![forbid(unsafe_code)]` + five `clippy` denials in `blastfile/mod.rs`; workspace clippy | — | n/a (gate) | ✓ (gate) |
| R25 `fmt` output re-parses, never grows past the cap | `blastfile_fmt.rs::output_reparses`, `::over_cap_is_refused`; `print.rs::a_printer_regression_is_refused_rather_than_written` (injects a bad printer) | `critical_blastfile_print.rs::a_query_with_a_quote`, `::fmt_refuses_a_file_it_would_grow_past_the_cap` | ✓d | ✓ |
| R26 error text: cut, escaped, never a secret | `blastfile_errors.rs::a_control_byte_is_escaped`, `::a_long_value_is_cut_at_120_characters`; `pattern.rs::a_refusal_is_one_line` | `critical_blastfile_errors.rs::a_value_is_escaped_and_cut`, `::the_regex_message_is_escaped_and_cut`, `::a_refusal_never_carries_a_secret` | ✓d | ✓ |
| R27–R29 retired name/dependency guard | `python/tests/test_docs_assets.py` + negative twin (`make docs-unit`) | — (Python gate tier) | ✓d (negative twin) | ✓ (verified in §6) |
| R30 unreached entrypoint warns, 405 with `Allow` | `validate.rs::entrypoint_not_reached` | `critical_php_manifest.rs::an_unreached_entrypoint_warns`, `critical_rewrite.rs::a_method_mismatch_is_405_with_allow` | ✓d | outside this node's files |

### 1.3 Nits with a verdict (the brief's list, each judged)

| # | Nit | Verdict |
|---|---|---|
| 1 | `UnrestrictedWithContent` is unreachable through the grammar | **equivalent**: `store_grant` sets `done = true` for `unrestricted` and returns, so no production puts a clause beside it and no document constructs the variant. A model-level test would pin dead code; it is a deletion candidate, not a test candidate. |
| 2 | `PgGrant::with_unrestricted` is `#[doc(hidden)] pub` and unused outside the crate | **dead API**: `write::expected` builds restrictions with `PgGrant::new(..)`; nothing calls it. Deletion candidate, no test owed. |
| 3 | `Manifest::from_parts` still takes `version`/`global_groups` | **useful**: `write::expected` passes both through explicitly and P4's comparison would catch a drop. Not a gap. |
| 4 | `lower.rs`'s one `#[allow(clippy::type_complexity)]` | style, not testable; outside this node's modules. |
| 5 | `critical_blastfile_fmt.rs`'s write-failure test returns silently under uid 0 | **weak oracle on a root host** (gap 7); on an unprivileged runner it proves what it says. |
| 6 | `fmt` reuses `TooLarge` for the *formatted* length | **pinned** by three tests that assert the cap number (`blastfile_fmt::over_cap_is_refused`, `critical_blastfile_print::fmt_refuses_a_file_it_would_grow_past_the_cap`, `print.rs::a_file_it_would_grow_past_the_cap_is_refused`). A dedicated variant would read better; nothing is unasserted. |
| 7 | A comment hoisted out of an unwrapped list is not covered by §14.3's owner oracle | **real but small**: `print.rs::p2b_comment_owners_survive` records each comment's parent node kind over the 15 fixtures and one commented document, none of which hoists a comment out of a list. The text-level oracle covers the shape (`critical_blastfile_print::a_comment_inside_a_list_is_kept_above_its_statement`), so the text cannot drift — only the owner-kind claim is unproven there. Folded into gap 6's task. |
| 8 | `cli.rs::one_line_text` duplicate; `fmt`'s re-parse; `edit.rs` entry points; §18 echo fixes | owned by `MB.21.4.1` (R25) and `MB.21.4.3`; **confirmed landed** (`print.rs::format_with` re-parses and `print.rs::a_printer_regression_is_refused_rather_than_written` fails if it is deleted; `apply_edits`/`write_atomic` are the only editor entry points). Nothing re-filed. |

## 2. Mutation results

### 2.1 Four launches, one reason

`cargo-mutants` re-copies the workspace per job, so the campaign lives or dies with the cgroup cap:

| Launch | Outcome dir | Jobs / cap | Result |
|---|---|---|---|
| run 1 | `results/` | 4 / 12 GiB | **OOM-killed** at 23:02 after 44 outcomes (12 GiB peak) |
| run 2 | `results-run2/` | 4 / 16 GiB | **OOM-killed** at 23:19 after 35 outcomes (16 GiB peak) |
| run 3 | `results-run3/` | 3 / 16 GiB + `ulimit -v 6 GiB` | completed `mod`, `edit`, `lexer`, `parser`, `pattern`, `print` (447 outcomes) |
| run 4 | `results-run4/` | 3 / 16 GiB + `ulimit -v 6 GiB` | `syntax`, `tokens`, `write` (341 mutants) |

The killer is not the audit: a mutant that erases a *consuming* helper (`one`, `word`, `env`, `quoted`,
`carriage_return` → `Ok(())`) makes `run()` loop without growing anything, and a mutant that removes a
token/trivia cap lets the buffers grow without bound. `--timeout 90` bounds the first class (it reports
`TIMEOUT`), and `ulimit -v` turns the second into an allocation abort instead of an OOM kill of the
whole campaign — which is what run 3 and run 4 add over runs 1–2. `git status --porcelain
crates/blastit-manifest` stayed empty throughout: `cargo-mutants` builds in its own copy.

Each file's ratio below comes from exactly one launch, so the two campaigns cannot double-count a
mutant. `unviable` mutants (the mutation does not compile, e.g. `-> Manifest with Default::default()`
where the type has no `Default`) are excluded from the denominator and counted in their own column.

### 2.2 Per-file ratios (run 3, complete modules)

```console
$ bash $HOME/.cache/blastit-mutants/MB.21.5.3/ratio.sh results-run3/mutants.out
file                   caught missed timeout unviable viable    ratio
src/blastfile/lexer.rs     78      1      9     11     88    88.6%
src/blastfile/parser.rs   206      4      0     11    210    98.1%
src/blastfile/pattern.rs   37      2      0      2     39    94.9%
src/blastfile/print.rs     72     12      0      8     84    85.7%
src/blastfile/mod.rs       11      2      0      5     13    84.6%
src/edit.rs                11      2      0      2     13    84.6%
TOTAL (run 3)             415     23      9     39    447    92.8%
```

```console
$ wc -l results-run3/mutants.out/{caught,missed,timeout,unviable}.txt
  415 caught.txt
   23 missed.txt
    9 timeout.txt
   39 unviable.txt
```

`lower.rs` is not re-run here: `MB.21.4.5` measured **93.0 %** (106/114) on the same base and nothing
in this node changed it. It is the seventh `blastfile/` module; `mod.rs` and `edit.rs` are the glue the
brief adds.

### 2.3 Run 4/5: the modules the first campaign never reached

Run 4 was killed by a host restart after 2 mutants (recorded, discarded). Run 5 re-ran
`syntax`, `tokens` and `write` as a **detached systemd user service**, which survives a session
restart (the transient `systemd-run --scope` runs 1–3 did not):

```console
$ bash ratio.sh results-run5/mutants.out
src/blastfile/syntax.rs    74      1      5     30     80    92.5%
src/blastfile/tokens.rs     5      0      0      0      5   100.0%
TOTAL (run 5)              79      1      5     30     85    92.9%
```

`syntax.rs` is **complete**: 110 mutants, of which 80 are viable (74 caught, 1 missed, 5 timeout) and
30 unviable — `-> impl Iterator` rewrites that do not compile (`once(Default::default())` on an
`Item = NodeId` iterator, ast-lowering replacement values), so they are excluded from the denominator
like every other unviable mutant. `tokens.rs` is complete at 5/5. `write.rs` never started in run 5
(the host was restarted into an 8-core/32 GiB shape with a foreign `cargo test` storm, load average
36–47), which is why §2.4 measures it separately.


### 2.4 `write.rs` against the editor's own suites

The full-suite campaign for the editor is 226 mutants; on a host at load 36–47 with a 20 s baseline
test it needs hours, and two full-suite attempts were lost (runs 1–2). The editor's only callers are
`--lib` (its unit tests and `lower`/`print`), `edit_branches`, `critical_manifest_edit` and
`critical_manifest_edit_executor`, so the campaign was scoped to exactly those targets — a mutant that
no other suite could see would appear here as a survivor, and none did:

```console
$ bash ratio.sh results-write/mutants.out
src/blastfile/write.rs    194     21      1     10    216    89.8%
```

(`226 mutants tested in 87m: 21 missed, 194 caught, 10 unviable, 1 timeouts` — the service's own
summary line. The one `TIMEOUT` is `write.rs:483:16` (`delete !` in `valid_view_name`'s caller): its log
already shows six trailing test binaries reporting `FAILED` when the 90 s cap hit, so it is killed in
substance and timed out in accounting.)

### 2.5 The whole group

| Module | Mutants | Caught | Missed | Timeout | Unviable | Kill rate |
|---|---|---|---|---|---|---|
| `src/blastfile/lexer.rs` | 99 | 78 | 1 | 9 | 11 | **88.6 %** |
| `src/blastfile/tokens.rs` | 5 | 5 | 0 | 0 | 0 | **100 %** |
| `src/blastfile/syntax.rs` | 110 | 74 | 1 | 5 | 30 | **92.5 %** |
| `src/blastfile/parser.rs` | 221 | 206 | 4 | 0 | 11 | **98.1 %** |
| `src/blastfile/pattern.rs` | 41 | 37 | 2 | 0 | 2 | **94.9 %** |
| `src/blastfile/print.rs` | 93 | 72 | 12 | 0 | 8 | **85.7 %** |
| `src/blastfile/write.rs` | 226 | 194 | 21 | 1 | 10 | **89.8 %** |
| `src/blastfile/mod.rs` | 18 | 11 | 2 | 0 | 5 | **84.6 %** |
| `src/edit.rs` | 15 | 11 | 2 | 0 | 2 | **84.6 %** |
| **Total (files above)** | **828** | **688** | **45** | **15** | **79** | **92.0 %** |
| `src/blastfile/lower.rs` (not re-run) | 132 | 106 | 8 | 0 | 18 | 93.0 % (`MB.21.4.5`) |

827 of the 828 mutants have an outcome: one `print.rs` mutant was in flight when the session was
killed, so `print.rs`'s 85.7 % is over 92 of its 93 mutants (recorded, not extrapolated).

Every module group is **above the 75 % bar**, the lowest at 84.6 % (`mod.rs` and `edit.rs`), and all
four misses in those two files are named gaps (§3 items 1–4), not equivalents. The 45 misses split into
**30 testable mutants — every one filed** — plus one boundary-only pair of mutants
(`print.rs:63:24`, `>`→`==` and `>`→`>=`), recorded rather than filed, and **13 equivalents/dead-code
mutants**, each with a one-line reason. The 15 `TIMEOUT`s are the non-terminating class (11) plus four
load artifacts whose twin already failed the suite (§3.2–§3.4).

## 3. Surviving mutants

### 3.1 Survivors with a verdict (run 3; run 5's list is appended in §3.3)

| # | Mutant (`file:line:col`) | Why it survived | Verdict |
|---|---|---|---|
| 1 | `blastfile/mod.rs:262:19` `>`→`<` in `read` | no in-crate test reads a file *under* the cap, so "every small file is `TooLarge`" is invisible here | **worth a test** → `MB.21.5.6` |
| 2 | `blastfile/mod.rs:259:39` `+`→`*` in `take(MAX_SOURCE_BYTES + 1)` | only observable when a regular file grows between `metadata` and `read`; no deterministic staging exists in one thread | **untestable by design** (equivalent in practice): the `+ 1` is the anti-race bound, and a test that wrote during the read would be a flaky race, not an oracle |
| 3 | `edit.rs:366:9` `source()`→`None` | nothing walks `EditError`'s chain | **worth a test** → `MB.21.5.9` |
| 4 | `edit.rs:367:13` delete the `Source` arm of `source()` | same | **worth a test** → `MB.21.5.9` |
| 5 | `lexer.rs:418:45` `==`→`!=` in `name_run` | `$lower` is refused; `$lower_x` is not tried | **worth a test** (message) → `MB.21.5.12` |
| 6 | `parser.rs:173:29` `&&`→`\|\|` in `starts_method` | no document writes `GET, post "/x"` | **worth a test** (message) → `MB.21.5.12` |
| 7 | `parser.rs:178:56` `&&`→`\|\|` in `starts_pattern` | no document writes `view v GET foo "/x"` | **worth a test** (message) → `MB.21.5.12` |
| 8 | `parser.rs:661:32` `peek_word() == Some("regex")`→`true` | for a Word to reach `pattern()` it is normally `regex`; the endpoint path is untried | **worth a test** (message) → `MB.21.5.12` |
| 9 | `parser.rs:1270:36` `peek_word() == Some("unrestricted")`→`true` | the guard is **unpinned**: no test writes a second bare word after a named store, so a regression that loosened it would accept `postgres db bogus` as an empty grant. The shipped parser refuses it (`parser.rs:1267-1274`, `_ => unexpected("unrestricted or {")`) — the product is correct, the oracle is missing | **coverage only**, post-0.2.0 → `MB.21.5.11` |
| 10 | `pattern.rs:138:64` `<=`→`>` in the parameter skip | an accepted parameter body holds no `{` or `*`, and every refused one is refused a few lines earlier | **equivalent** (traced: same fragment and same first refusal for every input) |
| 11 | `pattern.rs:307:73` `\|\|`→`&&` in `is_parameter_name` | no pattern names a parameter with `_` | **worth a test** → `MB.21.5.13` |
| 12 | `print.rs:63:24` `>`→`==` in `format_with` | over-cap text is still refused by the R25 re-parse, which names the same number; the arm differs only for a canonical form of *exactly* the cap, which it would refuse | **boundary-only** (the check is belt to the re-parse's braces; no 16 MiB test is worth the boundary) |
| 13 | `print.rs:63:24` `>`→`>=` in `format_with` | same boundary: only a canonical form of *exactly* `MAX_SOURCE_BYTES` differs | **boundary-only** (the parser accepts exactly the cap, so the pair would diverge at 16 MiB; a 16 MiB test costs more than the risk). Recorded, not filed |
| 14 | `print.rs:102:70` `<`→`<=` in `write_file` | `\|\| section` already covers the index that `<=` would add | **equivalent** (`section` is true exactly at that index) |
| 15 | `print.rs:159:5` erase `write_leading_comments` | every comment it writes is re-emitted by `inline_text`'s hoisting (header or whole-line path) | **equivalent — dead helper** (deletion candidate; no test owed) |
| 16 | `print.rs:161:36` guard `== Comment`→`false` | same dead helper | **equivalent** |
| 17 | `print.rs:166:13` delete its `Trivia(_)` arm | same dead helper | **equivalent** |
| 18 | `print.rs:315:5` `shares_line`→`true` | no document has a comment as the first element inside a block on its own line | **worth a test** → `MB.21.5.13` |
| 19 | `print.rs:347:9` `Line::is_empty`→`false` | `write_file`'s blank-due rule and `write_line`'s early return already produce the same text | **equivalent** (both call sites tolerate an empty line) |
| 20 | `print.rs:366:13` delete the `Token` arm of `comment_trails` | no grant clause carries a trailing comment; `body json # c` ends in a *node*, so the `Node` arm answers | **worth a test** (minor: `transactions # note`) → `MB.21.5.13` |
| 21 | `print.rs:409:49` `>`→`<` in the paren counter | the only wrapped column lists wrap after a `,`, which the `Comma` condition already continues | **worth a test** → `MB.21.5.13` |
| 22 | `print.rs:418:25` delete the `LParen` arm | same case | **worth a test** → `MB.21.5.13` |
| 23 | `print.rs:419:25` delete the `RParen` arm | same case | **worth a test** → `MB.21.5.13` |

### 3.2 The nine `TIMEOUT`s are one class, not nine gaps

All nine are in `lexer.rs` and all are mutants that stop the *cursor*: `rest -> "xyzzy"` (150),
`bump`'s offset `-`→`/` (170), `advance` erased (182) or its offset `-`→`/` (184), and the five
"erase a consuming helper" mutants `one` (244), `carriage_return` (250), `word` (262), `env` (269),
`quoted` (290) → `Ok(())`. With the consumer erased, `Lexer::run`'s `loop` re-reads the same character
forever and grows nothing, so no cap can trip and no assertion can be reached: the only observable is
non-termination. `--timeout 90` bounds each (the campaign spent ~20 min on the nine), and they are
excluded from the ratio by `cargo-mutants`' own accounting, as they must be.

This is the `LESSONS.md` `MB.1.4` class ("a mutant that cannot terminate is an untested loop"), and
the fix is code shape, not a test: `run` could require progress per iteration (e.g. record `at` before
`trivia()` and bail if it did not advance), which would turn each of these into a fast caught failure.
That belongs to `MB.21.3.1`'s lexer (and would be a `gate`-worthy property), so it is recorded here and
not filed: no assertion can kill them as the code stands, and the node's acceptance allows a bounded,
explained timeout.

### 3.3 Run 5 (syntax, tokens, write)

The five `TIMEOUT`s here are two classes. **Non-terminating by construction** (both twins hang): 
`syntax.rs:158:9` (`child_nodes` → `once(0)`; `every_element_is_visited_once` and `typed_accessors_walk_one_kind`
report "running for over 60 seconds") and `syntax.rs:166:44` (`+`→`*`; `grant_block_parses` hangs on the
wrong node id). **Load artifacts, caught by the twin**: `syntax.rs:221:9` (`entrypoint` → `None`, twin
failed 10 tests), `syntax.rs:226:9` (`stores` → `empty()`, twin failed 12) and `syntax.rs:265:9` (`grants`
→ `once(0)`, twin failed 6) — one twin of each crossed the 90 s cap while the 1 000 000-line
`a_file_it_would_grow_past_the_cap_is_refused` test ran on the loaded host. None is a missing assertion.

- **`syntax.rs:135:9` `Cst::root` → `Default::default()`** — **equivalent**: the tree's root is
  element 0 by construction (`Builder::build` pushes `root: 0` first), so `0` is the real answer.
  The mutant's log shows all 130-odd tests passing, i.e. nothing distinguishes it.
- **`syntax.rs:158:9` `child_nodes` → `once(Default::default())`** and **`syntax.rs:166:44` `+`→`*`**
  — **TIMEOUT, non-terminating mutant**: both make the accessor yield the root again, and every walk
  (`syntax::tests::every_element_is_visited_once`, `kinds()`) then loops forever. Same class as the
  lexer's nine (§3.2): an assertion cannot be reached, `--timeout 90` bounds it, and the fix is a
  walk/progress guard, not a test.
- **`syntax.rs:221:9` `Cst::entrypoint` → `None`** — **caught**: `cargo-mutants` generates this mutant
  twice and the second run failed 10 tests (`typed_accessors_walk_one_kind`, the parser's entrypoint
  cases, and the `validate` raw-ingress rows). One twin is recorded `TIMEOUT` because on the loaded
  host the 1 000 000-line `a_file_it_would_grow_past_the_cap_is_refused` test crossed 90 s; that is a
  load artifact, not a missing assertion — say so and do not file it.
- **The 30 `unviable` syntax mutants** are compile failures of the mutation itself (`-> impl Iterator`
  replaced by a concrete iterator of the wrong item type, and `Position`/`ManifestError` values that
  have no `Default`). They say nothing about coverage and are excluded, as the accounting rule requires.

`write.rs` is measured in §2.4 against the editor's own suites.

### 3.4 `write.rs` survivors (run against the editor's suites)

| # | Mutant | Why it survived | Verdict |
|---|---|---|---|
| 24 | `write.rs:333:60,68,76,84` `valid_view_name` | every proposed name is a bare word | **worth a test** → `MB.21.5.14` |
| 25 | `write.rs:345:62,70,78` `valid_method` | only `GET`/`POST` are proposed | **worth a test** → `MB.21.5.14` |
| 26 | `write.rs:738:42` `merge_patch` `&&`→`\|\|` | no call patches two different views | **worth a test** (a cross-view write would be a capability leak between views) → `MB.21.5.14` |
| 27 | `write.rs:839:24,63` `writable_word` | no `snake_case` column and no leading `-` in the tests | **worth a test** → `MB.21.5.14` |
| 28 | `write.rs:953:22`, `write.rs:1120:22,27` | no one-line **empty** grant block | **worth a test** → `MB.21.5.14` |
| 29 | `write.rs:1237:9` `statement_splice` `&&`→`\|\|` | no line holds two statements | **worth a test** (the splice would delete a neighbour) → `MB.21.5.14` |
| 30 | `write.rs:1340:38` `block_is_empty` `!` deleted | same one-line shapes | **worth a test** → `MB.21.5.14` |
| 31 | `write.rs:474:9` `is_noop`→`false` | the no-op path renders the same bytes and passes the same postcondition | **equivalent** (an optimization guard) |
| 32 | `write.rs:679:23` `>`→`>=` | `previous_line_blank(.., 0)` is `true` by definition | **equivalent** |
| 33 | `write.rs:1190:37` `directive_spans` `!=`→`==` | no tested block has a directive **before** `transactions`: the `!=` is what keeps `transactions` bounding the preceding directive, so a splice between them loses the grant and P4 refuses a valid edit | **worth a test** → `MB.21.5.14` |
| 34 | `write.rs:1214:16` `>`→`>=` | `end == start` needs a keyword at the closing brace | **equivalent** |
| 35 | `write.rs:1395:14` `>`→`>=` in `comment_run_start` | the loop breaks at `at == 0` anyway | **equivalent** |
| 36 | `write.rs:1411:5` `blank_line_at`→`false` | `blank_line_at` is false at every insertion point, so the guard is dead | **equivalent** (and a deletion candidate) |

One `TIMEOUT` (`write.rs:483:16`) is the `_001` class of §3.3: failures were already reported when the
cap hit. `write.rs`'s 21 misses therefore resolve into **sixteen test gaps and five equivalents**, all
filed or explained.
## 4. Protected suites: what holds, and what was rewritten

Assertion counts at each protected file's previous revision vs the audited tree
(`git show <last>^:<file> | grep -c assert` vs `grep -c assert <file>`; a "new" row means the file was
created by that commit, so nothing was removed):

| Suite | Last commit that touched it | Assertions before → after |
|---|---|---|
| `critical_blastfile_errors.rs` | `fa6dc39` | 16 → 20 |
| `critical_blastfile_lexer.rs` | `1cfdace` | 15 → 15 |
| `critical_blastfile_parser.rs` | `1cfdace` | 12 → 12 |
| `critical_blastfile_patterns.rs` | `1cfdace` | 18 → 18 |
| `critical_blastfile_print.rs` | `1cfdace` | 25 → 25 |
| `critical_blastfile_parity.rs` | `fa6dc39` | 102 → 102 |
| `critical_blastfile_traits.rs` | `fa6dc39` | 13 → 13 |
| `critical_manifest_edit.rs` | `fa6dc39` | 104 → 104 |
| `critical_manifest_edit_executor.rs` | `fa6dc39` | 32 → 32 |
| `critical_blastfile_body.rs`, `_entrypoint.rs`, `_security.rs` | `1d0d17b` (created there) | new (16 / 14 / 15) |

No protected suite lost an assertion. The error suite gained four (the regex message's escape and cut,
plus the secret-free sentence), which is the §18 tightening `MB.21.4.3` promised. `docs_fences.rs` is
**not** protected, deliberately: its doc comment and §20 say a protected copy would make every
documentation edit a `critical_guard` conflict.

What the protected tier pins, per file:

- **`critical_blastfile_lexer.rs`** — `read`'s refusals from metadata (16 MiB + 1 is `TooLarge` even on
  a mode-`000` file; a FIFO and `/dev/zero` are `NotRegularFile` within 10 s; a missing path is
  `Unreadable`), the 4 194 305-token refusal at the public entry point, and five hostile inputs that
  must refuse in one line. **Its accepted half is only the at-cap read** (gap 1).
- **`critical_blastfile_parser.rs`** — depth two in both shapes, a wall of braces/traits/views without
  recursion, a 512 KiB file of thousands of views, and 36 §6.7 rows through the public entry point.
- **`critical_blastfile_errors.rs`** — position format, the 120-`char` cut, control-byte escaping, the
  regex message cut, position-less rendering, one line per problem, and four secret-shaped refusals.
- **`critical_blastfile_print.rs`** — P2/P2b/P3 over the 15 fixtures and over a document using every
  production, plus R25's quote round-trip and the over-cap refusal. P2b's oracle is an independent
  scanner, not the crate's lexer, so the formatter cannot pass by agreeing with itself.
- **`critical_blastfile_security.rs`** — `HOME` is read *by the test* to prove the parser does not read
  it; a quoted DSN is refused without its value; six refusals yield no manifest at either entry point;
  an undeclared store grants nothing.
- **`critical_manifest_edit.rs` / `_executor.rs`** — surgical byte-equality (`SOURCE_EDITED` compared
  whole), idempotence, insertion above both catch-all spellings, the `$1`→`$?` normalisation, five
  refusal families, the atomic write (no temp file, mode preserved, planted symlink, stale temp, failed
  rename), and P4's no-write on a postcondition mismatch.

Two weak oracles, both named above: `critical_blastfile_lexer.rs`'s size cap has no accepted twin
(gap 1), and `crates/blastit/tests/critical_blastfile_fmt.rs:201` prints `skipped: … (uid 0)` and
returns, so it proves nothing on a root host (gap 7).

## 5. Findings and gaps → follow-up nodes

### 5.1 R24's purity gate is evadable (measured)

`scripts/check_manifest_purity.py` matches one literal, `NEEDLE = "std::env"`, over
`crates/blastit-manifest/src/**/*.rs`. A scratch tree under `/tmp/opencode/mb2153-purity` with two
files measured both directions:

```console
$ cat /tmp/opencode/mb2153-purity/crates/blastit-manifest/src/evasive.rs
pub fn home() -> Option<String> {
    use std::{env};
    env::var("HOME").ok()
}

$ UV_OFFLINE=1 uv run --no-project python scripts/check_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
exit=0

=== evasive + direct fixture (direct.rs holds `std::env::var("HOME")`)
crates/blastit-manifest/src/direct.rs:2: std::env is not allowed here: std::env::var("HOME").ok()
manifest-purity: 1 std::env use(s) under crates/blastit-manifest/src/; a Blastfile names variables and never reads them (R20/R24)
exit=1

=== evasive fixture alone (direct.rs removed)
manifest-purity: ok — 1 source file(s), no std::env
exit=0
```

So the gate catches the spelling the design wrote down and misses the same read through a nested use
list. The repository's own self-test (`scripts/test_manifest_purity.py`) passes — it only proves the
marker, which is the right behaviour and the wrong reach.

The follow-up node (gap 2) carries the fixture text, a widened needle that catches the evasion without
a false positive on any current file, and a negative self-test beside it:

```python
#: A `std::env` read, however the path is imported.
NEEDLE_PATTERNS = (
    re.compile(r"\bstd::env\b"),
    re.compile(r"\benv::(?:var|vars|var_os|args|current_dir|set_var|remove_var)\b"),
    re.compile(r"\buse\s+std::\{[^}]*\benv\b"),
)
```

`env::var`/`env::vars` are the only `std::env` entry points a manifest could plausibly want, and the
`use std::{…env…}` row catches the import even when the call is spelled differently. A stricter
alternative for the same node: clippy's `disallowed-methods` for the `std::env` functions, which
resolves the path through any import — but that is a `clippy.toml` change with a workspace-wide blast
radius, so the script needle is the smaller first step.

### 5.2 `blastfile::read`'s accepted path is untested here

`critical_blastfile_lexer.rs` reads exactly `MAX_SOURCE_BYTES` (successful) and nothing smaller, so
two mutants survive (see §3, items 1–2):

- `mod.rs:262:19` `>` → `<` in `if text.len() > MAX_SOURCE_BYTES` — every *small* file would be
  refused as `TooLarge`, and no in-crate test reads a small file, so the suite stays green. (The
  discovery suite in `crates/blastit` would fail, which is why this is a crate-local oracle gap rather
  than a total one.)
- `mod.rs:259:39` `+` → `*` in `take(MAX_SOURCE_BYTES as u64 + 1)` — only observable when a regular
  file grows between `metadata` and `read`, which no deterministic test can stage; see §3's
  equivalent/untestable verdict.

The fix is additive to a protected file (allowed): one 20-byte fixture read returns its text, one
mode-`000` file *under* the cap is `Unreadable` (the deny side of "size known before any read"), and
one file written after `metadata` is refused — the last is the only way to reach the `+ 1`, and it is
optional (see §3).

### 5.3 P4's two oracle holes

- `BlastfileEdits::apply_to` (`mod.rs:167`) is the public name of the postcondition oracle, and **no
  test calls it**: `critical_manifest_edit.rs` goes through `apply_edits`, which calls `write::expected`
  directly. A mutant of `apply_to` is unviable, and a mutant of `expected` that `apply` masks would
  still be missed through the public API. Gap 4: one test that asserts `apply_to` equals the manifest
  the edited text lowers to, for a fixture with a new view and a grant.
- `write.rs::first_difference` (`write.rs:281`) has no `unresolved_grants` arm, so a P4 mismatch caused
  by an unresolved grant is reported as **"the top-level keys"**. The protected test *pins* that
  wording (`critical_manifest_edit.rs:856`), so the improvement is a two-file change (arm + protected
  assertion) and therefore a task, not a drive-by. Gap 5.

### 5.4 The design's ≥ 20 corpus floor never landed

§20's "Beyond the table" says `MB.21.5.2` re-runs P1–P3 over the migrated corpus and asserts **≥ 20
files** ("15 fixtures + the documented documents"), so the final gate cannot pass vacuously.
`MB.21.5.2` rewrote `docs_fences.rs` and `python/tests/test_docs_assets.py` but did not touch
`critical_blastfile_print.rs`: `common::CORPUS_SIZE` is still 15 and four suites assert exactly 15.
`docs_fences.rs` parses and validates every documented document but does not run P1/P2/P3 over them.
Gap 6 adds the documented documents to the round-trip corpus (or to a second corpus in the protected
file) and raises the floor to ≥ 20, and folds in the list-hoisted comment owner case (§1.3 nit 7).

### 5.5 Gaps filed (node ids in `RESULT.md`)

| Node | Gap | Ruling (epic lead) | Paths |
|---|---|---|---|
| `MB.21.5.5` | R24's purity gate is evadable by `use std::{env};` (§5.1) | **pre-gate** (in flight now) | `scripts/check_manifest_purity.py`, `scripts/test_manifest_purity.py` |
| `MB.21.5.6` | `blastfile::read` has no accepted-path or under-cap `Unreadable` oracle in this crate (§5.2, survivors 1–2) | no — post-0.2.0 (see Release judgement) | `crates/blastit-manifest/tests/critical_blastfile_lexer.rs` |
| `MB.21.5.7` | §20's ≥ 20-document round-trip floor never landed; the hoisted-comment owner case is uncovered (§5.4) | **yes — the corpus floor** | `critical_blastfile_print.rs`, `tests/common/mod.rs`, `print.rs` |
| `MB.21.5.8` | P4's oracle: `apply_to` has no direct test, `first_difference` has no unresolved-grants arm (§5.3) | post-0.2.0 (message precision + a public-wrapper test) | `blastfile/mod.rs`, `blastfile/write.rs`, `critical_manifest_edit.rs`, `edit_branches.rs` |
| `MB.21.5.9` | `EditError::source()` untested (survivors 3–4) | post-0.2.0 (error-chain DX) | `tests/blastfile_errors.rs` |
| `MB.21.5.10` | `critical_blastfile_fmt.rs` silently skips under uid 0 (§4) | post-0.2.0 (CI runs unprivileged) | `crates/blastit/tests/critical_blastfile_fmt.rs` |
| `MB.21.5.11` | the guard at `parser.rs:1270` is unpinned; a regression would accept a typo'd grant (`postgres db bogus`) instead of refusing it. The shipped parser refuses it — the product is correct | coverage only, **post-0.2.0** | `blastfile/parser.rs`, `critical_blastfile_parser.rs`, `blastfile_grants.rs` |
| `MB.21.5.12` | four message-only survivors: `$lower_x`, `GET, post`, `view v GET foo`, `GET foo` (survivors 5–8) | post-0.2.0 (wrong sentence, right refusal) | `blastfile/lexer.rs`, `blastfile/parser.rs`, `critical_blastfile_parser.rs` |
| `MB.21.5.13` | printer/pattern survivors: `{a_b}`, first-in-block comment, wrapped column list, `transactions # note` (six mutants) | post-0.2.0 | `blastfile/print.rs`, `blastfile/pattern.rs`, `critical_blastfile_print.rs`, `blastfile_patterns.rs` |
| `MB.21.5.14` | the editor's sixteen testable survivors: name tails, cross-view patch merge, one-line empty blocks, two statements on one line, and `transactions` bounding a directive (survivors 24–30, 33) | post-0.2.0 (the cross-view merge and the one-line splice can drop a neighbour, but no shipped input reaches them untested) | `blastfile/write.rs`, `edit_branches.rs`, `critical_manifest_edit.rs` |

Not filed, deliberately: the `UnrestrictedWithContent` and `with_unrestricted` nits (no assertion could
kill a mutant there — the grammar cannot construct the variant and nothing calls the builder; deletion
candidates, §1.3 items 1–2), the `from_parts` parameters (already carried by `write::expected` and
caught by P4), and the `TooLarge` reuse (already pinned by three numeric assertions).

## 6. Verification commands run for this audit

All commands ran in the worktree `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`
(branch `release/0.2.0`, base `6bfd1cc`). `git status --porcelain crates/ python/ scripts/ Makefile
e2e/` was empty when the campaign and `make check` ran: this audit edited no source, test, harness or
Makefile file. (After the review, the sibling `MB.21.5.11` — now `active` — began landing its
named-store guard rows in `parser.rs`, `blastfile_grants.rs` and `critical_blastfile_parser.rs`; those
edits belong to that node, and this report describes the audited base.)

The host restarted four times during the campaign, and its second shape is 8 cores / 32 GiB with a
minimal `PATH` (`uv` and `cargo` live in `~/.local/bin` and `~/.cargo/bin`). That explains three things
recorded above: the campaign is split across launches (§2.1); a detached `systemd-run --user` **service**
survives a session restart where a `--scope` does not; and the first `make test-critical` — at load
average 36 — failed two `critical_php_embed` probe tests at their 250 ms probe budget (the test's own
message names `BLASTIT_PROBE_TIMEOUT_MS`), then passed unchanged on the quiet re-run pasted here. The
flake is the host's, not the tree's; no source in scope was touched between the runs.

### 6.1 `make test-critical` — exit 0, zero skips

```console
$ make test-critical > /tmp/opencode/mb2153-test-critical.log 2>&1; echo exit=$?
exit=0
$ grep -c "test result: ok" /tmp/opencode/mb2153-test-critical.log
41
$ tail -4 /tmp/opencode/mb2153-test-critical.log
python -m unittest discover -s python/tests/critical
..................................................
----------------------------------------------------------------------
Ran 50 tests in 13.062s

OK
```

`critical-skip-check` is a prerequisite of the target, so the 41 `ok` lines and the 50 Python tests
carry no skip marker.

### 6.2 `make check` — exit 0

```console
$ make check > /tmp/opencode/mb2153-make-check.log 2>&1; echo exit=$?
exit=0
$ grep -c "^test result: ok" /tmp/opencode/mb2153-make-check.log
166
$ grep -E "test result: FAILED|^error" /tmp/opencode/mb2153-make-check.log | head -3
$ tail -6 /tmp/opencode/mb2153-make-check.log
uv run --no-project scripts/check_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
uv run --no-project scripts/test_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 16 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
```

### 6.3 Docs and plan gates

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 1.08s
$ make docs-unit
107 passed, 65 subtests passed in 20.73s
$ uv run --no-project tools/plan.py validate
plan OK
```

(`test_the_real_tree_carries_no_retired_php_vocabulary` scans every live `NODE.md`; the first draft of
`MB.21.5.8`'s body used one retired word descriptively and the guard caught it — the body was reworded,
which is the guard doing its job. `make check` above runs the same test green.)

### 6.4 The campaign's per-outcome counts

```console
$ for d in results results-run3 results-run5 results-write; do
    printf "%s: " "$d"; wc -l < "$MUTANTS/$d/mutants.out/caught.txt" | tr '\n' ' '
    wc -l < "$MUTANTS/$d/mutants.out/missed.txt" | tr '\n' ' '
    wc -l < "$MUTANTS/$d/mutants.out/timeout.txt" | tr '\n' ' '
    wc -l < "$MUTANTS/$d/mutants.out/unviable.txt"; done
results: 30 4 0 10
results-run3: 415 23 9 39
results-run5: 79 1 5 31
results-write: 194 21 1 10
```

Only `results-run3`, `results-run5` and `results-write` are used in §2: run 1 and run 2 are the
OOM-killed partials (§2.1), recorded so the totals are not read as one campaign. Summing the three used
launches: 748 viable, 688 caught, 45 missed, 15 timeout, 79 unviable → **92.0 %** (827 of the 828
mutants generated; one `print.rs` mutant was in flight when the session was killed).

### 6.5 The tree and the report

```console
$ uv run --no-project tools/plan.py tree --under MB.21.5 --depth 2 | tail -11
  ● MB.21.5.4 [task/done] @visual-checker a1 ⇠MB.21.5.2 Browser proof of the Blastfile pages, nav and both colour schemes (visual-checker)
  ◐ MB.21.5.5 [task/active] @implementer a1 Close the purity gate's nested-use evasion (R24) and prove it in both directions
  ○ MB.21.5.6 [task/ready] @implementer Pin blastfile::read's accepted path so the read bound's mutants die in-crate
  ○ MB.21.5.7 [task/ready] @implementer Land the design's >= 20-document round-trip corpus floor and the hoisted-comment owner case
  ○ MB.21.5.8 [task/ready] @implementer Give P4's postcondition oracle a direct test and a truthful section for unresolved grants
  · MB.21.5.9 [task/proposed] @implementer Pin EditError::source so the editor's error chain is walked by a test
  · MB.21.5.10 [task/proposed] @implementer Remove the uid-0 silent skip from critical_blastfile_fmt's write-failure case
  ◐ MB.21.5.11 [task/active] @implementer a1 Pin the named-store grant guard (coverage; a regression would accept a typo'd grant)
  · MB.21.5.12 [task/proposed] @implementer Close the four message-only refusal-matrix survivors (lexer name_run, three parser guards)
  ○ MB.21.5.13 [task/ready] @implementer Close the printer and pattern-name survivors (six mutants, four corpus cases)
  ○ MB.21.5.14 [task/ready] @implementer Close the surgical editor's sixteen testable survivors (names, cross-view patches, one-line empty blocks, transactions bounding a directive)
$ ls docs/testing/AUDIT-MB.21.md
docs/testing/AUDIT-MB.21.md
```

### 6.6 R24, measured

```console
$ UV_OFFLINE=1 uv run --no-project python scripts/check_manifest_purity.py      # the real tree
manifest-purity: ok — 16 source file(s), no std::env          # exit 0
$ UV_OFFLINE=1 uv run --no-project python scripts/check_manifest_purity.py --root /tmp/opencode/mb2153-purity
crates/blastit-manifest/src/direct.rs:2: std::env is not allowed here: std::env::var("HOME").ok()
manifest-purity: 1 std::env use(s) under crates/blastit-manifest/src/…  # exit 1
$ rm /tmp/opencode/mb2153-purity/crates/blastit-manifest/src/direct.rs
$ UV_OFFLINE=1 uv run --no-project python scripts/check_manifest_purity.py --root /tmp/opencode/mb2153-purity
manifest-purity: ok — 1 source file(s), no std::env          # exit 0 — the evasion
$ UV_OFFLINE=1 uv run --no-project python scripts/test_manifest_purity.py
manifest-purity self-test: ok — 3 fixtures proved, 16 real source file(s)
```

