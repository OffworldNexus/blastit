# AUDIT MB.3.4 — the pure `blastit-fastcgi` crate (epic gate)

- **Date:** 2026-11-14
- **Scope:** `crates/blastit-fastcgi/**` — `src/{lib,record,param,cgi,lockdown}.rs`,
  `tests/critical_fastcgi_lockdown.rs`, `Cargo.toml`. Leaves `MB.3.4.1` (`872661d`) and
  `MB.3.4.2` (`0f441af`).
- **Base / tip read:** `0f441af` (epic head; `git status` clean apart from the `lead`'s own
  uncommitted `plan/` lines).
- **Requirements:** `docs/design/php-fastcgi.md` §4.5.1–§4.5.2 (codec vs scheduler client),
  §4.6.1–§4.6.3 (param contract), §4.7 (binary refusal matrix), §7.1 (tiers), §7.2
  (`R1`–`R20` → enforcing module → suite); `ADR-0008`; skill `testing` §1–§4, skill
  `security-invariants` §2 (headers & FastCGI param smuggling), §5 (never leak, never panic).
- **Not audited:** every §4.7 row this crate does not own (rows 2, 4, 15, 16, 19–22, 24, 25 —
  their owners are `blastit-http`, `blastit-scheduler`, `blastit-manifest`), `MB.3.5`/`MB.3.6`/
  `MB.9` suites (their nodes are `ready`, the files do not exist yet), `e2e/` (`MB.3.7`).

## Verdict

**PASS on tier, teeth and the mutation bar; 9 surviving mutants, 3 of them equivalent, and the
6 real ones plus the two `MB.3.4.2` MINORs are filed as three tasks. No BLOCKER.**

- **Tier:** correct. The 25-test `critical_fastcgi_lockdown.rs` is a protected
  `crates/*/tests/critical_*.rs` file, runs in `make test-critical` (the `rs-test-critical` glob
  picks it up), contains no `#[ignore]`, no `should_panic`, no `needs_root`, and no weakened
  assertion. The 41 in-module tests are the design's sanctioned "unit (pure)" tier (§7.1).
- **Teeth:** every §4.7 row asserts the denied path *and* its accepted twin, and the positive
  twin pins the **full** encoded byte string (`assert_eq!(encoded, want)`), so a builder that
  emitted nothing, reordered, or added a param fails. Two individual tests are positive-only
  (named in §"Tier & teeth"), and both are paired inside their own file.
- **Mutation:** **321 killed / 330 viable = 97.3 %** (0 timeouts, 18 unviable), against the
  75 % bar. Per file: `record.rs` 100 %, `cgi.rs` 98.8 %, `lockdown.rs` 97.3 %, `param.rs`
  96.3 %, `lib.rs` 0 % (2 survivors, both `Display`/`source`). After the 3 equivalences the
  ratio is **321/327 = 98.2 %**.
- **Two of the survivors look harmless and are not:** a `Status:` line accepts a `:` as a digit
  (`Status: 2:0` → `300`), and `SCRIPT_NAME` accepts a `?` (`/index.php?x` is emitted). Both were
  reproduced against the mutated crate (§"Empirical spot-checks") and both are one-line test gaps
  in the protected suite. Plus two branch-inspection findings cargo-mutants never generated a
  mutant for.

## Method

1. Read both children's `NODE.md`/`RESULT.md`/`LOG.md`, the epic node, `docs/design/php-fastcgi.md`
   §4.5–§4.7 and §7.1–§7.2, `tools/critical_guard.py`, the `Makefile` `mutants-rust` /
   `rs-test-critical` / `test-critical` / `critical-skip-check` targets, and every line of the
   five source files and the protected suite.
2. Ran the baseline at the tip: **41 passed** (`src/lib.rs` unittests), **25 passed**
   (`critical_fastcgi_lockdown`), **1 passed** (doc-test), 0 ignored, 0 failed.
3. `make mutants-rust PKG=blastit-fastcgi` with a **private** scratch
   (`MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-testcurator`, `MUTANTS_JOBS=4`), so the
   target's own prune could not touch another campaign's copies (LESSONS #26, #29). Exactly one
   command, one checkout, no concurrent campaign.
4. Recomputed the ratio from `mutants.out/{caught,missed,timeout,unviable}.txt` (LESSONS #25:
   27.1.0 writes no `outcomes.txt`), then read every survivor's `mutants.out/diff` and
   `mutants.out/log` entry and classified it by hand.
5. Branch-inspected the two paths cargo-mutants never mutates (the `head_end` early return, the
   `is_hop_by_hop` table) and the `resolve`/`build` split.
6. Reproduced the three behavioural claims that a diff alone cannot settle, against a throwaway
   copy of the crate under `/tmp` (never the workspace): the `head_end` early return, the
   `parse_status` `':'` hole, and the `script_name_bytes` `?` hole (§"Empirical spot-checks").

## Inventory & classification

| Path | Tests | Tier | Class | Verdict |
|---|---|---|---|---|
| `tests/critical_fastcgi_lockdown.rs` (`MB.3.4.2`) | 25 | critical (protected) | **critical** | right tier: one test per owned §4.7 row, the exact-stream positive twin, the 128 boundary, the 8 192-byte drop, one-decode |
| `src/lockdown.rs` inline (`MB.3.4.2`) | 3 | standard (in-module) | **useful** ×2, **misplaced** ×1 | `insert_enforces_the_pair_total_and_count_budgets` owns R19's 65 535 **total** cap — a protected requirement — from an unprotected file (accepted MINOR (a)) |
| `src/cgi.rs` inline (`MB.3.4.1`) | 17 | standard (in-module) | **useful** ×16, **misplaced** ×1 | `the_four_proxy_control_names_are_refused` is the **only** oracle for the sendfile-escape wall (R8 row 24 codec half) and it is unprotected; the design's protected owner `critical_fastcgi_dispatch` (`MB.3.6.2.2`) does not exist yet |
| `src/record.rs` inline (`MB.3.4.1`) | 14 | standard (in-module) | **useful** | right tier per §7.1; `over_long_content_is_refused`, `the_decoder_is_total_on_every_prefix`, `a_non_v1_header_is_refused`, `end_request_refuses_an_incomplete_protocol_status` carry the over-read / bad-`protocolStatus` wall |
| `src/param.rs` inline (`MB.3.4.1`) | 7 | standard (in-module) | **useful** | 1-byte/4-byte forms, terminator, truncation, 31-bit ceiling, log-safe `Debug` |
| `src/lib.rs` doc-test | 1 | docs | **useful** | the only runnable example; it exercises `encode_pair` + `encode` + `Record::decode` + `Decoder` end to end |
| `src/lib.rs` | 0 inline | — | — | `Error`/`Display`/`source`/`From` are the crate's two survivors (no test formats `Error`) |

**No test in scope is `fluff`.** Nothing asserts a constructor, a serde default or a getter in
isolation; the three in-module `lockdown` tests drive real boundaries (65 531 / 65 535 / 128 /
127-vs-128 length prefix). **No test is skipped or `#[ignore]`d** (`grep` for
`#[ignore]|should_panic|needs_root` over the crate returns nothing; `critical-skip-check` would
fail the build if one appeared).

Two tests are **`misplaced`**: they enforce a protected invariant from an unprotected file. That
is not a defect in the tests — it is a defect in *where* they live, and it is exactly the
`MB.3.4.2` MINOR (a) the `lead` already accepted.

## Tier & teeth (anti-fluff, `testing` §3)

For each file: the assertion that would still pass under the permissive answer, then what gives
it teeth.

| File | Passes under the permissive answer | Teeth |
|---|---|---|
| `critical_fastcgi_lockdown.rs` | nothing as a whole. Individually, `path_info_traversal_never_feeds_script_filename` would pass if `PATH_INFO` were always empty (it asserts the pinned `SCRIPT_FILENAME`, not `PATH_INFO`) — but `dotdot_without_static_keeps_script_filename_pinned` and `script_filename_is_pinned_ignoring_path_and_header` assert `PATH_INFO` verbatim, so the file fails an always-empty `PATH_INFO` | `documented_index_request_serialises_to_the_exact_param_stream` pins the **whole** `encode()` byte string literal by literal (a dropped, reordered or extra pair fails); `query_string_dash_d_stays_verbatim_query_data` asserts the `-d+allow_url_include=1` needle appears in **exactly** `[REQUEST_URI, QUERY_STRING]`; `over_budget_param_count_or_bytes_is_params_too_large` asserts 128 fits / 129 refuses **and** 112-without-body fits / 113 refuses |
| `src/cgi.rs` | `an_empty_head_is_200_text_html` passes if `parse_head` returned `Ok(Head{200, [], "text/html"})` unconditionally; `the_debug_view_never_prints_a_value` passes if `Debug` printed nothing at all | `status_refuses_1xx_and_out_of_range` (11 lines, incl. duplicate `Status`), `non_token_header_name_is_fault` (7 names + obs-fold), `the_four_proxy_control_names_are_refused` (4 names, mixed case), `the_caps_are_enforced` (exactly 64 KiB parses, +1 byte refuses; exactly 100 fields, 101 refuses) |
| `src/record.rs` | `the_debug_view_never_prints_content` / `errors_print_without_bytes` pass under a silent `Debug`/`Display` | `over_long_content_is_refused` asserts both directions at 65 535/65 536 **for all seven record types** and that a refused `encode` leaves `out` empty; `the_decoder_is_total_on_every_prefix` cuts a 16-byte record at every offset; `the_maximum_record_carries_no_padding` walks 65 529..=65 535 |
| `src/param.rs` | `the_debug_view_never_prints_a_value` passes under a silent `Debug` | `a_truncated_pair_is_an_error` cuts at every offset; `short_lengths_are_one_byte_and_long_ones_four` pins the 127/128 prefix switch as literal bytes |
| `src/lockdown.rs` | `insert_enforces_the_pair_total_and_count_budgets` is a real boundary test; `insert_refuses_reserved_names_even_inside_the_crate` asserts both the error **and** `params.is_empty()` | the three insert tests are the only oracle for the pair/total/count branch and the reserved-name wall — which is why their location is a `misplaced`, not a `fluff` |

## Requirement → test map

Only the rows §7.2 assigns to **this** crate are listed. `MB.3.4` owns `R7`, `R8` rows
1/3/5–14/17/18/23, `R19`'s param budgets, and the `R9` **codec** half (§4.5.1). Status is
`protected` (a `crates/*/tests/critical_*.rs` file enforces it), `standard` (in-module only), or
`gap`.

| Req / rule | Enforcing module | Test(s) | Status |
|---|---|---|---|
| `R7` `SCRIPT_FILENAME`/`DOCUMENT_ROOT` from declared values, never request bytes | `lockdown::resolve` | `script_filename_is_pinned_ignoring_path_and_header`, `document_root_is_pinned_ignoring_headers`, `path_info_traversal_never_feeds_script_filename`, `dotdot_without_static_keeps_script_filename_pinned` | protected |
| `R7` `PHP_VALUE`, `PHP_ADMIN_VALUE`, `auto_prepend_file`, `allow_url_include`, `open_basedir`, `disable_functions`, `REDIRECT_STATUS`, `HTTP_PROXY` never emitted | `lockdown::{resolve,is_reserved_name,derive_http}` | `php_value_…`, `php_admin_value_…`, `auto_prepend_directives_…`, `redirect_status_is_never_emitted`, `proxy_header_never_produces_http_proxy`; `lockdown::tests::insert_refuses_reserved_names_even_inside_the_crate` | protected + standard |
| `R8` row 1 (`E1`) `SCRIPT_FILENAME` override | `lockdown` | `script_filename_is_pinned_ignoring_path_and_header` | protected |
| `R8` row 3 (`E3`) `SCRIPT_FILENAME?query` | `lockdown` | `script_filename_never_absorbs_query_string` | protected |
| `R8` row 5 (`E5`) `PHP_VALUE` | `lockdown` | `php_value_never_emitted_from_any_input` | protected |
| `R8` row 6 (`E6`) `PHP_ADMIN_VALUE` | `lockdown` | `php_admin_value_never_emitted_from_any_input` | protected |
| `R8` row 7 (`E7`) `auto_prepend_file` chain | `lockdown` | `auto_prepend_directives_never_emitted` | protected |
| `R8` row 8 (`E8`, CVE-2012-1823) | `lockdown` | `query_string_dash_d_stays_verbatim_query_data` | protected |
| `R8` row 9 (`E9`, CVE-2024-4577) | `lockdown` | `soft_hyphen_probe_stays_verbatim_query_data` | protected |
| `R8` row 10 (`E10`, CVE-2024-8926) | `lockdown` | `best_fit_encoding_probes_never_become_params` | protected |
| `R8` row 11 (`E11`, CVE-2024-8927) | `lockdown` | `redirect_status_is_never_emitted` | protected |
| `R8` row 12 (`E12`) `DOCUMENT_ROOT` spoof | `lockdown` | `document_root_is_pinned_ignoring_headers` | protected |
| `R8` row 13 (`E13`, httpoxy) | `lockdown::{derive_http,is_hop_by_hop}` | `proxy_header_never_produces_http_proxy` | protected |
| `R8` row 14 (`E14`) duplicate param | `lockdown::derive_http` | `colliding_http_headers_keep_first_and_record_drop` | protected |
| `R8` row 17 (`E17`) over-budget params | `lockdown::Params::insert` | `over_budget_param_count_or_bytes_is_params_too_large` | protected |
| `R8` row 18 unknown bare param (`500`) | `lockdown::resolve` | `unknown_bare_param_is_refused_500` | protected |
| `R8` row 23 (`E23`) `..` with no `[route.static]` | `lockdown` | `dotdot_without_static_keeps_script_filename_pinned` | protected |
| §4.6.2 name alphabet / unrepresentable name dropped | `lockdown::{is_param_name,is_representable_header}` | `unrepresentable_header_name_is_dropped`; `lockdown::tests::insert_rejects_duplicates_and_malformed_names` | protected + standard |
| §4.6.2 value bytes (NUL/CR/LF) refused | `lockdown::has_forbidden_value_byte` | `nul_in_header_value_is_malformed`, `malformed_request_facts_are_refused`, `path_info_is_decoded_exactly_once` (`/%00`) | protected |
| §4.6.2 value > 8 192 dropped, not refused (`HTTP_*`) | `lockdown::derive_http` | `over_long_header_value_is_dropped_not_refused` | protected |
| §4.6.2 value > 8 192 for `CONTENT_TYPE` | `lockdown::build` | `over_long_content_type_is_dropped_not_refused` | protected for `build`; **gap for `resolve`** (accepted MINOR (b)) |
| §4.6.2 collision first-wins + `Param{Dropped}` | `lockdown::derive_http` | `colliding_http_headers_keep_first_and_record_drop` | protected |
| §4.6.2 count ≤ 128 | `lockdown::Params::insert` | `over_budget_param_count_or_bytes_is_params_too_large` (128 / 129, 112 / 113) | protected |
| `R19` one pair ≤ 65 531 | `lockdown::Params::insert` | `over_budget_param_count_or_bytes_is_params_too_large` (60 000-byte name + 8 192-byte value) | protected |
| **`R19` encoded ≤ 65 535 (total)** | `lockdown::Params::insert` | `lockdown::tests::insert_enforces_the_pair_total_and_count_budgets` only | **standard — gap** (accepted MINOR (a)) |
| §4.6.2 one decode for `PATH_INFO` | `lockdown::decode_path` | `path_info_is_decoded_exactly_once`, `best_fit_encoding_probes_never_become_params` | protected |
| §4.6.2 hop-by-hop names dropped before `HTTP_*` | `lockdown::is_hop_by_hop` | `http_star_derivation_folds_names_and_skips_content_headers` (`Connection` only), `proxy_header_never_produces_http_proxy` (`Proxy-Connection` only) | **7 of 9 names unasserted — gap** |
| §4.6.2 `SCRIPT_NAME` is `?`-free | `lockdown::script_name_bytes` | `malformed_pinned_paths_are_refused` puts the `?` on `script_filename`, never on `script_name` | **gap** (survivor #7) |
| §4.6.2 re-serialisation: `exchange` re-runs the same pure function | `lockdown::{build,resolve}` | `documented_index_request_serialises_to_the_exact_param_stream` | protected |
| `R18` no param **value** is ever written (log-safe `Debug`) | `Params`/`Pair`/`Record`/`Field`/`Head` `Debug` | `param::tests::the_debug_view_never_prints_a_value`, `record::tests::the_debug_view_never_prints_content`, `cgi::tests::{the_debug_view_never_prints_a_value,a_field_debug_never_prints_its_value}` | **gap for `Params`** (survivor #5) |
| `R9` record framing: total decoder, over-long refused, `CL+PL ≤ 65 535`, no padding at the cap | `record` | 14 in-module tests | standard (§7.1 unit tier) |
| `R9` `BEGIN_REQUEST` = role 1 / flags 0 / `reserved[5]=0`; `END_REQUEST` ignores `appStatus`/`reserved[3]`, non-`Complete` → `ProtocolStatus` | `record` | `begin_request_is_the_pinned_responder_body`, `end_request_ignores_app_status_and_reserved_bytes`, `end_request_refuses_an_incomplete_protocol_status` | standard |
| `R9` name-value codec: 1-byte/4-byte lengths, terminator, truncated pair | `param` | 7 in-module tests | standard |
| `R9`/`R8` row 24 codec half: `Status: 200..=599`, CTL-free values, non-token name, the four proxy-control names, caps | `cgi` | 17 in-module tests | **standard — the protected owner (`critical_fastcgi_dispatch`, `MB.3.6.2.2`) does not exist** |
| §4.5.1 `Status:` is **three digits** | `cgi::parse_status` | `status_refuses_1xx_and_out_of_range` — no case has a byte whose `byte - b'0' == 10` | **gap** (survivor #3) |
| §4.5.2 "first `\r\n\r\n` splits head/body"; §4.5.1 "a head with no fields at all" | `cgi::head_end` | `head_end_finds_the_terminator` never passes `b"\r\n\r\n"` or `b"\r\n"` | **gap** (branch inspection) |

**Requirements with no test at all:** §4.5.1's "three digits" for a `':'` byte; the
`head_end(b"\r\n\r\n") → Some(2)` early return and the legal empty head it exists for; the
`?`-free `SCRIPT_NAME` rule; `Params`'s log-safe `Debug`; 7 of the 9 hop-by-hop names; the
`R19` total-byte cap through the public `build` path; `resolve`'s `CONTENT_TYPE` cap.

## Mutation evidence

```console
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-testcurator \
    make mutants-rust PKG=blastit-fastcgi MUTANTS_JOBS=4
mutants: command=cargo mutants -p blastit-fastcgi -j 4 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 348 mutants to test
ok       Unmutated baseline in 0s build + 0s test
348 mutants tested in 52s: 9 missed, 321 caught, 18 unviable
make: *** [Makefile:168: mutants-rust] Error 2      # cargo-mutants' "some mutants survived", not a harness failure

$ wc -l mutants.out/{caught,missed,timeout,unviable}.txt
  321 mutants.out/caught.txt
    9 mutants.out/missed.txt
    0 mutants.out/timeout.txt
   18 mutants.out/unviable.txt
  348 total
```

One command, one checkout, a private scratch. `make mutants-rust` prunes only *its* `MUTANTS_TMP`
(`scripts/mutants-scratch.sh prune`), so no other campaign's copies were touched.

| File | caught | missed | timeout | unviable | killed / viable |
|---|---|---|---|---|---|
| `src/lockdown.rs` | 182 | 5 | 0 | 3 | **182 / 187 = 97.3 %** |
| `src/cgi.rs` | 79 | 1 | 0 | 3 | **79 / 80 = 98.8 %** |
| `src/record.rs` | 34 | 0 | 0 | 7 | **34 / 34 = 100 %** |
| `src/param.rs` | 26 | 1 | 0 | 2 | **26 / 27 = 96.3 %** |
| `src/lib.rs` | 0 | 2 | 0 | 3 | **0 / 2 = 0 %** |
| **overall** | **321** | **9** | **0** | **18** | **321 / 330 = 97.3 %** |

Raw over every generated mutant: 321 / 348 = 92.2 %. Bar is ≥ 75 % → **PASS**. After removing
the 3 equivalences: **321 / 327 = 98.2 %**.

The 18 `unviable` are 13 "replace a function with `Ok(Default::default())`" mutants on types
that have no `Default` (`Head`, `Header`, `Record`, `Pair`, `BeginRequest`, `EndRequest`,
`RecordType`, `Error`), plus three `&&`→`||` rewrites inside `let`-chains (`parse_status` ×2,
`decode_path`) and two `+`→`*` in `hex_digit` that overflow `u8` at compile time. They are not
evidence either way.

## Surviving mutants (9) — every one explained or tasked

| # | Mutant | Judgement |
|---|---|---|
| 1 | `lib.rs:87` `Display for Error::fmt -> Ok(())` | **needs a test** — no test formats `Error`, while both inner errors' `Display` *are* pinned (`record::tests::errors_print_without_bytes`, `cgi::tests::errors_print_without_names_or_values`); the wrapper is the crate's public error surface (`MB.3.4.3`) |
| 2 | `lib.rs:96` `Error::source -> None` | **needs a test** — same test: `source().is_some()` for both variants; nothing in the crate walks the chain today (`MB.3.4.3`) |
| 3 | `cgi.rs:221:66` `<` → `<=` in `parse_status` | **needs a test** — `':'` is byte 58, `58 - 48 = 10`, so `<= 10` accepts it: `Status: 2:0` parses as **`300`** and `Status: 20:` as **`210`** (reproduced, see §"Empirical spot-checks"), where the design says three *digits* and anything else is `BadHead`. A malformed pool head becomes a different status instead of a `502` (`MB.3.4.4`) |
| 4 | `lockdown.rs:109` `Display for LockdownError::fmt -> Ok(())` | **needs a test** — the four phrases are the adapter's diagnostic text (`MB.3.6.2.1`); same cheap assertion as #1 (`MB.3.4.3`) |
| 5 | `lockdown.rs:133` `Debug for Params::fmt -> Ok(())` | **needs a test** — this impl exists *only* to satisfy `R18` ("a value carries cookies"), and a `Debug` that prints nothing passes every current test. The other four log-safe `Debug` impls in the crate all have a positive+negative assertion; `Params` has none (`MB.3.4.3`) |
| 6 | `lockdown.rs:185` `Params::is_empty -> true` | **equivalent in scope** — the getter has no production caller (only `insert_refuses_reserved_names_even_inside_the_crate` asserts the `true` direction), and a getter-only assertion is fluff (`testing` §3). No task |
| 7 | `lockdown.rs:515` `\|\|` → `&&` in `script_name_bytes` | **needs a test** — the mutant And-s the control-byte check onto the `?` check, so `SCRIPT_NAME = "/index.php?x"` is emitted (reproduced, see §"Empirical spot-checks"). No test puts a `?` in `script_name`: `malformed_pinned_paths_are_refused` puts it on `script_filename` (`MB.3.4.3`) |
| 8 | `lockdown.rs:581:32` `\|` → `^` in `hex_pair` | **equivalent** — `hex_digit(high)? << 4` has its low nibble zero, so `\| low` and `^ low` are identical for every `low < 16` |
| 9 | `param.rs:60:37` `\|` → `^` in `encode_len` | **equivalent** — `len` is filtered to `≤ 0x7fff_ffff` first, so bit 31 is clear and `\| 0x8000_0000` == `^ 0x8000_0000` |

Two further holes were found by **branch inspection**, where cargo-mutants generated no mutant
at all (so they cost nothing in the ratio but are real):

- `cgi::head_end`'s `if buf.starts_with(b"\r\n") { return Some(2) }` is never asserted.
  `head_end(b"\r\n\r\n")` is untested, and if that early return regressed to the generic
  `windows(4)` search it would return `Some(4)`; `parse_head(b"\r\n\r\n")` then hits
  `if line.is_empty() { return Err(BadHead) }` and a **legal empty CGI head** becomes a `502`.
  The composed path `head_end → parse_head` for a field-less head has no oracle (`MB.3.4.4`).
- `lockdown::is_hop_by_hop`'s nine names: only `connection` and `proxy-connection` are asserted.
  `keep-alive`, `proxy-authenticate`, `proxy-authorization`, `te`, `trailer`,
  `transfer-encoding`, `upgrade` are unasserted (`MB.3.4.3`).

### Empirical spot-checks

Three claims above are behavioural, not just textual, so each was reproduced against a
throwaway copy of the crate under `/tmp` (the workspace was never edited):

```console
# unmutated crate: the field-less head is legal only because head_end returns 2
head_end(b"\r\n\r\n")     = Some(2)
parse_head(b"\r\n")       = Ok((200, "text/html", 0))
parse_head(b"\r\n\r\n")   = Err(BadHead)          # what head_end -> Some(4) would hand it
composed(\r\n\r\nbody)     = Ok((200, 0))          # head_end then parse_head: the real path

# cgi.rs:221 `<` -> `<=` (the surviving mutant)
Status: 2:0               = Ok(300)
Status: 20:               = Ok(210)

# lockdown.rs:515 `||` -> `&&` (the surviving mutant)
script_name="/index.php?x" -> Ok(Some("/index.php?x"))    # unmutated: Err(Malformed)
```

The first block proves the `head_end` early return is load-bearing (a `Some(4)` regression turns
a legal empty head into a `502`); the other two prove the two "looks harmless" survivors are
real behaviour changes, not equivalents.

## The two accepted MINORs from `MB.3.4.2`

Both are real and neither is closed by the current suite.

1. **The 65 535 total-byte cap is only driven through `build` via the pair cap.** The protected
   test's byte case is a 60 000-byte name + 8 192-byte value, which trips
   `pair > MAX_PARAM_PAIR_BYTES` (65 531) *before* the total check. The total branch
   (`encoded_len() + pair > 65 535`) is asserted only in
   `lockdown::tests::insert_enforces_the_pair_total_and_count_budgets` — an unprotected in-module
   test. The `lead`'s prescription (an 8 × 8 192-value build case) is the right shape: each such
   pair encodes to `1 + 4 + name + 8 192` ≥ 8 200, so eight of them exceed 65 535 while seven fit
   and no single pair reaches the 65 531 pair cap — the total branch is the only one that can
   fire, and it fires through the public `build` path in the protected file. **Filed as
   `MB.3.4.3`.**
2. **`resolve()` returns an over-long `CONTENT_TYPE` while `build` drops it.** `build`
   special-cases `CONTENT_TYPE` before calling `resolve`; `resolve` itself is `pub` and applies
   no 8 192-byte rule, so `resolve(b"CONTENT_TYPE", …)` with a 9 000-byte fact returns
   `Ok(Some(9000 bytes))`. §4.6.2 says the value is *dropped*, and §4.6.2's re-serialisation rule
   has `exchange` re-run "the same deterministic pure function" — a second caller of `resolve`
   would emit the over-long value. The invariant to pin is: **no public function of this crate
   returns a param value above `MAX_PARAM_VALUE_BYTES`.** **Filed as `MB.3.4.5`** (a code fix, not a
   test-only change).

## Gap tasks filed

| Task | Kind | Paths | Closes |
|---|---|---|---|
| `MB.3.4.3` protected lockdown-suite hardening | test-only | `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` | survivors #1, #2, #4, #5, #7; MINOR (a); the 7 unasserted hop-by-hop names |
| `MB.3.4.4` CGI head hardening + protected codec oracle | test-only | `crates/blastit-fastcgi/src/cgi.rs`, `crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs` (new) | survivor #3; the `head_end` empty-head branch; a protected oracle for the four proxy-control names until `MB.3.6.2.2` lands |
| `MB.3.4.5` `resolve()` applies the 8 192-byte value rule | code + test | `crates/blastit-fastcgi/src/lockdown.rs` | MINOR (b) |

No task touches another task's file, so the three can run in parallel. Adding
`tests/critical_fastcgi_codec.rs` is always allowed by `tools/critical_guard.py`; the one
modification of `critical_fastcgi_lockdown.rs` is covered by `MB.3.4.3`'s `paths`.

## Notes for the epic gate

- `blastit-fastcgi` is **not** named in the `Makefile`; it is covered by `rs-test`
  (`cargo test --workspace --all-targets`) and by `rs-test-critical`'s `crates/*/tests/critical_*.rs`
  glob. No `--test` line is missing.
- The crate is still zero-dependency, `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`, and
  contains no `unwrap`/`expect`/`panic!` outside `#[cfg(test)]` — the two `unwrap_or_default()`
  in `lockdown::split_target` are on `Option<&[u8]>` slices, not `Result`.
- No `Refusal`, `UpstreamError` or HTTP status appears in the crate (§4.5.1's "codec-local
  errors" wall holds), so the status mapping stays `MB.3.6.2.1`'s.
