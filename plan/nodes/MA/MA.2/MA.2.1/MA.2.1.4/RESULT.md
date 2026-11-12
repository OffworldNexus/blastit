# RESULT — MA.2.1.4

## Summary

Added five protected tests to `crates/blastit-ingress/tests/critical_ingress.rs` pinning canonical
JSON *string* handling (finding G1, §4.2 A, R14): the emitter's re-escaping of `\` and `"`, the
short control escapes plus lowercase `\uXXXX`, and the parser's strict-string rejections (raw
`< 0x20` byte, unknown escape, truncated `\uXXXX`, lone low surrogate, lone high surrogate followed
by text, invalid UTF-8). Test-only: no production code, no existing assertion, no `#[ignore]` or
`#[allow]` changed. Verified in a scratch copy that the two audited surviving mutants (M2 raw
backslash, M11 raw newline) are now **killed**, each by exactly the intended new test.

## Evidence

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-ingress --test critical_ingress --locked
...
test structured_body_with_content_encoding_is_rejected ... ok
test raw_transcode_is_identity ... ok
test suggest_fix_maps_samples_to_codecs ... ok
test transcoded_headers_keep_one_canonical_length ... ok
test transcoded_headers_strip_transfer_encoding ... ok
test transcoded_headers_without_a_content_type_emit_only_a_length ... ok
test undeclared_and_none_routes_allow_only_empty_bodies ... ok

test result: ok. 51 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Baseline before this node was `46 passed`, so the five new tests are the only delta.

Mutation proof (scratch copy at `/tmp/opencode/ma21-pin/ws`, sources restored after each run):

```console
M2 emitter emits a raw backslash: exit=101 KILLED
  killed by: json_re_escapes_backslash_and_quote
   test result: FAILED. 54 passed; 1 failed; ...
M11 emitter emits a raw newline: exit=101 KILLED
  killed by: json_re_emits_short_control_escapes_and_lowercase_unicode
   test result: FAILED. 54 passed; 1 failed; ...
restored baseline exit: 0
```

## Files

- `crates/blastit-ingress/tests/critical_ingress.rs` (+5 JSON tests; additions only — the file's
  cumulative diff for MA.2.1.4–MA.2.1.6 is `121 insertions, 0 deletions`, nothing removed)

## Follow-ups

- None for this node. The worktree also carries a concurrent sibling change to
  `crates/blastit-sqlpolicy/tests/critical_literals.rs` from MA.2.1.7; report-mode
  `critical_guard --base HEAD` lists both files, while `--node MA.2.1.4` authorizes only this
  node's file (and `--node MA.2.1.7` authorizes only the sqlpolicy one), so the lead should run the
  guard per node or stage the two nodes separately.
