# Result MB.1.5 — `blastit-sqlpolicy` statements / tables mutants

**STATUS: done** · attempt 2/3 · base `686b5b7` on `release/0.2.0`

Attempt 1's measurement was real but its test files were removed by a lead-side
collision, and six `statements.rs` mutants hung at the 90 s cap. This attempt
recreates the tests, and replaces the three hand-stepped cursor loops (plus one
unbounded mode loop) with structurally bounded ones, so **no mutant can hang**:

| | attempt 1 | attempt 2 |
|---|---|---|
| mutants | 170 | **175** |
| caught | 156 | **168** |
| missed | 1 | **0** |
| timeout | **6** | **0** |
| unviable | 7 | 7 |
| statements killed | 72/79 = 91.1 % | **84/84 = 100.0 %** |
| tables killed | 84/84 = 100.0 % | **84/84 = 100.0 %** |

## The zero-timeout bar (the one thing beyond attempt 1)

All six timeouts were the same defect class the epic already fixed in
`lexer.rs`/`edit.rs`: a scan whose progress *is* the mutated arithmetic. When
`i += 1` becomes `i *= 1` the cursor never moves, so the loop has no exit and the
90 s cap is the only thing that stops it — a `TIMEOUT` sits in the killed-ratio
denominator where no test can ever fail. The fix is to make the step the
iterator's, not a hand-counted index:

| attempt 1 timeout | now |
|---|---|
| `99:22` `+=`→`*=` in `skip_ws` | `take_while(is_space).count()` then one `pos += skipped` — there is no loop to spin |
| `119:22` `+=`→`*=` in `eat_word` (word run) | the run is `take_while(..).collect()`; its length comes from the result |
| `158:19` `+=`→`*=` in `eat_ident` (`i += 1`) | the quoted-identifier body is scanned by `quoted_ident_end`, driven by `slice::Iter::next`/`as_slice` |
| `126:9` `eat_keyword -> true` | `parse_modes` refuses a mode that consumed nothing (`if self.pos == before { return false }`) |
| `216:16` `delete !` in `parse_modes` | same guard: a mode that "succeeds" without consuming is a denial, not a spin |
| `228:9` `parse_mode -> true` | same guard |

The last three share one structural check. It changes no accepted syntax: a
successful `parse_mode` always eats a keyword, so in the real code the guard is
unreachable; it only converts a non-advancing mode into a refusal.

Proof that each former timeout is now **caught** (not merely gone), from the
run's own `caught.txt`:

```console
$ cd /tmp/opencode/mb1-sql-statements/mutants.out
$ for pat in "replace += with \*= in Parser::skip_ws" "replace += with \*= in Parser::eat_word" \
             "replace Parser::eat_keyword -> bool with true" "delete ! in Parser::parse_modes" \
             "replace Parser::parse_mode -> bool with true"; do printf '%-55s ' "$pat"; grep -c "$pat" caught.txt; done
replace += with *= in Parser::skip_ws                  1
replace += with *= in Parser::eat_word                 1
replace Parser::eat_keyword -> bool with true          1
delete ! in Parser::parse_modes                        1
replace Parser::parse_mode -> bool with true           1
```

(Attempt 1's sixth timeout, `158:19`, has no successor: the counter it mutated no
longer exists. Its replacement, `quoted_ident_end`'s `consumed += 1` /
`width += 1`, contributes 27 mutants and every one is caught.)

The parser's accepted and refused sets are byte-for-byte what they were: the
whole existing `statements::tests` module (11 tests) passes unchanged, and so do
`tests/critical_learn_policy.rs`'s transaction-control twins.

**Mutant count moved 170 → 175** because a structurally bounded scan is a
different shape: `skip_ws`/`eat_word` lost their in-loop `+=` pairs, `eat_ident`'s
index loop became `quoted_ident_end` (whose `-> Option<usize>` return adds three
`Some(…)` mutants and whose two counters add the rest). The four counts below add
to 175; `tables.rs` is unchanged at 91.

## Summary

`statements.rs` classifies a statement's kind and the transaction-control
grammar; `tables.rs` extracts the tables and columns a statement touches and
**is** the table-grant surface (`[route.db.tables]`). A survivor in the second one
under- or over-reports a table and either widens a grant or refuses a statement
the manifest allows, so every new case pairs an **allowed** statement with its
**denied** twin through `Allowlist`/`TableGrants::check` — the surface dbproxy
enforces — and never asserts only the allowed side.

**Raw counts, read the moment the run finished:**

| outcome | mutants |
|---|---|
| `caught` | **168** |
| `missed` | **0** |
| `timeout` | **0** |
| `unviable` | 7 |
| **total** | **175** |

### Per-module killed ratio (viable = caught + missed + timeout)

| Module | Mutants | caught | missed | timeout | unviable | viable | killed |
|---|---|---|---|---|---|---|---|
| `src/statements.rs` | 84 | 84 | 0 | 0 | 0 | 84 | **100.0 %** |
| `src/tables.rs` | 91 | 84 | 0 | 0 | 7 | 84 | **100.0 %** |
| **both** | **175** | **168** | **0** | **0** | **7** | **168** | **100.0 %** |

Both modules clear the 75.0 % bar with no survivors at all.

### Unviable (7) — the mutation does not build

All seven are the same thing, in `tables.rs`: `Columns` deliberately has no
`Default` (a column grant is `"*"` or a list, never "empty by default"), so every
`Default::default()` substitution fails with `E0277 the trait bound
tables::Columns: std::default::Default is not satisfied` —
`Columns::list`, `Columns::union`, `lock_update`, `serialize`, `deserialize`,
`visit_str`, `visit_seq`. The sibling `expecting -> Ok(Default::default())` *does*
build (`fmt::Result` is `Result<(), Error>`) and is caught:
`columns_serde_uses_the_two_manifest_spellings` asserts every bad value reports
`the string "*" or a list of column names` (this is the one viable mutant of the
`serde` impl, and it had no test before).

### 921 mutants measured across the crate — `MB.1.3`–`MB.1.5` close the epic

`analysis.rs` 299 + `lib.rs` 21 (baseline) + `lexer.rs` 206 + `learn.rs` 96
(`MB.1.3`) + `normalize.rs` 110 + `literals.rs` 4 + `lru.rs` 10 (`MB.1.4`) +
`statements.rs` 84 + `tables.rs` 91 (this leaf) = **921 = the crate's entire
mutant set**, so every module is now measured (`cargo mutants -p
blastit-sqlpolicy --list | wc -l` → 921).

The epic's earlier "1019" was the count *before* the loop fixes: making a
hand-stepped `i += 1` structural removes several mutants per loop (`lexer.rs`
298→206, `learn.rs` 103→96, `normalize.rs` 114→110, `statements.rs` 79→84 net of
its new scan), so the number to prove is "everything the tree has", not a
remembered constant.

## What the tests pin

`tests/policy_module_tables.rs` (new, 18 tests) drives the public surface
(`Allowlist::check`, `TableGrants::check/covering/to_toml/tables/table/functions/
locks_without_a_column/pick_locking_columns/merge_from`, `Columns`, `Denied`,
`analyze`); the private helpers are covered in the `src/*.rs` test modules.

- **The column model**: a list reaches exactly its columns and no other; `"*"`
  reaches everything and is per table; `[]` is neither `"*"` nor empty (it names
  the table).
- **Every operation is a grant on its own** (`select`/`insert`/`update`/`delete`
  each with `TableGrants::is_empty()` false), and `select = []` still is one.
- **The grant's names**: `public.t` is `t`, `audit.users` keeps its schema, a
  bare `public.`/`pg_catalog.` keeps its spelling (asserted on the map keys, not
  on a lookup, because both spellings look up the same way), `table()` answers
  canonically and `None` otherwise.
- **Reads vs writes, both ways**: a `SELECT` grant allows the read and denies
  `INSERT`, `UPDATE` and `DELETE` with the exact reason; an `INSERT`-only grant
  denies the read twin and a wider insert; `INSERT … VALUES` without a column
  list needs `"*"`.
- **The indirection is reported**: a table reached through a subquery, through a
  join alias, or as the read side of `INSERT … SELECT` is in the covering grant,
  and a grant that names only the other table is denied — the
  under-report/over-report pair the node asks for.
- **The lock rule**: `FOR UPDATE` needs `update` on one column (`update = ["id"]`,
  never `"*"`), the covering grant writes that entry and its comment, one column
  is enough for the lock but not for a real `UPDATE`, and a lock with no column
  to name is *reported* (`locks_without_a_column`, a `name one here` comment, no
  `update` invented) rather than widened.
- **A written `update = []`** renders as written, gets no "only locks its rows"
  comment, is not reported by `locks_without_a_column`, `pick_locking_columns`
  leaves it alone — and it still does not grant a lock.
- **Denials are actionable**: `reason()`, `Display` and `Into<String>` agree on
  one line naming the missing access, and a function denial names the function.
- **Merging** unions operations and absorbs a list into `"*"`.
- **The transaction grammar** through `Allowlist::new(…, true)`: `BEGIN`,
  `START TRANSACTION READ WRITE`, `COMMIT`, `ROLLBACK TO SAVEPOINT s1` and
  `SAVEPOINT "s1_x"` allowed; `BEGIN; DROP TABLE t`, `SET TIME ZONE 'UTC'`,
  `SELECT 1`, `BEGIN READ` and `ROLLBACK TO s1 extra` denied; without the flag
  `BEGIN` is denied too.

In `src/statements.rs` (test cases only): `N'…'` and `U&'…'` literals hide a `;`;
a quoted savepoint identifier continues past an escaped quote
(`SAVEPOINT "s""x"`, `SAVEPOINT """"` — the name `"`) while `SAVEPOINT "s""`,
`ROLLBACK TO s1 extra`, `ROLLBACK TO SAVEPOINT s1 extra` are refused; a mode that
parsed is not the last word (`BEGIN READ ONLY X`, `START TRANSACTION READ WRITE X`
are refused — this is what pins `parse_modes`' comma check).

In `src/tables.rs`: `key_toml` is bare only for `[A-Za-z0-9_-]+` (`a_b`,
`orders-order9`) and quotes `audit.users`, `a b` and the empty key; and the
`Columns` serde test above.

## Evidence

```console
$ df -h /tmp | tail -1
tmpfs            32G   22G   11G  68% /tmp

# 1. Crate tests green (new table/statement cases included).
$ CARGO_NET_OFFLINE=true cargo test -p blastit-sqlpolicy --locked
running 224 tests
test result: ok. 224 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.32s
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
running 25 tests
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 6 tests
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s
running 10 tests
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 18 tests
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s   # policy_module_tables.rs
running 8 tests
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

# 2. One file-scoped run (175 mutants, 10m). Same command as the node, detached
#    through systemd --user so a session restart cannot kill it (attempt 1 lost
#    three runs that way with `ERROR interrupted phase=Build`).
$ mkdir -p /tmp/opencode/blastit-mutants
$ cat /tmp/opencode/mb15b-run.sh
cd /home/remy/offworld-nexus/blastit || exit 1
export TMPDIR=/tmp/opencode/blastit-mutants
export CARGO_NET_OFFLINE=true
export PATH=/home/remy/.cargo/bin:$PATH
date -Is
exec cargo mutants -p blastit-sqlpolicy -f '**/src/{statements,tables}.rs' -j 2 --timeout 90 -o /tmp/opencode/mb1-sql-statements
$ systemd-run --user --collect --unit=mb15b-mutants \
    --property=StandardOutput=append:/tmp/opencode/mb15b-run.log \
    --property=StandardError=append:/tmp/opencode/mb15b-run.log /tmp/opencode/mb15b-run.sh
$ cat /tmp/opencode/mb15b-run.log
2026-11-13T18:17:11+01:00
Found 175 mutants to test
ok       Unmutated baseline in 27s build + 6s test
175 mutants tested in 10m: 168 caught, 7 unviable
$ for f in caught missed timeout unviable; do printf '%-9s %s\n' "$f" "$(wc -l < /tmp/opencode/mb1-sql-statements/mutants.out/$f.txt)"; done
caught    168
missed    0
timeout   0
unviable  7
$ cat mutants.out/{caught,missed,timeout,unviable}.txt | wc -l
175
$ test ! -s /tmp/opencode/mb1-sql-statements/mutants.out/timeout.txt && echo "0 timeouts"
0 timeouts

# 3. Per-module killed ratio.
$ for f in caught missed timeout unviable; do echo "== $f"; cut -d: -f1 /tmp/opencode/mb1-sql-statements/mutants.out/$f.txt | sort | uniq -c; done
== caught
     84 crates/blastit-sqlpolicy/src/statements.rs
     84 crates/blastit-sqlpolicy/src/tables.rs
== missed                       (empty)
== timeout                      (empty)
== unviable
      7 crates/blastit-sqlpolicy/src/tables.rs
# statements: 84/(84+0+0) = 100.0 %   tables: 84/(84+0+0) = 100.0 %

# 4. Survivors listed, each either tested or documented as unreachable.
$ cat /tmp/opencode/mb1-sql-statements/mutants.out/missed.txt
                                    # empty: no survivor
$ cat /tmp/opencode/mb1-sql-statements/mutants.out/timeout.txt
                                    # empty: no timeout
# the 7 unviable are listed above (Columns has no Default: the mutation cannot compile)

# 5. Green gate — see the substitution note below.
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-fmt
cargo fmt --all -- --check
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make rs-doc
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.16s
   Generated /home/remy/offworld-nexus/blastit/target/doc/blastit/index.html and 12 other files

# the measured set is the tree that ships: after the run, the only source change
# was one `use` list reordered by `make rs-fmt` (no mutant moved)
$ jq -r '.[].name' /tmp/opencode/mb1-sql-statements/mutants.out/mutants.json | sort > run-names
$ cargo mutants -p blastit-sqlpolicy -f '**/src/{statements,tables}.rs' --list | sort > now-names
$ diff run-names now-names && echo "MUTANT SET IDENTICAL to the measured run"
MUTANT SET IDENTICAL to the measured run
```

**Gate substitution (item 5).** The node asks for `make check`; per the dispatch
I ran `make rs-fmt`, `make rs-lint` and `make rs-doc` instead, because `make
check` builds a wheel into `dist/` and the lead runs the full gate. Everything
the gate would say about these files is covered: format, clippy with `-D
warnings` over `--all-targets`, and rustdoc with `-D warnings`, all green.

## Files

- `crates/blastit-sqlpolicy/tests/policy_module_tables.rs` (new, 18 tests)
- `crates/blastit-sqlpolicy/src/statements.rs` — the three structurally bounded
  scans (`skip_ws`, `eat_word`, `quoted_ident_end` behind `eat_ident`, all
  `+= 1`-in-a-loop steps gone) and `parse_modes`' no-progress guard; plus test
  cases only for the rest
- `crates/blastit-sqlpolicy/src/tables.rs` — test cases only (`key_toml`, the
  `Columns` serde test)
- `docs/knowledge/LESSONS.md` — one line: `rustfmt <file>` ≠ `cargo fmt`
- `plan/nodes/MB/MB.1/MB.1.5/RESULT.md` (this file)

## Follow-ups

1. `lexer.rs`/`learn.rs`/`normalize.rs`/`lru.rs`/`literals.rs` were being edited
   by `MB.1.3`/`MB.1.4` while this run was in flight; the campaign copied the
   tree at launch, so a *later* green test from those leaves was not part of the
   baseline. Nothing in those files can change these 175 mutants.
2. `statements.rs` has no remaining hand-stepped loop; if a future parser needs
   one, keep the same rule (the iterator is the step) or the class returns.
3. `Columns` has no `Default` on purpose, which is why 7 mutants in `tables.rs`
   can never compile. Worth a doc line on the type ("a grant is `"*"` or a list,
   never nothing") so the next reader does not add one.
