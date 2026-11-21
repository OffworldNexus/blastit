# Result MB.21.5.3

## Summary

Audited the Blastfile toolchain the way the release gate needs: 208 in-scope cases classified against
the design's §20 table, R1–R30 traced to concrete test names, and `cargo-mutants` run over all nine
in-scope modules (828 mutants, 748 viable). **92.0 % killed**, every module group above the 75 % bar
(84.6 %–100 %). The 45 misses resolve into **30 testable mutants — every one filed, as ten child nodes
(`MB.21.5.5`–`MB.21.5.14`) — plus two boundary-only mutants (`print.rs:63:24`, recorded) and 13
equivalents/dead-code mutants**, each explained in one line.

Three findings are not just missing assertions:
`scripts/check_manifest_purity.py` is **evadable** by `use std::{env};` (measured, with the fixture and a
widened needle in the report); `blastfile::read` has **no accepted-path oracle in this crate**, which
hides a `>`→`<` regression in the reader; and §20's ≥ 20-document round-trip corpus floor that `MB.21.5.2`
was assigned **never landed** (`CORPUS_SIZE` is still 15, and `docs_fences` never runs P1–P3 over the
documented documents). `make test-critical` (exit 0, 41 Rust suites, 50 Python tests, zero skips) and
`make check` (exit 0) are green on the audited tree, and no source, test or harness file was edited.

## Evidence

**1. `docs/testing/AUDIT-MB.21.md` (611 lines)** carries the classification table, the R1–R30 trace
with the deny-side column, the per-file ratios with per-outcome counts, all 45 survivors with a verdict
each, the protected-suite assertion diff, the measured purity evasion and the gap table. `ls`:

```console
$ ls docs/testing/AUDIT-MB.21.md
docs/testing/AUDIT-MB.21.md
```

**2. `cargo-mutants` (27.1.0) through the repo harness** — 828 mutants found over the nine files; the
campaign is split across launches because the host restarted four times and OOM-killed the first two
(12 GiB, then 16 GiB). Runs 3/5/write are the evidence:

```console
$ for d in results-run3 results-run5 results-write; do printf "%s: " "$d";
    wc -l < "$MUTANTS/$d/mutants.out/caught.txt" | tr '\n' ' '
    wc -l < "$MUTANTS/$d/mutants.out/missed.txt" | tr '\n' ' '
    wc -l < "$MUTANTS/$d/mutants.out/timeout.txt" | tr '\n' ' '
    wc -l < "$MUTANTS/$d/mutants.out/unviable.txt"; done
results-run3: 415 23 9 39
results-run5: 79 1 5 31
results-write: 194 21 1 10
# viable 748, caught 688, missed 45, timeout 15, unviable 79 -> 688/748 = 92.0 %
# (827 of the 828 generated: one print.rs mutant was in flight when the session was killed)
```

The launch used this node's own `MUTANTS_TMP` (`$XDG_CACHE_HOME/blastit-mutants/MB.21.5.3`, the
Makefile's documented per-node scratch), so the per-outcome files are **outside** the tree and the
node's `find . -path '*mutants.out*'` line finds nothing — `$MUTANTS` in the snippet above is that
directory, and `ratio.sh` beside it reproduces the table.

Per module: lexer 88.6 %, tokens 100 %, syntax 92.5 %, parser 98.1 %, pattern 94.9 %, print 85.7 %,
write 89.8 %, `blastfile/mod.rs` 84.6 %, `edit.rs` 84.6 % (`lower.rs` 93.0 % from `MB.21.4.5`, not
re-run). The nine `lexer` timeouts and two `syntax` ones are the frozen-cursor class (bounded by
`--timeout 90`, explained, not filed); one `write` timeout is a load artifact whose log already showed
the failures.

**3. `make test-critical`** — exit 0, zero skips:

```console
$ make test-critical > /tmp/opencode/mb2153-test-critical.log 2>&1; echo exit=$?
exit=0
$ grep -c "test result: ok" /tmp/opencode/mb2153-test-critical.log
41
$ tail -4 /tmp/opencode/mb2153-test-critical.log
python -m unittest discover -s python/tests/critical
..................................................
----------------------------------------------------------------------
Ran 50 tests in 13.062s

OK
```

(The first attempt, at load average 36, failed two `critical_php_embed` probe tests at their 250 ms
budget — the host's flake, reproduced fixed on the quiet re-run above; no in-scope file changed between.)

**4. `make check`** — exit 0, 166 `test result: ok` lines, no `FAILED`:

```console
$ make check > /tmp/opencode/mb2153-make-check.log 2>&1; echo exit=$?
exit=0
$ grep -c "^test result: ok" /tmp/opencode/mb2153-make-check.log
166
$ grep -E "test result: FAILED|^error" /tmp/opencode/mb2153-make-check.log | head -3
$ tail -4 /tmp/opencode/mb2153-make-check.log
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 3 fixtures proved, 16 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
```

**5. Docs and plan gates:**

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 1.08s
$ make docs-unit
107 passed, 65 subtests passed in 20.73s
$ uv run --no-project tools/plan.py validate
plan OK
```

**6. R24 measured** (scratch tree `/tmp/opencode/mb2153-purity`, no repo edit):

```console
$ uv run --no-project python scripts/check_manifest_purity.py --root /tmp/opencode/mb2153-purity
manifest-purity: ok — 1 source file(s), no std::env        # exit 0 with `use std::{env};` + env::var(..)
$ uv run --no-project python scripts/check_manifest_purity.py     # with direct.rs present
crates/blastit-manifest/src/direct.rs:2: std::env is not allowed here: std::env::var("HOME").ok()
manifest-purity: 1 std::env use(s) …                      # exit 1
```

**7. This audit edited nothing:**

```console
$ git status --porcelain crates/ python/ scripts/ Makefile e2e/   # when the campaign and make check ran
(empty)
```

After the review, the sibling `MB.21.5.11` (now `active`) began landing its guard rows in
`parser.rs`, `blastfile_grants.rs` and `critical_blastfile_parser.rs`; those uncommitted edits are that
node's, and this report's numbers describe the audited base `6bfd1cc`.

## Release judgement

The epic lead's ruling, applied in `docs/testing/AUDIT-MB.21.md` §5.5, in the node briefs and here:

| Node | Ruling | Why |
|---|---|---|
| `MB.21.5.5` | **pre-gate** (in flight now) | the purity gate is a build gate; an evasion makes it not a gate |
| `MB.21.5.7` | **pre-gate** | the design's round-trip corpus floor (≥ 20 documents) is what stops the final gate passing vacuously; the epic's own acceptance cites it |
| `MB.21.5.6` | **post-0.2.0** | coverage only: the bound is correct and `crates/blastit`'s discovery suite covers the behaviour |
| `MB.21.5.8`–`MB.21.5.14` | **post-0.2.0** | coverage/robustness; each stays filed and prioritised, none blocks the release |

None of these is a product fail-closed defect: `MB.21.5.5` is a cheap build-gate weakness (the parser
still never reads the environment), `MB.21.5.6` and `MB.21.5.11` are oracle gaps over correct code, and
the rest are message precision, error-chain shape, one uid-0 host, or editor branches no shipped input
reaches untested.

## Files

- `docs/testing/AUDIT-MB.21.md` (new; the audit)
- `docs/testing/AUDIT-MB.21.5.3.md` (new; two-line pointer so the dispatch brief's filename resolves too)
- `plan/nodes/MB/MB.21/MB.21.5/MB.21.5.{5,6,7,8,9,10,11,12,13,14}/` (new child nodes)
- `plan/nodes/MB/MB.21/MB.21.5/MB.21.5.3/{RESULT.md,LOG.md}`

No test or code file was modified. `MB.21.5.3/NODE.md`'s `paths` names `docs/testing/AUDIT-MB.21.md`;
the brief's `AUDIT-MB.21.5.3.md` is a pointer to it (the contract path wins, the brief's name is not a
second report).

## Follow-ups

Each is a child node of `MB.21.5`, with `paths` and an acceptance command:

1. `MB.21.5.5` — **pre-gate** (in flight): close the purity gate's nested-`use` evasion (fixture +
   widened needle + negative self-test).
2. `MB.21.5.6` — post-0.2.0 (coverage): pin `blastfile::read`'s accepted path and the under-cap
   `Unreadable` refusal, so the reader's `>`→`<` mutant dies in-crate.
3. `MB.21.5.7` — **pre-gate**: the design's ≥ 20-document round-trip corpus floor (the epic's acceptance
   cites it) and the hoisted-comment owner case.
4. `MB.21.5.8` — P4's oracle: a direct `apply_to` test and a truthful `first_difference` section for
   unresolved grants (needs the protected assertion update, justified in the node).
5. `MB.21.5.9` — `EditError::source()` walked by a test (post-0.2.0).
6. `MB.21.5.10` — remove the uid-0 silent skip from `critical_blastfile_fmt.rs` (post-0.2.0).
7. `MB.21.5.11` — coverage only (post-0.2.0): pin the guard at `parser.rs:1270`; the shipped parser
   already refuses `postgres db bogus`, but no test would notice if the guard were loosened.
8. `MB.21.5.12` — four message-only survivors (`$lower_x`, `GET, post`, `view v GET foo`, `GET foo`).
9. `MB.21.5.13` — six printer/pattern survivors (one corpus case each).
10. `MB.21.5.14` — the editor's sixteen testable survivors (name tails, cross-view patch merge,
    one-line empty blocks, two statements on one line, `transactions` bounding a directive).

Out of scope, recorded for the next lexer leaf: `Lexer::run` has no per-iteration progress guard, which
is why nine mutants can only time out (§3.2 of the audit).
