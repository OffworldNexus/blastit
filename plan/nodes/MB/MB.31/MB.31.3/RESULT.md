# Result MB.31.3 — attempt 1

STATUS: done. The second matcher is gone. `StaticRoot` now holds the manifest's own
`StaticImmutable` and `static_response` calls `StaticImmutable::matches`, so admission and the
cache header cannot disagree, a `"/assets/*"` crosses `/` at any depth, a `regex '…'` entry caches
exactly what it compiles to, and bare `immutable` year-caches only exact-file answers (S3 kept).
Nothing staged, nothing committed, no plan field edited, no `Cargo.toml`/`Cargo.lock` byte changed.

## What changed (paths only)

| File | Change |
|---|---|
| `crates/blastit-scheduler/src/static_asset.rs` | **Deleted** `ImmutableGlob` (enum + `new`/`matches`), `glob()` and `MAX_IMMUTABLE_PATTERN_BYTES`. `StaticRoot.immutable` is now `StaticImmutable`, cloned from the policy in `open_with`; nothing is compiled per request (the anchored programs were built at lower time), and no second syntax exists to disagree. |
| `crates/blastit-scheduler/src/static_response.rs` | `cache_control` is `file.kind == AnswerKind::Exact && root.immutable.matches(&file.url_path)`. Header bytes and `IMMUTABLE_CACHE`/`REVALIDATE_CACHE`/`NO_STORE_CACHE` are untouched. |
| `crates/blastit-manifest/src/model.rs` | `PartialEq for StaticPattern` compares **`text` AND `spelling`**; stale "retired glob / MB.31.3" prose on `Spelling`, `StaticPattern::is_regex` and `StaticMount::policy` replaced with the final truth. New in-file test `a_pattern_spelling_flip_is_not_the_same_manifest`. |
| `crates/blastit-scheduler/tests/critical_static_resolve.rs` | The retired-glob test replaced by `immutable_patterns_match_the_one_compiler` (both directions for quoted, `{file}`, literal text, regex, the three policy shapes, and the compiler's refusals). |
| `crates/blastit-scheduler/tests/critical_static_assets.rs` | Four new legs + a `/dashboard/settings` assertion; the regex test's doc comment corrected. No existing assertion edited or removed. |
| `crates/blastit-scheduler/tests/common/static_fixture.rs` | **Unchanged** — fixture S already declares one `"/assets/*"`, which is now the depth-crossing pattern. |

### The one deliberate behaviour change

A pattern that previously stopped at `/` now crosses it: `"/assets/*"` reaches
`/assets/chunks/vendor/x.js`, and `"/assets/*/*"` is now a **refusal** (a `*` must end the pattern
after `/`), so the old per-depth spellings are artefacts and were rewritten as the one-pattern row.

## Protected tests — fixture migration + additions only

`uv run --no-project tools/critical_guard.py --base 942cb16 --node MB.31.3` →
`OK: all 2 protected change(s) are authorized by node(s) MB.31.3`.

| File | Lines that changed | Why it is a migration |
|---|---|---|
| `critical_static_resolve.rs` | import 34 (`ImmutableGlob` → `blastit_manifest::blastfile::{StaticImmutable, StaticPattern}`); 750-781 → `immutable_patterns_match_the_one_compiler` at 759 | The old rows were the deleted API's own unit table (`ImmutableGlob::new`, `/assets/*/*` per-depth globs). Every row is replaced by an equal-or-stronger one on the manifest matcher: cross-`/` at depth, prefix-itself denial, `assets-secret` denial, `{file}` second-segment denial, `.map` both ways, the three policy shapes, the compiler's refusals, and the `..`/control guarantees moved to `segments` (the layer that actually refuses them). |
| `critical_static_assets.rs` | 708-717 (doc comment of `a_regex_immutable_entry_is_never_read_as_a_glob`, 7 lines → 8, **no assertion touched**); 698-707 (added `/dashboard/settings`); 749-963 (four new tests) | Additions only: `one_quoted_pattern_crosses_slash_at_any_depth` (756), `a_regex_entry_caches_exactly_what_it_matches` (824), `a_pattern_list_is_an_or_in_either_written_order` (897), `a_mount_prefix_is_restored_before_the_pattern_is_matched` (938). |

No traversal, NUL, dotfile, hidden-file, symlink, short-read, `.php`-source or status-table
assertion was weakened, deleted, reordered away or `#[ignore]`d.

## Evidence — the node's `# Acceptance`, in full

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK

$ cargo test -p blastit-scheduler
scheduler: passed=288 failed=0        # was 284 at 942cb16; 4 new tests
   … test result: ok. 20 passed; 0 failed …   (critical_static_assets)
   … test result: ok. 18 passed; 0 failed …   (critical_static_resolve)

$ cargo test -p blastit-scheduler --test critical_static_assets \
      --test critical_static_resolve --test critical_rewrite
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (critical_rewrite)
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (critical_static_assets)
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (critical_static_resolve)

$ cargo clippy -p blastit-scheduler --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.97s     # EXIT 0

$ grep -rn 'ImmutableGlob\|fn glob(\|MAX_IMMUTABLE_PATTERN_BYTES' crates/blastit-scheduler
(no output; grep exit 1 — one matcher in the tree)

$ cargo test -p blastit-manifest
manifest: passed=523 failed=0

$ cargo clippy -p blastit-manifest --all-targets -- -D warnings
    Finished `dev` profile …                                                # EXIT 0

$ cargo fmt --all -- --check
(clean, EXIT 0)

$ cargo build -p blastit --locked && scripts/check-no-inotify.sh target/debug/blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.84s
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)

$ make check
MAKE CHECK EXIT=0
$ grep -cE 'FAILED|^FAIL' <make check log>
0
48 fences: 30 executed, 18 accounted, 0 failed · 20.3s
Ran 19 tests in 2.581s
plan OK

$ make docs-test
DOCS-TEST EXIT=0
48 fences: 30 executed, 18 accounted, 0 failed · 37.0s

$ uv run --no-project tools/critical_guard.py --base 942cb16 --node MB.31.3
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.31.3

$ git diff --stat 942cb16 -- Cargo.toml Cargo.lock
(empty: no dependency added, lock byte-identical)
```

## Red proof — both directions, at the one chokepoint

Mutation A (`cache_control` → `if file.kind == AnswerKind::Exact`, i.e. "always immutable"):

```console
test a_regex_immutable_entry_is_never_read_as_a_glob ... FAILED
test a_mount_prefix_is_restored_before_the_pattern_is_matched ... FAILED
test a_pattern_list_is_an_or_in_either_written_order ... FAILED
test a_regex_entry_caches_exactly_what_it_matches ... FAILED
test every_response_table_row_answers_the_design ... FAILED
test one_quoted_pattern_crosses_slash_at_any_depth ... FAILED
test the_cache_header_is_keyed_on_the_answering_file ... FAILED
test result: FAILED. 13 passed; 7 failed
  "a path `/assets/*` does not name stays revalidating: /docs/index.html"
```

Mutation B (`cache_control` → `if false`, i.e. "never immutable"):

```console
test a_mount_prefix_is_restored_before_the_pattern_is_matched ... FAILED
test bare_immutable_caches_every_exact_file_and_no_index_answer ... FAILED
test a_pattern_list_is_an_or_in_either_written_order ... FAILED
test every_response_table_row_answers_the_design ... FAILED
test a_regex_entry_caches_exactly_what_it_matches ... FAILED
test if_none_match_answers_304_and_repeats_the_head ... FAILED
test the_cache_header_is_keyed_on_the_answering_file ... FAILED
test one_quoted_pattern_crosses_slash_at_any_depth ... FAILED
test the_etag_is_exact_stable_and_moves_with_the_mtime ... FAILED
test result: FAILED. 11 passed; 9 failed
  "`/assets/*` matches the restored client path"
  "every exact file is cached under bare `immutable`: /assets/app.4f1c2a9b7d3e.js"
```

Mutation C (`StaticPattern::matches` → `true`) and D (`→ false`), on the matcher table:

```console
thread 'immutable_patterns_match_the_one_compiler' panicked at …:817:
assertion `left == right` failed: "/assets/*" vs "/assets"                     # C
assertion `left == right` failed: "/assets/*" vs "/assets/app.4f1c2a9b7d3e.js" # D
test result: FAILED. 0 passed; 1 failed
```

Both restored; `cargo test -p blastit-scheduler -p blastit-manifest` → `passed=811 failed=0`.

What the new legs assert, both ways: `/assets/app.js` **and** `/assets/chunks/vendor/x.js` are
`200` + `public, max-age=31536000, immutable` from one `immutable "/assets/*"`, while
`/docs/index.html`, `/.well-known/…` stay `no-cache` and `/assets` itself is a `404` (a directory,
never a cached file); a `regex '…'` entry caches the hashed name and denies the unhashed one and
the two-segment path; bare `immutable` caches exact files while `/` and `/dashboard/settings`
(fallback) stay `no-cache`; a two-pattern list is an OR in either written order; `/assets/*` on a
`/assets`-prefixed mount matches the restored client path and a pattern written for the stripped
remainder matches nothing; `..` (400), `.env` (404) and `index.php` (404) under the very prefix the
pattern names stay refused.

Regex cost: unchanged from MB.31.2 — the program is compiled once at lower time, the `regex` crate
has no backtracking/look-around/back-references, `REGEX_SIZE_LIMIT` (`1<<20`) and
`REGEX_NEST_LIMIT` (64) bound the program, the source is capped at 4096 bytes, and a request does
at most 32 linear matches over a path the scheduler caps at 4096 bytes. No `unwrap`/`expect`/
`panic!`/unchecked indexing added outside `#[cfg(test)]`.

## Findings / follow-ups (not edited: outside `paths`)

1. **`docs/design/static-and-spa.md:261-268` is now stale.** Its
   **"Interim (`MB.31.2`, replaced by `MB.31.3`)"** paragraph still says the scheduler holds
   `ImmutableGlob`, matches with the retired stop-at-`/` `glob()`, skips `regex` entries and drops
   patterns over 256 bytes. All four statements are false in this commit. The file is not in this
   node's `paths`; the leaf that owns it (MB.31.4 or the gate) must replace the paragraph with the
   final truth: one matcher, `StaticImmutable::matches`, called from `cache_control`, no
   compilation per request, `*/` crossing at any depth, `regex` honoured, cap 4096.
2. **The node's `.map` expectation is not what the documented expression does.** Measured (both
   the manifest matcher and `re`): `regex '/static/.+\.[0-9a-f]{12}\.[^/]+'` **does** match
   `/static/js/app.0123456789ab.js.map`, because `[^/]+` admits a dot and the greedy `.+` absorbs
   the path. The tests therefore assert the true behaviour for the documented expression and add a
   `[^/.]+` row that genuinely denies `.map` ("one hash segment, nothing after it"). The epic's
   live transcript should not claim `.map` denial for the documented expression, or
   `docs/reference/blastfile.md:407` / `docs/frameworks/python/django.md` should spell the stricter
   `[^/.]+`. Those docs do not claim it today, so no doc is wrong — only the plan text.
3. `docs/design/static-and-spa.md` §5.2's glob vectors and §4.5's historical rows (MB.31.2
   follow-up 3) still describe the retired matcher — same file, same owner as (1).
4. `crates/blastit/src/lsp/hover.rs:120-124` still teaches the stop-at-`/` rule — MB.31.4 owns it.
5. A `LESSONS.md` line for the `[^/]+`/`.map` greed would be useful; `docs/knowledge/` is not in
   this node's `paths`, so it is recorded here instead.
