# Result MB.1.3

## Summary

Bound the two remaining `blastit-sqlpolicy` cursor loops and measure the whole `lexer.rs` module.

`lexer.rs`'s byte scanner no longer hand-steps a cursor (289 lines restructured into iterator/`match_indices`-driven scans with an explicit progress index) and `learn.rs::parse_form`'s `+=` step is gone, so the eight mutants that used to spin into the 90 s cap now terminate. The module is measured at **173 caught / 30 missed / 0 timeouts / 3 unviable = 85.2 %** (bar 75 %), with a new non-critical `tests/lexer_progress.rs` (181 lines) pinning the progress budgets.

**Provenance:** attempt 1 (implementer) bounded both loops and wrote the tests but was interrupted, and its own scope spillover into `{lru,statements,tables}.rs` was reset by the lead (those modules belong to `MB.1.4`/`MB.1.5`, which then closed them). Attempt 2 was interrupted again before measuring; the lead completed the measurement below and wrote this result. No production behaviour was changed for the measurement.

## Evidence

```console
$ df -h /tmp | tail -1
tmpfs            32G   23G  8.4G  74% /tmp

$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
219 passed … 10 passed … 25 passed … 6 passed … 8 passed (doctests), 0 failed

$ grep -c 'fn ' crates/blastit-sqlpolicy/tests/lexer_progress.rs
8                                                  # >= 4 required

$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -f '**/src/lexer.rs' -j 2 --timeout 90 \
    -o /tmp/opencode/mb1-sql-lexer3
206 mutants tested in 8m: 30 missed, 173 caught, 3 unviable

$ test ! -s /tmp/opencode/mb1-sql-lexer3/mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts

$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" "$(wc -l < /tmp/opencode/mb1-sql-lexer3/mutants.out/$f.txt)"; done
caught    173
missed    30
timeout   0
unviable  3
# caught/(caught+missed+timeout) = 173/203 = 85.2 % >= 75.0 %

$ TMPDIR=/tmp/opencode/blastit-mutants CARGO_NET_OFFLINE=true \
    cargo mutants -p blastit-sqlpolicy -F 'learn\.rs:6[56][0-9]' -j 2 --timeout 90 \
    -o /tmp/opencode/mb1-sql-learn
4 mutants tested in 2m: 4 caught            # 0 timeouts, 0 missed

$ make rs-lint && make rs-doc                # exit 0, exit 0 (fmt clean)
```

**Deviations, stated:** the module now lists **206** mutants rather than the pre-restructure 298 (iterator-driven scans have a different mutation surface — the same effect `MB.1.4` 128→124 and `MB.1.5` 170→175 recorded), and the `learn.rs` filter matches 4 mutants rather than the 11 estimated at planning time. `make check` was run by the lead at the epic gate rather than here, because another leaf owned the wheel-building run at the time.

## Files

- `crates/blastit-sqlpolicy/src/lexer.rs` (iterator-driven scans)
- `crates/blastit-sqlpolicy/src/learn.rs` (`parse_form` step)
- `crates/blastit-sqlpolicy/tests/lexer_progress.rs` (new, 8 tests)

## Follow-ups

- 30 `lexer.rs` survivors are real (not equivalent): the largest cluster is `grouped_digits` (`lexer.rs:353..362`, `delete !` and two `+= → *=`), plus digit/ident edge cases. They are above the module bar but are genuine coverage debt for a security-surface parser — a later leaf can close them with digit-separator and ident-boundary cases.
- The 8 former `lexer.rs:179..230` timeouts are gone; no `--timeout` change, no `#[ignore]`, no test weakened.
