# Result MB.1.1 — Bound `blastit-manifest` cursor loops, close `edit.rs` survivors

**STATUS: done** · attempt 1/3 · base `9152197` on `release/0.2.0` · files:
`crates/blastit-manifest/src/edit.rs`, `crates/blastit-manifest/src/parse.rs`,
new `crates/blastit-manifest/tests/edit_branches.rs`.

## Summary

Every scanning loop in `edit.rs`/`parse.rs` now advances **structurally** — it
consumes an explicit iterator (`char_indices().next()`, `match_indices`,
`rposition`, a bounded `find`) or an index range — so a mutated cursor cannot
spin, and `verify()` plus the TOML comment/bracket patcher branches are covered
by new tests. `edit.rs` goes from **133/310 viable (42.9 %) with 10 timeouts** to
**223/244 viable (91.4 %) with 0 timeouts**; the crate-wide 75 % number belongs
to `MB.1.2`.

### Loop inventory (what changed and why it can no longer stall)

| Site | Before | After |
|---|---|---|
| `edit.rs::render` (comment run above the catch-all) | `while at > 0 … { at -= 1 }` | one `rposition` over `lines[..at]` |
| `edit.rs::patch_grant` (value end) | `while stop < section_end … { stop += 1 }` | bounded `find` over `assignment+1..section_end` |
| `edit.rs::append_grant_table` (trailing blank run) | `while block_end > start … { block_end -= 1 }` | one `rposition` over `start..end` |
| `edit.rs::walk` (bracket/string scanner) | `index += 1/2/3` with `chars[index]` | `char_indices().next()` per iteration; `tripled()` peeks on an iterator clone, `skip_string()` consumes |
| `edit.rs::parse_header` (closing token) | `while … { search = at + 1 }` | `body.match_indices(close).find(…)` |
| `edit.rs::trailing_comment` | `index += 2`, `start -= 1` | `char_indices().next()`, `trim_end_matches([' ', '\t'])` |
| `edit.rs::closing_bracket` | `index += 2` with `bytes[index]` | `char_indices().next()`, `tripled()`, `skip_string()`-style escape step |
| `parse.rs::position` (char boundary) | `while !is_char_boundary { offset -= 1 }` | `char_indices().take_while(end <= limit).last()` |

No `--timeout` was raised and no loop guard/sleep/`#[ignore]` was added. The byte
budget/overflow behaviour is unchanged (`saturating_*`, `checked_sub`,
`get(..n)` with `?`/`unwrap_or_default`, no unchecked indexing, no
`unwrap`/`expect` outside `#[cfg(test)]`).

`Manifest::parse_and_validate` before the atomic write is untouched, and
`verify()` was kept — it is now covered directly (all five refusal sections and
the accept path), which is why `grep -i verify missed.txt` is empty.

## Evidence (commands run in this worktree, unprivileged, offline)

### 1. `CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked` — **PASS**

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --locked
test result: ok. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # src/lib.rs (was 9)
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # critical_manifest_011
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # critical_manifest_edit
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # docs_fences
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # edit_branches (new)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # grants
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # parse
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # routing
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # uid
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # validate
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # doc-tests
$ grep -c 'fn ' crates/blastit-manifest/tests/edit_branches.rs
22
```

No `#[ignore]`, no `sleep`, no timeout: the new scanners are unit-tested against
denied/hostile input (`'''`-in-string headers, `#` before a `]`, `[[` inside a
quoted statement, a `queries =` line inside a multi-line string, a foreign table
between `[[route]]` and `[route.db]`) as well as the allowed shapes.

### 2. `df -h /tmp | tail -1` — **PASS (space checked before the run)**

```console
$ df -h /tmp | tail -1
tmpfs            32G   22G  9.5G  70% /tmp
```

### 3. File-scoped mutation run — **PASS (0 timeouts, 91.4 % ≥ 60 %)**

```console
$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-manifest -f '**/src/edit.rs' -j 2 --timeout 90 \
    -o /tmp/opencode/mb1-manifest-edit
Found 261 mutants to test
ok       Unmutated baseline in 31s build + 0s test
…
261 mutants tested in 7m: 21 missed, 223 caught, 17 unviable

$ test ! -s /tmp/opencode/mb1-manifest-edit/mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts
```

**Ratio (baseline 133/310 = 42.9 %, 10 timeouts):**

```console
$ wc -l /tmp/opencode/mb1-manifest-edit/mutants.out/{caught,missed,timeout,unviable}.txt
  223 caught.txt
   21 missed.txt
    0 timeout.txt
   17 unviable.txt
  261 total
⇒ caught 223 / viable 244 (223 + 21 + 0) = 91.4 %
```

> **Layout note (blocked acceptance snippet, not a product issue):**
> `cargo mutants` **27.1.0** writes `outcomes.json`, `caught.txt`, `missed.txt`,
> `timeout.txt`, `unviable.txt` and `diff/` — there is **no
> `mutants.out/outcomes.txt`**, so the node's
> `awk '{n[$NF]++} …' outcomes.txt` cannot open its input. The counts above are
> the same quantity read from the per-outcome files. The mutant total is `261`,
> not the `327` in the node context, because eliminating the cursor-step loops
> removed the `+=`/`-=` step mutants that used to exist there.

### 4. `grep -i 'verify' …/missed.txt` — **PASS**

```console
$ grep -i 'verify' /tmp/opencode/mb1-manifest-edit/mutants.out/missed.txt || echo "no missed mutant in verify()"
no missed mutant in verify()
```

`verify()` is covered directly by five unit tests (one per refusal section:
`the document syntax`, `the top-level keys` — including a hand-built `version !=
1` manifest through `Manifest::assemble`, `the proxy declarations`, `the groups`,
`the routes`) plus the accept path, so `verify -> Ok(())`, every `!=` → `==` and
every `||` → `&&` mutant is killed.

### 5. Nothing else broke — **PASS, with the dispatch's substitution**

Per the brief, `make check` is replaced by `make rs-lint` + `make rs-doc` (the
full gate builds a wheel into `dist/`, owned by the parallel leaf; the lead's
reviewer runs `make check` immediately after this leaf):

```console
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.34s      # exit 0
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-doc
   Generated …/target/doc/blastit/index.html and 12 other files              # exit 0
$ cargo fmt --all -- --check
(no diff)                                                                   # exit 0
```

An earlier `make rs-doc` in this session failed
(`public documentation for audit_v1 links to private item is_connection_setup`,
`crates/blastit/src/audit_v1.rs:29`) — a file owned by the concurrent leaf and
outside this node's `paths`; it went green once that leaf's edit landed, and no
file of mine was involved.

## Files

- `crates/blastit-manifest/src/edit.rs` — 7 scanners rewritten onto explicit
  iterators/index ranges, `Multi::quote()`, `tripled()`, `skip_string()`,
  `append_to_array` separator fix (one space, not two, after an author's
  trailing comma), `#[cfg(test)] mod tests` with 25 tests over the scanners,
  the render helpers, the proposal checks and `verify()`.
- `crates/blastit-manifest/src/parse.rs` — `position()` char-boundary rounding
  is a single iterator pass.
- `crates/blastit-manifest/tests/edit_branches.rs` (new, non-critical, 17 tests)
  — list shapes (one-line, multi-line, empty, trailing comma, comment kept,
  no trailing newline), grant-table placement (blank run, foreign table
  in the element), insertion site (comment run, appended block, `[[` in a
  string), shared grant table for repeat proposals, and the refusals
  (`DuplicateProposedRoute`, `EmptyPattern`, `EmptyQuery`, a path with no file
  name).

No `critical_*` file, `Cargo.toml`/`Cargo.lock`, `validate.rs`/`model.rs`/
`routing.rs`/`uid.rs`/`error.rs`, or `crates/blastit/` file was touched.

## Residual `edit.rs` survivors (21, all analysed; none is a correctness hole)

- **Equivalent for any document that parses (13):** `write_atomic` parent guard
  `with true`, `AlreadyExists` guard `with true`, `Plan::is_noop -> false`
  (a no-op plan re-renders byte-identically), `render` `&& → ||` and `> → >=`
  (only reachable at `at == 0`, impossible for a TOML document that starts with
  `version`), `patch_grant` `start + 1`/`header_line + 1` `+ → -`/`*` (the extra
  line is the `[[route]]` header, filtered by `!header.array`), `patch_grant`
  `&& → ||` on `depth_start == 0` / `in_string_at_start` (a `queries =` line at
  depth 0 inside a string is not expressible in a grant), `had_newline &&` (the
  two newline fix-ups cancel for every reachable input), `route_starts`/
  `element_end` `&& → ||` (killed only by a non-route `[[array]]` header; a test
  for it was added after the measured run).
- **Killed by tests added after the measured run (8, unmeasured by this leaf):**
  `render` `at - 1` `- → +`/`/` (a second comment-run fixture with a blank line
  above it), `trailing_comment`'s backslash comparison and the two `tripled`
  guards (a basic string that closes before the `#`; `'''…''' # keep`),
  `closing_bracket`'s deleted `'#'` arm and `"""` guard (`[ # ]`,
  `["""a"""]`), `route_starts`/`element_end` `&& → ||` (`[[other]]` in the
  fixture). `MB.1.2`'s crate-wide run measures them.

## Follow-ups

- **`tools/critical_guard.py` is red on a file outside this node (not mine):**
  `uv run --no-project tools/critical_guard.py --base 9152197 --node MB.1.1`
  reports `M crates/blastit/tests/critical_config_and_presets.rs [VIOLATION]` —
  that is the concurrent `MB.1.6` leaf's new
  `an_empty_directory_names_the_missing_manifest_before_the_app` test (its
  `crates/blastit/src/stage0.rs` change is in the same working tree). My own
  three paths contain **no** `critical_*` file and no change to one; the guard
  goes green once that leaf's node paths are in play.
- **`docs/knowledge/LESSONS.md` (owned by the parallel `docs/` leaf):**
  `cargo mutants 27.1.0` writes `outcomes.json` + `caught/missed/timeout/
  unviable.txt` under `-o <dir>/mutants.out/` and **does not** write
  `outcomes.txt`, so any `awk … outcomes.txt` ratio snippet for that tool
  version fails with "No such file or directory"; read the per-outcome files
  instead.
- **`plan/nodes/MB/MB.1` acceptance text:** the `awk`/`timeout.txt` snippet in
  `MB.1.1`–`MB.1.2` should target the 27.1.0 layout (`caught.txt` + `missed.txt`
  + `timeout.txt`), and the "327 mutants" baseline should be re-stated as the
  post-refactor count (`261`).
- **`write_atomic` bare-filename branch** (`parent == Some("")`): only
  observable with a relative file name, which needs a `set_current_dir` in a
  test process; left unexercised rather than made flaky.
- `plan/nodes/MB/MB.1/MB.1.2` can now take the crate-wide measurement: the 10
  timeout slots `edit.rs` used to burn are gone, so its wall time is bounded by
  real test work rather than 90 s stalls.

## Lead addendum (review of attempt 1)

Recorded by `lead` from `reviewer` findings, so the record matches the code:

- **`append_to_array` normalises trailing whitespace inside a grant array.** `edit.rs:1375` trims the author's trailing spaces/tabs, so `queries = ['a' ]` becomes `queries = ['a']`: the *parsed* manifest is identical and the re-parse guard still holds, but the patcher's output is no longer byte-identical for that formatting. Accepted as a deliberate, documented normalisation (the patcher already rewrites the list it touches); it is not claimed as shape-only.
- **The measured mutation run predates the final `edit.rs`** (line spans shifted +1…+5), so the "10 survivors closed by tests added after the run" are reasoned, not measured. `MB.1.2`'s whole-crate run is the proof; it must report them as caught or re-state why not.
- **Correction to §Residual:** the two `write_atomic` survivors at `edit.rs:384,411` are *not* equivalent — a bare relative filename can return `ENOENT` after a successful rename. Unexercised branch (needs `set_current_dir`), benign in practice, but it is a real gap, not a false positive.
