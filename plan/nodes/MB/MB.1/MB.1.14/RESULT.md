# MB.1.14 — lexer numeric extents, whole-module `learn.rs`, protected boundary oracle

- **Node:** `MB.1.14` (attempt 1) — closes `docs/testing/AUDIT-MB.1.md` `F4` and `F6` for `blastit-sqlpolicy`.
- **Branch:** `release/0.2.0`, base `9c523e2`.
- **Touched:** `crates/blastit-sqlpolicy/src/lexer.rs` and `.../src/learn.rs` (test modules only, no
  production change), `crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs` (new, protected),
  and `crates/blastit-sqlpolicy/tests/lexer_progress.rs` (deleted; every case moved, none dropped).
  `docs/knowledge/LESSONS.md` gained one bullet (role brief). No production line changed, so the
  mutant sets are the shipped sets (`cargo mutants --list`: 206 for `lexer.rs`, 96 for `learn.rs`).

## Summary

1. **Tier (`F4` a).** `tests/lexer_progress.rs` (6 `#[test]`s) is now
   `tests/critical_lexer_boundaries.rs` — protected by `tools/critical_guard.py` — with the six cases
   moved verbatim plus a seventh (`numeric_literal_extent_decides_where_the_next_token_starts`) and a
   200 KB numeric literal added to the hostile-input case. **7 `#[test]`s, 0 cases removed.**
2. **Numeric extents (`F4` b / `F6` b).** The 30 survivors were not a coverage hole in
   `grouped_digits` (4) — 21 were in `number_end`, 4 in `radix_end`, 4 in `grouped_digits`, 1 in
   `lex`; `MB.1.3`'s "largest cluster is `grouped_digits`" and "8 tests" were both wrong (`F6`).
   They are now closed by six extent tests in `src/lexer.rs` that assert the **index** a numeric
   literal ends at (text-level assertions cannot see a number split into two adjacent tokens), and by
   the public-API twins in the critical file.
3. **`learn.rs` (`F6` a).** Re-measured **as a whole module on this tree** for the first time since
   the `parse_form` rewrite: the shipped `learn.rs` was at `84 caught / 7 missed / 0 timeout /
   5 unviable` = 96 (**92.3 %**), and six of the seven survivors were real gaps. Four tests added to
   its own test module close them: **`90 caught / 1 missed / 0 timeout / 5 unviable` = 96, killed
   98.9 %** (bar 75 %), the one survivor proved equivalent.
4. **`lexer.rs` re-measured:** `caught 202 / missed 1 / timeout 0 / unviable 3` = 206, killed
   **202/203 = 99.5 %** (baseline 173/203 = 85.2 %). The one survivor is proved equivalent below.

### Before / after, per module

| Module | Mutants | Before (MB.1, `51dced9`) | After (this tree) | Killed |
|---|---|---|---|---|
| `src/lexer.rs` | 206 (203 viable) | 173 caught, 30 missed, 0 timeouts | **202 caught, 1 missed, 0 timeouts, 3 unviable** | **85.2 % → 99.5 %** |
| `src/learn.rs` | 96 | last measured in parts: 4/4 in `parse_form`, the other 92 from `MA`'s pre-refactor `90/98` | **90 caught, 1 missed, 0 timeouts, 5 unviable** (shipped `learn.rs` first: 84/7/0/5) | **92.3 % → 98.9 %** (bar 75 %) |

### Corrected survivor distribution (the 30 that were, `MB.1.3`'s mis-attribution)

| Region | survivor entries in `/tmp/opencode/mb1-sql-lexer3/mutants.out/missed.txt` |
|---|---|
| `number_end` (lines 268–304) | **21** |
| `radix_end` (329–331) | **4** |
| `grouped_digits` (349–362) | **4** |
| `lex` (126) | **1** |
| total | **30** — not "the largest cluster is `grouped_digits`" (4 of 30) |

## Evidence

### 1. The boundary oracle is protected

```console
$ ls crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs
crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs
$ grep -c '#\[test\]' crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs
7
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked     # exit 0
test result: ok. 234 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # lib
test result: ok. 10 passed …   # critical_learn_policy
test result: ok. 7 passed …    # critical_lexer_boundaries   (the 6 moved cases + the new one)
test result: ok. 25 passed …   # critical_literals
test result: ok. 22 passed …   # critical_policy_surface
test result: ok. 10 passed …   # policy_module_edges
test result: ok. 18 passed …   # policy_module_tables
test result: ok. 8 passed …    # doctests
```

The file carries every case the standard-tier original had — `unterminated_quoting_is_one_opaque_region_to_the_end`,
`each_quoted_form_closes_at_its_own_boundary`, `placeholders_and_identifiers_split_where_scan_l_splits_them`,
`comments_end_at_their_terminator_and_nest`, `a_large_hostile_text_is_scanned_in_one_pass` and
`learn_rewrites_a_placeholder_in_code_only`, each assertion unchanged — so 6 → 7 tests, 0 removed, and
the 200 KB hostile-input case now also covers a 200 KB numeric literal.

### 2. Numeric-literal extent, allowed **and** denied twins

New unit tests in `src/lexer.rs` (`#[cfg(test)] mod tests`), each asserting the exact end index of the
literal at offset 0 — the assertion a text-level suite cannot make:

- `radix_literals_end_after_their_last_digit` — `0x1F`→4, `0x1_2`→5, `0xA_B_C`→7, `0o1_7`→5, `0B1_0`→5,
  and the denied side `0x1_`/`0o7_`/`0b1_`→3 (a trailing `_` is not part of the number).
- `a_radix_prefix_without_a_digit_keeps_one_separator` — the `…fail` form: `0x_`/`0o_`/`0b_`/`0x__`→3,
  `0x`/`0xz`/`0b$1`→2.
- `decimal_grouping_joins_exactly_one_separator` — `1_2`→3, `1_2_3`→5, `12_34_56`→8; denied
  `1_`→1, `1__2`→1, `1_2_`→3, `1_$1`→1.
- `a_numeric_literal_ends_before_a_second_dot` — `1.2`→3, `1.2.3`→3, `1..2`→**1**, `1.`/`1.a`→2,
  `.5`→2, `.5e1`→4; no literal at all for `.`/`.a`/`a` (`None`).
- `an_exponent_is_part_of_the_number_only_with_its_digits` — `1e2`→3, `1e+2`/`1e-2`→4, `12e3`→4,
  `12e+34`→6, `1.5e3`→5, `.5e-3`→5; denied `1e`/`1e+`/`1e-`/`1e+x`/`1E`→1 and `1.5e`→3.
- `a_number_lets_the_next_token_start_where_scan_l_does` — the boundary a policy check sees:
  `code_token_end` of `0x1F$1`→4, `0x_$1`→3, `1_0$1`→3, `1..2`→1, `1e+2$1`→4, and the denied
  glued forms `1e$1`→4, `0x1Fg$1`→7, `1_$1`→4, `0x__$1`→6 (then `placeholder_end`→`None`).

Public-API twins in the protected file (`numeric_literal_extent_decides_where_the_next_token_starts`),
exact text, both directions:

```console
normalize("SELECT 0x1F$1")     == "SELECT 0x1F$?"      // hex ends at its digit: `$1` is a parameter
normalize("SELECT 0x1Fg$1")    == "SELECT 0x1Fg$1"     // denied twin: `integer_junk` glues the ident
normalize("SELECT 0x1_2$1")    == "SELECT 0x1_2$?"
normalize("SELECT 0o17$1")     == "SELECT 0o17$?"
normalize("SELECT 0b11$1")     == "SELECT 0b11$?"
normalize("SELECT 0x_$1")      == "SELECT 0x_$?"       // the `…fail` form keeps exactly one `_`
normalize("SELECT 0x__$1")     == "SELECT 0x__$1"      // denied twin
normalize("SELECT 1_0$1")      == "SELECT 1_0$?"
normalize("SELECT 1_$1")       == "SELECT 1_$1"        // denied twin: trailing `_`
normalize("SELECT 1__0$1")     == "SELECT 1__0$1"      // denied twin: two `_`
normalize(r"SELECT 1..E'a\'b  c'")    == unchanged      // `..` then a token start: E-string
normalize(r"SELECT 0x1F.E'a\'b  c'")  == unchanged      // hex ends before the `.`
normalize(r"SELECT 1E'a\'b  c'")      == r"SELECT 1E'a\'b c'"   // denied twin: `integer_junk`
normalize(r"SELECT 1.E'a\'b  c'")     == r"SELECT 1.E'a\'b c'"  // denied twin: `{numeric}_junk`
normalize("SELECT 1e+2$1")     == "SELECT 1e+2$?"
normalize("SELECT 1e$1")       == "SELECT 1e$1"        // denied twin: failed exponent form
```

**Probe — the new tests fail on a mutated tree and pass on the shipped one.** The probe was applied in
place (the sandbox denies `/var/tmp`, and `/tmp` is tmpfs, so a scratch checkout there would spend
RAM); the shipped file was saved first and restored byte-for-byte afterwards:

```console
$ cp crates/blastit-sqlpolicy/src/lexer.rs /tmp/opencode/mb114-lexer-shipped.rs
$ sha256sum crates/blastit-sqlpolicy/src/lexer.rs /tmp/opencode/mb114-lexer-shipped.rs
ee8626c4…485a  crates/blastit-sqlpolicy/src/lexer.rs
ee8626c4…485a  /tmp/opencode/mb114-lexer-shipped.rs

# mutant 1: lexer.rs:331 `if run > 0` -> `if run >= 0` (the `0x_` fail form)
$ sed -i '331s/if run > 0 {/if run >= 0 {/' crates/blastit-sqlpolicy/src/lexer.rs
$ cargo test -p blastit-sqlpolicy --lib lexer --locked
test lexer::tests::a_number_lets_the_next_token_start_where_scan_l_does ... FAILED   # left 5, right 3
test lexer::tests::a_radix_prefix_without_a_digit_keeps_one_separator ... FAILED     # Some(2) vs Some(3)
$ cargo test -p blastit-sqlpolicy --test critical_lexer_boundaries --locked
test numeric_literal_extent_decides_where_the_next_token_starts ... FAILED
  left: "SELECT 0x_$1"  right: "SELECT 0x_$?"
test result: FAILED. 6 passed; 1 failed

# mutant 2: lexer.rs:357 `len += 2` -> `len *= 2` (grouped digits)
test lexer::tests::a_number_lets_the_next_token_start_where_scan_l_does ... FAILED
test lexer::tests::decimal_grouping_joins_exactly_one_separator ... FAILED          # Some(1) vs Some(3)
test lexer::tests::radix_literals_end_after_their_last_digit ... FAILED             # Some(4) vs Some(5)
test result: FAILED. 19 passed; 3 failed

# mutant 3: lexer.rs:302 `end + 2` -> `end * 2` (exponent sign)
test lexer::tests::a_number_lets_the_next_token_start_where_scan_l_does ... FAILED   # left 2, right 4
test lexer::tests::an_exponent_is_part_of_the_number_only_with_its_digits ... FAILED # Some(1) vs Some(4)
test result: FAILED. 20 passed; 2 failed

# shipped tree, restored from the saved copy
$ cp /tmp/opencode/mb114-lexer-shipped.rs crates/blastit-sqlpolicy/src/lexer.rs
$ sha256sum crates/blastit-sqlpolicy/src/lexer.rs
ee8626c4…485a  crates/blastit-sqlpolicy/src/lexer.rs               # identical to the saved bytes
$ cargo test -p blastit-sqlpolicy --locked
test result: ok. 230 passed …   ok. 10 passed …   ok. 7 passed …   ok. 25 passed …
test result: ok. 22 passed …    ok. 10 passed …   ok. 18 passed …   ok. 8 passed …
```

### 3. `lexer.rs` re-measured (file-scoped, disk scratch, cgroup cap)

```console
$ TMPDIR="$(make -s print-mutants-tmp)/mb114" CARGO_NET_OFFLINE=true \
    scripts/mutants-run.sh -p blastit-sqlpolicy -f '**/src/lexer.rs' -o /tmp/opencode/mb114-lexer
mutants: scratch TMPDIR=/home/remy/.cache/blastit-mutants/mb114 (must be on disk; …)
mutants: command=cargo mutants -p blastit-sqlpolicy -f **/src/lexer.rs -o /tmp/opencode/mb114-lexer -j 2 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 206 mutants to test
ok       Unmutated baseline in 35s build + 6s test
MISSED   crates/blastit-sqlpolicy/src/lexer.rs:126:13: replace < with <= in lex in 1s build + 7s test
206 mutants tested in 14m: 1 missed, 202 caught, 3 unviable

$ test ! -s /tmp/opencode/mb114-lexer/mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts
$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" "$(wc -l < /tmp/opencode/mb114-lexer/mutants.out/$f.txt)"; done
caught    202
missed    1
timeout   0
unviable  3
```

The `-o` directory is the text record only (a few MB of logs); the *scratch* — every copied workspace
and target dir — is `$(make -s print-mutants-tmp)/mb114` on disk, per `MB.1.11`.

**202/203 viable = 99.5 % killed, 0 timeouts.** The 30 survivors of `MB.1.3` are gone; the one
remaining survivor is proved equivalent:

- `lexer.rs:126:13 replace < with <= in lex` — **equivalent, no test can distinguish it.** The only
  iteration the mutation adds is `i == chars.len()`. In it, `construct_at(chars, len)` returns `None`
  at its first statement (`let c = *chars.get(i)?;`, and `chars.get(len)` is `None`), so the `Some`
  branch — the only place that pushes a region — is not entered; the `None` branch sets
  `i = code_token_end(chars, len).max(len + 1)`, and `code_token_end` at `len` returns `len + 1`
  (`chars.get(len)` matches neither ident-start nor a number, and `number_end` returns `None` through
  `decimal_end`'s `chars.get` guard). The loop then exits with exactly the regions it would have
  returned, and no tail region is added because `code_start <= len`. The extra iteration is also
  *harmless*, not a stall: `i` strictly increases (`len + 1 > len`), so no input can loop, and
  `len + 1` cannot overflow for a real `Vec<char>`. Empirically the campaign agrees: the mutated build
  was green on the crate's whole test suite (lib, the seven integration binaries and the doctests), so
  no assertion of any tier can see the difference.

### 4. `learn.rs` as a whole module, on this tree

**Run 1 — the shipped `learn.rs` before this node added the four tests in §4.2** (the tree as `9c523e2`
left it, `learn.rs` untouched):

```console
$ TMPDIR="$(make -s print-mutants-tmp)/mb114-learn" CARGO_NET_OFFLINE=true \
    scripts/mutants-run.sh -p blastit-sqlpolicy -f '**/src/learn.rs' -o /tmp/opencode/mb114-learn
mutants: scratch TMPDIR=/home/remy/.cache/blastit-mutants/mb114-learn (must be on disk; …)
mutants: command=cargo mutants -p blastit-sqlpolicy -f **/src/learn.rs -o /tmp/opencode/mb114-learn -j 2 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 96 mutants to test
ok       Unmutated baseline in 57s build + 3s test
MISSED   …/learn.rs:106:9: replace <impl std::fmt::Display for RefuseReason>::fmt -> std::fmt::Result with Ok(Default::default()) in 3s build + 10s test
MISSED   …/learn.rs:193:9: replace LearnVerdict::is_query -> bool with true in 2s build + 6s test
MISSED   …/learn.rs:377:14: replace > with == in introspection_node in 5s build + 7s test
MISSED   …/learn.rs:377:14: replace > with >= in introspection_node in 1s build + 5s test
MISSED   …/learn.rs:399:21: delete match arm "InsertStmt" | … | "DoStmt" in introspection_node in 1s build + 8s test
MISSED   …/learn.rs:422:87: replace + with * in introspection_node in 0s build + 7s test
MISSED   …/learn.rs:563:9: delete match arm Value::Array(items) in is_copy_program in 2s build + 8s test
96 mutants tested in 9m: 7 missed, 84 caught, 5 unviable

$ test ! -s /tmp/opencode/mb114-learn/mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts
$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" "$(wc -l < /tmp/opencode/mb114-learn/mutants.out/$f.txt)"; done
caught    84
missed    7
timeout   0
unviable  5
```

That is **84/91 = 92.3 %** on the whole module (the bar is 75 %), and the first whole-module number
`learn.rs` has had since the `parse_form` rewrite — the `MB.1` gate's "every mutant measured" was
resting on `MA`'s pre-refactor `90/98` for 92 of these 96 (`F6` a). Six of the seven survivors were
real gaps, not equivalence; §4.1 says what each one was and §4.2 is the fix. Run 2 (below) is the
shipped tree's number.

#### 4.1 Disposition of every survivor

| Survivor | Verdict | Why |
|---|---|---|
| `106:9` `Display for RefuseReason` → `Ok(())` | **real gap → fixed** | every `--learn` refusal is *printed*; an empty message would name neither the refusal nor the fix. `every_refusal_names_the_reason_and_the_fix` asserts all six renderings. |
| `193:9` `LearnVerdict::is_query` → `true` | **real gap → fixed** | `is_query` decides whether a refusal is reported as a grantable query; the accessor pair is now asserted in both directions (`a_refusal_reports_no_grant_text_and_the_query_reports_no_reason`). |
| `377:14` `depth > MAX` → `==` | **real gap → fixed** | the stack guard would only fire at *exactly* 128, so a 129+-deep tree keeps recursing — the one direction `MAX_INTROSPECTION_DEPTH` exists to stop. |
| `377:14` `depth > MAX` → `>=` | **real gap → fixed** | refuses at 128 instead of 129, i.e. a legal 128-deep introspection `SELECT` stops being replayed. |
| `422:87` `depth + 1` → `depth * 1` | **real gap → fixed** | the counter never grows, so the guard can never fire at all and the walk recurses as deep as the text nests. |
| `399:21` delete the `InsertStmt`/`UpdateStmt`/…/`DoStmt` arm | **real gap → fixed (widening)** | that arm is what stops a `SELECT` that *writes* (a data-modifying `WITH`, `INTO`, `CALL`, `DO`) from being replayed as session setup. `a_select_that_writes_is_not_session_setup` replays `WITH d AS (DELETE …) SELECT version()`-shaped statements and expects a refusal. |
| `563:9` delete `Value::Array(items)` in `is_copy_program` | **equivalent** | `is_copy_program` starts at the tree of the *one* statement (`statement_tree`), and `COPY … PROGRAM` is a utility statement: PostgreSQL's parser accepts it only as the statement root, where the node is the object `{"CopyStmt": {…}}` the object arm already reads. It cannot be a CTE body, a subquery, a `VALUES` row, an expression argument or a `PREPARE`d statement, so no single-statement tree this code is handed ever holds a `CopyStmt` inside an array. Probes (refused, so unreachable by the arm either way): `WITH c AS (COPY t TO PROGRAM 'sh') SELECT 1`, `SELECT * FROM (COPY t TO PROGRAM 'sh') s`, `PREPARE p AS COPY t TO PROGRAM 'sh'`, `EXPLAIN COPY t TO PROGRAM 'sh'`. |

### 4.2 The four tests that close six of them

In `src/learn.rs`'s `#[cfg(test)] mod tests` (no production line changed):

- `every_refusal_names_the_reason_and_the_fix` — the exact `Display` text of all six `RefuseReason`
  variants, including the sanitised parser message (a quoted span is reduced to `…`, and a reason built
  from an unsanitised message cannot leak the token) — this is the `106:9` oracle.
- `a_refusal_reports_no_grant_text_and_the_query_reports_no_reason` — `is_query`/`normalized`/`redacted`/
  `reason` in both directions, so a refusal can never hand over a grant text and a query can never look
  refused — the `193:9` oracle.
- `the_introspection_walk_stops_at_its_depth_bound` — `MAX_INTROSPECTION_DEPTH` nested arrays are
  accepted and one more is refused, on the private walk itself (`377:14` ×2 and `422:87`).
- `a_select_that_writes_is_not_session_setup` — data-modifying `WITH` against a *catalogue* relation
  (every other node in those statements is one this build replays, so the write node is the only
  possible reason for the refusal), `SELECT … INTO`, `FOR UPDATE`/`FOR SHARE` and `CALL`, each beside
  the read-only twin that *is* replayed (`399:21`).

**Probe — each closed survivor fails the new test on a mutated tree.** Same method as §2 (mutate in
place, restore from the saved copy, verify the hash):

```console
$ cp crates/blastit-sqlpolicy/src/learn.rs /tmp/opencode/mb114-learn-shipped.rs
$ sha256sum crates/blastit-sqlpolicy/src/learn.rs
5d84181d…985f  crates/blastit-sqlpolicy/src/learn.rs

# 193:9 `is_query -> true`, 377:14 `>` -> `>=`, 422:87 `+` -> `*` applied together
$ cargo test -p blastit-sqlpolicy --lib learn --locked
test learn::tests::a_refusal_reports_no_grant_text_and_the_query_reports_no_reason ... FAILED
test learn::tests::the_introspection_walk_stops_at_its_depth_bound ... FAILED
test result: FAILED. 14 passed; 2 failed

# 399:21, simulated by emptying the arm body (`return false;` deleted, line 401)
$ cargo test -p blastit-sqlpolicy --lib learn --locked
test learn::tests::a_select_that_writes_is_not_session_setup ... FAILED
test result: FAILED. 15 passed; 1 failed

# restored
$ cp /tmp/opencode/mb114-learn-shipped.rs crates/blastit-sqlpolicy/src/learn.rs
$ sha256sum crates/blastit-sqlpolicy/src/learn.rs
5d84181d…985f  crates/blastit-sqlpolicy/src/learn.rs
$ cargo test -p blastit-sqlpolicy --locked
test result: ok. 234 passed …   ok. 10 passed …   ok. 7 passed …       # exit 0
```

(The three mutations above were applied together in one run; `377:14 ==` cannot be batched with them
because the two `377` mutations cancel each other's observable effect, which is why the campaign — one
mutant at a time — is the authoritative count in §4.3.)


#### 4.3 Run 2 — the shipped `learn.rs` after the four tests (this node's tree)

```console
$ mkdir -p "$(make -s print-mutants-tmp)/mb114-learn2"
$ TMPDIR="$(make -s print-mutants-tmp)/mb114-learn2" CARGO_NET_OFFLINE=true \
    scripts/mutants-run.sh -p blastit-sqlpolicy -f '**/src/learn.rs' -o /tmp/opencode/mb114-learn2
mutants: scratch TMPDIR=/home/remy/.cache/blastit-mutants/mb114-learn2 (must be on disk; …)
mutants: command=cargo mutants -p blastit-sqlpolicy -f **/src/learn.rs -o /tmp/opencode/mb114-learn2 -j 2 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 96 mutants to test
ok       Unmutated baseline in 55s build + 4s test
MISSED   crates/blastit-sqlpolicy/src/learn.rs:563:9: delete match arm Value::Array(items) in is_copy_program in 0s build + 4s test
96 mutants tested in 7m: 1 missed, 90 caught, 5 unviable

$ test ! -s /tmp/opencode/mb114-learn2/mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts
$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" "$(wc -l < /tmp/opencode/mb114-learn2/mutants.out/$f.txt)"; done
caught    90
missed    1
timeout   0
unviable  5
```

**Counts add to 96 (90 + 1 + 0 + 5), 0 timeouts, killed 90/91 = 98.9 %** (bar 75 %). The one survivor is
the equivalent `is_copy_program` array arm argued in §4.1; six real gaps were closed, which is also how
`MB.1.5`'s "every mutant measured" claim now actually holds for `learn.rs` on this tree (`F6` a).

Both `learn.rs` runs (`-o /tmp/opencode/mb114-learn`, `/tmp/opencode/mb114-learn2`) used the
`MB.1.11` wrapper, so the workspace copies and target dirs lived in
`$(make -s print-mutants-tmp)/mb114-learn{,2}` on disk under a 12 GB cgroup cap; only the text records
(`caught/missed/timeout/unviable.txt`, logs) are under `/tmp/opencode`.

### 5. Gates

```console
$ ls crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs
crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs
$ grep -c '#\[test\]' crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs
7
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
     Running unittests src/lib.rs
test result: ok. 234 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.51s
     Running tests/critical_learn_policy.rs
test result: ok. 10 passed; 0 failed; 0 ignored; …
     Running tests/critical_lexer_boundaries.rs
test result: ok. 7 passed; 0 failed; 0 ignored; …
     Running tests/critical_literals.rs
test result: ok. 25 passed; 0 failed; 0 ignored; …
     Running tests/critical_policy_surface.rs
test result: ok. 22 passed; 0 failed; 0 ignored; …
     Running tests/policy_module_edges.rs
test result: ok. 10 passed; 0 failed; 0 ignored; …
     Running tests/policy_module_tables.rs
test result: ok. 18 passed; 0 failed; 0 ignored; …
   Doc-tests blastit_sqlpolicy
test result: ok. 8 passed; 0 failed; 0 ignored; …
                                               # exit 0 (234 = 230 + the four `learn.rs` tests)

$ cargo fmt --all -- --check                     # exit 0
$ rustfmt --edition 2024 --check crates/blastit-sqlpolicy/src/lexer.rs \
      crates/blastit-sqlpolicy/src/learn.rs \
      crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs       # exit 0
$ cargo clippy -p blastit-sqlpolicy --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.52s      # exit 0
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-sqlpolicy --no-deps --locked
   Generated target/doc/blastit_sqlpolicy/index.html                        # exit 0

$ make rs-lint
    Checking … Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.93s   # exit 0
$ make rs-doc
   Generated target/doc/blastit/index.html and 12 other files               # exit 0

$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.14
critical-guard: 51dced9..working tree, 55 changed file(s)
node MB.1.14: paths ['crates/blastit-sqlpolicy/src/lexer.rs', '…/learn.rs', '…/tests/lexer_progress.rs', '…/tests/critical_lexer_boundaries.rs']
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [VIOLATION: not covered by any node path]
  A  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs  [added (always allowed)]
  M  crates/blastit/tests/critical_check_engine.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 2 protected critical test(s) changed or deleted without a covering path …

$ uv run --no-project tools/critical_guard.py --base 51dced9 \
      --node MB.1.14 --node MB.1.13 --node MB.1.15
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_edit.rs]
  A  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs  [added (always allowed)]
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.1.14, MB.1.13, MB.1.15

$ uv run --no-project tools/plan.py validate
plan OK
```

**Reading the two guard runs.** The single-node run's two violations are **not this node's**: the shared
working tree also carries MB.1.13's `crates/blastit/tests/critical_check_engine.rs` and MB.1.15's
`crates/blastit-manifest/tests/critical_manifest_edit.rs`, both of which are in *their* `paths`
(verified with `--node MB.1.13 --node MB.1.15`, the guard's documented repeatable form for a shared
tree). `MB.1.14`'s own change set is `A tests/critical_lexer_boundaries.rs` (a new critical file,
always allowed), `D tests/lexer_progress.rs` (non-critical, in `paths`) and the two `src/*.rs` test
modules — three protected changes, all authorized. Nothing this node did needed a covering path for a
protected file, and no protected file was weakened.

`make check` was **not** run (it is the lead's epic gate and builds a wheel); `make e2e`/Docker was not
run (another agent owns it).

## Files

| Path | Change |
|---|---|
| `crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs` | **new, protected** — 7 tests: the six MB.1.3 boundary/200 KB cases moved verbatim + `numeric_literal_extent_decides_where_the_next_token_starts` + a 200 KB numeric literal |
| `crates/blastit-sqlpolicy/tests/lexer_progress.rs` | **deleted** — every case moved to the critical file; none dropped (`MB.1.3`'s 6 `#[test]`s → 6 of the 7 there) |
| `crates/blastit-sqlpolicy/src/lexer.rs` | test module only: +6 tests pinning numeric extents (`number_end`/`radix_end`/`grouped_digits`/`lex` boundaries). **No production line changed.** |
| `crates/blastit-sqlpolicy/src/learn.rs` | test module only: +4 tests (`Display` text, verdict accessors, `MAX_INTROSPECTION_DEPTH`, a `SELECT` that writes). **No production line changed.** |
| `docs/knowledge/LESSONS.md` | one bullet (role brief): a numeric literal read one character short is invisible in the normalised text |

`git diff --stat crates/blastit-sqlpolicy/` (production diff is empty — only test modules and the test
file):

```console
 crates/blastit-sqlpolicy/src/learn.rs            | 112 ++++++++++++++
 crates/blastit-sqlpolicy/src/lexer.rs            | 152 +++++++++++++++++++
 crates/blastit-sqlpolicy/tests/lexer_progress.rs | 181 -----------------------
 3 files changed, 264 insertions(+), 181 deletions(-)
```

## Follow-ups

1. **`lexer.rs:126 <`→`<=` is an equivalent mutant, not a gap** — proof in §3. If the epic wants a
   100 % score, `lex`'s loop needs a shape where a boundary iteration is observable; it has none, and
   the mutant set is the shipped set, so leave it and record the equivalence.
2. **`learn.rs:563` `is_copy_program`'s `Value::Array` arm is unreachable** for any parse tree this
   code is handed (argument in §4.1). Removing dead code is a production change no node owns; a later
   node may prefer `map.values().any(...)` + `items.iter().any(...)` folded into one recursive helper.
3. **Four node kinds in that arm are defensive, not reachable:** `is_introspection_select` gates the
   walk to a `SelectStmt` root, and a CTE body can only be `SELECT`/`INSERT`/`UPDATE`/`DELETE`, so
   `CopyStmt`, `CreateTableAsStmt`, `RefreshMatViewStmt`, `CallStmt`, `DoStmt` and `MergeStmt` cannot
   appear inside a tree the walk is handed (they are refused earlier as a different root kind, or do
   not parse in that position). The arm is the right shape for a widening; no test can reach those six
   labels, which is why the one mutant that deletes the whole arm is killed by the three that *are*
   reachable (`INSERT`/`UPDATE`/`DELETE` in a CTE).
4. **`number_end`/`radix_end`/`grouped_digits` still take `&[char]`** and are asserted at offset 0 only;
   a caller that starts mid-slice is not tested (there is none today). Cheap to add if a second caller
   appears.
