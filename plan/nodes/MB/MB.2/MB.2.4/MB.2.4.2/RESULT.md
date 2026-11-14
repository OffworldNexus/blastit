# RESULT — MB.2.4.2 (design MB.2.4b): static response construction

STATUS: done

Base `d37f765` on `release/0.2.0`. Landed: `crates/blastit-scheduler/src/static_response.rs`
(new), `crates/blastit-scheduler/src/lib.rs` (one `pub mod` line),
`crates/blastit-scheduler/tests/critical_static_assets.rs` (new protected suite).
`static_asset.rs`, `rewrite.rs`, `route.rs`, `scheduler.rs`, `config.rs`,
`tests/common/static_fixture.rs` and every existing critical file are untouched.

`StaticRoot::respond(file, method, if_none_match)` consumes the `StaticFile`, moves the
`OwnedFd` into `StaticBody` (never `dup`), never reads a byte, and builds the whole head:
closed `Content-Type`, `Content-Length`, quoted strong `ETag`, IMF-fixdate
`Last-Modified`, `nosniff`, the `(answer kind, answering path)` cache rule, and
`If-None-Match` → `304`. `http_date` is `pub` for the protected suite's vectors.

## Response table — rule → asserted

| §4.5 rule | Test | Assertion |
|---|---|---|
| `Content-Type` is the closed table, extension lowercased first | `the_content_type_table_is_closed`, `every_response_table_row_answers_the_design` | 30 extensions incl. every design row, `.PNG` → `image/png`, `.map` → `application/json`, `.well-known/…/token` (no extension) and `.zzz` → `application/octet-stream` |
| `Content-Length` = read-open `st_size`; `HEAD` included; absent on `304` | all table rows, `head_answers_the_get_head_and_no_body`, `if_none_match_answers_304_and_repeats_the_head` | byte-equal to the body read back; exactly one on 200/HEAD; zero on `304` |
| `ETag` quoted strong `"<size:x>-<mtime_ns:x>"`, no inode/device | `the_etag_is_exact_stable_and_moves_with_the_mtime`, every table row | equal to an ETag recomputed from `stat(2)` in the suite; stable across two GETs; equal across `/a.js`, `/a.js/`, `?v=2`, `/assets//a.js`; changes after `set_modified(1_700_000_000)`; same file as `Exact` (D `/index.html`) and as an `index` (`/`) share one ETag |
| `Last-Modified` IMF-fixdate, UTC | `http_date_is_the_imf_fixdate_inverse` + every row | the design's three vectors (`0`, `784111777`, `1700000000`) plus `1e9` and `-1`; each row equals `http_date(st_mtime)` |
| `nosniff` on every 200/304 | `check_case`, 304 test | `X-Content-Type-Options: nosniff` present in both |
| Cache: `Exact` + `immutable` glob → year | table (S, D `/index.html`), `the_cache_header_is_keyed_on_the_answering_file` | `public, max-age=31536000, immutable` |
| Cache: `Exact` with no matching pattern → `no-cache` | table (S `/index.html`, `/docs/index.html`, D asset), cache test | `no-cache` |
| Cache: every `DirectoryIndex`/`SpaFallback` → `no-cache`, whatever a pattern says | table (D `/`, `/dashboard`; S `/docs`, `/missing`), cache test | same bytes/ETag as the exact D answer, different `Cache-Control` |
| Cache key is the **answering** path, not the request path | `the_cache_header_is_keyed_on_the_answering_file` | S `/assets/missing.js` answers `/index.html` with `no-cache`; a front-controller policy that does declare `/assets/*` answers the same rows immutable asset / `no-cache` index |
| `If-None-Match` lists the ETag, `W/`-prefixed or `*` → `304` | `if_none_match_answers_304_and_repeats_the_head` | exact, `W/`, `*`, a list; repeats ETag/Last-Modified/Cache-Control/nosniff; no body, no `Content-Length`, no `Content-Type`; also for HEAD and for an index answer (`no-cache` repeated) |
| No match / absent → `200` + body | same test | `"not-the-tag"`, `W/"not-the-tag"`, `None` → 200 with a body |
| `HEAD` = the `200` head, no body | `head_answers_the_get_head_and_no_body` | the full header `Vec` is equal to GET's; body `None`; `Content-Length` still the would-be body's |
| `Range`/`If-Modified-Since`/`If-Match`/`If-Range`/`If-Unmodified-Since` ignored | by API (not parameters) + every row | a full `200`; named in the module docs |
| Zero-length file | `a_zero_length_file_is_a_200_with_length_zero` | 200, `Content-Length: 0`, GET body `len 0` / HEAD none, both backends |
| `static_max_bytes`; no whole-file buffering | `an_oversize_file_is_a_404_and_respond_never_promises_it` | `/big.bin` → `Hidden` → `decide` 404; a forged over-ceiling `StaticFile` is refused (404, no body); the same construction at the ceiling is a 200 with the exact length |
| Statuses come from `decide` | `statuses_come_from_decide_not_a_second_mapping` | Serve for exact/dir/SPA; `405` + `Allow: GET, HEAD` outranks a declared body for POST/PUT/OPTIONS/`get`; `413` for GET/HEAD with a body; `400` for rows 1–6/7 shapes; `404` for `.env`/`.git`/symlink/`mode000`/oversize; Forward with an app vs 404 without vs `index` under `spa`; `SpaLost` 404 |
| Never `Date`/`Server`/`Accept-Ranges`/`Location` | `check_case` on every row | all absent, and `Content-Encoding`/`Transfer-Encoding` too |

Double-direction oracles: immutable-for-answering-file vs no-cache-for-fallback,
304 vs 200-with-body, HEAD-head-equals-GET-head vs a body, and the over-ceiling refusal
vs the same construction served. Mutation sanity (temporary local edits, both reverted):
forcing `cache_control` to always `no-cache` fails 4 tests; giving `HEAD` a body fails
`head_answers_the_get_head_and_no_body` and the zero-length test.

## Deviations (named)

1. **`respond`'s one over-ceiling arm.** §4.2's type sketch comments `status` as
   `200` or `304`. `respond` additionally answers `404` (empty body, `Content-Length: 0`,
   `Cache-Control: no-store`) when a `StaticFile` is above `static_max_bytes`. Through
   `resolve` + `decide` it is unreachable (`classify` → `Hidden` → 404, asserted), but
   the alternative — promising a `Content-Length` this responder may not stream — would
   break §4.6's "no truncated body under a promised length".
2. **Fixture F/N declare no `immutable`.** The frozen `tests/common/static_fixture.rs`
   gives S and D the patterns and F/N `immutable = []`, while §5.2 rows 32/33/40 write
   `200 immutable` for the F/N columns. The table asserts `no-cache` for the frozen F/N
   policies (the honest answer for an empty `immutable`) and the cache test separately
   proves a front-controller policy that *does* declare `/assets/*` earns the year for
   the exact asset and not for an `index`. Follow-up for the lead/test-curator below.
3. **§7.2's "whole 40-row matrix" wording.** The resolution half of the matrix stays in
   the protected `critical_static_resolve.rs` (`MB.2.4.1`, landed); this file asserts
   every §4.5 response rule plus the status-producing rows **through `decide`**
   (Serve / Forward / 400 / 404 / 405 / 413, both directions). No response rule is left
   unasserted, and §5.2 is not re-mapped locally.
4. **Size budget.** `static_response.rs` is 317 lines (200 non-comment; the note says
   ≤ 260 "production lines") and the suite is 946 lines, so the leaf is ~1,264 changed
   lines against the 650 note. The overage is rustdoc for the design's normative rules
   and the one-table matrix; design §8 forbids truncating the matrix, and no dependency
   was added for `sha256` (a small in-suite implementation with its own NIST vectors is
   the test oracle).
5. **`make critical-guard BASE=develop`** (the node's literal line) fails on four
   protected files that `MB.1.6`/`MB.1.8` changed on `release/0.2.0`; `develop` predates
   those commits. The leaf-scoped run the dispatch specifies, `--base d37f765`, passes:
   this leaf touched no protected file. Evidence below.
6. **Short read → `Frame::RespAbort`** (§7.2, R10) belongs to the body pump
   (`MB.2.5.2`); `respond` never reads, so the pump owns that arm.

## Follow-ups

- `tests/common/static_fixture.rs` (frozen, `MB.2.4.1`) gives F/N `immutable = []`, but
  design §5.2 rows 32/33/40 say `200 immutable` for those columns. Either the fixture's
  F/N policies should carry `immutable = ["/assets/*"]` or the design's table should say
  `no-cache`; the suite currently asserts the frozen fixture and adds the pattern
  variant explicitly.
- `StaticResponse`/`StaticBody` deliberately have no `Debug` impl: a derived one would
  print header values, which §4.5/ARCHITECTURE §8.1 forbid in logs. `MB.2.5.2` should
  log status + header **names** only.

## Acceptance evidence

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_assets --locked
running 11 tests
test http_date_is_the_imf_fixdate_inverse ... ok
test the_suites_sha256_oracle_is_sha256 ... ok
test an_oversize_file_is_a_404_and_respond_never_promises_it ... ok
test a_zero_length_file_is_a_200_with_length_zero ... ok
test the_cache_header_is_keyed_on_the_answering_file ... ok
test if_none_match_answers_304_and_repeats_the_head ... ok
test statuses_come_from_decide_not_a_second_mapping ... ok
test the_etag_is_exact_stable_and_moves_with_the_mtime ... ok
test the_content_type_table_is_closed ... ok
test every_response_table_row_answers_the_design ... ok
test head_answers_the_get_head_and_no_body ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked
...
test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
   Running unittests src/lib.rs …               test result: ok. 45 passed; 0 failed
   Running tests/admission.rs …                 test result: ok. 11 passed; 0 failed
   Running tests/critical_ingress_enforcement.rs … test result: ok. 9 passed; 0 failed
   Running tests/critical_rewrite.rs …          test result: ok. 13 passed; 0 failed
   Running tests/critical_static_assets.rs …    test result: ok. 11 passed; 0 failed
   Running tests/critical_static_resolve.rs …   test result: ok. 14 passed; 0 failed
   Running tests/ctl_channel.rs …               test result: ok. 9 passed; 0 failed
   Running tests/fairness.rs …                  test result: ok. 17 passed; 0 failed
   Running tests/flow.rs …                      test result: ok. 6 passed; 0 failed
   Running tests/head_allocs.rs …               test result: ok. 2 passed; 0 failed
   Running tests/isolation.rs …                 test result: ok. 13 passed; 0 failed
   Running tests/lifecycle.rs …                 test result: ok. 18 passed; 0 failed
   Running tests/memory.rs …                    test result: ok. 14 passed; 0 failed
   Running tests/routing.rs …                   test result: ok. 14 passed; 0 failed
   Doc-tests blastit_scheduler …                test result: ok. 1 passed; 0 failed
# 15 suites, all `ok`
```

```console
$ make rs-lint
PYTHONHOME=… PYO3_PYTHON=… LD_LIBRARY_PATH=… cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.1.2 (…)
    Checking blastit v0.1.2 (…)
    Checking blastit-py v0.1.2 (…)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.98s
```

```console
$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-scheduler --no-deps --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.54s
   Generated …/target/doc/blastit_scheduler/index.html
$ make rs-doc
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.05s
   Generated …/target/doc/blastit/index.html and 12 other files
```

```console
$ cargo fmt --all -- --check
FMT_OK
```

```console
$ uv run --no-project tools/critical_guard.py --base d37f765 --node MB.2.4.2
critical-guard: d37f765..working tree, 7 changed file(s)
node MB.2.4.2: paths ['crates/blastit-scheduler/src/static_response.rs', 'crates/blastit-scheduler/src/lib.rs', 'crates/blastit-scheduler/tests/critical_static_assets.rs']
  no change to a protected critical test
```

```console
$ make critical-guard BASE=develop NODE=MB.2.4.2
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_check_engine.rs            [VIOLATION: not covered by any node path]
  M  crates/blastit/tests/critical_config_and_presets.rs      [VIOLATION: not covered by any node path]
  M  python/tests/critical/test_check_contract.py             [VIOLATION: not covered by any node path]
critical-guard: FAILED: 4 protected critical test(s) changed or deleted without a covering path
# Pre-existing MB.1.6/MB.1.8 changes on release/0.2.0 (`develop` predates them);
# the leaf-scoped `--base d37f765` run above is the honest comparison and passes.
```
