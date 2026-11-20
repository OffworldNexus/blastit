# The `php2shell` refusal matrix this driver replays

Provenance: this is §4.7 of `docs/design/php-fastcgi.md` **verbatim** — the design
of record when the matrix was accepted — copied here because `MB.3.32` deleted
that design corpus (18 files) when `ADR-0010` replaced `php-fpm` with embedded
`libphp`. The `E` column, the `php2shell #` column (`docs/research/php2shell.md`
§B, `#1`–`#20`) and the row numbering are unchanged: they are what
`e2e/php2shell.py --self-test` checks `PROBES` against, so a `PROBES` row that
drifts from the accepted matrix fails the run.

The three columns that no longer describe the implementation (`Blastit handling`,
`Outcome`, `Owner file::fn`) are kept for the record: `ADR-0010` deleted the
FastCGI param channel they were written about, so the driver's rows assert the
embedded-era observable that replaced each one. Read it as the threat list, not
as a claim about `blastit-php-sapi`.

### 4.7 The binary refusal matrix

**R8 single-owner rule, as it stands after `ADR-0010`:** the authoritative
executable witness of every row is the `e2e/php2shell.py` probe named in its `E`
column (`E1`–`E17`, `E19`–`E25`: 24 client-reachable probes; row 18 is an internal
builder-API `500` guard with no client HTTP path and is marked `—`, excluded from
`PROBES`). The `Owner` column is **historical**: it names the FastCGI-era Rust
test that asserted the same observable, and that suite
(`critical_fastcgi_lockdown`) was deleted with the FastCGI param channel
(`MB.3.17`, swept again by `MB.56`). No `critical_fastcgi_lockdown::` test exists
today — a row's owner is its probe, and `--self-test` keeps the matrix and
`PROBES` in step.

| # | E | Attack (`php2shell` #) | Bytes where | Blastit handling | Outcome | Design-era test (deleted) | P |
|---|---|---|---|---|---|---|---|
| 1 | E1 | `SCRIPT_FILENAME` override (1) | path `/app/elsewhere/evil`; header `SCRIPT_FILENAME` | path → `PATH_INFO`/`REQUEST_URI`; param pinned | `200` (`script_filename` == entry) | `critical_fastcgi_lockdown::script_filename_is_pinned_ignoring_path_and_header` | P1 |
| 2 | E2 | `SCRIPT_FILENAME` traversal (2, 5) | `../`, `/%2e%2e/`, `..` after script | static head gate → `400`; param pinned | `400` | `conformance_static::malformed_path` (dispatch-level 400; `critical_static_resolve` asserts `StaticOutcome`, not HTTP codes) | P1 |
| 3 | E3 | `SCRIPT_FILENAME?query` (3) | `?` + entry-like segment in path | pinned value has no `?`; query separate | `200` (`script_filename` has no `?`) | `critical_fastcgi_lockdown::script_filename_never_absorbs_query_string` | P1 |
| 4 | E4 | `PATH_INFO` folded (4) | `/uploads/shell.jpg/x.php` | `.php` under root → `EntryScript` `404` | `404` | `critical_fastcgi_dispatch::php_path_under_root_is_404_entry_script` | P2 |
| 5 | E5 | `PHP_VALUE` (6, 7, 9, 10, 11) | header/query/path naming `PHP_VALUE` | not in allowlist; `-d` is query data | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::php_value_never_emitted_from_any_input` | P3 |
| 6 | E6 | `PHP_ADMIN_VALUE` (8) | header/query/path naming `PHP_ADMIN_*` | not in allowlist | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::php_admin_value_never_emitted_from_any_input` | P3 |
| 7 | E7 | `auto_prepend_file` chain (10) | query + body `<?php …` | no `PHP_VALUE`; pool pins `php_admin_value` | `200` (body is data, canary absent) | `critical_fastcgi_lockdown::auto_prepend_directives_never_emitted` | P4 |
| 8 | E8 | CVE-2012-1823 (12) | `?-d+allow_url_include=1…` | no `php-cgi`/argv; query verbatim | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::query_string_dash_d_stays_verbatim_query_data` | P5 |
| 9 | E9 | CVE-2024-4577 (13) | `?%ADd+allow_url_include%3d1` | one decode for `PATH_INFO`; query verbatim | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::soft_hyphen_probe_stays_verbatim_query_data` | P6 |
| 10 | E10 | CVE-2024-8926 (13) | best-fit/encoding query variants | same control | `200` (`allow_url_include` == `"0"`) | `critical_fastcgi_lockdown::best_fit_encoding_probes_never_become_params` | P6 |
| 11 | E11 | `REDIRECT_STATUS` / CVE-2024-8927 (15) | header `Redirect-Status: 200` / query; `cgi.force_redirect` collision | bare `REDIRECT_STATUS` never emitted; `cgi.force_redirect` untouched | `200` (`"REDIRECT_STATUS"` ∉ `server_keys`) | `critical_fastcgi_lockdown::redirect_status_is_never_emitted` | P9 |
| 12 | E12 | `DOCUMENT_ROOT` spoof (16) | header `Document-Root` / query | `DOCUMENT_ROOT` pinned; `.user.ini` disabled | `200` (`document_root` == pinned) | `critical_fastcgi_lockdown::document_root_is_pinned_ignoring_headers` | P8 |
| 13 | E13 | `Proxy:` → `HTTP_PROXY` (14) | header `Proxy: http://evil` | `Proxy` dropped; `HTTP_PROXY` banned | `200` (`http_proxy` == `false`) | `critical_fastcgi_lockdown::proxy_header_never_produces_http_proxy` | P7 |
| 14 | E14 | duplicate param (17) | `X-Foo: 1` + `X_Foo: 2` / two `X-Foo` | first wins, later `Param { Dropped }` | `200` (first value wins) | `critical_fastcgi_lockdown::colliding_http_headers_keep_first_and_record_drop` | P10 |
| 15 | E15 | NUL in a value (18) | `%00` in path / raw NUL in header | http refuses NUL (`400`) | `400` | `critical_head_refusals::nul_byte_in_header_or_target_is_400` | P10 |
| 16 | E16 | over-long header name | `65 KiB` header name (`> 64 KiB` limit) | http head limit | `431` | `critical_head_refusals::header_name_over_max_header_bytes_is_431` | P10 |
| 17 | E17 | over-budget params (20) | `> 65 535` param bytes, or `> 128` params total — i.e. `> 110` `HTTP_*` headers with a body (18 fixed params incl. `CONTENT_TYPE`/`CONTENT_LENGTH`) and `> 112` without | `Refused { 400, ParamsTooLarge }` before connect | `400` | `critical_fastcgi_lockdown::over_budget_param_count_or_bytes_is_params_too_large` (asserts `LockdownError::ParamsTooLarge`); `conformance_fastcgi::params_too_large` is the 400 oracle | P10 |
| 18 | — | unknown bare param (19) | internal map check with bare `"FOO_BAR"` (no client HTTP path) | closed non-`HTTP_` allowlist → `LockdownError::UnknownParam` | `500` at the adapter (`Err(Refusal::UnknownParam)`) | `critical_fastcgi_lockdown::unknown_bare_param_is_refused_500` (asserts `LockdownError::UnknownParam`) | — |
| 19 | E19 | `.php` source disclosure | `GET /index.php` (and `POST /index.php`) | `Refusal::EntryScript` (`404`) before body gate | `404` (`no-store`, zero source bytes) | `conformance_static::entry_script_refused_404` | P11 |
| 20 | E20 | `.phar`/polyglot (4) | `/shell.jpg/x.php`, `/x.phar` | `.php`/`.phar` gate + `limit_extensions=.php` | `404` | `critical_fastcgi_dispatch::phar_and_polyglot_suffix_are_404` | P2 |
| 21 | E21 | header injection into param | `Cookie: a\r\nPHP_VALUE: …` | http refuses bare CR/LF/injection (`400`) | `400` | `critical_head_refusals::crlf_header_injection_is_400` | P10 |
| 22 | E22 | request smuggling | `CL`+`TE`, duplicate `Content-Length` | http refuses before scheduler (`400`) | `400` | `critical_head_refusals::cl_plus_te_and_duplicate_cl_are_400` | P12 |
| 23 | E23 | `..` with no `[route.static]` | `/../../etc/passwd` on FastCGI-only route | `PATH_INFO` data; `SCRIPT_FILENAME` pinned | `200` (`script_filename` == entry) | `critical_fastcgi_lockdown::dotdot_without_static_keeps_script_filename_pinned` | P1 |
| 24 | E24 | malformed / proxy-control CGI head | no colon/CRLF/1000 headers/`1xx`/non-token header name/`X-Accel-Redirect`/`X-Sendfile` | `Fault` before head → `502` | `502` | `critical_fastcgi_dispatch::malformed_or_proxy_control_cgi_head_is_502` | P12 |
| 25 | E25 | slow-loris / request timeout | declared `Content-Length`, stalled body/peer past `request_timeout` | timeout → `504` before head, `ABORT_REQUEST` sent, close, slot released | `504` + `ABORT_REQUEST` on peer | `critical_fastcgi_dispatch::request_timeout_answers_504_sends_abort_and_releases_slot` | P10 |

Rows 1, 3, 11, 12, 13, 14, 23 also assert via the e2e `/__inspect` fixture (§7.3,
including `server_keys` = `array_keys($_SERVER)`). “`200`” rows assert a non-empty
discriminating key-value subset from `/__inspect` (enforced by `e2e/php2shell.py
--self-test`), so a harness that forwarded the attacker's param fails.

### 4.8 Pool workers

