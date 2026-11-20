# RESULT — MB.56.2.4 (review fix)

Consolidated the two remaining §7 suites, then restored the assertions the
review found missing. No production logic changed: the only `src/**` edits are
the deletion of every inline `#[cfg(test)]` module (and the one test-only
`ResponseDefect::detail` helper).

| crate | `tests/` before | `tests/` after | cap |
|---|---|---|---|
| `blastit-sqlpolicy` | 5 080 LOC (7 files) | **1 492 LOC** (`sql_policy.rs` 593, `sql_grants.rs` 488, `sql_templates.rs` 411) | ≤ 1 500 |
| `blastit-http` | 3 854 LOC (8 files + `common/`) | **889 LOC** (`http_framing.rs`) | ≤ 800, **≤ 900 by the review's allowance** |

Tests: `blastit-sqlpolicy` 38 (18 + 10 + 10, plus 13 doctests), `blastit-http`
17 (plus 2 doctests). No `#[ignore]`, no skip, no `cfg(test)` left in either
crate (`grep -rn 'cfg(test)' crates/blastit-sqlpolicy crates/blastit-http` is
empty).

**Why 889 and not ≤ 800:** the file pays for a 300-line harness once (scripted
scheduler on a real `UnixStream::pair`, a generic client, the socketpair front
end) and then 17 tests; the review's 12 new cases cost ~200 lines. I merged six
groups to buy room — declared+chunked 413, vanish+idle cancellation,
CL/TE/duplicate-CL/TE-version refusals, upgrade+CONNECT+target refusals,
431+408-head, version+bare-LF — and dropped every response-encoding case. What
is left is the 13 promises the review named plus the four `body` policies and
the framing basics; nothing below 800 lines could keep them.

## Case map — `blastit-sqlpolicy`

| old (deleted) | new |
|---|---|
| `critical_policy_surface.rs` | `sql_policy.rs`: exact/reformatted matching, `IN` F1 escape + 8-length lists, dot guard, `(id)IN`, `VALUES` rows + separator, transaction grammar, column/star/read-write/insert-select/subquery/join/merge/lock extents, transaction grammar | 
| `critical_literals.rs` | `sql_policy.rs`: quoted-form boundaries, learn refusals, `COPY … PROGRAM`, dangerous functions (incl. `CALL` + 3-part), catalogue setup; `sql_templates.rs`: `literal_offset` |
| `critical_learn_policy.rs` | `sql_policy.rs`: learn verdicts, session-setup allow/deny; `sql_grants.rs`: group merge + function grant, `Allowlist::with_tables` denial text |
| `critical_query_template.rs` | `sql_templates.rs`: rewrite rows (15 → 10), separators/statements, markers in strings, qualified `in`, refusals + offsets, char-vs-byte offsets, caps, no-print, `parameterize` (canonical/idempotent/cap/never-widening) |
| `critical_lexer_boundaries.rs` | `sql_policy.rs`: quoted forms, placeholder/number extents, comments (nest/unbalanced), 200 KB hostile text, `$?`-rewrite in code only |
| `policy_module_tables.rs`, `policy_module_edges.rs` | `sql_grants.rs` / `sql_policy.rs`: table keys, covering directives, lock rendering, denial naming, merge, shape rules, `KNOWN_STATIC_SETUP_SQL` exactness — one assertion per promise, duplicates dropped |
| `src/analysis/tests.rs` | **new** `a_statement_a_table_grant_cannot_cover_is_refused_by_name` (18 statements × exact sentence, plus the runtime `Decision::Denied` with the same reason) and `the_analysis_reports_exactly_the_accesses…` |
| — | **new** `cte_writes_and_unions_report_every_extent`, `delete_using_and_for_update_of_report_their_extents` (exact `Access` lists, then the covering grant), `a_refusal_never_renders_a_literal_a_writer_built_by_hand` |

Restored from the review, with the facts the production text actually gives:

* `REVOKE …` reports **`GRANT`**: pg_query 16 lowers both to one `GrantStmt`
  (`statement_label`), so that row asserts `a table grant cannot cover GRANT…`.
* `MERGE …` is **refused by name** (`MERGE: only SELECT, INSERT, UPDATE and
  DELETE can be`), not analysed — it is a row of the same table, not an extent
  test.
* `INSERT … SELECT` reports `["Select:b:id", "Insert:a:id"]`; `WITH d AS
  (DELETE … RETURNING id)` reports `["Select:t:id", "Delete:t:*"]`; `UPDATE …
  RETURNING *` reports `["Select:t:*", "Select:t:b", "Update:t:a"]`; `UNION`
  reports both arms; `DELETE … USING` reports `["Select:u:*", "Delete:t:*"]`;
  `FOR UPDATE OF t` reports `["Select:t:*", "Select:u:*", "Update:t:*"]` and a
  derived `OF s` locks the table the alias reads.
* The old vacuous oracle is gone: each "the grant covers it" now pins the exact
  access list first. A covering grant for a **lock-only** statement is refused
  rather than widened (the `update` entry cannot be invented), which the test
  now asserts.

Still dropped (deliberate, each covered elsewhere or a rendering detail):
exhaustive short-input/permutation loops (idempotence only), `lru` eviction
order, per-token numeric *index* asserts (their observable effect is kept
through `normalize`, per `MB.1.14`), serde/`Display` of each struct, the exact
`to_blastfile` prose for `update = []` (the refusal is asserted), the
`public.`/`pg_catalog.` empty-rest spelling, the per-operation `is_empty` loop,
the `U&'a''b'` redaction spelling (still via `literal_offset` in
`sql_templates.rs`), and the double-space `SET TIME ZONE` redaction row.

## Case map — `blastit-http`

| old (deleted) | new |
|---|---|
| `bodies.rs` | `a_chunked_body_is_dechunked_and_its_hop_headers_dropped`, `a_body_crosses_before_all_of_it_has_arrived` (**new**), `a_declared_or_chunked_body_over_the_limit_is_413`, `a_client_that_goes_away_or_quiet_mid_body_cancels_the_stream`, `a_body_that_cannot_finish_in_body_timeout_is_408_and_cancelled` (**new**), `every_body_policy_crosses_the_framing_layer_verbatim`, `a_content_length_of_zero_reads_nothing_and_sends_no_interim`, `expect_100_continue_is_answered_before_the_body_is_read` (**new**) |
| `rejections.rs` | `a_head_over_the_limit_is_431_and_a_stalled_head_is_408` (**new**), `an_expectation_other_than_100_continue_is_417` (**new**), `request_framing_headers_are_refused`, `an_upgrade_a_connect_or_a_non_origin_target_is_refused` (**new** targets + `OPTIONS *`), `http_2_and_a_bare_lf_in_the_head_are_refused`, `http_1_1_needs_exactly_one_usable_host` |
| `limits.rs` | `connections_over_the_cap_are_answered_with_503` (**new**, real loopback `TcpListener` through `Server::run` — the crate's only acceptor is `TcpAcceptor`, so the "real listener" leg is a real TCP socket, not a `UnixListener`), the idle-bound half of the cancellation test |
| `critical_early_abort.rs` | the early 413 for a declared-over-cap body (10 MiB/chunked scripted variants were the same refusal) |
| `channel.rs` | `a_rejection_event_carries_no_header_a_target_or_a_body` (**new**: `encode_json` of the refusal must contain none of `secret`/`token`/`abc`/`hunter2`/`Authorization`/`content-length`/`chunked`) |
| `roundtrip.rs` | pipelining + response framing; the rest dropped |

Dropped from this crate's suite (each named, with why): response *encoding*
shapes (HEAD/204/304/1xx framing, response hop-by-hop stripping, bad
view-header/reason → 502, `RespAbort` before/after the head, a second head, an
unknown status, the empty-reason table), HTTP/1.0 close-delimited bodies, the
502-on-dead-scheduler and channel-death cases, the per-stream buffer/backpressure
and credit-window suite (`flowcontrol.rs`, §3.1 fairness), trusted-proxy
`X-Forwarded-For`/`-Proto` resolution (`forwarding.rs`, D5 — covered by the
scheduler), the `serve()`/`TcpAcceptor` wrapper-shape test, and the
chunked-incremental duplicate of `a_body_crosses_before_all_of_it_has_arrived`.
None of them is a §7 request-framing promise, and all are exercised by the
scheduler/e2e suites.

## Verification

`cargo test -p blastit-sqlpolicy -p blastit-http --locked`:

```
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (http_framing)
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (sql_policy)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (sql_grants)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (sql_templates)
test result: ok. 2 passed  / 13 passed                                       (doctests)
```

`make fast-check`: green (guard, `cargo fmt --all -- --check`, clippy
`--all-targets -D warnings`, rustdoc `-D warnings`, workspace tests, py-lint,
plan-validate, universal-smoke, install-test, docs-unit).
