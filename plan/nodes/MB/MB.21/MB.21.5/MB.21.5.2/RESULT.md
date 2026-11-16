# Result MB.21.5.2

## Summary

`docs_fences` is strict: one document opening (`text title="Blastfile"`), the
retired TOML column deleted, and a fence opened with either the retired spelling
or the transitional `blastfile` alias is **refused by name** so it can never be
counted as an illustration. The suite pins documents **and** illustrations per
page (`Counts` has two fields), pins the keyword-led statement lists beside them,
and scans `README.md` plus every `*.md` under `docs/**` except the five dated
record directories — so a document on an unpinned page fails and every
illustration-only page is reported by path.

The three self-test gaps `MB.21.3.6` left are closed by name:
`a_valid_document_fence_is_collected_parsed_and_counted`,
`a_pinned_page_that_lost_its_document_is_named`, and
`a_toml_fragment_passes_but_a_retired_complete_document_is_refused` (both
directions, driven by the now load-bearing `THE_RETIRED_OPENING_IS_GONE`).
`docs/frameworks/django.md`'s `static` fragment is parsed again
(`the_django_page_static_fragment_still_parses`).

`docs/blastfile.md`'s three console fences are live: `dev` against the `hello`
fixture, `check` against `shop`, and `presets`. `make docs-test` is **31
executed / 18 accounted / 0 failed** over 49 fences, and no accounting reason
mentions `MB.21.5.2` any more.

`test_the_real_tree_carries_no_retired_toml_manifest` is in
`python/tests/test_docs_assets.py` with **no allowlist**: it scans the front door
(`README.md`, `ARCHITECTURE.md`, `AGENTS.md`, `docs/VISION.md`), the published
pages via `fences.py:public_pages` **minus the one published record**
(`docs/changelog.md`), `crates/**/*.rs` + `Cargo.toml`, `python/**/*.py` +
`runnable_docs/*.toml`, `scripts/**`, `e2e/**`, `Makefile`, and every live brief.
A second test refuses `v1`/`gone:` in published pages, and the guard also refuses
a resurrected TOML dependency/lock entry and `docs/reference/manifest.md`. It
runs in `make docs-unit`, therefore in `make check`.

**`ARCHITECTURE.md` §6 does not join `PAGES` in this leaf.** Its fence is still
` ```blastfile ``, which the strict flip now refuses; changing it would be an
out-of-paths edit to `ARCHITECTURE.md`, and adding it to `PAGES` would need the
exhaustive scan to special-case a Markdown file outside `docs_dir`, weakening the
"PAGES = the published pages" partition assertion. Follow-up 1 names the exact
order of work.

## Evidence

All commands ran serially in
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` (branch
`release/0.2.0`, base `3452a7a`), on the final tree. `/proc/meminfo`:
`MemTotal 50331648 kB`, `MemAvailable 42137077 kB` — the host was not above the
high-watermark, so no case was environment-refused.

**1. `make docs-test` — the three language-page fences execute, nothing is
accounted-but-unexecuted on them.**

```console
$ timeout 900 make docs-test
PASS  docs/blastfile.md  fence 1  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/blastfile.md  fence 2  check                 0.8s  check reported a clean suite
PASS  docs/blastfile.md  fence 3  presets               0.0s  each preset exists and fails closed without capabilities
49 fences: 31 executed, 18 accounted, 0 failed · 27.3s
EXIT=0
```

Per-page executed totals: `docs/blastfile.md` **3 of 3** console fences executed
(it is the only language page with console fences; `docs/reference/blastfile.md`
carries statement lists and no `console` fence, so it has nothing to execute).
README.md 3/3, `docs/index.md` 1/1, `docs/getting-started.md` 3/3, `docs/local.md`
5/6 (the Docker recipe is the accounted `pgstub`), `docs/ci.md` 5/5,
`docs/frameworks/django.md` 1/1, `docs/frameworks/static-and-spa.md` 3/6 (three
`uvx`/Vite recipes are accounted against `e2e/static_assets.py`), and every other
executing page unchanged. The three inventory rows flipped from `static` to
`dev`/`check`/`presets`; no `console` fence was added, removed or reordered, so
the inventory kept its 49 entries.

**2. No reason says "the parser is missing".**

```console
$ grep -n '"static"' -A 2 python/tests/runnable_docs/inventory.toml | grep -c 'MB.21.5.2'
0
```

**3. Complete-manifest fences: one opener, the drift rule, the Django fragment,
and the three named self-tests.**

```console
$ timeout 300 cargo test -p blastit-manifest --locked --test docs_fences
running 11 tests
test a_document_on_an_unpinned_page_is_named ... ok
test a_page_whose_count_drifted_is_named ... ok
test a_plain_toml_manifest_on_a_pinned_page_is_refused ... ok
test a_pinned_page_that_lost_its_document_is_named ... ok
test a_toml_fragment_passes_but_a_retired_complete_document_is_refused ... ok
test a_document_that_does_not_parse_names_its_page_position ... ok
test an_unclosed_fence_is_refused ... ok
test an_invalid_regex_is_refused ... ok
test the_django_page_static_fragment_still_parses ... ok
test a_valid_document_fence_is_collected_parsed_and_counted ... ok
test every_documented_document_parses_and_validates ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Mutation proof that the per-file count rule still catches a drifted page — a
second document fence appended to `docs/frameworks/flask.md`:

```console
thread 'every_documented_document_parses_and_validates' panicked at ...docs_fences.rs:488:71:
docs/frameworks/flask.md: fence counts drifted: pinned 1 ````text title="Blastfile"` document(s) + 3 illustration(s); the page carries 2 ... + 3 ...
```

The file was restored (`git diff --stat docs/frameworks/flask.md` empty). The
fixture-level both-directions proof is in
`a_toml_fragment_passes_but_a_retired_complete_document_is_refused`: a plain
```` ```toml ```` fence is an illustration at any strictness, a complete retired
document passes as an illustration only with the flip off, and with it on the
failure is `FenceError::RetiredDocument` prefixed with the page name. The
exhaustive scan reports its illustration-only pages by path (`--nocapture`):

```console
unpinned: docs/ci.md (8 illustration(s))
unpinned: docs/reference/blastfile.md (3 illustration(s))
... 18 pages ...
```

**4. The flip: `THE_RETIRED_OPENING_IS_GONE` true, `COMPLETE_OPENINGS` one entry,
`Counts` two fields, no reader for the retired opening.**

```console
$ grep -n 'THE_RETIRED_OPENING_IS_GONE: bool = true\|COMPLETE_OPENINGS' crates/blastit-manifest/tests/docs_fences.rs
49:const COMPLETE_OPENINGS: [&str; 1] = [DOCUMENT_OPENING];
59:const THE_RETIRED_OPENING_IS_GONE: bool = true;
272:    if COMPLETE_OPENINGS.contains(&fence.opening.as_str()) {

$ grep -nE 'const RETIRED_OPENING|toml title="blastit' crates/blastit-manifest/tests/docs_fences.rs
exit=1        # no retired-opening constant and no contiguous retired literal
```

`Counts { documents, illustrations }` has two fields; the retired column, its
`parse_fence` branch and `retired_documents()` are gone. The only recognition of
the retired spelling is `is_retired_document()`/`retired_openings()`, which
compose the name from two literals and **refuse** the fence instead of parsing
it. `blastfile` is refused in the same predicate.

**5. The permanent guard: green on the real tree, red on a crafted one, no
allowlist.**

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
35 passed in 0.66s

$ ... -k 'retired_manifest or retired_format or retired_reader or word_rule' -v
======================= 8 passed, 27 deselected in 0.16s =======================
```

Those eight are the `v1`/`gone:` real-tree test, the anti-vacuity test and six
twins; the two name tests (the real tree and its mutation) were run separately
below. Across the eleven new tests: the two real-tree tests, the anti-vacuity
test, and eight negative/positive twins (published page fails; `docs/decisions/**`
record passes; the published changelog record passes while a product page still
fails; live brief fails; `status: done` brief passes;
crate/script/`Makefile`/`AGENTS.md` fail; `toml` dependency + lock entry +
`docs/reference/manifest.md` fail; `v1`/`gone:` fail; `apps/v1`, `:v1.26.1` and
`setup-uv@v10` pass).

Mutation proof — the retired name appended to a published page, then restored:

```console
--- mutated tree ---
E  AssertionError: docs/frameworks/php.md:112: 'blastit.toml': the v1 manifest format is retired and has no migration path: read `./Blastfile` or name one with `BLASTIT_MANIFEST`
FAILED python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_toml_manifest
--- restored tree ---
1 passed, 34 deselected in 0.21s
```

The other direction of the record rule (the review's MAJOR): a changelog entry
naming the retired file is **history**, not a claim, and does not fail:

```console
--- changelog mutated ---
.                                                                        [100%]
1 passed, 34 deselected in 0.41s
--- restored ---
git diff --stat docs/changelog.md   (empty)
```

`RECORD_PAGES = ("docs/changelog.md",)` filters the one published record out of
`published_page_files`, so the module docstring's "the changelog is outside the
scanned set" and the code now agree; `test_the_changelog_is_a_published_record`
pins both sides (the changelog passes, a product page in the same tree fails).
The name is composed (`RETIRED_MANIFEST_NAME = "blastit" + ".toml"`) so the
module scans itself without an exemption; `live_plan_briefs()` drops the PHP
guard's brief exemption and filters only on `done`/`superseded`. `ADR-0012`
still names the retired file twice and is not a published page.

**6. `make check` exit 0, and the guard is inside it.**

```console
$ timeout 1800 make check
...
49 fences: 31 executed, 18 accounted, 0 failed · 21.2s
...
manifest-purity self-test: ok — 3 fixtures proved, 16 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

Wiring: `Makefile:docs-unit` runs
`python/tests/test_docs_assets.py`, `make check` depends on `docs-unit`, so
`test_the_real_tree_carries_no_retired_toml_manifest` and
`test_no_published_page_names_the_retired_format` are gate steps. No new pytest
file, Makefile target or docs-test special case was added.

Other gates on the final tree:

```console
$ make docs-unit
107 passed, 65 subtests passed in 2.18s

$ make docs-check
Build started
No issues found
Build finished in 0.20s

$ cargo fmt --all -- --check          → fmt=0
$ cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.19s
```

## Files

| File | Change |
|---|---|
| `crates/blastit-manifest/tests/docs_fences.rs` | rewritten reader: one opener, two-field `Counts`, statement-list class, retired-spelling refusal, exhaustive `docs/**` scan, shape pins, Django fragment, 10 self-tests/reals |
| `python/tests/runnable_docs/inventory.toml` | `docs/blastfile.md` fences 1–3: `static` → `dev`/`hello`, `check`/`shop`, `presets` |
| `python/tests/test_docs_assets.py` | the retired-manifest guard, its anti-vacuity test and 9 negative/positive twins (including the changelog-record twin), plus the `v1`/`gone:` published-page rule |

No `Cargo.toml`/`Cargo.lock` edit, no new dependency, no `crates/` source change,
no `docs/**` change, no `plan/` status mutation.

## Follow-ups

1. **`ARCHITECTURE.md` §6 in `PAGES` — not done, and why.** The fence at
   `ARCHITECTURE.md:1042` still opens with ` ```blastfile ``, which this leaf's
   strict flip refuses everywhere, and `ARCHITECTURE.md` is outside `docs_dir`,
   so it is neither in `PAGES` nor in the exhaustive scan. Joining it needs, in
   this order: (a) the file's owner reopens §6 with ` ```text title="Blastfile" ``
   (out of this node's `paths`); (b) the scan grows a non-`docs_dir` spec-page
   set and `PAGES` gains the row with its pinned `Counts`, keeping the
   exhaustive `panic!` shape arm. Proposed node: `MB.21.5.3` (owner of
   `ARCHITECTURE.md` + `docs_fences.rs`).
2. **Statement lists are classified and counted, not parsed.** §16.1 wants
   `blastfile::parse` on each body with no lower/validate, but the parser is
   `pub(crate)` (`crates/blastit-manifest/src/blastfile/parser.rs`) and the
   crate exposes no parse-only entry point; `Manifest::from_blastfile_str`
   lowers, and the reference's Stores/connection/session fragments are refused
   by lowering (`DuplicateProxyName`, `GrantOnUnknownProxy`). `paths` exclude
   `crates/blastit-manifest/src/**`, so this cannot be added here. The counts are
   pinned (14 reference, 1 each on `docs/ci.md`, `docs/local.md`,
   `docs/frameworks/django.md`) and the Django `static` fragment **is** parsed
   through `from_blastfile_str`. Follow-up: add
   `Manifest::check_blastfile_syntax(&str)` or a `pub fn blastfile::parse` in the
   manifest crate, then call it per statement list.
3. **The exhaustive scan follows the brief's literal set** (every `docs/**/*.md`
   except `design|research|decisions|knowledge|testing`), so `docs/archive/`,
   `docs/spikes/`, `docs/VISION.md` and `docs/brand/*.md` are scanned as
   illustration-only pages. If one becomes a record that may quote the retired
   format, add it to `RECORD_DIRS` rather than to an allowlist.
4. **`v1` is refused with lookarounds, not as a bare word.** `apps/v1`,
   `networking.k8s.io/v1`, `pgbouncer:v1.26.1-p0` and `setup-uv@v10.3.2` are real
   page text, so `RETIRED_FORMAT_WORD` requires `v1` not preceded by
   `\w / : @ . -` and not followed by `\w . -`; the negative twin pins all four.
   A future page that legitimately writes `v1` as an API version with a space
   (`API v1`) would fail and must name the version differently.
5. **Not appended to `docs/knowledge/LESSONS.md`** (out of `paths`): the
   non-obvious fact is that `\bv1\b` is unsafe in this repo because Kubernetes
   paths and image tags trip it, and that a tag-boundary lookaround is the safe
   shape.
6. **Pre-existing, out of scope:** `docs-site-plan.md` (repository root) still
   names the deleted reference page and the retired format; the guard does not
   scan the repository root. Retired manifest syntax in Rust doc comments
   (`crates/blastit-ingress/src/lib.rs`, `crates/blastit-dbproxy/tests/postgres.rs`)
   from `MB.21.5.1` Follow-ups 5 is still open.

## Scope notes

- **`plan/nodes/ME/NODE.md` — out of `paths`, forced by this node's guard.**
  Commit `3452a7a` (on `MB.21.5.1`'s branch of work) re-spelled a live brief that
  still named the retired file, because the guard added here scans every live
  `plan/nodes/**/NODE.md`. `MB.21.5.1`'s `RESULT.md` journals the edit; this line
  records it from `MB.21.5.2` too, so the diff is auditable from either node. No
  other out-of-paths file was touched by this leaf.
- The worktree also carries uncommitted `plan/` changes for `MB.20` and
  `MB.21.5.1`; they belong to other in-flight nodes and were neither edited nor
  staged by this leaf.

## Review Resolution

- reviewer MAJOR → fixed → `python/tests/test_docs_assets.py:873-905` (with
  `test_the_changelog_is_a_published_record` at `:1098`). `published_page_files`
  now removes `RECORD_PAGES = ("docs/changelog.md",)` from the built set, so the
  code matches the module docstring and `NODE.md`; the twin shows a changelog
  entry naming the retired format passes while a product page still fails.
  Re-proof: the changelog mutation passes, the product-page mutation still fails
  (`docs/frameworks/php.md:112`).
- reviewer MINOR → fixed → `crates/blastit-manifest/tests/docs_fences.rs:296-338`
  (with `a_plain_toml_manifest_on_a_pinned_page_is_refused` at `:715`).
  `Scan` now counts plain `toml` fences and `verify_page` refuses any on a page in
  `PAGES`, naming the page and §16.2; the same fence outside `PAGES` is still an
  illustration (§16.1). This makes the §16.2 rule explicit instead of relying on
  count drift, and the new self-test proves a plain `toml` complete manifest on a
  pinned page is refused, not merely uncounted.
- reviewer MINOR → fixed → this file, `## Scope notes` above. The `ME/NODE.md`
  edit is recorded here as forced by the guard and journaled under `MB.21.5.1`.
- reviewer MINOR → fixed → `python/tests/test_docs_assets.py:1205`.
  `test_the_retired_manifest_guard_scans_a_real_tree` now pins
  `RECORD_PAGES == ("docs/changelog.md",)`, so the guard's only exemption cannot
  be widened silently: a later edit must change the assertion and the docstring
  together. Re-ran `pytest python/tests/test_docs_assets.py -q` (`35 passed`,
  exit 0) and `make docs-unit` (`107 passed, 65 subtests passed`, exit 0).
