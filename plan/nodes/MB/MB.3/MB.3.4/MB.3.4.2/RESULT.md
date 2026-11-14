# Result MB.3.4.2

STATUS: done

## What landed

- `crates/blastit-fastcgi/src/lockdown.rs` (new): `FastCgiPaths`, `RequestFacts`,
  `Scheme`, `LockdownError`, `Params`, `resolve` (closed allowlist) and `build`
  (pinned/derived params + `HTTP_*` derivation + budgets).
- `crates/blastit-fastcgi/src/lib.rs`: `pub mod lockdown;` plus the module bullet.
- `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` (new, protected):
  25 binary tests, one per owned §4.7 row plus the positive exact-stream twin.

No `Cargo.toml`/`Cargo.lock`, no socket, no I/O, no `unsafe`, zero new deps;
`#![forbid(unsafe_code)]` intact. `src/lockdown.rs` contains no `Refusal`.

## Row-by-row (§4.7 → test fn)

| Row | Test fn in `critical_fastcgi_lockdown.rs` |
|---|---|
| 1 | `script_filename_is_pinned_ignoring_path_and_header` |
| 2 (lockdown half) | `path_info_traversal_never_feeds_script_filename` — authoritative `400` oracle stays `conformance_static::malformed_path` |
| 3 | `script_filename_never_absorbs_query_string` |
| 5 | `php_value_never_emitted_from_any_input` |
| 6 | `php_admin_value_never_emitted_from_any_input` |
| 7 | `auto_prepend_directives_never_emitted` |
| 8 | `query_string_dash_d_stays_verbatim_query_data` |
| 9 | `soft_hyphen_probe_stays_verbatim_query_data` |
| 10 | `best_fit_encoding_probes_never_become_params` |
| 11 | `redirect_status_is_never_emitted` |
| 12 | `document_root_is_pinned_ignoring_headers` |
| 13 | `proxy_header_never_produces_http_proxy` |
| 14 | `colliding_http_headers_keep_first_and_record_drop` |
| 15 (lockdown half) | `nul_in_header_value_is_malformed` — head-refusal owner stays `critical_head_refusals::nul_byte_in_header_or_target_is_400` |
| 16 (lockdown half) | `unrepresentable_header_name_is_dropped` — head-refusal owner stays `critical_head_refusals::header_name_over_max_header_bytes_is_431` |
| 17 | `over_budget_param_count_or_bytes_is_params_too_large` (also the exact 128 / 65 531 / 65 535 boundaries and the 127/128 length-prefix boundary) |
| 18 | `unknown_bare_param_is_refused_500` |
| 23 | `dotdot_without_static_keeps_script_filename_pinned` |
| §4.6.2 value > 8192 | `over_long_header_value_is_dropped_not_refused` |
| §4.6.2 over-long `CONTENT_TYPE` | `over_long_content_type_is_dropped_not_refused` |
| §4.6.2 duplicate builder guard | `lockdown::tests::insert_rejects_duplicates_and_malformed_names` (crate-private insertion point) |
| §4.6.1 reserved-name guard | `lockdown::tests::insert_refuses_reserved_names_even_inside_the_crate` |
| §4.6.2 insertion budgets | `lockdown::tests::insert_enforces_the_pair_total_and_count_budgets` |
| positive twin | `documented_index_request_serialises_to_the_exact_param_stream` |
| `HTTP_*` derivation | `http_star_derivation_folds_names_and_skips_content_headers` |
| one decode | `path_info_is_decoded_exactly_once` |
| builder path/fact guards | `malformed_pinned_paths_are_refused`, `malformed_request_facts_are_refused` |

Rows 4, 19–22, 24, 25 are other leaves' (dispatch / head refusals); no row was
dropped. Every row asserts a typed `LockdownError::*`, never `Refusal`.

## Review resolution (attempt 2)

| Finding | Fix | Proof |
|---|---|---|
| 1. `Params::insert` was `pub` and accepted/emitted `PHP_VALUE`, `HTTP_PROXY`, `SCRIPT_FILENAME=/etc/passwd`, falsifying "`build` is the only producer" | `insert` is now `pub(crate)`; it also refuses the §4.6.1 "not emitted" names (`PHP_VALUE`, `PHP_ADMIN_*`, `auto_prepend_file`, `allow_url_include`, `open_basedir`, `disable_functions`, `REDIRECT_STATUS`, `HTTP_PROXY`, `FCGI_ROLE`, `PATH_TRANSLATED`, `SERVER_ADDR`) with `LockdownError::UnknownParam`. Raw-insert budget/duplicate assertions moved into `lockdown::tests` (the public critical suite now drives only `build`/`resolve`). | `lockdown::tests::insert_refuses_reserved_names_even_inside_the_crate`; `insert_rejects_duplicates_and_malformed_names`; `insert_enforces_the_pair_total_and_count_budgets`; `grep -n 'pub(crate) fn insert'` |
| 2. `CONTENT_TYPE` from `facts.content_type` bypassed the §4.6.2 ≤ 8192-byte value rule (a 9000-byte CT was emitted) | `build` now applies the same rule as `HTTP_*`: an over-long `CONTENT_TYPE` is dropped and recorded in `Params::dropped`, never refused. `CONTENT_LENGTH` is a decimal `u64` (≤ 20 bytes), so the rule cannot bite; a `u64::MAX` value is asserted emitted. | `over_long_content_type_is_dropped_not_refused` (8 192 emitted, 8 193 dropped + recorded, `u64::MAX` length emitted) |
| 3. The positive twin pinned only the encoded prefix + length | The twin now asserts the **full** expected `encode()` byte string, pair by pair as explicit length-prefix/name/value literals (`SERVER_SOFTWARE` splices the crate version). A reordering or an extra param fails. | `documented_index_request_serialises_to_the_exact_param_stream` (`assert_eq!(encoded, want)`) |

## Acceptance evidence

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
test unrepresentable_header_name_is_dropped ... ok
test proxy_header_never_produces_http_proxy ... ok
test over_budget_param_count_or_bytes_is_params_too_large ... ok

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

$ cargo test -p blastit-fastcgi --locked
     Running unittests src/lib.rs
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_fastcgi_lockdown.rs
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ grep -q 'pub mod lockdown;' crates/blastit-fastcgi/src/lib.rs && echo "lockdown module is wired"
lockdown module is wired

$ grep -c 'LockdownError' crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs
15

$ ! grep -n 'Refusal' crates/blastit-fastcgi/src/lockdown.rs && echo "status mapping is not this crate's"
status mapping is not this crate's

$ make check
57 fences: 30 executed, 27 accounted, 0 failed · 16.8s
uv run --no-project tools/plan.py validate
plan OK
EXIT:0
```

`make check` ran `guard`, `critical-guard`, `rs-fmt`, `rs-lint`, `rs-doc`,
`rs-test`, `test-critical`, `py-lint`, `docs-unit`, `docs-check`, `docs-test`
and `plan-validate`; the log tail is `plan OK` / `EXIT:0`.

## Budget

The node targets ≤ 550 changed lines; this leaf is **1 322 lines**
(`src/lockdown.rs` 706 incl. the crate-private insertion-point tests,
`tests/critical_fastcgi_lockdown.rs` 616, `src/lib.rs` 4). The overage is the
18-name closed allowlist plus 25 binary rows (exact expected stream,
128/65 531/65 535 boundaries, 8 192-byte drop, one-decode probes). Every row
landed; none was dropped, per the node's "never drop a row" rule.

## Decisions recorded

- `Content-Type`/`Content-Length` headers are **not** folded into
  `HTTP_CONTENT_TYPE`/`HTTP_CONTENT_LENGTH`; they are the
  `CONTENT_TYPE`/`CONTENT_LENGTH` params (RFC 3875 / nginx). A spoofed header
  cannot move either param (tested).
- A value byte NUL/CR/LF is `Malformed` (defense in depth; the front end
  refuses it as `400` first). An unrepresentable header name is dropped and
  recorded, never refused.
- `HTTPS` is `on`/`off` (never omitted), so `REQUEST_SCHEME`/`HTTPS` stay
  deterministic for every scheme.

## Follow-ups

- `MB.3.6.2.1` must map `LockdownError::{UnknownParam → 500, ParamsTooLarge |
  DuplicateParam → 400, Malformed → 400}` and replay `Params::dropped()` as
  `Observation::Param { decision: Dropped }`; `Params::encode()` produces the
  one `PARAMS` record content.
