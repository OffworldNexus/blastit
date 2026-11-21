# Result MB.21.3.6

## Summary

`crates/blastit-manifest/tests/docs_fences.rs` now reads **both** document openers. A complete
document is a ```` ```text title="Blastfile" ```` fence or a ```` ```blastfile ```` fence; each body
goes through `Manifest::parse_blastfile(body, UidLayout::DEFAULT_BASE)` and must declare a route. The
nine ```` ```toml title="blastit.toml" ```` pages still pass through `parse_and_validate` until
`MB.21.5.1` migrates them, and a plain ```` ```toml ```` fence is still a fragment.

The suite is now a **pinned count table**, not a "nine files, one fence each" assertion. `PAGES`
carries `(path, [text, blastfile, toml])` per page and `TOTAL` carries the sum; every failure renders
the three openings by name, so a red test says which one moved:

```console
assertion `left == right` failed: the documented set is 1 `text title="Blastfile"` + 0 `blastfile` + 9 `toml title="blastit.toml"`; a page that gained or dropped a complete document was not pinned in `PAGES` (§16.2)
```

That is `MB.21.5.2`'s flip surface: one `TOTAL` constant plus the row it moves, and one boolean
(`THE_RETIRED_OPENING_IS_GONE`) that turns on §16.2's "no ```` ```toml ```` fence on any pinned page"
rule. The flag's arm is exercised today through `check_counts(.., true)` in the drift self-test, so it
is not a dead branch when the flip lands.

**Facts reach the message.** A fence that does not parse is reported at the page's own
`file:line:col`: the fence body's lines *are* the page's lines, so `describe_failure` shifts each
refusal's body position by the opening line (`leading_position` reads the `line:col: ` the error
contract already renders). `docs/blastfile.md`'s real document is parsed this way.

**Three negative self-tests the suite lacked**, plus the collector test rewritten for both openers —
all on inline fixtures so they stay green while `MB.21.5.1` migrates the pages:

1. `both_document_openings_are_collected_and_a_fragment_is_not` (the old
   `only_a_complete_blastit_fence_is_collected`, widened) — both spellings collected, the fragment
   not, counts `[1, 1, 1]`, and the 1-based opening line recorded.
2. `an_unclosed_fence_is_refused` — `FenceError::Unclosed { opening, line: 2 }`, message names line 2.
3. `a_document_that_does_not_parse_names_its_page_position` — a `blastfile` fence whose body fails at
   `2:1` must surface `fixture.md:5:1:` (body starts on page line 4).
4. `a_page_whose_count_drifted_is_named` — the drift message names the page and the opening that
   moved, and the retired-opening arm fires only when the flip is on.

The standard-tier reasoning is kept in the header verbatim: `MA.3` owns `docs/**`, and a protected
copy would make every documentation edit a `critical_guard` conflict.

**No `docs/**` change and no library change.** The fence collector stays in the test file: it is
Markdown logic, not `Blastfile` logic, and `crates/blastit-manifest/src/blastfile/**` did not need a
helper (`paths` allowed it; nothing there changed). The only file touched is the suite.

## Evidence

All commands were run serially from the worktree at `release/0.2.0` (base `d652117`).

### 1. `cargo test -p blastit-manifest --locked --test docs_fences` — PASS

```console
$ cargo test -p blastit-manifest --locked --test docs_fences
running 9 tests
test a_document_that_does_not_parse_names_its_page_position ... ok
test a_page_whose_count_drifted_is_named ... ok
test an_unclosed_fence_is_refused ... ok
test an_invalid_regex_is_refused ... ok
test both_document_openings_are_collected_and_a_fragment_is_not ... ok
test the_static_page_manifest_has_its_documented_shape ... ok
test the_django_static_fragment_pins_its_documented_depth ... ok
test a_drifted_static_block_does_not_satisfy_the_documented_shape ... ok
test every_documented_document_parses_and_validates ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

The green run covers `docs/blastfile.md`'s real `text title="Blastfile"` document (the one page that
carries a complete document today) and the nine TOML pages.

### 2. The new tests fail when the reader is broken (mutation checks)

Both mutations were applied to the suite, run, and reverted.

**(a) Drop the `blastfile` opener from `DOCUMENT_OPENINGS`** — the alias stops being read:

```console
test a_document_that_does_not_parse_names_its_page_position ... FAILED
test both_document_openings_are_collected_and_a_fragment_is_not ... FAILED
assertion `left == right` failed
test result: FAILED. 7 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

**(b) Drop the `docs/blastfile.md` row from `PAGES`** — a page that carries a document is unpinned:

```console
thread 'every_documented_document_parses_and_validates' panicked at ...docs_fences.rs:358:5:
assertion `left == right` failed: the documented set is 1 `text title="Blastfile"` + 0 `blastfile` + 9 `toml title="blastit.toml"`; a page that gained or dropped a complete document was not pinned in `PAGES` (§16.2)
test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

The second tail is the "report the count of each opening" evidence: the failure names all three
openings with their counts.

### 3. `grep -n 'blastfile' crates/blastit-manifest/tests/docs_fences.rs` — the reader and its fixtures

```console
$ grep -n 'blastfile' crates/blastit-manifest/tests/docs_fences.rs
10://! Pygments has no `blastfile` lexer, §16.1) or `blastfile` (the transitional
11://! alias §19.8 records); both go through `Manifest::parse_blastfile`. The pages
34:const BLASTFILE_OPENING: &str = "```blastfile";
54:/// `blastfile`, `toml title="blastit.toml"` (§16.2).
60:        "{} `text title=\"Blastfile\"` + {} `blastfile` + {} `toml title=\"blastit.toml\"`",
81:    ("docs/blastfile.md", [1, 0, 0]),
246:/// names: `parse_blastfile` for a document, `parse_and_validate` for the
255:        Manifest::parse_blastfile(body, UidLayout::DEFAULT_BASE)
448:                    ```blastfile\napp \"shop\"\n```\n\n\
467:    let error = count_fences("intro\n```blastfile\napp \"shop\"\n")
483:    let document = "intro\n\n```blastfile\napp \"shop\"\nnot_a_directive\n```\n";
485:    let errors = Manifest::parse_blastfile(&fence.body, UidLayout::DEFAULT_BASE)
```

### 4. `cargo fmt --all -- --check` — PASS

```console
$ cargo fmt --all -- --check; echo "fmt=$?"
fmt=0
```

### 5. `cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings` — PASS

```console
$ cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings
    Checking blastit-manifest v0.2.0 (.../crates/blastit-manifest)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
```

### 6. `make check` — PASS (exit 0)

Run once, serially, at the end. No flake needed a re-run.

```console
$ make check
...
     Running tests/docs_fences.rs (target/debug/build/blastit-manifest/03058d3ba4b3eead/out/docs_fences-03058d3ba4b3eead)

running 9 tests
test a_page_whose_count_drifted_is_named ... ok
test an_unclosed_fence_is_refused ... ok
test both_document_openings_are_collected_and_a_fragment_is_not ... ok
test a_document_that_does_not_parse_names_its_page_position ... ok
test an_invalid_regex_is_refused ... ok
test the_static_page_manifest_has_its_documented_shape ... ok
test the_django_static_fragment_pins_its_documented_depth ... ok
test a_drifted_static_block_does_not_satisfy_the_documented_shape ... ok
test every_documented_document_parses_and_validates ... ok
...
packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)
doc-tone: ok — 28 user-facing page(s), no banned phrase
manifest-purity: ok — 17 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 17 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

161 `test result: ok` lines, 3190 passing tests, zero failures. Run twice: once on the suite alone,
then again on the final tree (after the one-line `LESSONS.md` append) — both exit 0, same counts.

## Files

| File | Change |
|---|---|
| `crates/blastit-manifest/tests/docs_fences.rs` | rewritten reader, count table, page-position failures, four self-tests |
| `docs/knowledge/LESSONS.md` | one line: rustfmt's `struct_lit_width` and a pinned const table (excluded from the site and from `doc-tone`) |

`git diff --numstat` for the suite: `341 75` (341 insertions, 75 deletions; `git diff --stat` reports
416 changed lines). One code file.

## Deviations

1. **Line budget.** 341 insertions / 75 deletions against the ≤ 350 "changed lines" guide. Read as
   insertions it is inside the guide; read as `git diff --stat`'s 416 it is 66 over. The leaf is a
   rewrite of a 272-line suite into 538 lines — the design's own §16 says "`docs_fences.rs`
   (rewritten)" — and the excess is the reader (`Fence`, `FenceError`, `collect_fences`), the count
   table, the `file:line:col` mapping and four self-tests. No other file changed, and nothing was
   dropped to fit.
2. **`Counts` is `[usize; 3]`, not a struct with named fields.** The budget above; the order is
   documented on the type alias and `label()` renders it by name in every message, so a drifted row
   still reads `pinned 0 `text title="Blastfile"` + 0 `blastfile` + 1 `toml title="blastit.toml"``.
3. **The table is pinned, not exhaustive.** §16.2's "scan every file under `docs/**` and fail on a
   document fence absent from `PAGES`" is `MB.21.5.2`'s strict flip; this leaf keeps the per-file
   count rule and proves it on fixtures. Recorded in Follow-ups.
4. **`an_invalid_regex_is_refused` stays TOML-shaped.** A `Blastfile` has path patterns, not regexes,
   so `ManifestError::InvalidRegex` cannot arise from a document; the canary keeps its TOML fixture
   until `MB.21.5.2` replaces it with a Blastfile-shaped refusal (see Follow-ups).

## Follow-ups

- **`MB.21.5.1` (per page it migrates):** change that page's row from `[0, 0, 1]` to `[1, 0, 0]` and
  keep `TOTAL` in sync (`[1, 0, 9]` today → one `text` more and one `toml` less per page). Add any
  page that gains a document; a page that drops one lowers its row. The pages must open documents
  with `text title="Blastfile"`, never `blastfile` (§16.1's "the site stays renderable" rule).
- **`MB.21.5.2` (the strict flip), exactly:**
  1. `const THE_RETIRED_OPENING_IS_GONE: bool = false;` → `true`.
  2. Delete the TOML column: `OPENING` from `COMPLETE_OPENINGS` and `parse_fence`'s branch, the third
     element of `Counts`, `PAGES` rows and `TOTAL`, and `label`'s third term — the retired opening
     then has no reader at all.
  3. Replace `an_invalid_regex_is_refused` with a Blastfile-shaped refusal (e.g. a `view` whose path
     pattern is refused by §8.3), since `InvalidRegex` is a TOML-only error.
  4. Add §16.2's exhaustive scan: every `*.md` under `docs/**` (excluding `docs/design`,
     `docs/research`, `docs/decisions`, `docs/knowledge`, `docs/testing`, which quote the erasure)
     plus `README.md`; a **document** fence on a page absent from `PAGES` fails, and an illustration
     outside `PAGES` is reported by path.
  5. Add the third class (statement lists: `blastfile::parse` on the body, no lower, no validate) and
     the illustration count — `docs/reference/blastfile.md` is measured at 14 + 3 in §16.2 and is
     absent from `PAGES` today.
- **`MB.21.5.1`/`MB.21.5.2`:** `docs/reference/blastfile.md` already exists and carries statement
  lists only; it is deliberately not in `PAGES` because this leaf reads documents, not fragments.
- No page carries a ```` ```blastfile ```` fence today, so the alias is proved by
  `both_document_openings_are_collected_and_a_fragment_is_not` and
  `a_document_that_does_not_parse_names_its_page_position` only. That is expected: the alias is
  transitional (§19.8) and no page should adopt it.
