# RESULT — MB.34.5

**STATUS: done.**

`blastit lsp` now advertises `codeActionProvider`, answers `textDocument/codeAction` with one
`quickfix` whose single `TextEdit` is `blastit_sqlpolicy::parameterize`'s own rewrite applied to
the whole `query` interior, and explains `$?`/`...` inside a `query` in the reference page's
words. `critical_lsp.rs` grew by ten cases and lost none.

Branch `release/0.2.0`, base `5f293985cc355b29dbac08421482b8ea84d1c677`. Attempt 1/3. Nothing was
committed, staged, pushed or planned by this leaf.

## What shipped

| File | Change |
|---|---|
| `crates/blastit/src/lsp/hover.rs` | `SQL_HOVER_TABLE` (the two paragraphs, transcribed from `docs/reference/blastfile.md`), `sql_hover`, and one unit test that joins the page's blockquotes and compares byte for byte |
| `crates/blastit/src/lsp/mod.rs` | `Lines::offset` + `Lines::point_at` (§4.6), `Document::sites` (one `OnceCell` cache, §13.3), `sites_of` (parse+lower, no validate), §7.6's token hover, the §7A handler, §7A.4's re-encoding, `"codeActionProvider": true`, and four unit tests |
| `crates/blastit/tests/critical_lsp.rs` | **+597 / −0**: a new "4a. The Quick Fix and the two SQL tokens" section with ten tests (C1–C8, H7, H8, P7) and four small helpers. No existing line was touched: `git diff -U0 … \| grep -c '^-[^-]'` → `0` |

No `Cargo.toml` change: `crates/blastit` already depends on `blastit-sqlpolicy`
(`crates/blastit/Cargo.toml:47`), so the handler calls `blastit_sqlpolicy::parameterize`
directly — `lsp/mod.rs` contains **no** `fn parameterize` and no literal scanner (`R66`; the
protected `the_lsp_module_is_a_text_tool` still passes).

## Where the brief was looser than the accepted design

1. **The interior's start point.** The brief writes `range == {point_at(interior.start),
   point_at(interior.end)}`; §7A.3 writes `Lines::point(q.at)` for the start. They are the same
   point (`q.at` *is* `interior.start` in 1-based `char`s), so the handler uses `point_at` for
   both ends — one conversion, and the test asserts the pair.
2. **`§7.6`'s inside-test is `start <= byte < end`, exactly** (corrected in the post-review
   pass below — an earlier draft widened it to `<= end`, which answered the hover on the closing
   quote). H7's third position is the **space inside** the interior after `$?` (`… price > $? AND
   …`), so the design's predicate satisfies H7 as written; the closing quote and the character
   after it answer `null`, and the protected suite pins both.
3. **C7's `\t` does not discriminate units** (a tab is one `char`, one UTF-16 unit and one byte);
   the surrogate pair does. C7 therefore puts `'😀'` **inside** the interior as well as `😀` above
   it, and the expected closing-quote character is **70** — a `char` count gives 69 and a byte
   count 72. Recorded in the test's doc comment.
4. **H7/H8's wording is compared against the page, not a third copy.** `critical_lsp.rs` joins
   the page's `> ` blockquote for each token (`include_str!`) and asserts equality, so the tier
   that owns the contract also catches a page/editor drift; the `lsp.rs`/`hover.rs` unit test
   checks the same bytes at the transcription site.
5. **`C5`'s "clean-query test (no diagnostics, `$1` present)".** The brief's C5 defect is
   "trusting `context.diagnostics`". The mutation below (M2) answers `[]` whenever the client's
   set is empty, and the test reds on the first of its three bodies.

## The handler, in one screen

`code_action(params)` — `uri` → the tracked `Document` (unknown ⇒ `[]`) → `range`'s two
`{line, character}` objects (missing/non-numeric ⇒ `-32602`) → `Lines::offset` twice (inverted ⇒
`[]`) → the one `QuerySite` whose interior the range intersects → `parameterize(&site.raw)`
(`None` ⇒ `[]`) → §7A.4 re-encoding → `newText == the interior` ⇒ `[]` (a real change, §7A.3(b))
→ `{title, kind, edit.changes[uri][one TextEdit]}`. `context` is never read. `Lines::point_at`
converts the interior's two byte ends.

## The ten new critical cases

| # | Test | Asserts |
|---|---|---|
| C1 | `code_action_provider_is_advertised_and_alone` | `codeActionProvider == true`, the exact five capability keys, no `codeActionKinds`/`resolveProvider` |
| C2 | `a_literal_query_gets_one_quick_fix` | one action, exact title/kind, one uri, one `TextEdit`, both range ends exact, `$?` and no `$1`, applied edit re-checks **Ok** and gets `[]` afterwards, no `diagnostics`/`isPreferred`/`command` |
| C3 | `both_spellings_are_parameterized` | `"…"` (escaped, value gone) and `'…'` (both `$n` folded) each: one action, exact range, applied re-checks clean |
| C4 | `nothing_is_offered_where_nothing_can_change` | `[]` for a clean `query`, for `SET TIME ZONE 'UTC'`, and for a redacted `…` |
| C5 | `context_diagnostics_is_never_trusted` | empty, fabricated and absent `context.diagnostics` all give the identical one action |
| C6 | `a_range_off_every_query_offers_nothing` | `[]` on a comment holding `$1`, on `body none`, on `regex '…'`, on the closing quote, and on an unknown `uri` — while the query's own caret still offers the fix |
| C7/P7 | `the_edit_range_is_utf16_across_a_surrogate_pair` | exact UTF-16 ends (15 … 70) past a surrogate pair **and** a tab, the bytes above the interior untouched, applied re-checks clean |
| C8 | `code_action_params_are_validated` | no `range` ⇒ `-32602`, non-numeric `character` ⇒ `-32602`, no `textDocument` ⇒ `-32602`, then the action — the session keeps serving |
| H7 | `hover_explains_the_placeholder` | `$?` at `$`, at `?` and just after it: byte-identical to the **page's** paragraph, range exactly the two characters |
| H8 | `hover_explains_the_variadic_marker` | `...` on all three dots, range over the three; `$?`/`...` in a comment and `regex '$?'` ⇒ `null`; `$DATABASE_URL` still answers |

Unit tests (`cargo test -p blastit --lib lsp`, 16 pass): `p7_the_offset_is_bytes_and_the_point_is_utf16`
(incl. the design's `offset(0,21) == 24`, `point_at(24) == (0,21)` and the round trip on every
boundary), `the_two_sql_tokens_are_source_spans`, `a_range_meets_an_interior_by_overlap_or_caret`,
`the_interior_is_re_encoded_for_its_spelling` (`SELECT "o".id …` → `SELECT \"o\".id …`),
`the_sql_paragraphs_are_the_pages_blockquotes`.

## Tests that can fail (each mutation applied, run, and restored with `cmp`)

```
================ M1 drop codeActionProvider
test code_action_provider_is_advertised_and_alone ... FAILED
  left: None
 right: Some(Bool(true))
test result: FAILED. 0 passed; 1 failed
RESTORED M1 drop codeActionProvider
================ M2 trust context.diagnostics          (early `[]` when the client's set is empty)
test context_diagnostics_is_never_trusted ... FAILED
  left: Some(0)
 right: Some(1)
RESTORED M2 trust context.diagnostics
================ M3 edit from the opening quote        (`interior.start.saturating_sub(1)`)
test a_literal_query_gets_one_quick_fix ... FAILED
  left: Object {"character": Number(14), "line": Number(4)}
 right: Object {"character": Number(15), "line": Number(4)}
RESTORED M3 edit from the opening quote
================ M4 first site, any range            (`.first()`, no `intersects`)
test a_range_off_every_query_offers_nothing ... FAILED
  left: Array [Object {"edit": …, "kind": String("quickfix"), "title": String("Parameterize SQL for Blastfile ($?)")}]
 right: Array []
RESTORED M4 first site, any range
================ M5 the SQL-token branch never fires  (`inside(site, byte) && false`)
test hover_explains_the_variadic_marker ... FAILED   left: Null   right: "`...` is Blastit's variadic marker, …"
test hover_explains_the_placeholder ... FAILED       left: Null   right: "markdown"
RESTORED M5
```

M3 is the brief's "edit only the literal instead of the interior": the start is one unit early,
which is exactly the opening quote. M4 is "a range outside a `query`" (C6). M2 is "trust
`context.diagnostics`". M1 is "dropping `codeActionProvider`". M5 is the hover half.

## Acceptance

### 1. `uv run --no-project tools/plan.py validate`

```console
$ uv run --no-project tools/plan.py validate
plan OK
exit=0
```

### 2. `cargo test -p blastit --lib lsp`

```console
$ timeout 300 cargo test -p blastit --lib lsp
test lsp::tests::p2_a_non_bmp_character_costs_two_utf16_units ... ok
test lsp::tests::the_body_bound_is_the_parser_cap_twice_plus_one_chunk ... ok
test lsp::tests::the_char_index_follows_utf16_units ... ok
test lsp::hover::tests::hover_table_is_bounded_and_transcribed ... ok
test lsp::tests::the_two_sql_tokens_are_source_spans ... ok
test lsp::tests::the_squiggle_covers_the_word_or_one_unit ... ok
test lsp::tests::the_word_under_the_cursor_is_the_run_and_its_dollar ... ok
test lsp::tests::the_interior_is_re_encoded_for_its_spelling ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.00s

exit=0
```

### 3. `cargo test -p blastit --test critical_lsp`

```console
$ timeout 300 cargo test -p blastit --test critical_lsp
test code_action_params_are_validated ... ok
test code_action_provider_is_advertised_and_alone ... ok
test a_literal_query_gets_one_quick_fix ... ok
test both_spellings_are_parameterized ... ok
test a_range_off_every_query_offers_nothing ... ok
test context_diagnostics_is_never_trusted ... ok
test hover_explains_the_placeholder ... ok
test hover_explains_the_variadic_marker ... ok
test nothing_is_offered_where_nothing_can_change ... ok
test the_edit_range_is_utf16_across_a_surrogate_pair ... ok

test result: ok. 52 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.18s

exit=0
```

(`52 = 42` pre-existing, all still present and unedited, `+ 10` new.)

### 4. `cargo clippy -p blastit --all-targets -- -D warnings`

```console
$ timeout 900 cargo clippy -p blastit --all-targets -- -D warnings
    Checking blastit v0.2.0 (…/.worktrees/release-0.2.0/crates/blastit)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.23s
exit=0
```

### 5. `make critical-guard NODE=MB.34.5`

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.34.5
critical-guard: HEAD..working tree, 54 changed file(s)
node MB.34.5: paths ['crates/blastit/src/lsp/mod.rs', 'crates/blastit/src/lsp/hover.rs', 'crates/blastit/tests/critical_lsp.rs']
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_lsp.rs  [authorized by crates/blastit/tests/critical_lsp.rs]
critical-guard: FAILED: 1 protected critical test(s) changed or deleted without a covering path in node(s) MB.34.5. Add the file to the node paths, or report STATUS: blocked and let the lead send it to the test-curator.
exit=1
```

**The one violation is not mine.** `crates/blastit-manifest/tests/critical_manifest_011.rs` is
`MB.34.4`'s uncommitted work in this shared worktree (`git diff --numstat` → `5 0`, the five
`at: Position::at(…)` constructions its RESULT.md documents); my node's `paths` cannot cover it.
Run with the sibling node named — the tool's documented multi-node form, and the state the lead
gets once MB.34.3/34.4 are committed — it is green, with my file the only other protected change:

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.34.5 --node MB.34.4
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_011.rs]
  M  crates/blastit/tests/critical_lsp.rs  [authorized by crates/blastit/tests/critical_lsp.rs]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.34.5, MB.34.4
exit=0
```

And the proof that I only **added** to the protected suite:

```console
$ git diff --numstat crates/blastit/tests/critical_lsp.rs
561     0       crates/blastit/tests/critical_lsp.rs
$ git diff -U0 crates/blastit/tests/critical_lsp.rs | grep -c '^-[^-]'
0
```

### 6. `make check`

The whole fast unprivileged gate is green on the delivered tree: `exit=0`, 184
`test result: ok` lines and zero failures, `zed-check: OK (47 properties checked)`,
`plan OK`. Its own `critical-guard` step runs **without** `NODE` (report only), which is why
it prints the same two files as “would fail an enforced run”; §5 above shows both are
authorized the moment the sibling node is named.

```console
$ timeout 3000 make check
uv run --no-project tools/guard.py
guard: OK (32 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD 
critical-guard: HEAD..working tree, 54 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_lsp.rs  [VIOLATION: not covered by any node path]
  report only: 2 breaking change(s) would fail an enforced run
cargo fmt --all -- --check
…
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
ok   cargo: workspace
ok   cargo: crate-type
ok   license: inside
ok   tree: no build artifacts
ok   tree: gitignore
ok   lib: which order
ok   lib: argv
ok   lib: shell_env
ok   lib: no process
ok   lib: error string
ok   lib: no capabilities
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (47 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
........................
----------------------------------------------------------------------
Ran 24 tests in 4.725s

OK
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

## The recorded session (not gated): Zed is not installed here

`which zed` → `no zed` on this host, so no GUI screenshot can be recorded honestly. What is
recorded instead is the exact wire transcript the extension produces, against a real
`target/debug/blastit lsp` — lightbulb → edit → squiggle gone. **Server resolution:** the
extension asks `worktree.which("blastit")` first and spawns `blastit lsp`, else falls back to
`uv run --frozen --no-sync --quiet blastit lsp` (`editors/zed/src/lib.rs:31-42`); here it was a
workspace build, `cargo build -p blastit --bin blastit` → `target/debug/blastit lsp`.

```console
$ uv run --no-project /tmp/opencode/live_session.py target/debug/blastit
<- initialize.result.capabilities.codeActionProvider = true

=== a pasted literal (a squiggle)
> the buffer:
postgres $DATABASE_URL

view v "/x" {
    postgres {
        query "SELECT id FROM products WHERE email = 'a@b.com'"
    }
}
<- publishDiagnostics (before) [1]: [
 {
  "message": "route \"v\" (index 0): queries[0] inlines a quoted literal (grant on proxy \"db\"); bind the value with `$?` instead of writing it in the statement",
  "range": {
   "end": {
    "character": 54,
    "line": 4
   },
   "start": {
    "character": 53,
    "line": 4
   }
  },
  "severity": 1,
  "source": "blastit"
 }
]
<- codeAction.result[0].title = "Parameterize SQL for Blastfile ($?)"
<- codeAction.result[0].kind  = "quickfix"
<- codeAction.result[0].edit = {"newText": "SELECT id FROM products WHERE email = $?", "range": {"end": {"character": 62, "line": 4}, "start": {"character": 15, "line": 4}}}
-> the buffer after the edit:
postgres $DATABASE_URL

view v "/x" {
    postgres {
        query "SELECT id FROM products WHERE email = $?"
    }
}
<- publishDiagnostics (after) [0]: []

=== a psql paste (a $1, no squiggle)
> the buffer:
postgres $DATABASE_URL

view v "/x" {
    postgres {
        query "SELECT id FROM products WHERE email = $1"
    }
}
<- publishDiagnostics (before) [0]: []
<- codeAction.result[0].title = "Parameterize SQL for Blastfile ($?)"
<- codeAction.result[0].kind  = "quickfix"
<- codeAction.result[0].edit = {"newText": "SELECT id FROM products WHERE email = $?", "range": {"end": {"character": 55, "line": 4}, "start": {"character": 15, "line": 4}}}
-> the buffer after the edit:
postgres $DATABASE_URL

view v "/x" {
    postgres {
        query "SELECT id FROM products WHERE email = $?"
    }
}
<- publishDiagnostics (after) [0]: []

=== the protocol's own setup statement
> the buffer:
postgres $DATABASE_URL

view v "/x" {
    postgres {
        query "SET TIME ZONE 'UTC'"
    }
}
<- publishDiagnostics (before) [0]: []
<- codeAction.result = [] (nothing to offer)

-> shutdown: {"id": 99, "jsonrpc": "2.0", "result": null}
-> exit: 0 | stderr: ''
```

The transcript also shows the design's own `SET TIME ZONE 'UTC'` refusal at the protocol level,
and that the `$1` paste offers the fix **without** a squiggle (the second trigger).

## Follow-ups (out of scope)

1. **`make critical-guard NODE=MB.34.5` is red only because MB.34.4's uncommitted change to
   `critical_manifest_011.rs` is in the same working tree.** Once MB.34.3/34.4 are committed (or
   the guard is run with both nodes), it is green — shown above. Nothing for this node to fix.
2. **`crates/blastit-sqlpolicy/src/template.rs` is being edited by a sibling node while this
   session ran** (mtime moved during the run; at one point `parameterize` returned the identity
   text for a clean query because of a temporary `&& !true`). The handler does not depend on that
   transient state: §7A.3(b)'s "a real change" guard (`newText == the interior ⇒ []`) keeps the
   clean-query answer correct either way, and the final recorded run is against the current file.
3. **`the_lsp_module_is_a_text_tool` forbids the substring `fn parse`** in `lsp/{mod,hover}.rs`,
   so the site helper had to be named `sites_of` rather than `parse_sites`. Worth a line in
   `docs/knowledge/LESSONS.md`; not appended here because `docs/**` is outside this node's
   `paths`.

## Post-review MINORs (2026-11-18, reviewer VERDICT: PASS)

Three code MINORs, fixed as asked; nothing else moved (no grammar, no docs, no manifest).

1. **`inside()` accepted `byte == interior.end`.** Now `site.interior.start <= byte && byte <
   site.interior.end`, the design's line (`docs/design/blastfile-lsp-zed.md:1228`), and the
   function's doc comment no longer claims a boundary rule the code does not have. The
   protected suite gained the case the reviewer reproduced: in `hover_explains_the_placeholder`,
   a `query '… WHERE price > $?'` (a statement that **ends** with the token) is hovered on the
   closing quote and on the character after it, and both answer `null`. H7's original three
   positions stay green because its third one is the **space inside** the interior.
2. **The third `...` branch in `sql_token` was unreachable.** Deleted, with the doc comment's
   "or the run starting at the cursor". A 3-byte slice at `at` that reads `...` implies
   `bytes[at] == b'.'`, which the dot-run branch has already answered.
3. **Four assertions tested the test's own values.** `both_spellings_are_parameterized` now binds
   `edit["newText"]` and asserts `$1`/`$2`/`@` on the server's bytes, and
   `the_edit_range_is_utf16_across_a_surrogate_pair` asserts `edit["newText"]` against the real
   expected statement (`… tag = $? AND email = $?`, the tab folded by `normalize`); the
   `applied.starts_with(…)` / `applied.len() == …` pair is gone, because the two range
   assertions already imply it.

### Each fix is shown able to fail

```
================ G1 inside() accepts the closing quote, again
test hover_explains_the_placeholder ... FAILED        (the new closing-quote case)
  left: Object {"contents": Object {"kind": String("markdown"), "value": String("`$?` is one bind
        placeholder: …")}, "range": Object {"end": {"character": Number(48), "line": Number(4)},
        "start": {"character": Number(46), "line": Number(4)}}}
 right: Null
RESTORED G1
================ G2a the dot-run branch disabled (`if false`, the live branch)
test lsp::tests::the_two_sql_tokens_are_source_spans ... FAILED
  left: None
 right: Some((34, 37))
RESTORED G2a
================ G2b the deleted branch, re-inserted behind the shipped answer
  (temporary equivalence test: 8 fixtures × every byte offset)
equivalence: 163 positions, 0 differences
test tmp_the_deleted_branch_never_changed_an_answer ... ok
RESTORED G2b
================ G3a the newText keeps a trailing space
test both_spellings_are_parameterized ... FAILED
  left: String("SELECT id FROM t WHERE email = $? ")
 right: "SELECT id FROM t WHERE email = $?"
test the_edit_range_is_utf16_across_a_surrogate_pair ... FAILED
  left: String("SELECT id FROM products WHERE tag = $? AND email = $? ")
 right: "SELECT id FROM products WHERE tag = $? AND email = $?"
RESTORED G3a
================ G3b the newText keeps `$1` numbering
test both_spellings_are_parameterized ... FAILED
  left: String("SELECT id FROM t WHERE email = $1")
 right: "SELECT id FROM t WHERE email = $?"
RESTORED G3b
================ G3b' the newText keeps the literal value
test both_spellings_are_parameterized ... FAILED
  left: 0
 right: 1
RESTORED G3b'
```

G2b's temporary test is not in the delivered tree (added, run, removed with `cmp`): it proves the
deletion by construction — with the branch re-inserted behind the shipped answer, no position of
any fixture changes, so the branch could not have been answering anything.

### Acceptance after the fixes (full tails)

```console
$ PATH=… cargo fmt --all -- --check
FMT-OK

$ uv run --no-project tools/plan.py validate
plan OK
exit=0

$ timeout 300 cargo test -p blastit --lib lsp
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.01s
exit=0

$ timeout 300 cargo test -p blastit --test critical_lsp
test result: ok. 52 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.39s
exit=0

$ timeout 900 cargo clippy -p blastit --all-targets -- -D warnings
    Checking blastit v0.2.0 (…/.worktrees/release-0.2.0/crates/blastit)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.73s
exit=0

$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.34.5 --node MB.34.4
  M  crates/blastit/tests/critical_lsp.rs  [authorized by crates/blastit/tests/critical_lsp.rs]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.34.5, MB.34.4
exit=0
```

`git diff --numstat` after the pass: `mod.rs 434/17`, `hover.rs 84/1`, `critical_lsp.rs **597/0**`
— still no deleted line in the protected suite.

### `make check` after the fixes

```console
$ timeout 3000 make check
uv run --no-project tools/guard.py
guard: OK (32 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD 
critical-guard: HEAD..working tree, 55 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_lsp.rs  [VIOLATION: not covered by any node path]
  report only: 2 breaking change(s) would fail an enforced run
cargo fmt --all -- --check
…
ok   highlights: captures
ok   highlights: variable pair
ok   highlights: sql captures
ok   indents: capture
ok   brackets: captures
ok   outline: captures
ok   highlights: copies agree
ok   cargo: workspace
ok   cargo: crate-type
ok   license: inside
ok   tree: no build artifacts
ok   tree: gitignore
ok   lib: which order
ok   lib: argv
ok   lib: shell_env
ok   lib: no process
ok   lib: error string
ok   lib: no capabilities
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (47 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
.........................
----------------------------------------------------------------------
Ran 25 tests in 4.817s

OK
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

**The lead committed this node (and MB.34.3/34.4/34.6) as `d2b9f07` while the second `make check`
was still running**, so that run's report-mode `critical-guard` line saw the pre-commit tree
(“55 changed file(s)”). The commit carries exactly the post-review state — strict `inside()`, no
third `sql_token` branch, `critical_lsp.rs` +597/−0 — and on the committed tree:

```console
$ timeout 300 cargo test -p blastit --test critical_lsp
test result: ok. 52 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.25s
exit=0

$ timeout 300 cargo test -p blastit --lib lsp
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 246 filtered out; finished in 0.00s
exit=0

$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.34.5
critical-guard: HEAD..working tree, 5 changed file(s)
node MB.34.5: paths ['crates/blastit/src/lsp/mod.rs', 'crates/blastit/src/lsp/hover.rs', 'crates/blastit/tests/critical_lsp.rs']
  no change to a protected critical test
exit=0
```

Nothing was staged, committed, pushed or planned by this leaf; the only file this pass leaves
modified is this `RESULT.md`.
