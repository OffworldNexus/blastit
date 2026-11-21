# RESULT MB.21.5.7

## Summary

§20's round-trip corpus is now **derived and floored**, not the fixture set:

- `crates/blastit-manifest/tests/common/mod.rs` gained a fence reader (a copy of
  `docs_fences.rs`'s scanner, since a test binary cannot depend on another), the
  §16.1 classification (`Document` / `Fragment` / `Illustration`), and
  `round_trip_corpus()`: `tests/fixtures/*.blastfile` **enumerated** plus every
  `text title="Blastfile"` fence off `README.md` and `docs/**` **enumerated**,
  labelled `fixtures/<file>` / `<page>:<line>`. It asserts
  `len() >= ROUND_TRIP_FLOOR` (20) and that the documented half is non-empty, so
  a corpus that shrinks to three files fails by name. `CORPUS_SIZE` keeps its
  exact-15 **fixture** meaning for the fixture-only suites.
- `critical_blastfile_print.rs` runs **P1, P2, P2b and P3** over that corpus
  (today 15 fixtures + 12 documented documents = 27, measured by the helper, no
  literal). New `p1_no_token_or_comment_is_dropped` is P1 at the public
  boundary: byte-level `print_lossless` is `pub(crate)` (§14.3.1, module test),
  so the integration oracle is "no token and no comment is dropped, duplicated,
  reordered or rewritten" through the suite's own scanner.
- Documented **fragments** (keyword-led plain `text` fences = statement lists)
  are classified explicitly and checked, never skipped:
  `the_documented_fragments_are_classified_and_checked` proves the three classes
  partition every fence, then runs P1/P2/P2b per fragment and asserts formatting
  never changes whether a fragment lowers. Two fences are *menus of alternative
  spellings* (`docs/reference/blastfile.md:113` declares two unnamed stores on
  purpose, `:342` a second `app`); they are printed by page:line, derived per
  fence, never tabled.
- `print.rs::p2b_comment_owners_survive` gained the §1.3 nit 7 shape: a comment
  written inside an unwrapped grant list (`select products(id), #c categories`),
  plus a targeted assertion that the hoisted comment's owner node is the node
  that owns the `select` statement (`grant_block`, and its direct children
  include the `select` token).

**Protected-test edit, authorised by `paths`: yes. Goal reason:** §20 requires
`critical_blastfile_print.rs` to run P1–P3 over `>= 20` documents, and the
fixture-only loops in that file were the gate that could pass vacuously. Every
existing assertion is kept: `assert_eq!(common::CORPUS_SIZE, 15)` and
`assert_eq!(common::corpus().len(), CORPUS_SIZE)` still pin the fixture set, and
`the_corpus_is_canonical` still loops the 15 fixtures only. The diff is additive
apart from moving three loops onto the wider corpus.

## Evidence per acceptance item

### 1. P1/P2/P2b/P3 over `>= 20` documents (fixtures + documented documents)

```console
$ timeout 300 cargo test -p blastit-manifest --locked --test critical_blastfile_print
running 11 tests
test p1_no_token_or_comment_is_dropped ... ok
test p2_fixed_point_over_the_corpus ... ok
test p2b_comments_survive ... ok
test p3_over_the_corpus ... ok
test the_documented_fragments_are_classified_and_checked ... ok
test the_corpus_is_canonical ... ok
... (6 more)
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.82s
```

The corpus size is printed by the helper's own refusal message when the floor is
tripped (see item 5): `15 fixtures + 12 documented` = **27**, derived from the
directory and the pages. The node brief said "ten" documents; the pages carry
twelve today, and the helper counts them, so nothing drifts.

### 2. The floor is an assertion; `CORPUS_SIZE` keeps exact-15

`common::round_trip_corpus()`:

```rust
assert!(!documents.is_empty(), "§20's corpus needs the documented documents, ...");
assert!(corpus.len() >= ROUND_TRIP_FLOOR, "§20's round-trip corpus holds {} ...; the floor is {ROUND_TRIP_FLOOR}");
```

plus the same floor restated in `p2_fixed_point_over_the_corpus`
(`assert!(corpus.len() >= 20, ...)`). `CORPUS_SIZE` is unchanged (`= 15`) with its
doc comment tightened to "the fixture set, not the corpus".

### 3. The hoisted-comment owner case

```console
$ timeout 300 cargo test -p blastit-manifest --locked --lib blastfile::print
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 116 filtered out; finished in 2.83s
```

### 4. `blastfile_fmt`'s fixture loop still asserts 15

```console
$ timeout 300 cargo test -p blastit-manifest --locked --test blastfile_fmt
test the_corpus_is_the_fifteen_fixtures ... ok
test p2_idempotent_over_the_corpus ... ok
test p3_meaning_preserved_over_the_corpus ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.85s
```

### 5. Anti-vacuity: a shrinking corpus fails, both halves

(a) corpus truncated to 3 (temporary edit, reverted):

```console
thread 'p2_fixed_point_over_the_corpus' panicked at tests/common/mod.rs:453:5:
§20's round-trip corpus holds 3 document(s) (15 fixtures + 12 documented); the floor is 20
test result: FAILED. 0 passed; 1 failed; ...
```

(b) the documented half emptied (a page yields no document, temporary, reverted):

```console
thread 'p2_fixed_point_over_the_corpus' panicked at tests/common/mod.rs:439:5:
§20's corpus needs the documented documents, but no ```text title="Blastfile" fence was found on README.md or under docs/**
test result: FAILED. 0 passed; 1 failed; ...
```

(c) the hoisted-owner assertion is not vacuous (temporary `Some(NodeKind::File)`,
reverted):

```console
assertion `left == right` failed: TEMPORARY: the owner assertion is not vacuous
  left: Some(GrantBlock)
 right: Some(File)
```

### 6. Full crate + repo gates

```console
$ timeout 600 cargo test -p blastit-manifest --locked
# every suite `test result: ok`, 0 failures (130 lib + all integration + 7 doctests)

$ timeout 900 make docs-test
48 fences: 30 executed, 18 accounted, 0 failed · 24.4s          # EXIT=0

$ timeout 1700 make check
manifest-purity self-test: ok — 44 assertions, 18 fixtures proved, 16 real source file(s)
uv run --no-project tools/plan.py validate
plan OK                                                          # EXIT=0, 166 "test result: ok"
```

```console
$ timeout 300 uv run --no-project tools/critical_guard.py --base 7062ded22d6eb68ab74e764d719112894eaf6360 --node MB.21.5.7
  M  crates/blastit-manifest/tests/critical_blastfile_parser.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-manifest/tests/critical_blastfile_print.rs  [authorized by crates/blastit-manifest/tests/critical_blastfile_print.rs]
critical-guard: FAILED: 1 protected critical test(s) changed ... EXIT=1
```

That one violation is **not this node's**: `critical_blastfile_parser.rs` is the
uncommitted work of the concurrently active `MB.21.5.11`, whose `paths` name it.
With both nodes named, the tree is clean:

```console
$ uv run --no-project tools/critical_guard.py --base 7062ded2... --node MB.21.5.7 --node MB.21.5.11
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.21.5.7, MB.21.5.11   # EXIT=0
```

Also: `cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings`
clean, `cargo fmt --all -- --check` clean.

## Files

- `crates/blastit-manifest/tests/common/mod.rs` (+283): fence reader, §16.1
  classification, `documented_fences/documents/fragments`, `round_trip_corpus`,
  `ROUND_TRIP_FLOOR = 20`, `CORPUS_SIZE` doc tightened.
- `crates/blastit-manifest/tests/critical_blastfile_print.rs` (+174/−13, protected,
  authorised): P1 test, P2/P2b/P3 moved to the derived corpus (fixture-15
  assertions kept), fragment classification test.
- `crates/blastit-manifest/src/blastfile/print.rs` (+67): the hoisted-list
  document, `owner_of_comment`, and the "owner is the statement's node"
  assertion in `p2b_comment_owners_survive`.
- `plan/nodes/MB/MB.21/MB.21.5/MB.21.5.7/RESULT.md` (this file).

No `Cargo.toml`/`Cargo.lock` change, no new dependency.

## Follow-ups

1. `docs/design/blastfile-dsl.md` §20/§21 still say "the ten documented
   documents"; the pages carry twelve. The helper derives the count, so nothing
   breaks, but the sentence is stale (design docs are outside this node's paths).
2. `critical_guard --node MB.21.5.7` alone reports `MB.21.5.11`'s uncommitted
   `critical_blastfile_parser.rs`; re-run the guard after that node commits, or
   name both nodes (as above). Not a defect of this node.
3. `docs/knowledge/LESSONS.md` (outside this node's paths): a keyword-led
   `text` fence is not necessarily a lowerable file — `docs/reference/blastfile.md:113`
   puts three alternative store spellings in one fence and lowers to
   `store "db" is declared twice`. Fragment checks must therefore classify
   "lowers" vs "alternatives" per fence, never assume a statement list is a file.
4. `tests/docs_fences.rs` duplicates the fence scanner with this helper because a
   test binary cannot import another; a future `tests/support` dev-crate could
   remove the copy (needs `Cargo.toml`, so not here).

## Review Resolution

Round 1's verdict: accepted with two MINORs, both mine. The review's **MAJOR is
external** and not this node's: the uncommitted `MB.23.1`/`MB.23.2` docs document
`blastit dev --no-reload` before `cli.rs` accepts it; the lead has logged it as an
external blocker.

| reviewer finding | resolution | file:line |
|---|---|---|
| MINOR 1 — demoting one page's document is undetected: the corpus drops to 26 and still clears the floor of 20, so "a page that fails to yield a document fails the count" was not true | **fixed.** `DOCUMENTED_PAGES` pins the 12 pages that publish a document **as pages** (data, not a count), and `documented_documents()` asserts each one still yields at least one document, naming the page that dropped out; the check lives in the reader so `round_trip_corpus()` and the fragment suite inherit it. No bare total was added. | `crates/blastit-manifest/tests/common/mod.rs:413-449` |
| MINOR 2 — `illustrations = all.len() - documents.len() - fragments.len()` proves nothing, while the comment claimed a partition proof | **fixed.** The three counts are now independent (`documented_documents()`, `documented_fragments()`, and a direct `FenceClass::Illustration` filter over the scan) and asserted `documents + fragments + illustrations == all.len()`; the comment now says "the three filters are exhaustive over the scanned fences". The `illustrations > 0` subject check is kept. | `crates/blastit-manifest/tests/critical_blastfile_print.rs:541-566` |

### Mutation proofs

(a) one page's document fence demoted in `documented_fences` (temporary, reverted)
— the floor alone still passes (26 >= 20), the new per-page assertion fails and
names the page:

```console
thread 'p2_fixed_point_over_the_corpus' panicked at tests/common/mod.rs:450:9:
docs/local.md publishes a ```text title="Blastfile" document in §20's corpus and now yields none: a page that fails to yield a document fails the count
```

(b) 12 fences classified twice (the fragment list extended with the documents,
temporary, reverted) — the exhaustive assertion fails with independent counts:

```console
thread 'the_documented_fragments_are_classified_and_checked' panicked at tests/critical_blastfile_print.rs:560:5:
assertion `left == right` failed: the three filters are exhaustive over the scanned fences: 12 document(s) + 28 fragment(s) + 92 illustration(s) != 120 fence(s)
  left: 132
 right: 120
```

### Re-run after the restore (`make docs-test`/`make check` deliberately skipped: the tree is red from another epic's uncommitted docs)

```console
$ timeout 300 cargo test -p blastit-manifest --locked
33 x "test result: ok", 0 x "FAILED"                                                   # EXIT=0

$ timeout 300 cargo test -p blastit-manifest --locked --test critical_blastfile_print
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.91s

$ make docs-unit
107 passed, 65 subtests passed in 1.45s                                                 # EXIT=0

$ timeout 120 uv run --no-project tools/critical_guard.py --base 06874ae --node MB.21.5.7
  M  crates/blastit-manifest/tests/critical_blastfile_parser.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-manifest/tests/critical_blastfile_print.rs  [authorized by ...]
critical-guard: FAILED: 1 protected critical test(s) changed ...                       # EXIT=1

$ uv run --no-project tools/critical_guard.py --base 06874ae --node MB.21.5.7 --node MB.21.5.11
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.21.5.7, MB.21.5.11   # EXIT=0
```

The single-node run's only violation is still `MB.21.5.11`'s uncommitted
`critical_blastfile_parser.rs` (28 added lines, its own node's path); my file is
reported `authorized`. Re-run the single-node command once that node commits.
`cargo fmt --all -- --check` is clean and no temporary mutation is left in the
three files.
