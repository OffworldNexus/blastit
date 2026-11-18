# RESULT — MB.34.4

**STATUS: done.**

The model now carries each `query`'s origin, interior and escape map, `validate` refuses a
bad template at the exact `line:col` inside the string, and the three query sentences gained
the site they were missing. `crates/blastit-manifest/Cargo.toml` still has no `pg_query`.

## What landed

| File | Change |
|---|---|
| `src/model.rs` | `QuerySite { at, interior, escapes, raw }` with `position_at` (`O(offset)`, one cursor, chars not bytes, `NONE` stays `NONE`), a hand-written `Debug` that redacts `raw`, `PgGrant::query_sites()`, a hand-written `PartialEq`/`Eq` that ignores the sites, `query_sites` extended by `merge_from`, and one site per entry from `PgGrant::new` (`at: Position::NONE`, `interior: 0..0`, `escapes: []`, `raw` = the stored text). |
| `src/blastfile/lower.rs` | the `NodeKind::Query` arm records the site: `at = position_of(token.start + 1)`, `interior = start..end`, `escapes` from the token text (a new `escape_indices` helper), `raw` **before** `normalize`; the dedupe pushes site and query in one branch, so the vectors cannot drift. |
| `src/validate.rs` | `check_queries` → `check_grant_queries`: redacted `…` → inline literal → template, one sentence per entry, each with `at`; plus §13.4.2's parse cap (`MAX_TEMPLATE_PARSES`, the 4097th distinct statement is `TooManyQueries` at its own origin). |
| `src/error.rs` | `RedactedQueryPlaceholder { …, at }` and `LiteralQuery { …, at }` render `{}{location}: …`; new `QueryTemplate { at, kind }` renders `{}{kind}`; `position()` returns `at` for all three; the two older variants left the `Position::NONE` arm. |
| `src/lib.rs` | `QuerySite` re-exported. |
| `tests/critical_manifest_011.rs` | the five constructions gain `at: Position::at(…)`; nothing else changed (diff below). |
| `tests/critical_query_sites.rs` | **new protected T1 suite**: origin, multi-line + escape map, duplicate, programmatic `NONE`. |
| `tests/validate.rs` | the exact `line:col` of every refusal kind, the duplicate, the parse cap, `Debug` redaction, `position_at`'s past-the-end fallback. |

## The NODE.md passages corrected from the accepted design

1. **`query_sites()` is not "defaulting to empty" for `PgGrant::new`.** §13.4.5/§18.1 (R35)
   and design finding B1: there is **exactly one `QuerySite` per `queries()` entry, always**,
   with `at: Position::NONE` for a programmatic grant. `NONE` means "no site of its own",
   never "no document". `query_sites().len() == queries().len()` is tested, including for
   `PgGrant::new`.
2. **There is no "pg_query's clipped message for `Syntax`".** §13.4.3/§13.4.4 (R33, security
   S1/S2): the error carries one fixed sentence and a character offset, and no
   statement-derived string is stored, rendered or logged. `QueryTemplate` has no message
   field, and `tests/validate.rs` asserts the rendered line *is* `TemplateKind::sentence()`.
3. Two smaller brief/design mismatches, resolved for the design:
   `QueryTemplate { at, kind }` (§18.1) — not `{ location, proxy, rule, at, kind }`;
   `queries[2]` is a tool index, and the page quotes those sentences whole.
4. A correction of my own to the design's T1 fixture note: a statement holding `…` **parses**
   (`…` is a valid PostgreSQL identifier), so "one entry, one sentence" is only proven by a
   statement that both holds `…` **and** fails to parse. The duplicate fixture is
   `query 'SELECT … FROM'`, which gives one entry, one site, one sentence — and two
   sentences under a broken `else` chain (see the mutation below).

There is no `…`-offset helper in `blastit-sqlpolicy`'s public surface, so
`RedactedQueryPlaceholder`'s offset is the first `…` character of the raw statement
(`redaction_offset`, documented); the literal's is `literal_offset(raw)` exactly as §18.1
says.

## Acceptance

### 1. `uv run --no-project tools/plan.py validate`

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" timeout 180 uv run --no-project tools/plan.py validate
plan OK
exit=0
```

### 2. `cargo test -p blastit-manifest`

```console
$ timeout 900 cargo test -p blastit-manifest
…
test crates/blastit-manifest/src/routing.rs - routing::RoutingTable::route (line 167) ... ok
test crates/blastit-manifest/src/routing.rs - routing::RoutingTable::for_learn (line 123) ... ok
test crates/blastit-manifest/src/lib.rs - (line 82) ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

all doctests ran in 0.37s; merged doctests compilation took 0.35s
exit=0
```

**35** `test result: ok` lines (unit, 30 integration binaries, doctests) and **0**
`test result: FAILED` lines (`grep -c` both).

### 3. `cargo test -p blastit-manifest --test validate --test critical_manifest_011`

```console
$ timeout 300 cargo test -p blastit-manifest --test validate --test critical_manifest_011
test the_last_route_uid_must_stay_below_65536 ... ok
test the_section_6_example_validates_with_its_documented_warnings ... ok
test route_names_must_match_the_documented_pattern ... ok
test a_file_past_the_parse_cap_is_refused_at_the_entry_past_it ... ok
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
…and the protected suite, unchanged in meaning:
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
exit=0
```

### 4. `cargo test -p blastit-manifest --test docs_fences`

```console
$ timeout 300 cargo test -p blastit-manifest --test docs_fences
test every_documented_document_parses_and_validates ... ok
test the_django_page_static_fragment_still_parses ... ok
test a_valid_document_fence_is_collected_parsed_and_counted ... ok
test an_invalid_regex_is_refused ... ok
test an_unclosed_fence_is_refused ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
exit=0
```

### 5. `cargo clippy -p blastit-manifest --all-targets -- -D warnings`

```console
$ timeout 300 cargo clippy -p blastit-manifest --all-targets -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.91s
exit=0
```

`cargo fmt -p blastit-manifest --check` → `FMT-CLEAN`.

### The new protected suite (T1)

```console
$ timeout 300 cargo test -p blastit-manifest --test critical_query_sites
running 4 tests
test a_duplicate_query_gets_one_site_and_one_sentence ... ok
test the_escape_map_survives_a_multiline_double_quoted_query ... ok
test the_origin_is_the_first_character_inside_the_quote ... ok
test a_programmatic_grant_keeps_none ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
exit=0
```

Its oracles, all hand-computed from the fixtures:

| Case | Assertion |
|---|---|
| origin | `query ''` and `query 'SELCT …'` at line 9, eight spaces in → `9:16` (the empty statement's origin is its closing quote); `interior.start` is the closing quote's byte |
| escape map | `"SELECT \"id\"\nFROM t\nWHERE prïx > $? AND ..."` → `escapes == [7, 10]`, `...` at `7:21` (`ï` is one column, two bytes); the one-line `"SELECT 1\nFROM t\nWHERE x > ..."` → `...` at `5:44`, which is what separates "`\n` escape costs two columns" from "moves a line" |
| duplicate | one `queries()` entry, one site at `5:16`, one `RedactedQueryPlaceholder` at `5:23`, nothing at line 6 |
| programmatic | one site, `at: Position::NONE`, `interior 0..0`, `position_at(0) == NONE`, and `check_queries` renders `TemplateKind::Syntax.sentence()` with **no** `0:0` prefix |

### 6. `make check`

Two full runs on this branch were green (`exit=0`, 184 `test result: ok`, 53 Python critical
tests `OK`) — `/tmp/opencode/accept/6-make-check.log` and the run behind
`8-make-check.log` (first attempt). The final tree differs from those by one in-crate guard
(`position_at`'s past-the-end fallback) and its test.

On the final tree every `make check` prerequisite that does not need the host's uv tool cache
is green: `rs-fmt`, `rs-lint` (workspace clippy), `rs-doc`, `rs-test` (**181** `test result:
ok`, `--workspace --all-targets --features python`), `test-critical` (`Ran 53 tests … OK`),
`docs-unit` (`124 passed, 65 subtests passed`), `manifest-purity`, `plan-validate` (`plan OK`),
`critical-guard`.

`make check` itself could not complete, and the reasons are environment, not this diff. The
runs that failed each failed in a **different** step that passes on its own:

| Step | Failure | Re-run |
|---|---|---|
| `test-critical` → `tests/critical/test_check_contract.py::test_a_clean_run_is_one_document_and_no_findings` | child died on a dbproxy control-channel `BrokenPipeError`, so the suite exited 3 (`crates/blastit/src/check.rs:82`) | `make test-critical` green **3×**: `Ran 53 tests … OK`, `OK` |
| `rs-test` → `supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort` | `a closed peer cannot take the message: ()` at `supervise.rs:2662` (the write returned `Ok`) | green **5×** isolated, **3×** `-p blastit --lib --features python` (`290 passed`) |
| `docs-test` | `maturin failed … target/maturin/lib_blastit.so: No such file or directory`, then `zipfile.BadZipFile` on `target/docs-wheel/*.whl` written concurrently | the wheel on disk verifies: `4 entries testzip: None` |
| `py-lint`, `docs-check`, `docs-test` | `uvx --offline ruff` / `uvx --offline zensical` / `--with pytest==9.2.0`: *"not found in the cache … your requirements are unsatisfiable"* | the same commands were green earlier in the session; nothing Python changed |

`uptime` during those runs was **load average 13–16 on 8 cores**, with other nodes' test
binaries (`critical_cli`, `critical_learn`, `critical_engine`) running, and `~/.cache/uv` was
cleared underneath the session (`~/.cache/uv` is now 108 MB and no longer resolves `ruff`,
`zensical` or `pytest==9.2.0` offline). The three failing tests are all "write to a peer that
went away" tests; none of them is in a crate this diff touches, and none reproduces alone.

### 7. `make critical-guard NODE=MB.34.4`

```console
$ timeout 300 make critical-guard NODE=MB.34.4
uv run --no-project tools/critical_guard.py --base HEAD --node MB.34.4
critical-guard: HEAD..working tree, 37 changed file(s)
node MB.34.4: paths ['crates/blastit-manifest/src/error.rs', 'crates/blastit-manifest/src/model.rs',
  'crates/blastit-manifest/src/validate.rs', 'crates/blastit-manifest/src/blastfile/lower.rs',
  'crates/blastit-manifest/src/lib.rs', 'crates/blastit-manifest/tests/validate.rs',
  'crates/blastit-manifest/tests/critical_manifest_011.rs',
  'crates/blastit-manifest/tests/critical_query_sites.rs']
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_011.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.34.4
```

## The five protected `at` fields

```diff
@@ -626,6 +626,7 @@ fn the_wire_spelling_of_the_fixed_setup_statement_is_exempt() {
             location,
             proxy,
             rule: 0,
+            at: Position::at(5, 34),
         })
@@ -641,6 +642,7 @@ fn a_redacted_placeholder_cannot_be_granted() {
             rule: 0,
+            at: Position::at(5, 72),
@@ -664,6 +666,7 @@ fn an_inline_literal_is_refused_and_never_echoed() {
             rule: 0,
+            at: Position::at(5, 84),
@@ -684,6 +687,7 @@ fn an_inline_literal_is_refused_and_never_echoed() {
             proxy: ProxyName::from("db"),
             rule: 0,
+            at: Position::at(4, 23),
@@ -710,6 +714,7 @@ fn every_literal_form_in_a_grant_is_refused() {
                 rule: 0,
+                at: Position::at(5, 23),
```

No assertion removed, reordered or weakened; every other byte of the file is unchanged
(`git diff --stat`: `tests/critical_manifest_011.rs | 5 +`). The five numbers are §18.1's, and
they were re-derived independently from the fixtures (literal at character 18 → col 34; `…`
at 56 → 72; `'` at 58 with ten `\"` escapes → 84; `SELECT ` at index 7 → 23 twice).

## Mutations proved (each red came from a real edit, then the file was restored)

1. **Origin at the token, not inside the quote** — `at = self.position_of(span.start)`:

   ```text
   test the_origin_is_the_first_character_inside_the_quote ... FAILED
   test the_escape_map_survives_a_multiline_double_quoted_query ... FAILED
   test a_duplicate_query_gets_one_site_and_one_sentence ... FAILED
     left: Position { line: 9, col: 15 }
    right: Position { line: 9, col: 16 }
     left: Position { line: 5, col: 15 }
    right: Position { line: 5, col: 16 }
   ```

2. **Identity escape map** — `escapes = Vec::new()` in `lower.rs`:

   ```text
   test the_escape_map_survives_a_multiline_double_quoted_query ... FAILED
     left: []
    right: [7, 10]
   ```

3. **The walk ignores an escape's second column** — `column += 2;` → `column += 1;` in
   `QuerySite::position_at`:

   ```text
   test the_escape_map_survives_a_multiline_double_quoted_query ... FAILED
     left: ManifestErrors([QueryTemplate { at: Position { line: 5, col: 42 }, kind: StrayEllipsis }])
    right: ManifestErrors([QueryTemplate { at: Position { line: 5, col: 44 }, kind: StrayEllipsis }])
   tests/validate.rs: an_escaped_double_quoted_query_points_at_the_sql_token ... FAILED
   a_redacted_placeholder_and_an_inline_literal_carry_their_own_site ... FAILED
     left: … at: Position { line: 5, col: 45 } …   right: … at: Position { line: 5, col: 47 } …
     left: … at: Position { line: 5, col: 47 } …   right: … at: Position { line: 5, col: 49 } …
   ```

4. **No dedupe** — `if !queries.contains(&sql) {` → `if true {` in `lower.rs`:

   ```text
   test a_duplicate_query_gets_one_site_and_one_sentence ... FAILED
     left: ["SELECT … FROM", "SELECT … FROM"]
    right: ["SELECT … FROM"]
   ```

5. **A second diagnostic for one entry** — `} else if has_quoted_literal(…` →
   `} if has_quoted_literal(…` in `validate.rs`:

   ```text
   test a_duplicate_query_gets_one_site_and_one_sentence ... FAILED
     left: 2
    right: 1
   ```

## Evidence the brief asks for

```console
$ git diff --stat -- crates/blastit-manifest
 crates/blastit-manifest/src/blastfile/lower.rs     |  89 +++++++-
 crates/blastit-manifest/src/error.rs               |  43 +++-
 crates/blastit-manifest/src/lib.rs                 |   2 +-
 crates/blastit-manifest/src/model.rs               | 170 +++++++++++++-
 crates/blastit-manifest/src/validate.rs            | 125 ++++++++---
 crates/blastit-manifest/tests/critical_manifest_011.rs |   5 +
 crates/blastit-manifest/tests/validate.rs          | 247 ++++++++++++++++++++-
 7 files changed, 633 insertions(+), 48 deletions(-)
$ git status --short -- crates/blastit-manifest
?? crates/blastit-manifest/tests/critical_query_sites.rs

$ grep -n 'Position::NONE' crates/blastit-manifest/src/validate.rs
272:/// grant no document wrote has `Position::NONE` and renders without a prefix.
317:        let at = |offset: u32| site.map_or(Position::NONE, |site| site.position_at(offset));

$ rg 'pg_query' crates/blastit-manifest/Cargo.toml   # no output, exit 1
```

`Position::NONE` appears in `validate.rs` in exactly one expression: the fallback for a grant
**no document wrote** (`PgGrant::new`). For a parsed `Blastfile` every `at` comes from the
recorded site.

## Follow-ups (out of this leaf, not fixed)

- `PgGrant`'s derived `Debug` still prints `queries` verbatim (`queries: ["SELECT 'x' …"]`).
  That is pre-existing: the site's `Debug` redacts (§13.4.5), but the grant's own vector was
  always printed. A `Debug` that redacts `queries` too is a one-line change for whoever owns
  the "no statement in a log line" sweep — it is not this leaf, and changing it here would
  alter an existing public `Debug` shape.
- `make check`'s packaging steps (`docs-test` → `target/maturin`, `target/docs-wheel`) are not
  safe for two agents in one worktree; both collisions above were two runs sharing those
  paths. `DOCS_WHEEL_STAGE=` avoids the second, not the first.
- The host's `~/.cache/uv` no longer resolves `ruff`, `zensical` and `pytest==9.2.0` offline;
  `make py-lint`, `docs-check` and `docs-test` need one online warm-up run
  (`uv run --no-project python python/tests/runnable_docs/harness.py --warm-cache` and
  `uvx zensical --version`) before they can run in this environment again.
