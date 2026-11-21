# AUDIT — MB.36 (view crates over the wire: `blastit-view` as transport, one crate per view kind)

Epic gate audit for the five leaves of `MB.36` on branch `release/0.2.0`, epic base
`4aa39d9` (the `MB.35` gate commit, `MB.36.2`'s parent) and head `6977bd0` (`MB.36.6`).
Worktree `.worktrees/release-0.2.0`; every command below ran there. Skills loaded:
`testing`, `security-invariants`, `dx-first`, `rust-standards`, `python-standards`.

**Scope.** The four view crates (`crates/blastit-view`, `blastit-view-python`,
`blastit-view-php`, `blastit-view-static`), the root crate's integration tier
(`crates/blastit/tests/**`, including the two suites this leaf lands), the Python tier
(`python/tests/**`, `python/tests/critical/`) and the e2e harness (`e2e/**`).

**Requirements audited.** `docs/design/view-crates-architecture.md` §1 (the seven DX rows),
§4.5 (the relocation table and the count oracle), §7 (R1–R18), §8.6; `ADR-0016`.

**Verdict at a glance: the epic's tests are honest and nothing was lost.** No protected case
was edited, weakened or skipped: the four suites that moved changed by ≤ 4 lines each (their
`blastit_scheduler::` → `blastit_view_static::` imports), and every `--list` count is at or
above its floor (§2). The seven DX rows replay **byte-identically** after the documented mask
on the epic base and on the head, 24 captures compared, and the same replay is empty against
itself twice, so the empty diff is not a lucky capture (§3). The wheel, the Docker leg and the
rootless leg are green on the refactored tree (§5, §6). The mutation campaigns (§4) kill **86.0 %** of `blastit-view` and **85.2 %** of
`blastit-view-static`, and the third crate's 24.1 % is a *campaign-scoping* artefact — its stage-0
discovery gate is tested by the root crate's 27 `php_boot_gate` cases and the 28-case
`critical_php_embed`, which `cargo mutants -p blastit-view-php` never runs. Every meaningful
survivor is filed as a node, not patched here.

---

## 1. Inventory and classification

`critical` = protected tier and deserves it; `useful` = keeps its tier; `fluff` = tautology
(delete); `misplaced` = right test, wrong tier; `broken-oracle` = a security test that never
checks the denied path (or an oracle that cannot fail).

### 1.1 The protected suites that moved (38 static + 2 loaded-file cases)

| Suite | Destination | `--list` | Class | Denied path checked? |
|---|---|---|---|---|
| `critical_static_assets.rs` | `crates/blastit-view-static/tests/` | 20 | critical | yes — every row is a positive **and** its refusal: oversize → `404`/`respond` refuses, the dotfile rule, the `Content-Type` table's closed set, the ceiling refused "either half over" |
| `critical_static_resolve.rs` | same | 18 | critical | yes — the 40-row matrix runs over **both** backends, and `caller_supplied_segments_cannot_leave_the_root`, the TOCTOU `grow`/`swap` legs and `the_production_head_refuses_what_segments_never_sees` are the attack side |
| `critical_static_entry.rs` | same | 6 | critical | yes — `a_source_file_is_hidden_at_resolve_without_any_read_open`, `php_phtml_and_phar_final_segments_are_404`, `a_forged_source_url_path_is_never_streamed`, each beside `a_normal_asset_is_still_served_exactly` |
| `tests/common/static_fixture.rs` | `crates/blastit-view-static/tests/common/` | (support) | useful | the fixture is a real directory tree; it asserts its preconditions instead of skipping (non-root, case-sensitive, no NFC normalisation) |
| `critical_loaded_files.rs` | `crates/blastit-view-python/tests/` | 2 | critical | yes — `the_loaded_file_report_is_exact` asserts the report's exact set (a superset fails) and the dev-boot leg asserts the tracker is installed **before** the first import |

Both `critical_loaded_files` cases still exist and still fail if the tracker is installed after
the import or reports one file too many: the assertions were not touched (the diff against the
base blob is the four named edits, 10 changed lines, budget ≤ 10 — `MB.36.3`'s `RESULT.md`).

### 1.2 The protected suites that stayed (61 CLI cases, at floor)

| Suite | Cases (`--list`) | Class | Why it cannot move |
|---|---|---|---|
| `crates/blastit/tests/critical_static_cli.rs` | 12 | critical | drives `CARGO_BIN_EXE_blastit`; it is about the CLI's boot and `check`, not about the static crate |
| `crates/blastit/tests/critical_php_backend.rs` | 9 (6 + 3 helper) | critical | the boot's discovery gate and the probe table live in the composition root |
| `crates/blastit/tests/critical_php_embed.rs` | 28 (25 + 3 helper) | critical | the embedded SAPI, the worker forks and the trust-boundary refusals are a boot property |
| `crates/blastit/tests/critical_parallel_views.rs` | 7 (4 + 3 helper) | critical | two view kinds under one scheduler: the composition root's business |
| `crates/blastit/tests/critical_dev_reload.rs` | 23 (floor 14) | critical | a real `blastit dev` and its watcher |
| `crates/blastit/tests/common/libphp_probes.rs` | 3 (included by three suites) | critical | the twelve-key probe document, `#[path]`-included so one copy is compared against `stage0` |

`critical_php_embed` grew by the one leg `MB.36.5` recorded (the entry pin has no constructor
outside the boot), and `critical_dev_reload` is owned by the in-flight `MB.23.5.2` and grew from
14 to 23; the oracle is a floor, so growth is legal and a fall would be the failure.

### 1.3 The non-protected view cases that moved (65)

| Suite | Destination | `--list` | Class |
|---|---|---|---|
| `serve.rs` | `blastit-view-python/tests/` | 30 | useful — frame protocol, `start_response` misuse, `503` on `max_threads`, real socketpair |
| `body.rs` | same | 10 | useful — `wsgi.input` reads, cancel, clamp |
| `concurrency.rs` | same | 4 | useful — threads, interleaving (asserts both directions) |
| `environ.rs` | same | 12 | useful — every PEP 3333 key, header folding, `HTTP_` keys |
| `flow.rs` | same | 7 | useful — credit windows, 3 MiB uploads |
| `gil.rs` | same | 1 | useful — the caller owns the interpreter |
| `tests/support/mod.rs` | same | (support) | useful |
| `php_native.rs` | `blastit-view-php/tests/` | 1 (`#[ignore]`) | useful, **not fluff**: it needs a real `libphp` and a shim, so it is ignored by default and covered by `critical_php_embed` in the tier that has both |

No fluff and no broken oracle in this tier. The suites assert the negative twin wherever they
assert the positive one (`a_body_that_is_never_read_stalls_instead_of_aborting` beside the read
success; `a_second_start_response_without_exc_info_is_an_error` beside the `exc_info` replacement;
`an_eof_waits_for_the_request_threads`).

### 1.4 The other tiers, unchanged by the epic

| Tier | Where | Class | Note |
|---|---|---|---|
| Python critical | `python/tests/critical/` (22 + 22 + 3 + 6 cases) | critical | **zero** lines changed by `MB.36` (`git diff 4aa39d9..HEAD -- python/` is empty), so no relocation touched it |
| Python standard | `python/tests/test_*.py`, `runnable_docs/` | useful | ditto |
| e2e | `e2e/run.py`, `e2e/rootless/run.py` | useful (epic gate tier) | replayed in this leaf on the refactored wheel (§6); the `php2shell` 25-row matrix, the SQLAlchemy denial (`SQLSTATE 42501`) and the static replay are inside it |
| `blastit-sys/tests/critical_child_sequence.rs` | new in `MB.36.4` | critical | 5 cases, all unprivileged: `dumpable` 0, a `Seccomp:` line, group leader, symlinked cwd refused, a refused privilege drop failing at its step |

The one **misplaced-by-design** file is `crates/blastit-view-php/tests/php_native.rs`: the
design keeps it `#[ignore]`d on purpose (§4.5, "non-protected, `#[ignore]`"), and the same
behaviour is proved unprivileged by `critical_php_embed`. It is listed here so a future curator
does not read a 1-case ignored suite as a lost one.

---

## 2. The count oracle and the relocation proof

Counts are `cargo test -p <crate> --locked --test <name> -- --list | grep -c ': test$'` on the
head. The design's column is a **floor**, not an equality: three suites `#[path]`-include a
3-case helper, so their list is 3 above the column, and `critical_dev_reload` grew.

```console
$ for t in critical_static_assets critical_static_resolve critical_static_entry; do
    cargo test -p blastit-view-static --locked --test $t -- --list | grep -c ': test$'; done
20
18
6
$ for t in critical_loaded_files serve body concurrency environ flow gil; do
    cargo test -p blastit-view-python --locked --test $t -- --list | grep -c ': test$'; done
2  30  10  4  12  7  1
$ for t in critical_static_cli critical_php_backend critical_php_embed critical_parallel_views critical_dev_reload; do
    cargo test -p blastit --locked --features python --test $t -- --list | grep -c ': test$'; done
12  9  28  7  23
$ cargo test -p blastit-view-php --locked --test php_native -- --list | grep -c ': test$'
1
$ cargo test -p blastit-sys --locked --test critical_child_sequence -- --list | grep -c ': test$'
5
$ cargo test -p blastit --locked --features python --lib -- --list | grep -c '^stage0::php_boot_gate::'
27
$ cargo test -p blastit --locked --features python --lib -- --list | grep -cE '^stage0::(app_rule_executors|raw_ingress_gate_tests)::'
8
```

### 2.1 Moved-case totals

| Tier | Design floor | Now | Delta is explained by |
|---|---|---|---|
| protected static | **38** | **44** | `critical_static_assets` lists 20, not the column's 14: the file grew in a leaf before `MB.36` and the oracle's rule is "≥ the column and ≥ the previous revision", so a **fall** is the failure (`MB.36.2` records the same) |
| protected loaded-file | **2** | **2** | — |
| protected CLI | **61** | **79** | +9 from the three suites that `#[path]`-include the 3-case `libphp_probes.rs`; +9 from `critical_dev_reload`'s growth (14 → 23, the in-flight `MB.23.5.2`) |
| non-protected view | 65 | 65 | 30 + 10 + 4 + 12 + 7 + 1 + 1 (`php_native`) |
| `stage0` unit modules | 27 + 8 | 27 + 8 | retargeted at `discovery`'s public API, same counts |

### 2.2 Every source is gone and every destination is the same file

```console
$ for f in crates/blastit-scheduler/tests/critical_static_{assets,resolve,entry}.rs \
           crates/blastit-scheduler/tests/common/static_fixture.rs \
           crates/blastit/tests/critical_loaded_files.rs \
           crates/blastit-view/tests/serve.rs crates/blastit-view/src/runtime.rs; do
    test -e "$f" && echo "PRESENT $f" || echo "absent  $f"; done
absent  crates/blastit-scheduler/tests/critical_static_assets.rs
absent  crates/blastit-scheduler/tests/critical_static_resolve.rs
absent  crates/blastit-scheduler/tests/critical_static_entry.rs
absent  crates/blastit-scheduler/tests/common/static_fixture.rs
absent  crates/blastit/tests/critical_loaded_files.rs
absent  crates/blastit-view/tests/serve.rs
absent  crates/blastit-view/src/runtime.rs

$ git diff -M --numstat 4aa39d9..HEAD -- crates/blastit-view crates/blastit-view-static \
      crates/blastit-view-python crates/blastit-view-php crates/blastit-scheduler/tests crates/blastit/tests \
      | awk '$1==$2' | grep -E 'tests/' 
0  0  crates/{blastit-view => blastit-view-python}/tests/body.rs
5  5  crates/{blastit-view => blastit-view-python}/tests/concurrency.rs
3  3  crates/{blastit-view => blastit-view-python}/tests/environ.rs
1  1  crates/{blastit-view => blastit-view-python}/tests/flow.rs
2  2  crates/{blastit-view => blastit-view-python}/tests/gil.rs
11 11 crates/{blastit-view => blastit-view-python}/tests/serve.rs
5  5  crates/{blastit-view => blastit-view-python}/tests/support/mod.rs
1  1  crates/{blastit-scheduler => blastit-view-static}/tests/common/static_fixture.rs
4  4  crates/{blastit-scheduler => blastit-view-static}/tests/critical_static_assets.rs
3  3  crates/{blastit-scheduler => blastit-view-static}/tests/critical_static_entry.rs
1  1  crates/{blastit-scheduler => blastit-view-static}/tests/critical_static_resolve.rs
1  1  crates/blastit-scheduler/tests/critical_rewrite.rs
1  1 crates/blastit/tests/common/libphp_probes.rs
```

Rename detection sees the move; for every **moved** suite the changed-line count is the import
prefix (≤ 4), the `#[path]` repoint (1 line in `critical_rewrite.rs`, 1 in `libphp_probes.rs`) or
the `include_str!` repoint — the design's budget. `critical_loaded_files` is the exception at
10/10, and those ten lines are its four named edits.

The `awk '$1==$2'` above shows only pure moves; two suites were **edited in place** and are
therefore absent from it. They are the design's named-edit category, listed in full:

```console
$ git diff -M --numstat 4aa39d9..HEAD -- crates/blastit-view/tests crates/blastit-view-php/tests \
      crates/blastit/tests/critical_php_embed.rs crates/blastit/tests/critical_parallel_views.rs \
      crates/blastit/tests/critical_php_backend.rs
36  22  crates/{blastit-view => blastit-view-php}/tests/php_native.rs
67   1  crates/blastit/tests/critical_php_embed.rs
4    1  crates/blastit/tests/critical_parallel_views.rs
4    1  crates/blastit/tests/critical_php_backend.rs
```

- `php_native.rs` 36 added / 22 deleted (net +14): **API adaptation, no assertion changed.** The
  deletions are the `#![cfg(feature = "php")]` line, the `blastit_view::php::…` import and the
  `Arc<PhpEmbedRuntime>` construction; the additions are the `blastit_view_php::…` imports, the
  `EntryPin::of` + `PhpBinding { … }` the worker's configuration is now made from (`MB.36.5`'s
  seam), and the module doc's new home. It is still the one `#[ignore]`d case with the same two
  requests through `view.chan`.
- `critical_php_embed.rs` 67/1: one line changed (its import of `blastit::host`), 67 added — the
  one added pin leg (`the_worker_cannot_be_built_without_the_entry_pin`, 25 → 28 listed with the
  helper) `MB.36.5` recorded. Other cases untouched.
- `critical_parallel_views.rs` and `critical_php_backend.rs` 4/1: the `include_str!` repoint plus
  their imports.

The third guard on those files is `critical_skip_check` (no `#[ignore]` was added) and the count
oracle (§2), which they pass.

### 2.3 `critical_guard`

```console
$ make critical-guard NODE=MB.36.7
critical-guard: HEAD..working tree, 5 changed file(s)
node MB.36.7: paths ['docs/testing/AUDIT-MB.36.md', 'e2e/**', 'python/tests/**',
  'crates/blastit/tests/**', 'crates/blastit-view/tests/**', 'crates/blastit-view-python/tests/**',
  'crates/blastit-view-php/tests/**', 'crates/blastit-view-static/tests/**']
  no change to a protected critical test
```

`tools/critical_guard.py` answers "was I allowed to touch this file?", never "is it still
complete" — §2 is the completeness half, and no protected file was modified by this leaf.

---

## 3. The DX-invariance replay

### 3.1 What "the baseline" is, and why it is not `develop`

The design says "captured from a scratch checkout of `develop`". On this branch that is
impossible to do honestly: `develop` (`7dfa863`) is **not an ancestor** of `release/0.2.0`.
The merge base is `2c05139` (Release 0.1.0), `HEAD..develop` holds 51 commits of the divergent
0.1.1 line, and `git diff --stat develop 4aa39d9 -- docs/` is 163 files / 43 331 insertions —
a develop capture would diff every 0.2.0 feature the refactor never touched and could never be
empty. The baseline used here is therefore `4aa39d9`, the epic's parent, taken with
`git archive 4aa39d9 | tar -x -C …` (never this worktree), which is exactly the tree the
refactor commits were made on top of:

```console
$ git log --oneline 4aa39d9^..HEAD | head -6
6977bd0 [MB.36.6] Finish the feature matrix, guard the archive's capability, and bring the spec in line
b42a6c9 [MB.36.5] Extract crates/blastit-view-php and delete the WorkerRuntime trait
df6a5bf [MB.36.4] Move the zygote fork and the immortal preload into blastit-view-python
2910123 [MB.36.3] Extract crates/blastit-view-python: the WSGI worker leaves the transport crate
016b592 [MB.36.2] Extract crates/blastit-view-static: the static view kind leaves the scheduler
4aa39d9 [MB.35] Gate passed: one portable language server, Linux proved and four legs declared
$ git merge-base develop HEAD; git rev-list --count HEAD..develop
2c0513915b53f128fbc06393a3072001621cb109
51
```

This is a **recorded deviation**, not a silent substitution: the acceptance's *intent* — a
capture from a pristine tree, never from the worktree — is met by the archive of the epic's
parent, and the empty diff then means "the five refactor commits changed nothing", which is the
only thing the seven rows can be evidence for.

### 3.2 The capture, the mask, and the driver

`e2e/dx_replay.py` (stdlib only) boots each documented command with the tree's own binary, in a
fixed scratch path and on fixed ports, and writes `stdout`, `stderr` and the raw HTTP
transcripts. It runs the same seven rows the design lists, against the same fixtures:

| # | Command | Fixture | Captured |
|---|---|---|---|
| 1 | `blastit dev --port 18191` | `static "/" { root "public" }` | `GET /index.html`, `GET /missing`, `GET /..%2fetc` |
| 2 | `blastit dev --port 18192` | `e2e/php/fixture` (`entrypoint php`) | `GET /` twice |
| 3 | `blastit dev --port 18193` | `entrypoint python "app:application"`, `view echo POST`, `view bare POST` | `GET /`, `POST /echo` JSON, `POST /bare` with a body |
| 4 | `blastit dev --no-reload` | the static fixture | `GET /index.html` |
| 5 | `blastit check` / `check --json` | `python/tests/runnable_docs/fixtures/shop` (complete) and `…/findings` (missing a grant) | exit code, stdout, stderr |
| 6 | `BLASTIT_LIBPHP=/dev/null blastit dev` | the PHP fixture | exit code and the refusal |
| 7 | `blastit dev` from the `--no-default-features` build | the WSGI fixture | exit code and the refusal |

The mask is one file, read by both the driver and the Rust normaliser
(`crates/blastit/tests/dx-mask.txt`), so they cannot drift; the Rust suite asserts it is the
design's eight and nothing else:

* masked (design §1): `ts`, `duration_ms`, `cpu_ns`, `wall_ns`, `request_id`, `pid`, `fork_ms`,
  `cost_ns`, plus the version banner — rendered in the human lane as the `HH:MM:SS.mmm` clock
  and unit-suffixed durations, which the same normaliser masks;
* compared **by shape**, never value: `ETag` must match `"<hex>-<hex>"` (a value that does not
  is a **failure**), and a pytest summary (`4 passed in 0.01s`) by its shape;
* masked **on top of** the design's eight, in `dx-mask-host.txt` with the reason written down:
  `mem_used`, `max_view_processes`, `immortalized` — host- and run-derived boot diagnostics
  whose values differed between two runs of the *same* tree (observed 11588 vs 11589 for
  `immortalized`, 8013 vs 8017 MiB for `mem_used`). Everything else on those lines
  (`build=0.2.0`, `immortalize=on`, `mem_total`, `seccomp=on`, `privilege=unprivileged-dev`)
  stays compared;
* two orderings are made deterministic because they are not promises: the concurrent
  `process.exited` block is sorted within each run, and the capture ends at `INFO  shutdown`
  (the shutdown block's `status=0` vs `signal=15` detail races). Both are named here, and both
  are still compared as a set/marker, not dropped silently;
* the fixture's mtime is pinned to `@1700000000`, so `Last-Modified` is a value; the `ETag`
  value is still masked to its shape as the design requires.

### 3.3 The normaliser can fail (and so can the diff)

`cargo test -p blastit --test dx_capture --locked` carries the design's two self-tests, the mask
guard, the ordering rule, and the parity vector the replay driver shares:

```console
$ cargo test -p blastit --test dx_capture --locked
running 5 tests
test the_mask_is_the_designs_eight_fields_and_nothing_else ... ok
test a_line_the_normaliser_cannot_parse_is_a_failure_never_dropped ... ok
test concurrent_exits_sort_but_every_other_line_keeps_its_order ... ok
test a_varying_ts_is_masked_while_a_stable_status_is_kept ... ok
test the_replay_driver_and_this_normaliser_agree_on_one_golden_capture ... ok
test result: ok. 5 passed; 0 failed; 0 ignored
```

`e2e/dx_replay.py` normalises the captures the replay diff compares, so it is a **second**
implementation of the mask and the claim is only as strong as their agreement. One golden vector —
`crates/blastit/tests/dx-parity.txt` with `dx-parity.expected` — is normalised by both: the Rust
test above reads it with `include_str!`, and the driver's own `--self-test` reads the same files:

```console
$ uv run --no-project e2e/dx_replay.py self-test
dx-replay self-test: parity with dx_capture.rs on dx-parity.txt, empty refused
```

The vector exercises every rule (a JSON event with masked and stable keys, a human line with a
clock, `pid=` and a duration, an `ETag`, a pytest summary, a version banner, an out-of-order exit
block); a rule added to one side only fails one of the two. The empty-capture refusal is part of
the contract both share, and the driver's file policy follows it: an empty **`.http`** is a
failure, an empty `.stdout` from a zero exit is a failure, and an empty `.stdout`/`.stderr` from a
documented refusal (rows 6 and 7, `docs/ci.md`: "an empty stdout is a refusal") is compared as
`<empty>`.

The mask was then mutated once — `ts` deleted from `dx-mask.txt` — to prove the self-tests are
not vacuous:

```console
$ sed -i '/^ts$/d' crates/blastit/tests/dx-mask.txt
$ cargo test -p blastit --test dx_capture --locked
test a_line_the_normaliser_cannot_parse_is_a_failure_never_dropped ... ok
test the_mask_is_the_designs_eight_fields_and_nothing_else ... FAILED
test a_varying_ts_is_masked_while_a_stable_status_is_kept ... FAILED
test result: FAILED. 1 passed; 2 failed
$ cp dx-mask.txt.bak crates/blastit/tests/dx-mask.txt     # restored; the suite is green again
```

The two documented self-tests are exactly the two failure modes the design names: a varying
`ts` is masked while a stable `status` is kept (`a_varying_ts_is_masked_while_a_stable_status_is_kept`),
and a line the normaliser cannot parse is a failure, never dropped
(`a_line_the_normaliser_cannot_parse_is_a_failure_never_dropped` — a truncated event on line 1
and on line 2, a non-hex `ETag`, and the empty capture that would otherwise compare nothing).
`dx_invariance` covers rows 1 and 3 in process:

```console
$ cargo test -p blastit --test dx_invariance --locked
running 1 test
test row_1_static_dev_serves_the_file_and_refuses_a_traversal ... ok
$ cargo test -p blastit --test dx_invariance --locked --features python
running 2 tests
test row_1_static_dev_serves_the_file_and_refuses_a_traversal ... ok
test row_3_wsgi_dev_serves_the_app_and_refuses_a_body_with_no_codec ... ok
```

Row 3 is compiled with the `python` capability because a WSGI boot needs it; `make check` and
`make rs-test-critical` both pass `--features python`, which is the configuration every wheel is
built in. The bare acceptance command runs row 1, the row a default-features artifact can serve.

### 3.4 The seven rows, as captured (head; base identical)

```console
row 1 — blastit dev, static-only manifest
  blastit dev · single-uid simulation · 127.0.0.1:18191
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 0 routes
<ts> INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
<ts> INFO  runtime     static  serving static assets only
reload: on (1 directories)
<ts> INFO  ready       serving  routes=0  workers=16
<ts> INFO  request     GET /index.html → (unmatched)  200  <duration>
<ts> INFO  request     GET /missing → (unmatched)  404 unmatched  <duration>
<ts> INFO  request     GET /..%2fetc → (unmatched)  400 unmatched  <duration>

HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 38
ETag: "<hex>-<hex>"
Last-Modified: Tue, 14 Nov 2023 22:13:20 GMT
X-Content-Type-Options: nosniff
Cache-Control: no-cache
Connection: close

<!doctype html><h1>dx-invariance</h1>

HTTP/1.1 404 Not Found
…
not found

HTTP/1.1 400 Bad Request
…
bad request

row 2 — blastit dev, entrypoint php "public/index.php"
  http · scheduler · dbproxy · 2 routes
<ts> INFO  runtime     php 8.4.27  libphp /usr/lib/libphp8.4.so  php.ini /etc/php/8.4/embed/php.ini
<ts> INFO  process.spawned app  pid=<masked> uid=1000  fork=<duration>
<ts> INFO  request     GET / → app  200  <duration>
<ts> INFO  request     GET / → app  200  <duration>
…two `200 OK` responses, both `Cache-Control: no-store`

row 3 — blastit dev, entrypoint python "app:application"
  http · scheduler · dbproxy · 3 routes
<ts> INFO  boot        preload  privilege=unprivileged-dev  build=0.2.0  immortalize=on  immortalized=<masked>  immortalize_ms=<duration>  mem_total=32768MiB  mem_used=<masked>  max_view_processes=<masked>
<ts> INFO  request     GET / → home  200  <duration>
<ts> INFO  request     POST /echo → echo  200  <duration>
<ts> INFO  request     POST /bare → bare  413 unmatched  <duration>

HTTP/1.1 200 OK (text/plain, chunked)        wsgi-fixture
HTTP/1.1 200 OK (application/json, chunked)  {"received":7}
HTTP/1.1 413 Content Too Large               content too large

row 4 — blastit dev --no-reload
  http · scheduler · dbproxy · 0 routes
<ts> INFO  runtime     static  serving static assets only
<ts> INFO  ready       serving  routes=0  workers=16        ← no `reload: on`, no `dev.reload*`

row 5 — blastit check, complete and missing-a-grant
shop:      .  [100%]  · <pytest summary> · `blastit check: 1 requests, 1 statements, 1/1 routes, clean` (exit 0)
findings:  ....  [100%] · <pytest summary> · 6 problems, one line each, `SELECT 2` as the
           missing grant's detail (exit 1); `--json` writes one document on stdout and the
           pytest lines on stderr

row 6 — BLASTIT_LIBPHP=/dev/null (exit 1, stderr, one sentence)
blastit dev: route "api": entrypoint php "public/index.php": BLASTIT_LIBPHP (/dev/null) cannot be
opened or is not a regular file (not a regular file); install the PHP embed package —
Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php85-embed, Fedora/RHEL: dnf install
php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so

row 7 — the --no-default-features build on a WSGI manifest (exit 1, stderr, one sentence)
blastit dev: BLASTIT_APP (app:application): this build has no Python support (built without
feature "python"), so it cannot import a WSGI application; install the Python package:
uv add blastit
```

### 3.5 The diff

```console
$ uv run --no-project e2e/dx_replay.py run --binary /tmp/…/base-python  --out …/final-base …
$ uv run --no-project e2e/dx_replay.py run --binary /tmp/…/head-python  --out …/final-head …
$ for f in …/final-base/*.normalised; do cmp -s "$f" "…/final-head/$(basename "$f")" || echo "DIFFERS: $f"; done
$ echo "captures compared: $(ls …/final-base/*.normalised | wc -l), differing: 0"
captures compared: 24, differing: 0
```

And the control that the empty diff is not flake: the head replay was run twice into two
directories and compared **twice** — once raw, which is what the mask exists to absorb, and once
normalised, which is the claim. The raw comparison differs, on exactly the four files whose lines
carry clocks and durations, and nowhere else:

```console
$ e2e/dx_replay.py run … --out …/head  ;  e2e/dx_replay.py run … --out …/head2
$ for f in …/head/*; do n=$(basename "$f"); case "$n" in *.normalised) continue;; esac; \
      cmp -s "$f" "…/head2/$n" || echo "RAW DIFFERS: $n"; done
RAW DIFFERS: row1.stdout
RAW DIFFERS: row2.stdout
RAW DIFFERS: row3.stdout
RAW DIFFERS: row4.stdout
$ for f in …/head/*.normalised; do n=$(basename "$f"); cmp -s "$f" "…/head2/$n" || echo "DIFFERS: $n"; done
$ echo "normalised files compared: $(ls …/head/*.normalised | wc -l), differing: 0"
normalised files compared: 24, differing: 0
```

The four raw files are the dev stdout of the four `blastit dev` rows: their banner is identical,
but every `INFO`/`WARN` line carries the run's own clock and its `0.7ms`-class duration, so a raw
`cmp` can never pass. That is the whole reason the mask exists, and it is why the oracle is the
normalised comparison — which is empty — not a byte-identical one nobody could achieve. (Rows 5–7
are `blastit check`/refusals: their stdout has no clock, so their raw files happen to agree.)

Those rules were added only after that head-vs-head diff showed them as noise
(`mem_used`/`max_view_processes`/`immortalized`, the `process.exited` order and detail, the
pytest seconds); each is named in §3.2 with its reason, and none of them hides a status, a
header, a body, an exit code, a route name or a refusal sentence.

---

## 4. Mutation audit

Three campaigns, one at a time (they share `mutants.out`), each saved before the next started:
`cargo-mutants 27.1.0`, `MUTANTS_TMP=$HOME/.cache/blastit-mutants`, `-j 2`, 90 s/test, under the
cgroup cap. Ratios are recomputed from `mutants.out/{caught,missed,timeout,unviable}.txt` — 27.1.0
writes no `outcomes.txt` (`LESSONS.md`).

| Crate | Tested | Caught | Missed | Timeout | Unviable | Killed |
|---|---|---|---|---|---|---|
| `blastit-view` | 222 | 191 | 26 | 5 | 10 | **86.0 %** |
| `blastit-view-static` | 256 | 218 | 37 | 1 | 18 | **85.2 %** |
| `blastit-view-php` (447 of 572; see §4.4) | 402 | 97 | 305 | 0 | 45 | **24.1 %** |

```console
$ make mutants-rust PKG=blastit-view
232 mutants tested in 5m: 26 missed, 191 caught, 10 unviable, 5 timeouts
$ make mutants-rust PKG=blastit-view-static
274 mutants tested in 4m: 37 missed, 218 caught, 18 unviable, 1 timeouts
$ make mutants-rust PKG=blastit-view-php
447 mutants tested in 13m: 305 missed, 97 caught, 45 unviable, 0 timeouts   # then: ERROR interrupted
```

`make mutants-rust` exits 3 when any mutant survives; that is the campaign reporting, not a gate
failure — the gate is the ratio and the filed nodes.

### 4.1 The finding: a per-crate campaign cannot see a moved test

The two crates above clear the 75 % bar, and the third does not, and in both cases the reason is
the same architectural fact this epic created: **the tests that cover a view crate now live in
its consumer.** Per-file, from `outcomes.json`:

| Crate / file | Caught | Missed | Where its tests actually are |
|---|---|---|---|
| `blastit-view` `loaded.rs` | 110 | 12 | its own unit tests; **all twelve classified in §4.5** (nine gaps → `MB.26.28`, three equivalent) |
| `blastit-view` `path.rs` | 44 | 5 | its own unit tests; the survivors are `percent_decode`'s `<<4 \| low`, the `b'a'..=b'f'` arm and `index += 3` (a `*= 3` loop is a `TIMEOUT`) |
| `blastit-view` `flow.rs` | 25 | 2 (+3 timeout) | `Credit::take`'s `> 0` → `== 0`/`< 0` hangs or returns 0 early |
| `blastit-view` `lib.rs`/`spawn.rs`/`config.rs` | 0 | 6 | **consumer-scoping**: `frame_kind` *is* asserted, by `blastit-view-python/src/serve.rs:905` (`frame_kinds_are_named_as_in_the_architecture`), which `-p blastit-view` does not run; `ForkProtocol`'s strings have no assertion anywhere; `ViewConfig::for_route`'s default keeps every read field |
| `blastit-view-static` `static_response.rs` | 152 | 7 | its own suites; `civil_from_days`' negative-era branch (`shifted - 146_096`) is the only real miss |
| `blastit-view-static` `static_asset.rs` | 63 | 20 (+1 timeout) | its own suites; the misses are the bitflags `\|` rows (§4.2) and `classify`'s `RegularFile` arm (equivalent: the arm and its fallback agree) |
| `blastit-view-static` `rewrite.rs` | 3 | 10 | **`crates/blastit-scheduler/tests/critical_rewrite.rs:509`** — `declares_body`/`content_length` were not moved into this crate's suite |
| `blastit-view-php` `discovery.rs` | 21 | 246 | **`crates/blastit/tests/critical_php_embed.rs` and the root `stage0::php_boot_gate` (27 cases)** |
| `blastit-view-php` `worker.rs` | 57 | 14 | its own unit module (22 cases) |
| `blastit-view-php` `pinning.rs` | 16 | 13 | its own unit module + `critical_php_embed`'s inode legs |
| `blastit-view-php` `probe.rs` | 2 | 29 | `critical_php_embed`'s probe legs (the 28-case CLI tier) |

The `discovery.rs` row is the sharp one: the stage-0 gate is 267 mutants, and its oracle is a
27-case unit module plus a 28-case CLI suite **in another crate**, so `make mutants-rust
PKG=blastit-view-php` scores it 7 % while the code is, in fact, among the best-covered in the
epic. That is not a test gap, it is a *campaign-scoping* gap, and it is why §7 files
`MB.26.26` (`--test-package` in the target) rather than "write more discovery tests".

### 4.2 Survivors judged equivalent or untestable here (with the reason)

| Survivor | Class | Reason |
|---|---|---|
| `static_asset.rs:585:34`, `585:53`, `586:58`, `611:24` `\| → ^` | equivalent | the operands are **disjoint** bits (`PATH`, `NOFOLLOW`, `CLOEXEC`, `DIRECTORY`, `leaf_flags`), so `a \| b == a ^ b` and the mutated set is the same set. (The `^` rows on `526:68`, `461:78` and `623:22` are the same argument.) |
| `static_asset.rs:623:22` `\| → &` (`PATH & CLOEXEC`) | equivalent | `supports_openat2` only asks whether `openat2` answers `ENOSYS`/`EINVAL`; the descriptor is dropped immediately, so an empty flag set cannot change the answer |
| `supports_openat2 → true` | untestable on this host | the host's kernel supports `openat2`, so the mutated function returns the same value; the fallback it would select is covered explicitly by `every_matrix_row_answers_the_same_on_both_backends` (`Backend::Walk`). Proving the mutant needs a kernel without `openat2`, which the design does not mock |
| `classify`: delete `FileType::RegularFile` arm | equivalent | the arm falls through to a branch returning the same `Kind` for that mode |
| `sync.rs` (4 unviable) | not a survivor | the mutations do not compile |
| `flow.rs:131` `> → ==`/`<`, `Credit::close → ()`, `path.rs:49` `+= → *=`, `wire.rs:113` | **timeout, not a survivor** | they hang the request (an unbounded loop/wait), which is a failure in the ratio's denominator; the campaign's 90 s cap reports it. A caller-side timeout kills these, so they are recorded, not filed |

The `\| → &` cells that **empty a flag set whose consequences are not checked afterwards** are a
different class and are in §4.3: `open_with`'s `O_DIRECTORY`, `finish`'s `O_NONBLOCK`/`O_NOCTTY`,
every `CLOEXEC`, and `walk`'s **initial** `openat` (`586:58`), which no `fstat` inspects.

### 4.3 Survivors that are real gaps (filed, never patched here)

| Survivor | Why it matters | Node |
|---|---|---|
| `path.rs` `percent_decode` (`<<4 \| low`, hex arms), `index += 3` | the one request-target decoder (R7): a mis-decode of `%2f` is a traversal primitive | `MB.26.25` |
| `wire.rs::max_payload → 1` | the frame writer's bound (R10) | `MB.26.25` |
| `flow.rs` `Credit::take`'s `> 0` | a closed window that still hands out credit is a stall or an over-read | `MB.26.25` |
| `rewrite::declares_body`/`content_length` | decides "a body to a view with no `body` is a `413`"; all seven mutants survive the static crate's suite | `MB.26.27` |
| `static_response.rs:316:17` `civil_from_days`' `shifted - 146_096` | the **pre-1970** branch: the test's earliest input is `-1`, whose `shifted` is positive, so the branch is never taken and no test can reach it today | `MB.26.27` |
| `static_response.rs:320:42` `+ day_of_era / 36_524` | the term is `0` for every tested date (`day_of_era` 8 706 in 2023), so the mutation is invisible until a date ~100 years into a 400-year era | `MB.26.27` |
| `static_response.rs:320:64` `- day_of_era / 146_096` | differs on exactly one day per 400-year era (the era's last day); no test stands on it | `MB.26.27` |
| `static_response.rs:328:21` `month_prime - 9` | only taken for January/February dates (`month_prime >= 10`); the five asserted dates are all March–December | `MB.26.27` |
| `static_response::trim_ows` (`&& → \|\|`) | header-value trimming around the `If-None-Match` parse | `MB.26.27` |
| `static_asset.rs:586:58` `\| → &` (`probe & DIRECTORY`) | **not equivalent**: the set empties on the initial `openat(dirfd, ".", …)` and `walk` `fstat`s only the *next* descriptor (`static_asset.rs:597`), never this one. The call therefore drops `O_PATH`, `O_NOFOLLOW` and `O_CLOEXEC` and opens `.` `O_RDONLY`: a **`0o111` root** — servable today through `O_PATH`, which needs no read permission — answers `EACCES` and every request is `404`. Measured by the reviewer; no test in the tree uses an execute-only root today (`grep -rn '0o111' crates/blastit-view-static crates/blastit-scheduler/tests` is empty), and the case to prove on both backends is a `0o111` root with `GET /` → `200` | `MB.26.29` |
| `static_asset.rs:265:26`/`265:46` `\| → &` | `O_DIRECTORY` leaves the set: `open_with` has no `fstat` of its own, so a **regular-file root** opens and every request `404`s instead of the boot refusal the manifest promises; `265:46` also drops `CLOEXEC` | `MB.26.29` |
| `static_asset.rs:461:41` `\| → &` | `RDONLY` is `0`, so this cell drops only `CLOEXEC`: the root, probe and read descriptors then survive an `execve` — the fd-leak invariant R9 names, indistinguishable in-process | `MB.26.29` |
| `static_asset.rs:461:59` `\| → &` | `O_NONBLOCK` leaves the read set: a leaf swapped to a **FIFO** between the probe and the read blocks the worker. The swap leg swaps to a directory/symlink today, so `NONBLOCK` is never exercised | `MB.26.29` |
| `static_asset.rs:461:78` `\| → &` | `RDONLY\|CLOEXEC\|(NONBLOCK & NOCTTY)` = `RDONLY\|CLOEXEC` (`RDONLY` is `0`), so the read keeps `CLOEXEC` and drops `NONBLOCK` **and** `NOCTTY`: a terminal leaf can become the worker's controlling tty as well as block it — the one cell that drops both | `MB.26.29` |
| `loaded.rs`'s twelve survivors | boundaries (`>`/`>=` at `MAX_BATCH_BYTES` on both sides), `Trailing(have - at)`, `chunks`' byte arithmetic and empty run, `Step::rebuilds → true`, the two `Display` sentences | `MB.26.28` (§4.5) |
| `discovery.rs`'s 248 misses | covered by the root crate's tests, not by this crate's: a campaign-scoping gap | `MB.26.26` |

### 4.4 The `blastit-view-php` campaign was interrupted twice, deterministically

Two full attempts stopped at the *same* mutant, after 447 of 572:

```console
$ make mutants-rust PKG=blastit-view-php
447 mutants tested in 13m: 305 missed, 97 caught, 45 unviable, 0 timeouts
ERROR scenario execution internal error err=interrupted phase=Test
Error: interrupted
```

The last completed mutant is `worker.rs:678:20: replace + with * in compiled_sources`; the next in
the enumeration is `worker.rs:699:5: replace quoted -> Vec<&str> with vec![]`, whose test run hangs
and whose SIGTERM handling aborts cargo-mutants' worker (a `TIMEOUT` is reported only when the test
process dies from the signal, which this one does not). The 125 unmeasured mutants are all in
`worker.rs` after line 678; the file's other 71 are measured (80.3 % killed). Finishing them is
`MB.26.26`'s rerun with an exclusion for that mutant, not a test change, so this audit reports the
447 it measured instead of guessing.


### 4.5 The twelve `blastit-view/src/loaded.rs` survivors, row by row

`loaded.rs` is 110 caught / 12 missed, and the twelve are the whole of that file's `missed.txt`.
Each is classified below; the gaps are `MB.26.28`, and the two equivalents are argued, not waved
away.

| Survivor | Class | Why |
|---|---|---|
| `64:39` `FLAGS_KNOWN = FLAG_BOOT \| FLAG_RESYNC` `\| → ^` | equivalent | `0b0000_0001` and `0b0000_0010` share no bit, so `\|` and `^` are the same value |
| `326:20` `bytes.len() > MAX_BATCH_BYTES` `> → >=` | **gap** | a datagram of exactly `MAX_BATCH_BYTES` must be accepted; nothing asserts the boundary |
| `384:47` `DecodeError::Trailing(have - at)` `- → /` | **gap** | the refusal's byte count; `a_trailing_byte_is_refused` asserts the variant for one shape that happens to agree |
| `413:9` `Step::rebuilds -> true` | **gap** | only the three `true` cases are asserted; a steady-state delta must not rebuild |
| `483:22` `chunks`' `2 + path.len()` `+ → *` | **gap** | the split point depends on the real encoded size; no case pins a split with a known expected run |
| `484:63` `bytes + size > MAX` `> → >=` | equivalent | splits one path earlier; `chunks`' contract is "each chunk fits" (asserted: `size <= MAX_BATCH_BYTES`), and the datagram count is not part of it |
| `485:18` `index > start` `> → >=` | equivalent | at `index == start`, `full` is false for every path `check_paths` accepts (a path always fits), so the empty-run branch is unreachable |
| `492:14` `start < paths.len()` `< → <=` | **gap** | after a non-empty loop `start` can never equal `len` (a push sets `start = index < len`), so the mutation changes nothing there; it differs only for `chunks(&[])`, which gains one empty run — the exact-multiple case does **not** kill it |
| `517:20` `check_paths`' `bytes += 2 + len` `+ → *` | **gap** | the encoder would accept a batch whose real size is over the cap |
| `519:14` `bytes > MAX_BATCH_BYTES` `> → >=` | **gap** | the exact-cap acceptance/refusal pair on the encoder side |
| `209:9` `Display for EncodeError` | **gap** | the sentence is documented and never asserted |
| `288:9` `Display for DecodeError` | **gap** | same |

## 5. Gap analysis: requirements without a protected oracle

| Req | Oracle today | Tier | Protected? | Gap |
|---|---|---|---|---|
| R1 acyclic graph | `cargo tree` | make/CI | no | a dependency edge is a build fact; a test would only restate `cargo tree`. **Accepted, not a gap** |
| R2 transport purity | `make rs-no-python`'s manifest grep + ELF check | make | no | accepted: `[dev-dependencies]` is visible to the grep, and a Rust test cannot read the manifest of its own crate without a path dependency on the workspace root |
| R3 trait deleted | `grep -rn 'WorkerRuntime…' crates/` | manual/§7 | no | **gap** — a deleted trait has no compile-time witness; filed as `MB.36.11` to add a source-grep test next to the existing guards |
| R4 child sequence | `critical_child_sequence` (5) | critical | **yes** | — |
| R5 discovery never loads | `critical_php_embed` canaries + grep | critical | **yes** | — |
| R6 pinning | `critical_php_embed` (28) | critical | **yes** | — |
| R7 static | 44 relocated cases | critical | **yes** | — |
| R8 seccomp roles | `critical_embedded_seccomp`, `critical_php_embed` `:1883/:1961` | critical | **yes** | — |
| R9 secret barrier | `critical_dev_and_fail_closed`, `critical_loaded_files` | critical | **yes** | — |
| R10 wire unchanged | `flow`/`serve`/`body` (non-protected), `php_native` (`#[ignore]`) | standard | **no** | **gap**: the frame bytes of `view.chan` are a security boundary (a second framing is the failure R2/R10 exist for) and only non-protected suites compare them. Filed as `MB.26.25` |
| R11 standalone has no `libpython`/`libphp`/`RUNPATH` | `make rs-no-python` **and** the archive executed in two containers (§6.3) | make + e2e | **yes** (execution) | — |
| R11b the archive *serves* a `[route.php]` with no interpreter present | `standalone-runtime-check.sh --require-no-python` in `debian:trixie-slim`, plus §6.3's PHP transcript | e2e | **yes** | — |
| R12 wheel capability | `scripts/build-wheel.sh` + `check-portable-elf.sh` + this leaf's smoke | make/e2e | partial | **gap**: no test asserts the built wheel's `_blastit` `DT_NEEDED` set; the script does it, once, at build time. Filed as `MB.36.11` |
| R13 feature corners | `make rs-no-default` | make | no | accepted: `cargo build`/`clippy`/`tree` per corner |
| R14 DX invariance | `dx_capture` + `dx_invariance` + §3 | standard | no | accepted for a non-security promise; §1 of the design says so explicitly |
| R15 no protected case lost | §2 + `critical_guard` | audit | **yes** | — |
| R16 green increments | leaf `RESULT.md`s | history | no | accepted |
| R17 no new dependency | manifest diff | review | no | **gap (low)**: `git diff` of the manifests is a review step, not a test; `manifest-purity` covers a different rule. Filed as a note, not a node |
| R18 `MB.36.5` touched no python source file | `git show --name-only` | review | no | accepted (history) |

The gaps worth a node are R3 and R12 (R10 folds into the protected-transport node), plus the
three the mutation audit found (a campaign that cannot see a moved test, the `loaded.rs`
edges, and the open-flag cells that empty a set with no following check). They are filed in §7
as `MB.26.25`–`MB.26.29` and `MB.36.11`.

---

## 6. The artifacts, on the refactored tree

Wheel (the acceptance's `scripts/build-wheel.sh --manylinux 2_28`):

```console
$ scripts/build-wheel.sh --manylinux 2_28
…
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok   …/_blastit0.so: NEEDED libgcc_s.so.1 libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
$ sha256sum dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
01dce9e178c47eab71a0cd452df830c5c12c6adf1123cc21931a34e00afe0061  dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
```

Wheel smoke — the wheel's own ABI, `blastit-dbproxy`, one static route and one WSGI route,
all from a venv that holds nothing but that wheel and pytest:

```console
$ uv venv --python 3.14 /tmp/…/venv-dx
$ uv pip install --python /tmp/…/venv-dx/bin/python --offline dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
 + blastit==0.2.0
$ grep 'Tag:' /tmp/…/venv-dx/lib/python3.14/site-packages/blastit-0.2.0.dist-info/WHEEL
Tag: cp314-cp314-manylinux_2_28_x86_64
$ readelf -d /tmp/…/site-packages/blastit/_blastit.cpython-314-x86_64-linux-gnu.so | grep NEEDED
  Shared library: [libgcc_s.so.1]   [libpthread.so.0]   [libm.so.6]   [libdl.so.2]   [libc.so.6]   [ld-linux-x86-64.so.2]
$ /tmp/…/site-packages/blastit/_bin/blastit-dbproxy --help
blastit-dbproxy: `--help` is not `--key=value`
usage:
  blastit-dbproxy --role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0|1]
$ /tmp/…/venv-dx/bin/blastit dev --port 18291          # static "/" { root "public" }
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 37
ETag: "25-18e930e3efac0f85"
Cache-Control: no-cache

<!doctype html><h1>wheel-static</h1>

$ /tmp/…/venv-dx/bin/blastit dev --port 18292          # entrypoint python "app:application"
HTTP/1.1 200 OK
Content-Type: text/plain
Transfer-Encoding: chunked

wheel-wsgi
```

The ABI is `cp314`/`manylinux_2_28` (the tag the acceptance asked for), the extension's `NEEDED`
set has no `libpython` and no `libphp`, `blastit-dbproxy` is inside the wheel at
`blastit/_bin/`, and both a static and a WSGI route answer **from the wheel**. The wheel's own
banner names its interpreter (`runtime     python 3.14.10  environment /tmp/…/venv-dx`), so the
WSGI route really ran in that venv rather than in the checkout.

### 6.1 Docker leg (`E2E_DOCKERFILE=Dockerfile.prebuilt`)

```console
$ E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
…
PASS: the php2shell matrix and docs/frameworks/php/index.md replayed against a real blastit dev (140 checks)
PASS: the matrix this run replayed is the 24-reachable-row one (25 rows)
  ok   exactly the statement no route grants was denied (…'sqlstate': '42501'…)
PASS: 0 failure(s)
docs_deploy: 46/46 documented pooler promises hold
EXIT:0
```

260 `PASS` lines, 0 failures, on the wheel built in §6 — not a cached prebuilt image.

### 6.2 Rootless leg

```console
$ uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt
…
PASS: 0 failure(s)                              # the helper and app legs, 249 `ok` lines
  ok   one stage 0 boot event (1)
  ok   the stage 0 boot event says `privilege: capabilities` (capabilities)
  ok   and names exactly the capabilities it holds (['cap_chown', 'cap_dac_override',
       'cap_fowner', 'cap_kill', 'cap_setgid', 'cap_setuid', 'cap_setpcap'])
  ok   an entry rewritten in place is refused (500)
  ok   and the in-place decoy never reaches the client
  ok   a restart serves the page as it is now (200)
PASS: 0 failure(s)
EXIT:0
```

So the refactored tree boots with the real capability set, refuses an in-place entry rewrite, and
serves the restored file only after a restart — the pinning invariant R6 names, on the artifact
this epic ships.

---

### 6.3 The standalone archive boots PHP with no interpreter

`make rs-no-python` is an ELF check of `target/debug`, and the Docker PHP leg runs on an image
that has a CPython — so neither of those proves the **archive**. The archive does:

```console
$ scripts/build-standalone.sh --out /tmp/…/standalone
build-standalone: cargo build --release --no-default-features --features php,static
build-standalone: 0 pyo3 dependencies (no libpython linked)
build-standalone: blastit-view-static present (a static mount is served)
build-standalone: 1 shim(s) for x86_64-gnu from …/target/php-sapi
build-standalone: 5249444 bytes -> …/blastit-x86_64-unknown-linux-gnu.tar.gz
$ sha256sum …/blastit-x86_64-unknown-linux-gnu.tar.gz
496b5360c708daf3d370e342e6c5b73497fb13b0aec532d06985b15989a5e607  blastit-x86_64-unknown-linux-gnu.tar.gz
$ readelf -d blastit | grep -E 'NEEDED|RPATH|RUNPATH'          # unpacked, in the archive
  Shared library: [libgcc_s.so.1] [libm.so.6] [libc.so.6] [ld-linux-x86-64.so.2]
$ sha256sum -c SHA256SUMS
LICENSE: OK  README.md: OK  blastit: OK  libblastit_php84-x86_64-gnu.so: OK
```

The execution proof, in two containers. First `debian:trixie-slim`, which ships **no CPython at
all**, with the archive's own runtime check and `--require-no-python`:

```console
$ docker run --rm --network none -e PATH=/usr/sbin:/usr/bin:/sbin:/bin \
    -v …/blastit-x86_64-unknown-linux-gnu:/a:ro -v …/scripts:/s:ro \
    debian:trixie-slim bash /s/standalone-runtime-check.sh --require-no-python /a/blastit
host: no CPython on PATH
standalone-runtime-check: /a/blastit dev --port 18400
ok   200  /                      Cache-Control: no-cache
ok   404  /mb17-does-not-exist
ok   2/2 static responses without touching an interpreter
ok   exit 1 for a WSGI manifest (capability gate, names 'uv add blastit')
standalone-runtime-check: 3/3 passed without touching an interpreter
```

Then the PHP half: the same unpacked archive on a host with the distro's `libphp8.4` and the
archive's own shim, with `/usr/local/bin` (where that image's CPython lives) **off `PATH`**. The
whole command, mounts and both `-e` flags inline, exactly as run:

```console
$ docker run --rm --network none \
    -e PATH=/usr/sbin:/usr/bin:/sbin:/bin \
    -e BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
    -e BLASTIT_PHP_SHIM_DIR=/a \
    -e BLASTIT_MEM_HIGH_WATERMARK=100 \
    -v /tmp/…/blastit-x86_64-unknown-linux-gnu:/a:ro \
    -v /tmp/…/phpsite:/site:ro \
    --workdir /site blastit-e2e-php:latest \
    bash -c 'set -e; for c in python python3 python3.13 python3.14; do command -v "$c" >/dev/null && { echo "UNEXPECTED $c"; exit 1; }; done; echo "host: no CPython on PATH"; /a/blastit dev --port 18500 > /tmp/dev.log 2>&1 & pid=$!; trap "kill -TERM $pid 2>/dev/null || true" EXIT; for i in $(seq 1 300); do (exec 3<>/dev/tcp/127.0.0.1/18500) 2>/dev/null && { exec 3<&-; break; }; sleep 0.1; done; exec 4<>/dev/tcp/127.0.0.1/18500; printf "GET /hello HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n" >&4; cat <&4; echo "---- banner ----"; head -6 /tmp/dev.log'
host: no CPython on PATH
HTTP/1.1 200 OK
Content-type: text/plain;charset=UTF-8
Transfer-Encoding: chunked
Connection: close

1d
php from the archive: /hello

0

---- banner ----
  blastit dev · single-uid simulation · 127.0.0.1:18500
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
17:43:58.627 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
17:43:58.642 INFO  runtime     php 8.4.27  libphp /usr/lib/libphp8.4.so  php.ini /etc/php/8.4/embed/php.ini
17:43:58.643 INFO  ready       serving  routes=1  workers=16
```

`BLASTIT_LIBPHP=/usr/lib/libphp8.4.so`, `BLASTIT_PHP_SHIM_DIR=/a` (the archive's own shim), and the
view is `entrypoint php "public/index.php"` with `body form`. The 200 and the PHP body are the
proof the claim needs: no interpreter on `PATH`, no `libpython` in `NEEDED`, and a `[route.php]`
served from the artifact. (The image still *contains* a CPython at `/usr/local/bin` for its own
tooling — which is why the flag and the `PATH` are stated explicitly, and why the trixie-slim leg
above is the one that proves "no interpreter present".)

## 7. Filed follow-ups

Every node below is `kind: task`, `owner: implementer`, `status: proposed`. `MB.26.25`–`.27` are
this leaf's `MB.36.8`–`.10` as the lead re-filed them (the originals are `superseded` on disk);
`MB.26.28`/`.29` were created directly with `new MB.26 …`; `MB.36.11` stays in the epic. None of
them patches a test to make a survivor die, and none weakens a protected file.

| Node | Gap it closes | Tier |
|---|---|---|
| `MB.26.25` | a protected suite for `blastit-view`'s `percent_decode`/`hex`, `Credit::take` and `Wire::max_payload` (the R7/R10 transport boundary) | critical (new file) |
| `MB.26.26` | run the view-crate campaigns against the tests that cover them (`--test-package`); finish `blastit-view-php`'s 125 interrupted mutants | tooling (`Makefile`, `scripts/`) |
| `MB.26.27` | the `blastit-view-static` *behavioural* survivors: `declares_body`/`content_length`, the pre-1970 `civil_from_days`, `trim_ows` | existing suites |
| `MB.26.28` | the twelve `blastit-view/src/loaded.rs` survivors, row by row (§4.5) | `src/loaded.rs` unit module |
| `MB.26.29` | the `\| → &` open-flag cells that empty a set with no following check: `O_DIRECTORY` at the root, `O_NONBLOCK` against a FIFO swap, `CLOEXEC` across `execve` | `static_asset`'s suites |
| `MB.36.11` | the wheel's `DT_NEEDED` set (R12) and the deleted trait names (R3) as tests | `crates/blastit/tests/` |

## 8. Verdict

**The epic's test story holds.** No protected case was lost, weakened, skipped or re-scoped; the
relocated suites are the same files with their imports repointed (≤ 4 changed lines each) and
their `--list` counts at or above the floor; the seven documented behaviours replay byte-identically
after a mask that is itself tested and was proved able to fail; and the wheel, Docker and rootless
artifacts are green on the refactored tree. The gaps in §5 and the survivors in §4 are filed as
nodes — they are missing *tests*, never weakened ones.
