# RESULT — MA.2.1.5

## Summary

Added two protected tests to `crates/blastit-ingress/tests/critical_ingress.rs` pinning R15/ADR-0006
(finding G2, §4.2 A): a raw `;` in an `application/x-www-form-urlencoded` body is *data*, not a pair
separator (`a=1;b=2` is one pair re-encoded as `a=1%3Bb%3D2`), `&` still separates pairs, and a
lowercase `%3b` escape decodes to `;` and re-encodes as `%3B`. Test-only: no production code and no
existing assertion changed. Verified in a scratch copy that the audited surviving mutant (M3: form
decoder also splits on `;`) is now **killed** by the new raw-semicolon test.

## Evidence

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --test critical_ingress --locked
...
test suggest_fix_maps_samples_to_codecs ... ok
test transcoded_headers_keep_one_canonical_length ... ok
test transcoded_headers_strip_transfer_encoding ... ok
test transcoded_headers_without_a_content_type_emit_only_a_length ... ok
test undeclared_and_none_routes_allow_only_empty_bodies ... ok

test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

MA.2.1.4 left the suite at `51 passed`, so the two new tests are the only delta.

Mutation proof (scratch copy at `/tmp/opencode/ma21-pin/ws`, sources restored after the run):

```console
M3 form decoder also splits on ';': exit=101 KILLED
  killed by: form_treats_a_raw_semicolon_as_data
   test result: FAILED. 54 passed; 1 failed; ...
restored baseline exit: 0
```

## Files

- `crates/blastit-ingress/tests/critical_ingress.rs` (+2 form tests; additions only)

## Follow-ups

- None. The raw-`;` differential is now exercised end-to-end through
  `transcode_buffered`, so the previously partial oracle `form_reencodes_semicolon` (which only
  feeds `%3B`) is no longer the sole cover for the ADR-0006 rule.
