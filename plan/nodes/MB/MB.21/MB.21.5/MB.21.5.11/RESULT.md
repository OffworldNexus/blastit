# Result MB.21.5.11

## Summary

**The product already refused the input; this node pins the existing guard; the audit's
"blocking product defect" characterisation was wrong (see `MB.21.5.3`'s review).**
`Parser::store_grant` (`crates/blastit-manifest/src/blastfile/parser.rs:1267-1274`) already fell through
`_ => Err(unexpected("`unrestricted` or `{`"))`, so `postgres db bogus` was a `line:col` refusal in the
shipped tree. The `cargo-mutants` survivor was the **mutant** (`parser.rs:1270:36`, guard → `true`) that
accepted the word and lowered it to an empty `PgGrant` — exactly the dead-grant footgun the audit
described. This node adds no behaviour: a coverage-only matrix pins the refusal and its siblings, and a
hand-applied copy of the audited mutant is now killed.

No `parser.rs` behaviour was changed (and none had to be reverted): `git diff` for that file touches only
`mod tests` (six `every_refusal_row` rows plus a comment). The protected `critical_blastfile_parser.rs`
edit is additive (two refusal rows and one accept-side test), authorised by this node's `paths` and
proved by `critical_guard`.

## Evidence

### 1. Deny-side: `postgres db bogus` / `… select` / `… unlimited`, and the grant-block siblings

`blastfile_grants.rs` names the sentence at the word's position, so an accept (and a lowering error)
cannot satisfy it:

```console
$ cargo test -p blastit-manifest --test blastfile_grants --locked
test a_second_bare_word_after_a_store_name_is_refused ... ok
test a_stray_word_inside_a_grant_block_is_refused ... ok
...
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The rows are pinned at both parser layers too:

```console
$ cargo test -p blastit-manifest --test critical_blastfile_parser --locked
test every_legal_store_and_grant_form_parses ... ok
test the_refusal_matrix ... ok
...
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
```

`every_refusal_row` (the unit matrix, `parser.rs`) carries the same rows: `postgres db bogus` →
`2:15: expected `unrestricted` or `{`, found `bogus``, `postgres db select` (a keyword) → the same at
`select`, a trait's `postgres db bogus`, and stray words after `select products` / `transactions` /
a trait's `select t(id)` (`expected a newline, `;` or `}``). `blastfile_grants.rs` adds
`postgres db unlimited` (a near miss for the one allowed word) and `postgres bogus` (no body: refused at
the newline).

### 2. The audited mutant is caught

Hand-applied copy of `parser.rs:1270:36` (`TokenKind::Word if true =>`), then the new suite:

```console
$ sed -i '1270s/self.peek_word() == Some("unrestricted")/true/' crates/blastit-manifest/src/blastfile/parser.rs
$ cargo test -p blastit-manifest --test blastfile_grants --locked
test a_second_bare_word_after_a_store_name_is_refused ... FAILED
  ... PgGrant { queries: [], transaction_control: false, unrestricted: false, tables: … }
test result: FAILED. 16 passed; 1 failed
$ cargo test -p blastit-manifest --test blastfile_grants --locked \
    -- --skip a_second_bare_word_after_a_store_name_is_refused
test result: ok. 16 passed; 0 failed; … 1 filtered out      # the old suite missed it
# restored: TokenKind::Word if self.peek_word() == Some("unrestricted") =>
```

The same mutation fails `critical_blastfile_parser::the_refusal_matrix` (`left: "1:8: view \"v\" has no
endpoint"`) and the lib's `every_refusal_row`. `cargo-mutants` (27.1.0, repo harness) re-measures the
mutant itself:

```console
$ MUTANTS_JOBS=3 TMPDIR=…/mb2159 scripts/mutants-run.sh -p blastit-manifest \
    -F 'parser\.rs:1270:36' --cargo-test-arg=--test --cargo-test-arg=blastfile_grants
Found 3 mutants to test
ok       Unmutated baseline in 89s build + 0s test
3 mutants tested in 3m: 3 caught
$ cat mutants.out/caught.txt
crates/blastit-manifest/src/blastfile/parser.rs:1270:36: replace match guard self.peek_word() == Some("unrestricted") with true in Parser<'a>::store_grant
crates/blastit-manifest/src/blastfile/parser.rs:1270:36: replace match guard self.peek_word() == Some("unrestricted") with false in Parser<'a>::store_grant
# missed.txt is empty
```

Deviations from the NODE snippet, both environmental: `scripts/mutants-run.sh` reads `TMPDIR`, not
`MUTANTS_TMP` (without `TMPDIR` it exits 2), and the whole-suite baseline was red on another session's
in-flight `docs_fences` edit at that moment, so the mutant run scoped its tests to `blastfile_grants`.

### 3. The accepted twins stay green

`critical_blastfile_parser::every_legal_store_and_grant_form_parses` parses `postgres db { transactions;
query 'SELECT 1'; select t(id) }`, the unnamed `postgres { … }`, `postgres unrestricted`,
`postgres db unrestricted`, a trait's `postgres { transactions; select t(id) }`, and a named store
(`postgres analytics $ANALYTICS_URL`). `blastfile_grants::a_named_store_grant_keeps_its_legal_forms`
additionally lowers and reads back the named block's `transaction_control`, `queries()` (`SELECT 1`) and
table grant, the unnamed store, `unrestricted`, and the trait's group grant.

### Crate-wide test run

```console
$ timeout 300 cargo test -p blastit-manifest --locked
EXIT=0
33 test result: ok; 0 FAILED
```

### `make check`

```console
$ make docs-unit
107 passed, 65 subtests passed in 1.39s

$ (all Rust gates before docs-test passed in one make check run: guard, critical-guard, rs-fmt,
   rs-lint, rs-doc, rs-test, test-critical, py-lint, docs-unit, docs-check)
$ make rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test \
    doc-tone manifest-purity plan-validate
EXIT=0
```

The `make check` run stops at the **last** leg only, on another live session's in-flight doc/CLI change
in this shared worktree (`docs/reference/cli.md` documents `blastit dev --no-reload` while
`crates/blastit/src` has no such flag yet; the file's diff is theirs, not this node's):

```console
FAIL  docs/reference/cli.md  fence 1  dev-flags  0.2s  blastit dev exited before ready
stderr:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 30.9s
make: *** [Makefile:331: docs-test] Error 1
```

Earlier attempts also died on ENOENT test binaries and a transient `docs_fences` count because two other
sessions were running `make check`/`cargo test` in the same worktree and target dir; those legs are green
once the other edits settled. `make docs-test` alone is green (48 fences, 0 failed) when run with a
private `DOCS_WHEEL_STAGE`.

### `critical_guard`

```console
$ uv run --no-project tools/critical_guard.py --base 7062ded22d6eb68ab74e764d719112894eaf6360 \
    --node MB.21.5.11
node MB.21.5.11: paths [...]
  M  crates/blastit-manifest/tests/critical_blastfile_parser.rs  [authorized by itself]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.21.5.11
```

## Files

- `crates/blastit-manifest/src/blastfile/parser.rs` — `every_refusal_row` rows only (no production change).
- `crates/blastit-manifest/tests/blastfile_grants.rs` — deny-side second-bare-word + grant-block stray-word tests, and the accept-side legal-forms test (plus `document_refusal` helper).
- `crates/blastit-manifest/tests/critical_blastfile_parser.rs` — two additive refusal rows and the accept-side legal-forms test (protected, authorised).

## Follow-ups

- Planner: `MB.21.5.11`'s `# Goal`/`# Context` should say *coverage for a surviving mutant*, not
  "an accept-side hole in the parser" — the shipped parser refused the input.
- `scripts/mutants-run.sh` reads `TMPDIR`; the NODE acceptance snippet sets `MUTANTS_TMP` only, so the
  literal command exits 2. Either read `MUTANTS_TMP` as a fallback or write `TMPDIR=` in the snippet
  (candidate `docs/knowledge/LESSONS.md` line, out of this node's `paths`).
- Shared-worktree hazard (candidate LESSONS): concurrent `make check` runs in one worktree alternate
  `blastit`'s feature-dependent build-script output dirs, deleting test binaries mid-run
  (`could not execute process … (os error 2)`), and transient doc/corpus edits make `docs_fences` /
  `critical_blastfile_print` red; coordinate or use a private target dir.
- `docs/reference/cli.md` advertises `blastit dev --no-reload` before the binary accepts it (MB.22 reload
  owner), so `make docs-test` is red on the current tree.
- Nothing else found in the store/grant grammar: `postgres <name>` with no body, `$IDENT` after a grant
  name, and a stray word in a grant block all refuse at their own position.
