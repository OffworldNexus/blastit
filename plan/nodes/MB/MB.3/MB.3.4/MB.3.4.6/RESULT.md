# Summary

Oracle-only node: the two branch-inspection residuals from
`docs/testing/AUDIT-MB.3.4-gate.md` ("Branch-inspection residuals") now have
protected assertions. **No `src/` change** — both behaviours were already
correct; the diff is `crates/blastit-fastcgi/tests/**` only (`+25/−1`,
one fixture line, no assertion removed, weakened or reordered).

1. `critical_fastcgi_codec.rs::head_total_cap_is_exactly_64_kib` — added
   `assert_eq!(head_end(&at_cap), Some(at_cap.len()))` on the existing
   exactly-`MAX_HEAD_BYTES` buffer and `assert_eq!(head_end(&over_cap), None)`
   on the one-byte-over twin, so `cgi::head_end`'s
   `(end <= MAX_HEAD_BYTES).then_some(end)` (§4.5.2 "≤ 64 KiB total") is now
   pinned at the *equal-to-cap* boundary, which cargo-mutants never generates a
   mutant for. Every pre-existing `parse_head` assertion is kept, and the
   at-cap `head_end` assertion sits before them.
2. `critical_fastcgi_lockdown.rs::unrepresentable_header_name_is_dropped` — the
   fixture grew from 3 to 5 rows: `(b"1x", b"4")` (a legal RFC 7230 token that
   still starts with a digit after folding, §4.6.2) and the builder-API-only
   `(b"", b"4")` (the `None` arm of `is_representable_header`). `dropped()`
   now pins `HTTP_1X` and `HTTP_` in arrival order, `params.len() == 16` still
   holds (the fixed set only), and both names are asserted absent from
   `pairs()`.

Test counts are unchanged (assertions added, no test added): **44 / 12 / 30 /
1**, identical to the gate audit's baseline.

# Evidence

## 1. Codec suite (acceptance)

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_codec --locked
running 12 tests
…
test head_total_cap_is_exactly_64_kib ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

## 2. Lockdown suite (acceptance)

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
running 30 tests
…
test unrepresentable_header_name_is_dropped ... ok
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

## 3. Whole crate (acceptance)

```console
$ cargo test -p blastit-fastcgi --locked
     Running unittests src/lib.rs (…/blastit_fastcgi-b0ea3a55ce11f1db)
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_fastcgi_codec.rs (…/critical_fastcgi_codec-142dcb4462d745b0)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/critical_fastcgi_lockdown.rs (…/critical_fastcgi_lockdown-0264b7802ae706e8)
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests blastit_fastcgi
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## 4. Teeth (skill `testing` §3) — both new oracles fail under the regression

Proved **outside the repo** (a throwaway copy of `src/` + `tests/` in
`/tmp/opencode/mb34-6-teeth` with its own manifest, `cargo test --offline`),
because this node may not touch `src/`; the scratch tree was deleted
afterwards and `git status` shows only the two test files.

```console
# src/cgi.rs: (end <= MAX_HEAD_BYTES) -> (end < MAX_HEAD_BYTES)
thread 'head_total_cap_is_exactly_64_kib' panicked at tests/critical_fastcgi_codec.rs:229:5:
assertion `left == right` failed
  left: None
 right: Some(65536)
# the pre-existing parse_head assertions above it stay green: only the new
# head_end assertion catches the boundary regression.

# src/lockdown.rs: first.is_ascii_uppercase() -> (… || first.is_ascii_digit())
thread 'unrepresentable_header_name_is_dropped' panicked at tests/critical_fastcgi_lockdown.rs:314:5:
assertion `left == right` failed: only the fixed params
  left: 17
 right: 16

# src/lockdown.rs: is_representable_header's None arm false -> true
thread 'unrepresentable_header_name_is_dropped' panicked at tests/critical_fastcgi_lockdown.rs:314:5:
assertion `left == right` failed: only the fixed params
  left: 17
 right: 16
```

Each oracle therefore asserts the denied path **and** the accepted side: the
at-cap buffer must read as complete *and* the over-cap twin must not; the
digit-leading and empty names must be dropped *and* the 16 fixed params must
survive untouched.

## 5. Mutation campaign (acceptance) — unchanged, 3 tolerated survivors

```console
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-6-mut CARGO_NET_OFFLINE=true \
    make mutants-rust PKG=blastit-fastcgi
Found 352 mutants to test
ok       Unmutated baseline in 0s build + 0s test
MISSED   crates/blastit-fastcgi/src/lockdown.rs:185:9: replace Params::is_empty -> bool with true
MISSED   crates/blastit-fastcgi/src/lockdown.rs:589:32: replace | with ^ in hex_pair
MISSED   crates/blastit-fastcgi/src/param.rs:60:37: replace | with ^ in encode_len
352 mutants tested in 2m: 3 missed, 331 caught, 18 unviable
make: *** [Makefile:168: mutants-rust] Error 2   # cargo-mutants "survivors exist"
```

Same 352/331/3/18 and the same three tolerated survivors as
`AUDIT-MB.3.4-gate.md`; no new mutant, since no `src/` byte changed (the
`is_empty`, two `|`→`^` equivalences remain deliberately untested).

## 6. `make check` (acceptance) — green

```console
$ make check            # log: /tmp/opencode/mb34-6-check-green.log
uv run --no-project tools/guard.py
guard: OK (43 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
  M  crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs  [VIOLATION: not covered by any node path]
  report only: 2 breaking change(s) would fail an enforced run
cargo fmt --all -- --check          # no output: clean
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile … in 0.14s
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    Finished `dev` profile … in 0.14s
   Generated …/target/doc/blastit/index.html and 13 other files
…
     Running tests/critical_fastcgi_codec.rs  … test result: ok. 12 passed
     Running tests/critical_fastcgi_lockdown.rs … test result: ok. 30 passed
…
56 fences: 29 executed, 27 accounted, 0 failed · 19.8s
uv run --no-project tools/plan.py validate
plan OK
make exit=0
```

**History (window closed).** An earlier run of this target was red at
`rs-test -p blastit-manifest --test docs_fences`
(`docs/frameworks/php.md does not boot … route "app": fastcgi listen: unknown
key`) because the sibling `MB.3.5.1` leaf's grammar had landed before that page
was moved to the entry-only manifest. That window is closed — the page now
writes the entry-only manifest, and the committed run above is green (exit `0`,
56 fences, 0 failed, `plan OK`) with no change to anything in this node.

The `critical-guard` lines above are its `--base HEAD` **report-only** mode
(the `check` target passes no `--node`), by design. With this node named:

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.3.4.6
node MB.3.4.6: paths ['…/critical_fastcgi_codec.rs', '…/critical_fastcgi_lockdown.rs']
  M  crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs  [authorized by crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs]
  M  crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs  [authorized by crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.3.4.6
```

## 7. Diff is additions only

```console
$ git diff --numstat -- crates/blastit-fastcgi
5       0       crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs
20      1       crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs
$ git status --porcelain -- crates/blastit-fastcgi/src
(empty)
```

The single `−1` is the 3-row `headers` fixture line replaced by the 5-row one;
all three old rows and both old assertions survive verbatim.

# Files

- `crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs` — two `head_end`
  assertions in `head_total_cap_is_exactly_64_kib` (+5 lines).
- `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` —
  `unrepresentable_header_name_is_dropped` fixture +2 rows, `dropped()` +2
  entries, a no-pair loop (+20/−1).

No `src/`, `Cargo.toml`/`Cargo.lock`, `crates/blastit-manifest/**` or `plan/`
change.

# Follow-ups

- **Tooling gotcha (worth a LESSONS line by whoever owns that file).** The
  editor/agent format-on-save hook in this checkout rewrites a whole file with
  the *pre-2024* rustfmt style (legacy import ordering, legacy `assert!`
  wrapping), while `make rs-fmt` (`cargo fmt --all -- --check`, edition 2024)
  wants the modern style. Editing a rustfmt-2024 file with that hook silently
  reformats untouched imports and helpers, so a "test-only, additions-only"
  node can arrive with unrelated churn. It was repaired here by re-applying the
  base formatting with a scripted patch (no editor hook) and re-checking
  `cargo fmt --all -- --check`; the final diff is additions-only.
- `AUDIT-MB.3.4-gate.md` §"Notes for the epic gate" still lists
  `critical_fastcgi_codec.rs` as absent from design §7.1's critical-file list;
  unchanged by this node.
- `record::FCGI_KEEP_CONN` and `Params::is_empty` stay documented-only, as the
  audit decided; this node deliberately adds no getter-only assertion.
