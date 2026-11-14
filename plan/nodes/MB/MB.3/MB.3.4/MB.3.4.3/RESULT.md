# Result MB.3.4.3 — protected lockdown-suite hardening

## Summary

Appended five tests to `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs`
(pure insertion: 187 added lines, 0 removed/changed). No `src/`, `Cargo.toml` or
`Cargo.lock` edit; no existing test or assertion touched. The five audit items
are closed:

1. `script_name_with_a_query_is_malformed` — `SCRIPT_NAME = "/index.php?x"` is
   `Err(Malformed)` through `build`; the accepted twin still builds and carries
   no `?` (kills `lockdown.rs:515` `||`→`&&`, audit survivor #7).
2. `the_total_param_byte_budget_is_enforced_through_build` — eight 8 192-byte
   `X-A`..`X-H` values are `Err(ParamsTooLarge)` through the public `build`;
   `0..=7` headers build, each carried value stays 8 192 bytes and is never
   dropped, and the measured pair width (8 205) is under the 65 531 pair cap
   while the eighth pair pushes the total past 65 535 (closes MINOR (a)).
3. `params_debug_prints_names_never_values` — `format!("{params:?}")` holds
   `names`, `HTTP_COOKIE`, `HTTP_X_FOO`, and neither `session` nor `secret`
   (kills `lockdown.rs:133` `Debug`→`Ok(())`, survivor #5).
4. `every_hop_by_hop_name_is_dropped` — all nine `is_hop_by_hop` names in mixed
   case appear in `dropped()` as their folded `HTTP_*` name, are absent from
   `pairs()`, and add no param (16 fixed params remain).
5. `error_display_and_source_are_not_empty` — both `blastit_fastcgi::Error`
   variants format as their inner phrase and expose `source()`, and the four
   `LockdownError` phrases are non-empty and pairwise distinct (kills
   `lib.rs:87`, `lib.rs:96`, `lockdown.rs:109`; survivors #1, #2, #4).

Mutation campaign: 348 mutants, **4 missed, 326 caught, 18 unviable** — all five
tasked survivors moved into `caught.txt`; the four remaining are the two
`|`→`^` equivalences, `Params::is_empty` (deliberately untested), and
`cgi.rs:221` `parse_status <`→`<=` (audit survivor #3, owned by `MB.3.4.4`).

## Review response

Reviewer MINOR cleared: deleted the derived `assert!(params.encoded_len() <= MAX_PARAMS_BYTES)` (an invariant `Params::insert` already guarantees); the file is 187 added lines, 0 removed, and the suite is still 30 passed.

## Evidence

### 1. Protected suite: 30 passed

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

### 2. Whole crate: 41 / 30 / 1 passed

```console
$ cargo test -p blastit-fastcgi --locked
     Running unittests src/lib.rs (…/blastit_fastcgi-b0ea3a55ce11f1db)
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_fastcgi_lockdown.rs (…)
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests blastit_fastcgi
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 3. Critical guard: no violation

```console
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.3.4.3
critical-guard: HEAD..working tree, 5 changed file(s)
node MB.3.4.3: paths ['crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs']
  M  crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs  [authorized by crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.4.3
```

### 4. Mutants: 348 tested, 4 missed (private scratch)

```console
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-3-mut make mutants-rust PKG=blastit-fastcgi
mutants: command=cargo mutants -p blastit-fastcgi -j 2 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 348 mutants to test
ok       Unmutated baseline in 0s build + 0s test
MISSED   crates/blastit-fastcgi/src/cgi.rs:221:66: replace < with <= in parse_status
MISSED   crates/blastit-fastcgi/src/lockdown.rs:185:9: replace Params::is_empty -> bool with true
MISSED   crates/blastit-fastcgi/src/lockdown.rs:581:32: replace | with ^ in hex_pair
MISSED   crates/blastit-fastcgi/src/param.rs:60:37: replace | with ^ in encode_len
348 mutants tested in 79s: 4 missed, 326 caught, 18 unviable
make: *** [Makefile:168: mutants-rust] Error 2   # cargo-mutants' "some mutants survived", not a harness failure
```

Recomputed from the per-outcome files (LESSONS #25 — 27.1.0 writes no
`outcomes.txt`):

```console
$ wc -l mutants.out/{caught,missed,timeout,unviable}.txt
  326 mutants.out/caught.txt
    4 mutants.out/missed.txt
    0 mutants.out/timeout.txt
   18 mutants.out/unviable.txt
  348 total
```

The five tasked survivors are now killed:

```console
$ grep -nE "lib.rs:87|lib.rs:96|lockdown.rs:109|lockdown.rs:133|lockdown.rs:515" mutants.out/caught.txt
1:crates/blastit-fastcgi/src/lib.rs:87:9: replace <impl fmt::Display for Error>::fmt -> fmt::Result with Ok(Default::default())
2:crates/blastit-fastcgi/src/lib.rs:96:9: replace <impl std::error::Error for Error>::source -> Option<&(dyn std::error::Error +'static)> with None
82:crates/blastit-fastcgi/src/lockdown.rs:109:9: replace <impl fmt::Display for LockdownError>::fmt -> fmt::Result with Ok(Default::default())
83:crates/blastit-fastcgi/src/lockdown.rs:133:9: replace <impl fmt::Debug for Params>::fmt -> fmt::Result with Ok(Default::default())
211:crates/blastit-fastcgi/src/lockdown.rs:515:9: replace || with && in script_name_bytes
```

### 5. Lint: clean

```console
$ make rs-lint
PYTHONHOME=… cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking blastit-fastcgi v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit-fastcgi)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.22s

$ cargo fmt --all -- --check
FMT_OK
```

### 6. `make check`: exit 0

```console
$ make check            # log: /tmp/mb343-check.log
critical-guard: HEAD..working tree, 5 changed file(s)
  M  crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs  [VIOLATION: not covered by any node path]
  report only: 1 breaking change(s) would fail an enforced run
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo test --workspace --all-targets --locked
test result: ok. 41 passed; …            # blastit-fastcgi unittests
test result: ok. 30 passed; …            # critical_fastcgi_lockdown
…
test crates/blastit-fastcgi/src/lib.rs - (line 30) ... ok
…
57 fences: 30 executed, 27 accounted, 0 failed · 26.2s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

The `[VIOLATION: not covered by any node path]` line is the **report-only**
mode of `make critical-guard` (no `--node` means no candidate paths, and the
target deliberately exits 0 — `Makefile:73-74`). The enforced form, which is
this node's acceptance command, passes (§3 above).

## Files

- `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` — 188 lines
  appended (five `#[test]` fns, each with its rule doc comment); no other
  change in the working tree besides the `lead`'s own `plan/` edits and this
  `RESULT.md`/`LOG.md`.

## Follow-ups

1. **The node's expected-survivor list names one wrong line.** `# Acceptance`
   and `DONE MEANS` list `lib.rs:96 Error::source->None` among the four
   remaining, but that mutant is now **caught** (asserted for both variants);
   the actual fourth survivor is `cgi.rs:221:66` `replace < with <= in
   parse_status` — audit survivor #3, which `MB.3.4.4` owns by design. The
   required counts (348 / 4 / 326 / 18) and "every one of #1,#2,#4,#5,#7 must
   be caught" both hold; only that one line of the node prose is stale.
   Suggest the lead correct it so `MB.3.4.4` is credited with survivor #3.
2. `make mutants-rust` writes `mutants.out/` at the repo root (shared by every
   campaign); this run replaced the `test-curator` audit's copy (its numbers
   are preserved in `docs/testing/AUDIT-MB.3.4.md`). The private scratch
   `$HOME/.cache/blastit-mutants/mb34-3-mut` was not shared with anyone.
3. No source behaviour was changed, so nothing further is needed for
   `resolve()`'s `CONTENT_TYPE` cap (`MB.3.4.5`) — still open, as filed.
