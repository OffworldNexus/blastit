# Result MB.1.4 — `blastit-sqlpolicy` normalize / literals / lru mutants (attempt 2)

**STATUS: done** · attempt 2/3 · base `686b5b7` on `release/0.2.0` · the two
attempt-1 timeouts are **gone (0 timeouts)** and the two test artefacts the
collision deleted are back:

- new `crates/blastit-sqlpolicy/tests/policy_module_edges.rs` (non-critical, 10
  tests) — recreated;
- `crates/blastit-sqlpolicy/src/lru.rs` — **+47 lines of inline tests only**, the
  three cases attempt 1 described (`a_reinserted_key_counts_as_a_use_and_evicts_nothing`,
  `a_capacity_of_one_keeps_only_the_last_key`, `a_key_that_is_not_found_is_never_inserted`);
- `crates/blastit-sqlpolicy/src/normalize.rs` — **the one production change this
  task allows**: the two code-region scans whose `i += 1` step mutation never
  advanced now take their step from the iterator position instead (§2.1).

No `critical_*` file was touched, no accepted syntax was narrowed, no lint, test
or `--timeout` was weakened, no `#[mutants::skip]`, no `Cargo.toml`/`Cargo.lock`
edit.

## Summary

| outcome | mutants |
|---|---|
| `caught` | **108** |
| `missed` | **1** (equivalent, §4) |
| `timeout` | **0** |
| `unviable` | **15** |
| **total** | **124** |

Run summary line: `124 mutants tested in 7m: 1 missed, 108 caught, 15 unviable`.

### Per-module killed ratio (viable = caught + missed + timeout)

| Module | Mutants | caught | missed | timeout | unviable | viable | killed |
|---|---|---|---|---|---|---|---|
| `src/normalize.rs` | 110 | 97 | 1 | 0 | 12 | 98 | **99.0 %** |
| `src/literals.rs` | 4 | 4 | 0 | 0 | 0 | 4 | **100.0 %** |
| `src/lru.rs` | 10 | 7 | 0 | 0 | 3 | 7 | **100.0 %** |
| **the three modules** | **124** | **108** | **1** | **0** | **15** | **109** | **99.1 %** |

Every module is past the node's 75.0 % bar; `normalize.rs` (the
allowlist-bypass surface) is at 99.0 % with one equivalent survivor and — the
bar attempt 1 missed — **no timeout at all**.

### 2.1 Why the mutant total is 124 and not 128

The node's 128 was the *pre-fix* `--list`. Closing the two timeouts necessarily
removes mutants: a counter increment is the only arithmetic in those loops, and
that is exactly what cargo-mutants rewrites. Measured, not assumed:

```console
$ diff <(sed 's/:[0-9]*:[0-9]*:/:/' list-before.txt | sort) \
       <(sed 's/:[0-9]*:[0-9]*:/:/' list-after.txt  | sort)     # 128 → 124
< crates/blastit-sqlpolicy/src/normalize.rs: replace += with *= in normalize
< crates/blastit-sqlpolicy/src/normalize.rs: replace += with -= in normalize
< crates/blastit-sqlpolicy/src/normalize.rs: replace += with *= in tokenize
< crates/blastit-sqlpolicy/src/normalize.rs: replace += with -= in tokenize
```

Four mutants out (the two `i += 1` steps at old lines `125:27` and `314:27`,
`-=` caught and `*=` the timeout), none in: the `while i < text.len()`
comparison became `if i < resume`, which yields the same three `<` mutants the
old condition did. `128 − 4 = 124`, and the four counts add to **124** =
108 + 1 + 0 + 15.

What the fix is, and why it is structural rather than a wider timeout:

```rust
// before                                     // after
let mut i = 0;                                let mut resume = 0;
while i < text.len() {                        for (i, &c) in text.iter().enumerate() {
    if is_space(text[i]) {                        if i < resume { continue; }
        pending_space = true;                     if is_space(c) {
        i += 1;                                       pending_space = true;
        continue;                                     continue;
    }                                             }
    ...                                           ...
    i = end;                                      resume = end;
}                                             }
```

The step of the loop is now the iterator's own position, so **no mutation of any
expression in the loop body can stop it advancing** — `resume` only says how
many characters the iterator is about to reach are already in `out`
(`a$1`, `f(`, `::` are several characters each). Behaviour is unchanged: all
224 lib tests pass, including `a_deeply_nested_text_is_normalised_in_linear_time`
and `a_deeply_nested_text_does_not_exhaust_the_stack` (200 000 nested
parentheses, each under the existing 2 s bound), and the pre-existing
`an_in_list_*` / `a_run_of_identical_values_rows_*` shape cases are untouched.

A second measurement win came with it: the five `code_token` `i + 1 → i * 1`
mutants used to return `end == i`, which made the *caller's* old `while` loop
push one empty token per iteration until the process aborted on
`memory allocation of 134217728 bytes failed` — a catch that depended on an OOM
abort winning the race against the 90 s timeout, which is why attempt 1 needed
`ulimit -v`. These now simply fail an assertion (§2.3). `ulimit -v 8000000` is
kept in the wrapper as belt-and-braces, but nothing was caught by it: no mutant
log contains `memory allocation of`, `SIGABRT` or a test-harness timeout.

## Evidence

### 0. `df -h /tmp` before the run — **PASS**

```console
$ df -h /tmp | tail -1          # at session start
tmpfs            32G   23G  8.4G  74% /tmp
$ df -h /tmp | tail -1          # before the campaign (after the two builds)
tmpfs            32G   22G   11G  68% /tmp
$ df -h /tmp | tail -1          # after the campaign and both gates
tmpfs            32G   24G  8.3G  74% /tmp
```

### 1. Crate tests green (new tests included) — **PASS**

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
     Running unittests src/lib.rs               224 passed; 0 failed   # +3 inline `lru` tests
     Running tests/critical_learn_policy.rs      10 passed; 0 failed
     Running tests/critical_literals.rs          25 passed; 0 failed   # protected, unchanged
     Running tests/lexer_progress.rs              6 passed; 0 failed   # MB.1.3's
     Running tests/policy_module_edges.rs        10 passed; 0 failed   # new (this node)
     Running tests/policy_module_tables.rs       18 passed; 0 failed   # MB.1.5's
   Doc-tests blastit_sqlpolicy                    8 passed; 0 failed
```

`policy_module_edges` alone, at the moment it was written:

```console
$ cargo test -p blastit-sqlpolicy --locked --test policy_module_edges
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 2. One file-scoped run over the three modules — **PASS** (7 min, 124 mutants, 0 timeouts)

```console
$ sed -n '8,16p' /tmp/opencode/mb14-attempt2-run.sh   # cap + private scratch
MUT_TMP=/tmp/opencode/blastit-mutants/mb14-attempt2-$$
mkdir -p "$MUT_TMP" /tmp/opencode/mb1-sql-normalize
echo "scratch: $MUT_TMP"
TMPDIR="$MUT_TMP" CARGO_NET_OFFLINE=true cargo mutants \
  -p blastit-sqlpolicy \
  -f '**/src/{normalize,literals,lru}.rs' \
  -j 2 --timeout 90 \
  -o /tmp/opencode/mb1-sql-normalize

$ /tmp/opencode/mb14-attempt2-run.sh
scratch: /tmp/opencode/blastit-mutants/mb14-attempt2-1637979
Found 124 mutants to test
ok       Unmutated baseline in 28s build + 3s test
MISSED   crates/blastit-sqlpolicy/src/normalize.rs:629:33: replace && with || in trailing_word in 0s build + 7s test
124 mutants tested in 7m: 1 missed, 108 caught, 15 unviable
cargo-mutants exit: 2

$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" "$(wc -l < /tmp/opencode/mb1-sql-normalize/mutants.out/$f.txt)"; done
caught    108
missed    1
timeout   0
unviable  15          # 108 + 1 + 0 + 15 = 124

$ test ! -s /tmp/opencode/mb1-sql-normalize/mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts
```

`cargo-mutants exit: 2` is "some mutants were missed" (its documented exit code
for a campaign with survivors), not a failed run: the baseline built and every
one of the 124 mutants was tested.

**Two documented deviations, neither touching test selection, timeouts or the
mutant set:**

1. **`TMPDIR` is private**, `-o` is the node's own path. The node's snippet
   shares `TMPDIR=/tmp/opencode/blastit-mutants`, and another campaign
   (`lexer`/`MB.1.5`) was running under that parent at the same time:
   `cargo-mutants` 27.1.0 deletes stale `cargo-mutants-*.tmp` under `$TMPDIR` on
   start, so a shared parent kills one of the two runs (LESSONS, `MB.1.2`);
   observed live — the sibling run used
   `/tmp/opencode/blastit-mutants/cargo-mutants-blastit-*.tmp` while this one
   used the nested `mb14-attempt2-1637979/`, and both completed. `-o` is
   exactly `/tmp/opencode/mb1-sql-normalize`, so every command in §2–§4 reads
   the paths the node writes.
2. **The brace glob is used verbatim** (`-f '**/src/{normalize,literals,lru}.rs'`);
   `cargo mutants --list` with it and with three separate `-f` flags lists the
   same 124 mutants (`diff` of the two lists is empty), so the measured set is
   the one the node specifies.

### 2.3 The former OOM-abort catches now fail an assertion — **PASS**

```console
$ grep 'phase=Test' debug.log | grep 'code_token' | grep -o 'process_status=[A-Za-z]*([0-9]*)\|elapsed=[0-9.]*m\?s' | sort -u | head -4
process_status=Failure(101)  elapsed=3.07408854s
process_status=Failure(101)  elapsed=4.095871127s
process_status=Failure(101)  elapsed=5.039910077s
process_status=Failure(101)  elapsed=718.041747ms
$ grep -c "memory allocation of" /tmp/opencode/mb1-sql-normalize/mutants.out/log/*.log | grep -v ":0" | wc -l
0
```

Every `code_token` mutant is a plain test failure in 1–6 s (the suite itself
runs in ~4 s), and no mutant log shows an allocation abort or a timeout.

### 3. Per-module killed ratio, each ≥ 75.0 % — **PASS**

```console
$ for f in caught missed timeout; do echo "== $f"; cut -d: -f1 \
    /tmp/opencode/mb1-sql-normalize/mutants.out/$f.txt | sort | uniq -c; done
== caught
      4 crates/blastit-sqlpolicy/src/literals.rs
      7 crates/blastit-sqlpolicy/src/lru.rs
     97 crates/blastit-sqlpolicy/src/normalize.rs
== missed
      1 crates/blastit-sqlpolicy/src/normalize.rs
== timeout
(no output)
```

- `normalize.rs`: 97 / (97 + 1 + 0) = **99.0 %**.
- `literals.rs`: 4 / (4 + 0 + 0) = **100.0 %**.
- `lru.rs`: 7 / (7 + 0 + 0) = **100.0 %** (`Lru` is `pub(crate)`, so these are
  inline unit tests; the three unviable ones are `get`'s
  `Some(Box::leak(Box::new(Default::default())))`, which needs `V: Default` the
  impl's bounds do not carry, and the two `&&` → `||` rewrites of the let chains
  in `insert`, which do not compile).

### 4. Survivors: tested, or unreachable with a reason — **PASS**

```console
$ cat /tmp/opencode/mb1-sql-normalize/mutants.out/missed.txt
crates/blastit-sqlpolicy/src/normalize.rs:629:33: replace && with || in trailing_word

$ cat /tmp/opencode/mb1-sql-normalize/mutants.out/timeout.txt
(empty — 0 timeouts)
```

The one survivor is the same **equivalent** mutant attempt 1 identified (line
612 then, 629 now — the edit moved it):

```rust
// src/normalize.rs:626
let item = *parent.items.last()?;
let piece = pieces.get(item.start)?;
(item.end == item.start + 1 && piece.kind == Kind::Word).then(|| (item, piece.word()))
```

Its only caller, `in_list`, compares the returned word with `in`
(`eq_ignore_ascii_case`). The `||` version also returns a word for an item that
is *not* one `Word` piece; such an item is either a group (its first piece is
the `(` token, whose word is `(`) or a single piece of another kind — an
operator, a number, `$?`, or a quoted region, whose text still carries its
quotes (`"in"`, `'in'`, `$$in$$`). None of those words is `in`, and the only
item whose stripped first word *is* `in` is a bare `in` word token, which
satisfies `end == start + 1` **and** `kind == Kind::Word` — so both versions
answer `Some((item, "in"))` on exactly the same inputs. No input can distinguish
them, so no test can fail on it. All 15 unviable mutants are compile failures
(`Default::default()` for `Kind`, `Token`, `Item`, `Joined`, `Frame` and a
generic `V`, none of which implement `Default` in those positions, plus the two
`||` let-chain rewrites in `Lru::insert`).

### 5. Green gate — **PASS** (`make rs-lint` + `make rs-doc`, per the dispatch)

`make check` builds a wheel into `dist/`, which the lead owns; the dispatch
substitutes its two Rust halves. Both are workspace-wide, so they also cover
every crate this node did not touch:

```console
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-lint
    Checking blastit-sqlpolicy v0.1.1 … blastit-manifest … blastit-dbproxy …
    Checking blastit-scheduler … blastit-env … blastit … blastit-py
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.18s

$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-doc
 Documenting blastit-py v0.1.1 …
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.23s
   Generated …/target/doc/blastit/index.html and 12 other files

$ cargo clippy -p blastit-sqlpolicy --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.65s

$ rustfmt --edition 2024 --check \
    crates/blastit-sqlpolicy/src/lru.rs \
    crates/blastit-sqlpolicy/src/normalize.rs \
    crates/blastit-sqlpolicy/tests/policy_module_edges.rs
fmt rc=0
```

`cargo fmt --all` was never run (three other leaves are editing this crate);
the three files this node owns were formatted with `rustfmt --edition 2024`
(the crate is edition 2024, which `--edition 2021` refuses on `lru.rs`).

## Files

- `crates/blastit-sqlpolicy/src/normalize.rs` — **+39/−11**: the two code-region
  scans (`normalize` and `tokenize`) now step with
  `for (i, &c) in text.iter().enumerate()` and a `resume` index instead of a
  counter, so no arithmetic mutation can stall them; comments record why.
  Behaviour unchanged (224 lib tests, 10 new integration tests).
- `crates/blastit-sqlpolicy/src/lru.rs` — **+47**, appended tests only (three
  cases); no production line changed.
- `crates/blastit-sqlpolicy/tests/policy_module_edges.rs` — **new**, 10 tests,
  allowed ↔ denied twins through `Allowlist::check` (see below).
- Not touched: `src/lexer.rs`, `src/learn.rs`, `src/statements.rs`,
  `src/tables.rs`, `src/analysis*`, `src/lib.rs`, any `tests/critical_*.rs`,
  `tests/lexer_progress.rs`, `tests/policy_module_tables.rs`,
  `Cargo.toml`/`Cargo.lock`, `e2e/`, `plan/` (other than this node's
  `RESULT.md`/`LOG.md`).

What the new integration file pins, all as pairs that differ in exactly one way:

- **Reformatting keeps the verdict, a bypass does not**: the `notes` statement
  re-laid-out (whitespace runs, a leading and a glued `/* */` comment, padding,
  repeated `;`, placeholders renumbered `$12`) is allowed; a second statement
  after it, a statement a `--` comment does not hide (its twin, a `--` comment
  that runs to the end of the text, *is* allowed — PostgreSQL runs one
  statement), a different `RETURNING` column, a literal where the grant has a
  placeholder, casing, and a no-break space where the grant has a space are all
  denied.
- **`IN` lists**: 1–8 placeholders with padding are one shape; `()`, `($1,)`,
  `('x')`, `($1, 'x')`, `('x', $1)`, `($1::int, $2)`, a subquery list and
  `NOT IN` are eight different statements.
- **The `.` is the guard, not its left operand**: `SELECT 1 IN ($1, $2)`,
  `SELECT (1 + 2) IN ($1, $2)` and `SELECT f(1) IN ($1, $2)` still collapse to
  `IN ($?, ...)`, while `probe_s.in(...)`, `s . in(...)`, `s.IN(...)` and
  `"in"(...)` keep their arity (a grant for `in($1)` does not cover `in($1, $2)`).
- **`IN` needs no space in front of it**: `(id)IN ($1, $2)` → `(id)IN ($?, ...)`
  and `(id)  IN (...)` → `(id) IN (...)`, i.e. the marker keeps the whitespace
  that *was* there, so a grant spelled with the space does not cover the other.
- **`VALUES`**: 2–8 identical rows are one shape; one row, a different row
  width, and tuple lists after anything but the keyword (`SELECT f(1), (1), (1)`,
  `SELECT s.values(1), (1), (1)`) keep every row.
- **A separator is never collapsed away**: `VALUES ((1;2)), ((1;2))`,
  `VALUES (f(1;2)), (f(1;2))` and the three-row form are unchanged, while the
  same rows without a separator are `row, ...`.
- **Redaction**: every literal form becomes one `…` with its prefix gone
  (`E'`, `B'`, `X'`, `N'`, `U&'`, `$$`, `$tag$`), is idempotent, leaks no byte
  (`s3cr3t`, `\0073`), and identifiers, `U&"…"`, placeholders and comments are
  left alone.
- **`KNOWN_STATIC_SETUP_SQL` is exempt only as an exact match**:
  `SET  TIME ZONE 'UTC'` (two spaces), `set time zone 'UTC'`, a trailing space,
  the constant carried as a literal, and the second constant with `$1` instead
  of `$?` are all redacted.
- **A placeholder list is a shape only after `IN`**: `($1, $2)`, `f($1, $2)`,
  `VALUES ($1, $2)` and an unterminated `IN ($1` keep their text, while `f($12)`
  still matches a grant for `f($1)` and `f($1, $2)` does not.
- **A grant matches the bulk statement** for 2/4/8 identical rows, and
  normalisation and redaction are idempotent on its result.

The three inline `Lru` cases: re-inserting a key that is *not* the oldest evicts
nothing, counts as a use and makes the *other* key the one an eviction takes;
a capacity-1 cache holds one entry; a key that is never found is never inserted
and does not disturb the eviction order.

## Follow-ups

- **The mutant total for these three modules is now 124, not 128** (§2.1). The
  epic (`plan/nodes/MB/MB.1/NODE.md`) sums 1019 mutants; whoever reconciles the
  total should record the −4, because the alternative is keeping a loop step
  whose `+=` mutation can never be killed. Per-module ratios, which is what the
  epic's acceptance actually measures, all improved.
- **The `trailing_word` `&&` → `||` mutant stays equivalent** (§4). If a future
  milestone wants this file at a literal 100 %, the conjunction can be spelled
  without `&&` (e.g. `matches!(item.end.checked_sub(item.start), Some(1))` plus a
  separate `piece.kind == Kind::Word` guard), which moves no behaviour and
  trades this unkillable mutant for a killable one. Recorded, not done.
- **`MB.1.3`/`MB.1.5` share this crate**: the new cases depend on the lexer's
  region kinds and `code_token_end`, and on the shape rules `MB.1.5` is
  measuring. The paired denied twins are what will notice a change; the
  `normalize` scans no longer have a step a mutation can stall.

## Blocker

None. The two attempt-1 blockers are closed: the deleted test artefacts are
recreated (and reproduced green in §1), and the two timeouts are structurally
impossible now (0 timeouts in §2, empty `timeout.txt` in §4).
