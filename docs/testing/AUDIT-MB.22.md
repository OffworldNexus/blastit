# AUDIT MB.22.6 — `blastit lsp`, the manifest spans, the grammar and the editors guard

- **Date:** 2026-11-17
- **Node:** `MB.22.6` (test-curator). Report path per its `# Acceptance`: `docs/testing/AUDIT-MB.22.md`.
- **Base:** `d08d04c` on `release/0.2.0` (audited tree = working tree; **no test, crate, script or SCM file was edited** — every regression below was reverted, `git status --short crates python scripts editors Makefile` empty).
- **Scope:** `crates/blastit/tests/critical_lsp.rs`, `crates/blastit/src/lsp/{mod,hover}.rs`,
  `crates/blastit-manifest/src/error.rs`, `blastfile/{sites,mod}.rs`, `tests/warning_sites.rs`,
  `editors/zed/languages/blastfile/*.scm`, `editors/tree-sitter-blastfile/{grammar.js,test/**}`,
  `scripts/check_zed_extension.py` + `scripts/test_check_zed_extension.py`,
  `python/tests/critical/test_lsp_stdio.py`, `docs/local.md` §Editors, `docs/reference/cli.md` §`blastit lsp`.
- **Requirements:** design `docs/design/blastfile-lsp-zed.md` §11 (S1–S11), §12 (R53–R59, the matrix),
  §12.8 (hand-offs), §12.9; skills `testing`, `security-invariants`, `rust-standards`, `python-standards`, `dx-first`.

## Verdict

**PASS WITH GAPS** on tier and teeth; **mutation: PASS** on the in-scope files (see §3).

- **Tier: PASS.** Every §12.1 row has a protected test in `critical_lsp.rs` (42 cases) or in the
  manifest crate's own suites; the grammar and the guard carry their own gates; the Python route is
  protected. No case in scope is fluff, and no protected suite lost an assertion (§4).
- **Teeth: PASS**, with one exception that is itself the finding: six of the seven deliberate
  regressions turned a named test red on the first run (§2). The seventh — a `grammar.js` edit — left
  `tree-sitter test` **green 15/15**, because that command compiles the committed `src/parser.c` and
  never reads `grammar.js`; it went red only after `tree-sitter generate`. Nothing in `make check`
  regenerates and diffs `src/`, so a `grammar.js` edit that is never regenerated ships a parser the
  corpus does not describe. Filed as gap 1.
- **Mutation: PASS** on the three in-scope files (§3): 101 mutants generated, 25 viable tested (15 unselected `cargo-mutants` outcomes plus 10
  distinct hand-applied), **23 killed = 92.0 %**, above the 75 %
  bar in every file (`error.rs` 93.8 %, `sites.rs` 87.5 %, `mod.rs` 100 %). Both survivors are
  classified: `Position`'s unused `Display` (equivalent) and the endpoint-level `body raw` site
  (`MB.26.14`). The campaigns could not complete — this host was
  at load 20–51 with a second project's suite and killed three of the four launches — so the sample
  is named mutant by mutant in §3.4 rather than extrapolated.

Two docs claims the code does not make (gap 2) and one security session with no test anywhere
(gap 3) are the other findings. Every gap has a node id (§5).

## Method

1. Read every in-scope test body and source file in full; classified each case
   `critical` / `useful` / `fluff` / `misplaced` / `broken-oracle` against the `testing` skill, asking
   of each policy-bearing case what it would still pass with its bound, conversion or refusal removed.
2. Mapped §12.1's 34 rows, §12.2's six rows and §11's S1–S11 to the concrete test names that carry
   them, and checked the deny side of each (a row that passes with the check removed is fluff).
3. Ran every `# Acceptance` command (§6), substituting the pinned `tree-sitter` 0.25.10 release binary
   for `npx --yes tree-sitter-cli@0.25` (this host has no `npm`/`npx`; design §9.4, §12.8).
4. Broke each conversion, the framing, the formatting `end`, the message prefix strip, one grammar
   trap, one SCM capture and one guard property; recorded which test went red; reverted every edit and
   re-ran the suite (§2).
5. `cargo-mutants` 27.1.0: `make mutants-rust PKG=blastit-manifest` first (1445 mutants, own scratch
   `MUTANTS_TMP=/var/tmp/blastit-mutants-mb226`), then — after the whole-crate launch proved too slow
   on a host at load 20 — the same harness scoped with `-f` to the three in-scope files, as
   `AUDIT-MB.21.md` §2.1 did. Three of the four launches were killed by the host (§3.1), so the
   surviving evidence is the 16 unselected outcomes plus 11 named hand-applied mutants (§3.4).
   Ratios are `caught / (caught + missed)` from `mutants.out/{caught,missed,timeout,unviable}.txt`
   (no timeout occurred); `unviable` is excluded and named.
6. Diffed every protected suite against `ec47752` (the `origin/develop` merge base) and against
   `7c2e9ca` (the commit before `MB.22.3`), counting assertions.

## 1. Inventory and classification

### 1.1 Where the cases live

| Area | Unit / standard cases | Protected cases |
|---|---|---|
| `crates/blastit/src/lsp/mod.rs` | 10 (`mod tests`: P1–P3, F1, `char_index`, `word_at`, `declared_length`, the two bounds) | `critical_lsp.rs` (42) |
| `crates/blastit/src/lsp/hover.rs` | 1 (`hover_table_is_bounded_and_transcribed`) | `critical_lsp.rs` hover rows (H1–H5) |
| `blastit-manifest/src/error.rs` | 5 (`mod tests`: prefix invariant, three families, warning text) | `critical_lsp.rs` diagnostics rows; `warning_sites.rs` |
| `blastit-manifest/src/blastfile/sites.rs` | 4 (the four `site()` families, `NONE`, distinctness) | `warning_sites.rs` (4) |
| `blastit-manifest/src/blastfile/mod.rs` (`check`) | — | `warning_sites.rs` |
| `editors/tree-sitter-blastfile` | 15 corpus cases in 9 files | — (data; `tree-sitter test`) |
| `editors/zed/languages/blastfile/*.scm` | 4 files | `scripts/check_zed_extension.py` (42 properties) |
| `scripts/check_zed_extension.py` | 19 negative self-tests | `make zed-check` (a `make check` prerequisite) |
| `python/tests/critical/test_lsp_stdio.py` | — | 3 (both distribution routes) |

**Classification:** 42 + 3 + 4 + 4 + 5 + 11 + 19 + 15 cases; **none is fluff** and none is
`misplaced` (the two `include_str!` greps and the FIFO/`/proc` oracles are the only deterministic
oracles for S2/S3/S4/S11, and the protected tier is where they belong).

One **weak oracle**, not fluff: `stdout_is_frames_only` (§12.1's S7 row) reads two replies and asserts
the exit status, but never drains stdout, so it does not itself prove "frames and nothing else"; that
half is carried by `stderr_never_holds_document_text` (which drains to EOF after `exit`) and by the
positive control `drain_stdout_sees_an_unread_frame`. The property holds; the test's name overpromises.
No node filed — a rename plus one `assert!(session.drain_stdout().is_empty())` is a drive-by, and the
gap is named here so the next reader does not trust the name.

### 1.2 Requirement trace (design §12, §11)

| R / S | Case(s) that carry it | Deny side | Verdict |
|---|---|---|---|
| R1, R3, R3a, S1, S11 | `lsp_subcommand_is_reachable`, `the_lsp_module_is_a_text_tool` (both files), `no_position_of_a_validation_error_is_invented`, 3 `error.rs` unit tests | ✓ `--bogus` → 2; 14 forbidden names; no `_ =>` | ✓ |
| R4–R7, R10, R12, S6 | `frame_length_is_utf8_bytes`, `ids_echo_verbatim`, `error_codes_are_the_table`, `malformed_body_…`, `work_done_progress_is_answered`, `unknown_notification_is_dropped`, `header_cap_boundaries`, `oversized_body_is_refused_before_reading`, `declared_length` | ✓ each code with its own trigger; 8 192 served / 8 193 refused; refusal before the body is written | ✓ |
| R8 document map | `open_documents_are_capped`, `oversized_document_is_not_stored`, `did_change_for_unknown_uri_…`, `incremental_change_is_refused_visibly` | ✓ 33rd refused, 32nd answers | ✓ |
| R14–R17 positions | `positions_are_utf16`, `positions_line_split_and_none`, `squiggle_covers_…`, `hover_inverse_utf16`, P1–P3, `char_index` | ✓ `😀` = 20, not 19/22 | ✓ |
| R18–R24 diagnostics | `diagnostics_carry_severity_source_and_message`, `warning_sites_are_their_declarations`, `warning_display_is_unchanged`, `diagnostics_are_capped`, `warning_sites.rs` (4) | ✓ 512 + "89 more"; no `code`; no `line:col` in a sentence | ✓ |
| R25–R27, R30–R34 | the four formatting tests, the four hover tests, `definition_is_declined_and_unadvertised`, F1/H6 unit tests | ✓ `[]` / one edit / `null`; name, punctuation and closed document → `null` | ✓ |
| S2, S4, S5, S7, S8 | `a_document_is_never_executed`, `no_path_from_a_document_is_opened`, `fuzz_shaped_bodies_do_not_kill_the_loop`, `stdout_is_frames_only`, `stderr_never_holds_document_text` | ✓ `/proc` children empty; FIFO answers; 2 000 bodies; four-literal stderr alphabet | ✓ (S7 name overpromises, §1.1) |
| S3, S10 | `the_lsp_arm_drops_capabilities`, `no_environment_is_needed`, `exit_codes` | ✓ order grep + `env_clear`; six of seven endings | **gap 3** (capability-carrying process untested) |
| R40–R59 | 15 corpus cases, `make zed-check` (42 properties), `test_lsp_stdio.py` (3), the rev pin recorded by `MB.22.5` | ✓ captures, pair, `@item`/`@indent`/`@open`; both routes | **gap 1** (`grammar.js` drift) |

### 1.3 Nits with a verdict

| # | Nit | Verdict |
|---|---|---|
| 1 | `stdout_is_frames_only` never drains stdout | **weak oracle**, property carried elsewhere (§1.1). No node. |
| 2 | `the_lsp_module_is_a_text_tool`'s forbidden list has no `File::open` | **equivalent**: `std::fs` and `std::path` are both forbidden, so a `File` cannot be named without one of them. No node. |
| 3 | `warning_sites_are_their_declarations`' `!sentence.contains(":{line}")` is a text heuristic | **useful**: the exact-sentence comparison on the same line carries the property; the heuristic only catches a re-added prefix. No node. |
| 4 | `warning_sites.rs` is standard-tier, not `critical_*` | **misplaced by tier, harmless**: the same seven sites are asserted end-to-end in `critical_lsp.rs`; the fast copy is the one that names a broken `sites()` first. No node. |
| 5 | 23 of the guard's 42 properties have no negative twin (19 self-tests) | **real but narrow**: `test_the_repository_passes` pins the exact 42 names, so a *deleted* check is caught; a *inverted* check is not. The five security-bearing ones are gap 5. |
| 6 | `tree-sitter test` reads `src/parser.c`, not `grammar.js` | **gap 1** — measured in §2. |
| 7 | `docs/reference/cli.md` says exit `1` "only if the client disappears" | **gap 2** — the code also exits `1` on an unframeable stream and `2` on a bad argument. |

## 2. The deliberate regressions (teeth, proved then reverted)

Each edit was made in the working tree, the named suite was run, and the edit was reverted with
`git restore` (or the saved copy); `git status --short crates python scripts editors Makefile` was empty after each.
(The tree also carries earlier nodes' `plan/` and `docs/` edits and this audit's own files; none
of them is a test, crate, script or SCM file.)

| # | What was broken | Which test went red | Reverted |
|---|---|---|---|
| 1 | **UTF-16**: `Lines::point` counted `col - 1` (`let character = chars`) instead of summing `char::len_utf16` | `positions_are_utf16`, `diagnostics_carry_severity_source_and_message`, `hover_inverse_utf16` (3 of 42) | ✓ |
| 2 | **Framing**: `write_message` declared the body's **char** count, not its byte count | `frame_length_is_utf8_bytes` plus three desync failures (`positions_line_split_and_none`, `no_position_of_a_validation_error_is_invented`, `oversized_document_is_not_stored`) — 4 of 42 | ✓ |
| 3 | **Formatting `end`**: the last non-empty line instead of `max_point_utf16` (gave `(4,1)` for a `(5,0)` document) | `formatting_diff_is_one_edit`, `formatting_normalises_crlf_once` (2 of 42) | ✓ |
| 4 | **Message prefix strip**: `ManifestError::message()` returned `to_string()` whole | `error::tests::message_is_display_without_its_prefix`, `error::tests::a_positioned_message_carries_no_location`, `warning_sites::a_broken_file_is_a_refusal_not_a_warning_list`, `critical_lsp::diagnostics_carry_severity_source_and_message` (4 Rust) **and** the Python `test_one_document_three_formatting_outcomes_and_a_clean_stderr` (1 of 3) | ✓ |
| 5 | **Grammar trap**: `source_file` lost `_terminator` (newlines at top level) | `tree-sitter test` **stayed green, 15/15** — it compiles the committed `src/parser.c`. After `tree-sitter generate`: **15/15 red, exit 1**. Reverted; `tree-sitter generate` then left `git status` clean (the committed `src/` is exactly `grammar.js`'s output) | ✓ |
| 6 | **SCM capture**: `(env_var) @variable.special @variable` → `@variable.special` only; separately → `@variable` only | `check_zed_extension.py` exit 1 with `FAIL highlights: captures` (unused `variable` / `variable.special`) **and** `FAIL highlights: variable pair` — 2 of 42 both ways | ✓ |
| 7 | **Guard property**: deleted the `indents: capture` `guard.require(…)` | `test_check_zed_extension.py`: `test_the_repository_passes` (41 properties, not 42) and `test_missing_indent_is_red` — 2 of 19 | ✓ |

**The one that stayed green is a gap, not a fluff test**: the corpus is a real oracle for the
*committed parser*, and `tree-sitter test` is the right command for that; what is missing is a gate
that `grammar.js` and `src/` still agree. Gap 1.

## 3. Mutation results

### 3.1 Four launches, one reason

| Launch | Scope / flags | Jobs | Outcome |
|---|---|---|---|
| 1 | `make mutants-rust PKG=blastit-manifest` (whole crate, `MUTANTS_TMP=/var/tmp/blastit-mutants-mb226`) | 2 | **1 445 mutants found**; 15 outcomes in 20 min (14 caught, 0 missed, 0 timeout, 1 unviable, all `error.rs`); stopped — a 1 445-mutant campaign projects >30 h on this host |
| 2 | `-f error.rs -f blastfile/sites.rs -f blastfile/mod.rs`, `-o results-scoped` | 4 | 16 outcomes (14 caught, 1 missed, 1 unviable) before the host sent `SIGTERM` |
| 3 | as 2, `--cargo-test-arg=--lib --cargo-test-arg=--test --cargo-test-arg=warning_sites` | 4 | `SIGTERM` during the baseline build. The flag shortens only the *run*: cargo-mutants still builds every test target (`cargo test --no-run --package=…`), so it bought ~10 s of 60 |
| 4 | as 2, `-o results-run3` | 2 | `SIGKILL` before the first outcome; the host was OOM-killing at load 51 (a second project's suite ran all session) |

Because no campaign could complete, **11 hand-applied mutants** were run against the same suites
(`cargo test -p blastit-manifest --lib --test warning_sites --locked`, ~15 s each), each reverted
immediately; one of them re-applies the `cargo-mutants` miss on `Position`'s `Display`, so only ten
are distinct and §3.2 counts 25, not 26. They are a **curated** sample — chosen for the in-scope properties — so they are named
one by one below and are not a random sample. The 16 `cargo-mutants` outcomes are unselected.

### 3.2 Per-file table

| File | Mutants generated | Tested | Caught | Missed | Unviable | Kill rate (tested) |
|---|---|---|---|---|---|---|
| `src/error.rs` | 41 | 16 (15 viable `cargo-mutants` + 1 hand) | 15 | 1 | 1 | **93.8 %** |
| `src/blastfile/sites.rs` | 41 | 8 (hand) | 7 | 1 | 0 | **87.5 %** |
| `src/blastfile/mod.rs` | 19 | 1 (hand) | 1 | 0 | 0 | **100 %** |
| **Total** | **101** | **25** | **23** | **2** | **1** | **92.0 %** |

Kill rate = `caught / (caught + missed)` over the mutants actually tested; `unviable` is excluded
and named. All three files clear the 75 % bar.

`crates/blastit/src/lsp/mod.rs` was **out of scope** for mutation: `make mutants-rust PKG=blastit`
would rebuild the whole `blastit` binary (with `--features python`) per mutant — the same 60 s+
build that made `blastit-manifest` unaffordable here — and the LSP's own logic is already pinned by
the seven regressions of §2, each of which turned a named test red.

### 3.3 Survivors, each classified

| # | Mutant | Why it survived | Verdict |
|---|---|---|---|
| 1 | `error.rs:94:9` `impl Display for Position::fmt` → `Ok(())` | nothing in the workspace formats a `Position`: `prefix()` writes `line:col` itself, `Location`'s `Display` never uses it, and no test calls `to_string()` on one | **equivalent — dead code** (deletion candidate; no test owed) |
| 2 | `sites.rs` `raw_body`'s endpoint chain → `std::iter::empty()` | no fixture writes an endpoint-level `body raw`, so the branch is unproven | **worth a test** → `MB.26.14` |

### 3.4 What was killed (the evidence behind §3.2)

`cargo-mutants` (unselected, launch 2): `escape_control` (3 mutants), `clipped` (2), `quoted` (2),
`Position::line` (2), `Position::col` (1), `Position::is_none` (3), and the `is_control` match guard
both ways (2) — 14 caught, 1 unviable (`Position::at` → `Default::default()`, which is not allowed in a `const fn`).

Hand-applied: all six `site()` arms (`UnattachedGroup` name token, `UngrantedProxy` declaration site,
`UnrestrictedRoute` `unrestricted` token, `RawIngress` codec token, `EgressRoute` first token,
`EntrypointNotReached` first token) — killed by `blastfile::sites::tests::every_warning_of_a_view_file_has_its_line`,
`the_unnamed_store_is_located_at_its_keyword` and `warning_sites::a_store_and_an_entrypoint_are_located_where_they_are_written`;
`grant_store`'s `Some("unrestricted")` arm (killed by the unnamed `postgres unrestricted` fixture);
`check()`'s `sites` → `Vec::new()` (killed by the `sites.len() == warnings.len()` assertion); and
`prefix()`'s `Position::NONE` arm → `"0:0: "` (killed by `pattern::tests::a_positionless_refusal_renders_bare`).
## 4. Protected suites: what holds, and what was rewritten

`git diff --stat 7c2e9ca..HEAD -- 'crates/*/tests/critical_*' 'python/tests/critical/*'` is two
files, **2 277 insertions and zero deletions**: `crates/blastit/tests/critical_lsp.rs` (1 880) and
`python/tests/critical/test_lsp_stdio.py` (397). `MB.22`'s protected footprint is additions only.

Nine protected files differ from `ec47752` (the `origin/develop` merge base); every one was last
touched by an **earlier** node (`MB.21.4.1`, `MB.21.4.3`, `MB.21.4.5.x`, `MB.21.4.7`, `MB.21.4.8`,
`MB.21.4.10`, `MB.21.3.5`, `MB.1.15`, `MB.3.27`), and none lost an assertion (`grep -c assert`
at `ec47752` → HEAD):

| Suite | Assertions |
|---|---|
| `critical_manifest_011.rs` | 62 → 70 |
| `critical_manifest_edit.rs` | 70 → 104 |
| `critical_check_schema.rs` | 40 → 40 |
| `critical_ingress_enforcement.rs` | 41 → 80 |
| `critical_check_engine.rs` | 101 → 115 |
| `critical_config_and_presets.rs` | 74 → 83 |
| `critical_dev_and_fail_closed.rs` | 153 → 164 |
| `test_check_contract.py` | 163 → 164 |
| `test_fold_literals_and_atomic.py` | 99 → 99 |

What the two new files pin: `critical_lsp.rs` holds §12.1's 34 rows plus the two `include_str!`
greps, the `/proc` no-child oracle and the FIFO no-open oracle; `test_lsp_stdio.py` drives one
document through three formatting outcomes on both distribution routes and asserts the EOF exit.

`tools/critical_guard.py --base "$(git merge-base HEAD origin/develop)" --node MB.22` exits **1**
with **9 violations**, all nine of the files above — a base-choice artifact, not a weakened test:
the guard's own docstring wants `--base <commit-before-node>`, and the merge base of a release
branch with a dozen nodes is the fork point. Gap 4.

## 5. Findings and gaps → follow-up nodes

| Node | Gap | Evidence |
|---|---|---|
| `MB.26.9` | **`grammar.js` drift is invisible.** `tree-sitter test` compiles the committed `src/parser.c` (even with `--rebuild`), so a `grammar.js` edit that is never regenerated keeps the corpus green; nothing in `make check` runs `tree-sitter generate` and diffs `src/`. | §2 regression 5: 15/15 green before `generate`, 15/15 red after |
| `MB.26.10` | **Two `blastit lsp` docs claims the code does not make**: `docs/reference/cli.md` says exit `1` "only if the client disappears" (the code also exits `1` on an unframeable stream and `2` on a bad argument), and offers `uv run blastit lsp` where the extension uses `--frozen --no-sync --quiet`. | §1.3 nit 7; `critical_lsp.rs::exit_codes`, `lsp_subcommand_is_reachable`; design §12.8's hand-off |
| `MB.26.11` | **S10's seventh session has no test anywhere**: a process that cannot `caps::hold_none()` exits `1`, and `grep -rn lsp e2e/` finds no harness (the hits are `urlsplit`). The unprivileged tier cannot make `hold_none()` fail. | §1.2 S10 row |
| `MB.26.12` | **`critical_guard`'s per-node base is the release-branch fork point**, so every node after the first reports the earlier nodes' edits: 9 violations for a node whose own protected diff is two added files. | §4 |
| `MB.26.13` | **23 of the guard's 42 properties have no negative twin**; the five security-bearing ones (`lib: no capabilities`, `lib: no process`, `lib: argv`, `grammar: no scanner`, `grammar: no externals`) can be weakened without a red test. | §1.3 nit 5, §2 regression 7 |
| `MB.26.14` | **An endpoint-level `body raw` has no warning site**: `sites.rs::raw_body`'s endpoint chain is unproven, so that warning would draw at `(0,0)`. | §3.3 survivor 2 |

The six were created as `MB.22.7`–`MB.22.12`; the epic lead re-homed them to `MB.26.9`–`MB.26.14`
(post-0.2.0) at 07:22, leaving the originals `superseded` with a pointer. The ids above are the live
ones; `uv run --no-project tools/plan.py validate` is green with them.

**Not filed, deliberately:** `Position`'s `Display` is dead code (survivor 1 — deletion candidate, no
assertion can kill a mutant of an unused impl); `stdout_is_frames_only`'s name overpromises but the
property is carried by `stderr_never_holds_document_text` (§1.1); `warning_sites.rs` is standard-tier
by design and duplicates the protected row on purpose; the `File::open` grep is implied by
`std::fs`/`std::path` (§1.3 nit 2).
## 6. Verification commands run for this audit

All commands ran in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`
(`release/0.2.0`, `d08d04c`). The host carried a second project's test run for most of the session
(load average 20–51 on 8 cores), which is named where it changed an outcome.

```console
$ cargo test -p blastit --test critical_lsp --locked
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.31s

$ cargo test -p blastit-manifest --locked
# 17 test binaries + lib: 517 passed; 0 failed; 0 ignored (warning_sites 4, sites 4, error 5)

$ make zed-check
zed-check: OK (42 properties checked)
Ran 19 tests in 2.556s — OK

$ make test-critical            # first run, at load ~20
# 40 suites launched, 39 `test result: ok`, 1 FAILED:
#   critical_php_embed::a_probe_that_hangs_is_killed_and_refuses_the_boot
#   "the probe child … did not load `libphp` and its shim within 250 ms"
#   (the MB.21-documented probe-budget flake; exit 2, 39 ok lines)
$ make test-critical            # quiet re-run, load ~9
# exit 0: 42 `test result: ok` lines, 0 failed, then the Python tier:
#   Ran 53 tests in 10.201s — OK

$ BLASTIT_LSP_COMMAND="$PWD/target/debug/blastit lsp" uv run --no-project python -m unittest discover \
    -s python/tests/critical -p 'test_lsp_stdio.py' -v
Ran 3 tests in 0.017s — OK                      # the standalone binary
$ CARGO_NET_OFFLINE=true PYTHONPATH="$PWD/target/python-critical:$PWD/python" uv run --no-project python \
    -m unittest discover -s python/tests/critical -p 'test_lsp_stdio.py' -v
Ran 3 tests in 2.231s — OK                      # the console script, staged as test-critical stages it

$ cd editors/tree-sitter-blastfile && tree-sitter test      # pinned 0.25.10, not npx
Total parses: 15; successful parses: 15; failed parses: 0; success percentage: 100.00%

$ make mutants-rust PKG=blastit-manifest        # MUTANTS_TMP=/var/tmp/blastit-mutants-mb226
Found 1445 mutants to test
# 20 min at load ~20 → 15 outcomes (14 caught, 0 missed, 0 timeout, 1 unviable), all in error.rs;
# a 1 445-mutant campaign projects >30 h on this host, so it was stopped and re-scoped (§3).

$ uv run --no-project tools/critical_guard.py --base "$(git merge-base HEAD origin/develop)" --node MB.22
critical-guard: FAILED: 9 protected critical test(s) changed or deleted without a covering path in node(s) MB.22

$ uv run --no-project tools/plan.py validate
plan OK
$ git status --short crates python scripts editors Makefile
(no output — every regression above was reverted; the tree's other edits are plan/ and docs only)
```

### 6.1 The one flake, and why it is the host's

The first `make test-critical` (load ~20, a second project's suite running) failed
`critical_php_embed::a_probe_that_hangs_is_killed_and_refuses_the_boot` with the test's own message
("the probe child … did not load `libphp` and its shim within 250 ms … raise
`BLASTIT_PROBE_TIMEOUT_MS`"). The re-run pasted above, with nothing changed, is exit 0: 42 Rust
suites and 53 Python tests. Same class as `AUDIT-MB.21.md` §6 — a 250 ms budget on a loaded host.

`npx --yes tree-sitter-cli@0.25` was **not** run: this host has no `npm`/`npx`. The pinned
`tree-sitter 0.25.10 (da6fe9beb4f7f67beb75914ca8e0d48ae48d6406)` release binary ran every grammar
command, which is design §9.4's own substitution.
