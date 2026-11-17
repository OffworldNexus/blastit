# Result MB.31.2 — attempt 2

STATUS: done. `immutable` is lowered, validated and printed through the **one** pattern
compiler (`blastfile::pattern` + `routing::compile_pattern`), in the three shapes
`MB.31.1` documented. Nothing staged, nothing committed, no plan field edited.

## What landed (paths only)

| File | Change |
|---|---|
| `blastfile/parser.rs` | `immutable` takes an **optional** `pattern (',' pattern)*` list through the view endpoint's `_pattern` item parser (`pattern_list`), so each item is a `Pattern` node compiled at its own quote. Bare `immutable` is legal (newline, `;` or `}` ends it). |
| `blastfile/lower.rs` | `statics()` lowers each item's stored fragment through `routing::compile_pattern` into `model::StaticPattern`; bare → `StaticImmutable::All`; the list cap is checked at the 33rd pattern's node. |
| `model.rs` | `StaticPolicy.immutable: StaticImmutable` — `None` · `All` (bare) · `Patterns(Vec<StaticPattern>)`, plus `is_all`, `patterns`, `texts`, `matches(url_path)`. `StaticPattern` keeps the text and the **anchored matcher built at lower time**, so a request does one match per pattern and never compiles; equality is on text. |
| `blastfile/mod.rs` | `MAX_PATTERNS = 32` lives beside the other grammar budgets (one spelling of the cap); re-exports the typed pattern surface (see follow-up 1) and the compiler's two entry points. |
| `validate.rs` | the bespoke per-pattern rows are **gone** (`ANCHORED_PATTERN`, `AT_MOST_PATTERNS`, `AT_MOST_PATTERN_BYTES`, `MAX_PATTERN_BYTES = 256` and `check_static_immutable`). Nothing else replaced them: the type can only hold compiler-built patterns. |
| `error.rs` | `StaticImmutablePattern` deleted; `StaticImmutableTooManyPatterns { at }` added (positioned, so it carries the 33rd pattern's `line:col`). |
| `static_asset.rs` | `ImmutableGlob` is `EveryFile | Pattern(String)`; `open_with` maps `All` → `EveryFile` and `Patterns` → the entities it already built. **Bare `All` is live end to end.** |
| printer (`print.rs`) | no code change needed — the printer is CST-generic, so all four spellings already print canonically; proven by new round-trip/fixed-point legs. |
| `docs/design/static-and-spa.md` | `immutable` key table + `rule` list rewritten: no globs, no 256-byte ceiling; the row points at the `view` grammar, 4096 bytes per pattern, 32 per mount, and the dead `StaticImmutablePattern` row is gone. |

Unmodified although in `paths`: `blastfile/pattern.rs` (correct as written — the node says
do not refactor it), `blastfile/syntax.rs` (the `Pattern` node and its accessors already
existed), `blastfile/print.rs` (the printer is CST-generic, so no code change was needed),
and `static_response.rs` (`cache_control` reads `root.immutable.iter()` and each
`ImmutableGlob::matches`, which the new enum kept).

One refusal set, proven from both sides: `PatternNotAnchored`, `PatternStarPosition`,
`PatternParameterName/Type/Duplicate/Unclosed/Unnamed`, `PatternRegex`, `PatternTooLong`
at the pattern's own `line:col`, exactly as for a `view`; only the **list** cap is a
mount-level sentence. `regex` is still pre-compiled on its own and wrapped `^(?:…)$`, so
`x)|(.*` cannot become a catch-all. Retired rows (`..`, control byte) died with the node's
instruction: a pattern is not a filesystem path, so `/a/../*` and `/a\0b` are literal text
that no sanitized request path can contain (`400` before matching, unchanged, asserted by
the untouched scheduler resolve suite) — exactly the `view` rule.

## Protected tests — fixture migration only

`tools/critical_guard.py --base 30b864f --node MB.31.2` → `OK: all 5 protected change(s)
are authorized by node(s) MB.31.2`. No traversal, dotfile, hidden-file or `.php`-source
assertion was touched, no `#[ignore]`, nothing reordered away.

| File | Lines that changed | Why it is a fixture migration |
|---|---|---|
| `critical_manifest_edit_executor.rs` | import 25-27; struct literal 189-195 | `immutable: vec![…]` → `Patterns(vec![quoted(…), quoted(…)])` with the same two patterns in the same order. The `assert_eq!(written.mounts(), […])` comparison and everything else are byte-identical. |
| `critical_static_assets.rs` | import 27; line 622-624 | `front.immutable = vec![]` → typed field assignment, same one pattern. **Plus** a new test appended at 637-700 (`bare_immutable_caches_every_exact_file_and_no_index_answer`): new coverage for `All`, no existing assertion edited. |
| `critical_blastfile_parity.rs` | import 32; `mount()` helper 213-224 | the builder now compiles its `&[&str]` into `None`/`Patterns`; every row that uses it asserts the same lowered manifest as before. |
| `critical_blastfile_print.rs` | new test 618-679 | additions only: fmt fixed point + P3 for bare / one quoted / quoted+typed+regex. |
| `critical_static_manifest.rs` | imports 26-30; typed literals 155-175; `bad_pattern()` helper deleted; `"/assets/*/*"` → `"/assets/*"` (that spelling is now a star-position refusal); `immutable 1`'s expected class `a "…" pattern` → `a pattern`; the old 256-byte/32-row test replaced by three stronger ones (426-616) | every removed row asserted a **behaviour the node deletes** (a 256-byte ceiling, a `static immutable "x"` vocabulary, control-byte and `..` pattern rows). Each replacement asserts more: the shared sentences with positions, the 33rd-pattern refusal, and allowed/denied matching for bare, depth-crossing `/*`, literal dot, `{id:int}`, `{uuid}`, and `regex`. |

## Evidence (tails, attempt 2)

```console
$ cargo test -p blastit-manifest            # EXIT 0; 521 passed, 0 failed across 30 targets
test result: ok. 140 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.82s
   … all targets: 0 failed …
$ cargo test -p blastit-scheduler           # EXIT 0; 283 passed, 0 failed
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.58s
$ cargo test -p blastit-manifest --doc      # EXIT 0
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.29s; merged doctests compilation took 0.29s
$ cargo clippy -p blastit-manifest --all-targets -- -D warnings   # EXIT 0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.82s
$ cargo clippy -p blastit-scheduler --all-targets -- -D warnings  # EXIT 0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.29s
$ cargo fmt --all -- --check                # EXIT 0 (clean)
$ grep -rn 'StaticImmutablePattern\|256' crates/blastit-manifest/src/validate.rs crates/blastit-manifest/src/error.rs
(no output)
$ uv run --no-project tools/plan.py validate
plan OK
$ uv run --no-project tools/critical_guard.py --base 30b864f --node MB.31.2
critical-guard: OK: all 5 protected change(s) are authorized by node(s) MB.31.2
$ git diff --stat 30b864f -- Cargo.toml Cargo.lock
(empty: no dependency added, lock byte-identical)
$ make docs-test                            # EXIT 0
PASS  docs/security.md  fence 1  static  … statically checked every command, flag and snippet
48 fences: 30 executed, 18 accounted, 0 failed · 21.7s
$ make check                                # EXIT 0, 0 FAILED / 0 FAIL
… zed-check: OK (42 properties checked)
Ran 19 tests in 1.807s
OK
$ uv run --no-project tools/plan.py validate
plan OK
$ grep -cE "FAILED|^FAIL" <make check log>
0
```

New legs (allowed **and** denied for every shape, in `crates/blastit-manifest/tests/`):
bare `immutable` = `All` (matches `/`, `/a/b/c.js`) vs no directive = `None` (matches
nothing); `"/static/*"` matches `/static/admin/js/vendor/select2/i18n/de.js` and not
`/static`, `/staticfiles/app.js`, `/assets/app.js`; `"/v1.0/app.css"` vs `/v1x0/app.css`;
`"/orders/{id:int}/invoice.pdf"` vs `/orders/abc/…`; `{id:uuid}` vs `not-a-uuid`;
`regex '/static/.+\.[0-9a-f]{12}\.[^/]+'` vs the unhashed name; a two-item list in written
order; refusals for `"assets/*"`, `"/a*b"`, `"/a/*/b"`, bad `{…}`, `regex '('`, 4097 bytes
and the 33rd item, each with its sentence and `line:col`. A matcher that always (or
never) answered "immutable" fails all of them. `fmt` fixes each of the four spellings
byte-identically on the second pass and lowers to the same policy.

Regex cost, spelled out: an author-supplied `regex` is compiled **once at lower time**, so
a request pays no compile; the `regex` crate has no backtracking, no look-around and no
back-references, and the program is bounded by `REGEX_SIZE_LIMIT` (`1<<20`) and
`REGEX_NEST_LIMIT` (64), with the source capped at 4096 bytes. A request costs at most 32
linear matches over a path the scheduler already caps at 4096 bytes. No `unwrap`,
`expect`, `panic!` or unchecked indexing was added outside `#[cfg(test)]`.

## Sequencing: what is still MB.31.3 (stated plainly)

`crates/blastit-scheduler/src/static_asset.rs` still matches `Patterns` with the retired
`glob()`, whose `*` stops at `/` (`Some(b'/') | None => return false`). So **the cross-`/`
behaviour is not live in the scheduler in this commit**: a `"/static/*"` mount reaches one
segment there until MB.31.3 deletes `ImmutableGlob`/`glob()`/
`MAX_IMMUTABLE_PATTERN_BYTES` and calls `StaticImmutable::matches` (already implemented,
already tested here). Consequences of that interim, now all fail-closed (no year is pinned
where the manifest allows one): a nested file under `"/static/*"`, and one over the retired
256-byte guard, caches nothing yet. **Corrected in attempt 3:** the attempt-2 sentence here
claimed the interim was fail-closed for `regex '…'` too, and it was not — every `Patterns`
entry, quoted or regex, was handed to `glob()`, so `immutable regex '/static/*'` cached
`/static/app.js`, a path the manifest's own matcher denies. That is over-caching, the
opposite of fail-closed; `open_with` now skips a regex entry and
`critical_static_assets.rs`'s `a_regex_immutable_entry_is_never_read_as_a_glob` goes red if
it regresses. Bare `immutable`
**is** live now (`ImmutableGlob::EveryFile`), and an `index`/`fallback` answer stays
`no-cache` under it (`S3`), proven in `critical_static_assets.rs`.

## Follow-ups

1. `crates/blastit-manifest/src/lib.rs` is not in this node's `paths`, so the two new
   public types are reachable as `blastit_manifest::blastfile::{StaticImmutable,
   StaticPattern}` (re-exported from the module that lowers them) rather than from the
   crate root. Move the `pub use` to `lib.rs` when a leaf owns that file.
2. `crates/blastit/src/lsp/hover.rs:120-124` still tells the reader `*` stops at `/` and a
   nested tree needs one glob per depth — MB.31.4 owns it.
3. `docs/design/static-and-spa.md` §5.2's glob vectors and §4.5's historical rows are kept
   as the retired record, marked superseded by MB.31.1; MB.31.3 should re-point them at
   `StaticImmutable::matches`.
4. No `LESSONS.md` line: the only non-obvious bit is a plan-scope fact, recorded in
   attempt 1 below.

## Attempt 1 (blocked) — record

The first attempt stopped because `paths` omitted every struct literal that names
`immutable` (`routing.rs`, `uid.rs`, `critical_manifest_edit_executor.rs`,
`static_fixture.rs`, `critical_static_assets.rs`) and `docs/design/static-and-spa.md`;
the field-type change cannot compile without them (rustc `E0308`: expected
`StaticImmutable`, found `Vec<_>`). Attempt 2's `paths` include them, so the change landed
as planned, with `StaticImmutable::{None, All, Patterns}` and no `Deref` shim needed
because `static_asset.rs` is in lane this time.

## Attempt 3 — review fixes

Three findings, exactly as reviewed. Nothing else was touched: the model/parser/lower/print
work of attempt 2 is unchanged apart from the spelling field.

**FIX 1 [MAJOR — over-caching]** `crates/blastit-scheduler/src/static_asset.rs:357`.
Every `Patterns` entry, quoted **or** `regex`, was handed to the retired `glob()`, so a
regex's *text* was re-read as a glob: `immutable regex '/static/*'` (which the manifest
compiles as "`/static` then zero or more `/`") made `glob` accept `/static/app.js`, and
`cache_control` emitted `public, max-age=31536000, immutable` for a path the manifest's own
matcher denies. Now `model::StaticPattern` records `Spelling::{Quoted, Regex}` (set by
`lower`, `quoted` and `regex`; exposed as `is_regex()`), and `open_with` builds an
`ImmutableGlob` **only from quoted entries** — a `regex` entry caches nothing until MB.31.3
calls `StaticImmutable::matches`, which honours it. Red proof, with the `.filter(...)` line
removed again:

```console
$ cargo test -p blastit-scheduler --test critical_static_assets a_regex_immutable
thread 'a_regex_immutable_entry_is_never_read_as_a_glob' panicked at …/critical_static_assets.rs:725
assertion `left == right` failed: a regex entry may never year-cache a path the manifest's matcher denies
  left: Some([112, 117, 98, …])   # "public, max-age=31536000, immutable"
 right: Some([110, 111, 45, 99, 97, 99, 104, 101])   # "no-cache"
test result: FAILED. 0 passed; 1 failed
```

Restored, the same file is `16 passed; 0 failed`. Regresses to red:
`critical_static_assets.rs::a_regex_immutable_entry_is_never_read_as_a_glob` (scheduler) and
`critical_static_manifest.rs::an_immutable_pattern_records_its_spelling` (manifest, proves
`is_regex()` is recorded and that the regex denies `/b/app.js` while matching `/b`). The
attempt-2 "fail-closed" sentence in the Sequencing section above is corrected in place.

**FIX 2 [MAJOR — design doc]** `docs/design/static-and-spa.md:257`. The present-tense claim
that `blastit-scheduler` "never compiles a pattern and never keeps a second matcher" is
replaced by an explicit **Interim (`MB.31.2`, replaced by `MB.31.3`)** paragraph: the
scheduler still holds `ImmutableGlob` and matches with the retired stop-at-`/` `glob()`, so
only quoted patterns are honoured there, `*` stops at `/`, and a `regex '…'` entry (or one
over the retired 256-byte guard) caches nothing until MB.31.3. Regresses to red: no test —
it is prose; the review finding named the line, and the text now says what the code does.

**FIX 3 [MINOR — honest cap]** `crates/blastit-scheduler/src/static_asset.rs:56,188`.
`MAX_IMMUTABLE_PATTERN_BYTES` is re-documented as "the **retired** `immutable`
pattern-length guard" that MB.31.3 deletes, explicitly **not** what validation accepts (the
compiler's 4096), and noting that a legal 300-byte quoted pattern is dropped by it — no
year pinned, the safe direction. `ImmutableGlob`'s own doc says the same and states that
only quoted entries become a `Pattern`. Regresses to red: no test (prose); the constant's
behaviour is unchanged and still exercised by the untouched
`critical_static_resolve.rs::immutable_globs_match_the_design_vectors`.

### Evidence (tails, attempt 3)

```console
$ cargo test -p blastit-scheduler            # EXIT 0; 284 passed, 0 failed
$ cargo test -p blastit-manifest             # EXIT 0; 522 passed, 0 failed
$ cargo clippy -p blastit-scheduler -p blastit-manifest --all-targets -- -D warnings  # EXIT 0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.87s
$ cargo fmt --all -- --check                 # EXIT 0 (clean)
$ cargo build -p blastit --locked            # EXIT 0
$ scripts/check-no-inotify.sh target/debug/blastit   # EXIT 0
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files
$ make check                                 # EXIT 0; grep -cE "FAILED|^FAIL" = 0
Ran 19 tests in 2.858s
OK
$ uv run --no-project tools/plan.py validate
plan OK
$ make docs-test                             # EXIT 0
48 fences: 30 executed, 18 accounted, 0 failed · 40.6s
$ uv run --no-project tools/critical_guard.py --base 30b864f --node MB.31.2
critical-guard: OK: all 5 protected change(s) are authorized by node(s) MB.31.2
$ git diff --stat 30b864f -- Cargo.toml Cargo.lock
(empty)
```

Protected files in this attempt: additions only (`critical_static_assets.rs` gained
`a_regex_immutable_entry_is_never_read_as_a_glob`, `critical_static_manifest.rs` gained
`an_immutable_pattern_records_its_spelling`). No assertion weakened, deleted or `#[ignore]`d.

### Not mine — remaining steps (flagged, not edited)

1. `docs/reference/blastfile.md:406` and `docs/frameworks/python/django.md:63` still say a
   trailing `/*` "crosses `/`", which is only true after MB.31.3: at HEAD the scheduler's
   retired glob stops at `/`, so a nested file under `"/static/*"` gets `no-cache` in the
   served tree even though the manifest's matcher says yes. MB.31.3 makes the sentence true;
   until then the docs over-promise a cache, never a security property.
2. `python/tests/runnable_docs/harness.py:1136` has a SIGINT shutdown race that made the
   reviewer's first `make check` exit 2 on `docs/local.md` fence 5. It did not trip this
   attempt's `make check` (EXIT 0). It is a gate-harness race, not this node's code; the
   fence must never be reordered or skipped to hide it.
