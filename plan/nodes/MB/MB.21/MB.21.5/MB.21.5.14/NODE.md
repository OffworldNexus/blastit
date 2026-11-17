---
id: MB.21.5.14
title: "Close the surgical editor's sixteen testable survivors (names, cross-view patches, one-line empty blocks, transactions bounding a directive)"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-manifest/src/blastfile/write.rs"
  - "crates/blastit-manifest/tests/edit_branches.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T04:10"
updated: "2026-11-17T04:55"
---

# Goal

Close the editor's surviving mutants. `MB.21.5.3` measured `write.rs` at **89.8 %** (194 caught / 216
viable, 21 missed, 1 timeout, 10 unviable) against the editor's own suites (`--lib` plus
`edit_branches`, `critical_manifest_edit`, `critical_manifest_edit_executor`). **Sixteen are testable;
five are equivalent.**

| Mutant | Why it survived | Case that kills it |
|---|---|---|
| `write.rs:333:60,68,76,84` in `valid_view_name` | every proposed view name in the tests is a bare word (`orders`, `products`): no digit, `_` or `-` | `apply_edits` accepts `ProposedView { name: "orders_v2" }` and `"api-2"` and writes `view orders_v2 …`; `"Orders"`/`"_x"` stay refused |
| `write.rs:345:62,70,78` in `valid_method` | the tests only propose `GET`/`POST` | a proposal with `X-TRACE` (and `M-SEARCH`) is accepted and written; `get`/`` stays refused |
| `write.rs:738:42` `&&`→`\|\|` in `merge_patch` | no single call patches two *different* views | one call with a grant for `products` **and** one for `catalogue` writes each into its own block |
| `write.rs:839:24,63` in `writable_word` | table/column names in the tests are `id`, `name` | `select order_items(created_at)` is written (inner `_`), and a name starting with `-` is refused |
| `write.rs:953:22` `==`→`!=` in `insertion_at` | no one-line *empty grant block*: the existing cases are multi-line or a one-line view | `postgres { }` on one line gains `query '…'` with one space (no leading `;`) |
| `write.rs:1120:22,27` in `table_splice` | same shape | the same one-line empty block gains `select t(id)` without a leading `;` |
| `write.rs:1190:37` `!=`→`==` in `directive_spans` | no tested block writes a directive **before** `transactions`; the `!=` is what keeps `transactions` bounding the preceding directive, so a splice between them loses the grant and P4 then refuses a valid edit | a `select t(id)` line followed by `transactions` in one block, then a table-scope proposal for that block: the edited text keeps `transactions` **after** the directive, still binds the grant, and `apply_edits` returns `Ok` |
| `write.rs:1237:9` `&&`→`\|\|` in `statement_splice` | no line holds two statements | `postgres { select a; select b }` keeps `select a` when `select b`'s scope is rewritten |
| `write.rs:1340:38` delete `!` in `block_is_empty` | same one-line shapes | the one-line empty block above still gains exactly one statement |

Equivalent, do not test: `write.rs:1214:16` (`end == start` needs a keyword at the closing brace, which the grammar cannot produce), `write.rs:1395:14` (the loop already breaks at `at == 0`), `write.rs:1411:5` (`blank_line_at` is false at every insertion point, so the guard is dead), and `write.rs:474:9` (`Plan::is_noop`→`false`, the no-op path renders to the same
bytes and passes the same postcondition), `write.rs:679:23` (`>`→`>=`: `previous_line_blank(.., 0)` is
`true` by definition, so the `at == 0` branch is unchanged).

# Context

- Audit: `docs/testing/AUDIT-MB.21.md` §2.4 and §3.4. Design rows: §14.5 (the editor's seven rules),
  §6.7 (names), §13.1 (grant spellings).
- `--learn` infers names from paths, so a digit/`_`/`-` in a view name is the normal case, not an edge;
  and real SQL columns are `snake_case`, so `created_at` is the *common* shape the current oracle misses.
- The one-line empty block is what a person writes by hand (`postgres { }`), so the splice must not
  produce `postgres { ; query … }`.

# Acceptance

```console
$ cargo test -p blastit-manifest --test edit_branches
$ cargo test -p blastit-manifest --test critical_manifest_edit
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb2162 MUTANTS_JOBS=4 \
  scripts/mutants-run.sh -p blastit-manifest -f crates/blastit-manifest/src/blastfile/write.rs \
  -- --lib --test edit_branches --test critical_manifest_edit --test critical_manifest_edit_executor
```

1. The sixteen mutants above are caught, and `write.rs`'s ratio rises above 96 %. The
   `write.rs:1190:37` fixture is a table-scope splice on a block whose `select …` line sits **before**
   `transactions`: the edited text must keep `transactions` and the grant, and P4 must not refuse it.
2. Every new case round-trips through `Manifest::parse_blastfile` (no edit may produce a file that does
   not boot), like the existing ones.
3. The five equivalent mutants stay unkilled and are named as such in `RESULT.md`.

# Notes

- The scoped `cargo test` targets are the editor's only callers; keep them so the campaign stays
  under an hour on a loaded host (the audit's full-suite attempt needed 4 h and was lost twice).
