# RESULT — MA.2.1.6

## Summary

Added two protected tests to `crates/blastit-ingress/tests/critical_ingress.rs` closing finding G3
(§4.2 A, R36): `probe_sample(…, MAX_CHECK_SAMPLE_BYTES as u64)` is **not** truncated and still
validates as JSON (the existing `cap + 1` case stays truncated), and `transcode_buffered` itself
answers `413 BodyNotAllowed` for bytes on `GET`/`HEAD`/`OPTIONS` while a zero-byte body still returns
`Ok(b"", None)`. Test-only: no production code and no existing assertion changed. Verified in a
scratch copy that both audited surviving mutants (M1 `>` → `>=`, M8 dropped bodyless `413`) are now
**killed**, each by exactly the intended new test.

## Evidence

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --test critical_ingress --locked
...
test suggest_fix_maps_samples_to_codecs ... ok
test transcoded_headers_keep_one_canonical_length ... ok
test transcoded_headers_strip_transfer_encoding ... ok
test transcoded_headers_without_a_content_type_emit_only_a_length ... ok
test undeclared_and_none_routes_allow_only_empty_bodies ... ok

test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

MA.2.1.5 left the suite at `53 passed`, so the two new tests are the only delta (46 baseline).
The existing `cap + 1` truncation test is unchanged and still green.

Mutation proof (scratch copy at `/tmp/opencode/ma21-pin/ws`, sources restored after each run):

```console
M1 probe_sample cap uses >=: exit=101 KILLED
  killed by: probe_sample_accepts_exactly_the_cap
   test result: FAILED. 54 passed; 1 failed; ...
M8 transcode_buffered drops the bodyless 413: exit=101 KILLED
  killed by: buffered_bodyless_methods_reject_any_bytes
   test result: FAILED. 54 passed; 1 failed; ...
restored baseline exit: 0
```

Full gate:

```console
$ make check
...
     Running tests/critical_ingress.rs (target/debug/deps/critical_ingress-a6f07d37e5e97ca4)

running 55 tests
test bodyless_safe_methods_reject_bodies ... ok
test buffered_bodyless_methods_reject_any_bytes ... ok
...
test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
...
no Python critical tests yet (python/tests/critical/)
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s
uv run --no-project tools/plan.py validate
plan OK
make check exit=0
```

## Files

- `crates/blastit-ingress/tests/critical_ingress.rs` (+2 tests; additions only)

## Follow-ups

- The probe test keeps the crate's existing calling convention: `raw_prefix` is the first bytes of
  the body while `total_len` is the real length, so the boundary is exercised through `total_len`
  alone (as specified by §4.2 A) without allocating a 64 KiB fixture.
- Worktree note shared with MA.2.1.4/.5: `crates/blastit-sqlpolicy/tests/critical_literals.rs`
  (+46) is a concurrent sibling change from MA.2.1.7. Report-mode
  `critical_guard --base HEAD` lists both changed files; enforced runs with `--node` authorize each
  node's own path only, so the lead must run the guard per node or stage the nodes separately.
