# AUDIT MB.23.6 — live reload in `blastit dev`: the runnable fence, both ban guards, the production strace and the `dev_reload` floor

- **Date:** 2026-11-17
- **Node:** `MB.23.6` (test-curator), audit report path per its `# Acceptance`: `docs/testing/AUDIT-MB.23.md`.
- **Base:** `695e40a` on `release/0.2.0`; epic base `e7b9581` (the commit before `MB.23.1`). The audited tree = the working tree; **no test, crate, script or SCM file was edited** — `git status --short crates scripts Makefile Cargo.toml Cargo.lock` is empty after the mutation campaign.
- **Scope:** `crates/blastit/tests/critical_dev_reload.rs` (23 rows), `crates/blastit/src/dev_reload.rs` (23 unit tests), `crates/blastit/src/supervise.rs` (policy channel/cycle), `crates/blastit-sys/src/fanotify.rs` + `tests/critical_fanotify.rs` (4), `crates/blastit/tests/critical_loaded_files.rs` (2), `crates/blastit-view/src/{runtime,loaded}.rs`, `crates/blastit-php-sapi/tests/critical_file_sink.rs` (8), `crates/blastit-dbproxy/src/{lib,server,learn,config}.rs`, `crates/blastit-frame/src/{frame,decoder}.rs`, `crates/blastit-scheduler/tests/pool.rs`, `scripts/check-no-inotify.sh`, `Makefile` (`docs-*`, `test-critical`, `mutants-*`), `python/tests/runnable_docs/*`, `docs/reference/cli.md` fence 1.
- **Requirements:** design `docs/design/dev-fanotify-reload.md` §4.10 (budgets), §5 (S1–S10), §7 (T1–T35), §8; research `docs/research/2026-11-16-dev-fanotify-reload.md` §3 (its per-route rows are superseded by the epic and §6); epic `plan/nodes/MB/MB.23/NODE.md` `# Acceptance` 1–8; skills `testing`, `security-invariants`, `rust-standards`, `python-standards`, `dx-first`.

## Verdict

**PASS WITH GAPS.** The gates are green, the mutation floor is cleared, and the dropped behaviour is genuinely gone — but six accepted requirements have no protected oracle, and the mutation campaign found two real holes rather than only equivalents.

- **Tier: PASS.** Every epic acceptance item has at least one protected test except the eight named in §1.4; no case in the reload scope is fluff, and no protected suite was weakened (§1.1, §2).
- **Gates: PASS.** `make check` exit 0 in 266 s with zero `FAIL|FAILED` lines; 47 Rust critical suites + 53 Python critical tests; `make docs-test` 48 fences / 30 executed / 0 failed with the `--no-reload` line's own `executed=True`; both `check-no-inotify.sh` runs print `ok`; the dependency and public-docs greps are empty; `seccomp.rs`, `Cargo.toml` and `Cargo.lock` are byte-identical across the epic; the two `strace` transcripts contain zero `fanotify_init`/`fanotify_mark` lines (§4).
- **Mutation: PASS.** 180 mutants generated, **123 caught, 22 missed, 35 unviable, 0 timeout = 84.8 % killed**, above the 75 % floor. Of the 22 survivors, 6 are equivalents (4 XOR-precedence, `note_change`, `HUP|ERR`), 3 are dead code or a dead return, 1 is a weak accessor, 3 are weak oracles, and **9 are meaningful** (the two dropped trigger bits, the pre-fork ordering, the path boundary, both `reconcile` mutants, and the three `EAGAIN` guards). Every meaningful survivor has a sentence and a node (§3.3).
- **Dropped behaviour: PASS.** No test asserts a single-view restart, an untouched route's pid staying the same, or a per-route map; the one "untouched view" string is a *content* assertion, paired in the same row with "every view pid changes" (§2.1).

Findings: **0 BLOCKER, 5 MAJOR, 13 MINOR.** Every gap has a follow-up node (§5): `MB.23.7`–`MB.23.15`.

## Method

1. Read every in-scope test body and the policy, supervisor, watcher, codec, frame/scheduler and dbproxy policy sources in full; classified each case `critical` / `useful` / `fluff` / `misplaced` / `broken-oracle` against the `testing` skill, asking what would still pass with its bound, denial or refusal removed.
2. Mapped epic `# Acceptance` 1–8 and design `T1`–`T35` to the concrete carrying test and checked the deny side of each security-bearing row.
3. Ran every `MB.23.6` acceptance command and the whole `make check` (§4.6), capturing full logs; the `--no-reload` fence was re-run through a driver that prints each outcome's own `executed` flag because the stock harness prints only aggregate counts.
4. Ran `scripts/check-no-inotify.sh` on the standalone binary and on the **wheel's** `_blastit*.so` (unpacked from the wheel `make docs-test` built; the path is recorded in §4.2).
5. Ran the production evidence: `strace -f -e trace=fanotify_init,fanotify_mark` on `blastit check` and on a production-mode fail-closed boot, plus a real `blastit dev` boot for the gate host's `reload: on (N directories)` line.
6. Audited determinism (`grep -n "sleep("`) and the dropped-behaviour assertions by reading all 23 live rows and grepping for `unchanged`/`untouched`/`only one`/`pid`/per-route.
7. `cargo-mutants 27.1.0`: `--in-place --file crates/blastit/src/dev_reload.rs --features python -- --locked --lib --test critical_dev_reload`, one serial campaign, output at `/tmp/opencode/mutants-mb236`; `make mutants-clean`, `git status` clean, no surviving process (§3.1).

## 1. Inventory, classification and requirement trace

### 1.1 Where the cases live

| Area | Cases | Class | Protected? |
|---|---|---|---|
| `crates/blastit/tests/critical_dev_reload.rs` | 23 | 21 `critical`, 2 `useful` | yes (`critical_*`) |
| `crates/blastit/src/dev_reload.rs` (`mod tests`) | 23 | 23 `critical` (policy units) | no, unit tier |
| `crates/blastit-sys/tests/critical_fanotify.rs` | 4 | 4 `critical` | yes |
| `crates/blastit-sys/src/fanotify.rs` (pure helpers) | 15 | 15 `critical`/`useful` | no, unit tier |
| `crates/blastit/tests/critical_loaded_files.rs` | 2 | 2 `critical` (one is a source-order witness, admitted) | yes |
| `crates/blastit-view/src/runtime.rs` | 5 | 5 `critical` (T25) | no, unit tier |
| `crates/blastit-view/src/loaded.rs` | 22 | 22 `useful` (codec bounds/round trip) | no, unit tier |
| `crates/blastit-php-sapi/tests/critical_file_sink.rs` | 8 | 8 `critical` | yes |
| `crates/blastit/src/supervise.rs` | 16 | 16 `critical`/`useful` (policy channel, cycle, ack) | no, unit tier |
| `crates/blastit-dbproxy/src/{lib,server,learn,config}.rs` | 19 policy | 19 `critical` | no, unit tier |
| `crates/blastit-frame/src/{frame,decoder}.rs` | 19 (`0x45` in 2) | 2 `critical`, 17 `useful` | no, unit tier |
| `crates/blastit-scheduler/tests/pool.rs` | 2 reload | 2 `critical` | no; `pool.rs` is not `critical_*` |
| `python/tests/runnable_docs` fence 1 | 4 commands | 1 `critical` (`--no-reload`) | via `make docs-test` |

**Classification:** no fluff and no `misplaced` in the reload scope. The two `useful` live rows are `an_atomic_save_is_one_reload` (the unit `T6` already pins the exact edges; the live row is the end-to-end twin) and `a_queue_overflow_is_one_all_views_reload` (the deterministic policy twin of the real-kernel overflow row). The one **weak oracle** is `critical_loaded_files::the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup`: a source-text order witness that says so in its own docstring; the behavioural half is `the_loaded_file_report_is_exact`. No node.

### 1.2 The live rows, one by one

| Row (`critical_dev_reload.rs`) | What it actually pins | Deny side |
|---|---|---|
| `saving_a_view_reloads_every_view_once…` (`:1059`) | one `dev.reload`, `routes` = every route in manifest order, every old view pid gone, each answer's pid is a live replacement, new body | untouched route's *content* (`products-1`) is asserted, never its pid |
| `saving_a_shared_module_leaves_no_view_on_the_old_version` (`:1133`) | one reload, both routes `version == 2`, two pids | a one-view restart would leave one route at 1 |
| `outside_the_project_reloads…` (`:1164`) | outside editable module reloads both views; `.git`, `node_modules`, a log do not and are never marked | positive control is the outside save |
| `an_atomic_save_is_one_reload` (`:1388`) | temp write + rename is one event set | `SETTLE` + `len == 1` |
| `a_worker_cache_write_never_triggers_a_reload` (`:1421`) | worker-written cache file in a marked dir fires nothing | positive control then `len == 1` |
| `no_reload_watches_nothing…` (`:1466`) | one fewer fd (report socket), no `fanotify`, no events, old body | reload-on control in the same test |
| `a_queue_overflow_reloads_every_view_once…` (`:1540`) | real 20 000-file burst → one `overflow`, every pid changed, one line | `SETTLE` then `len == 1` |
| `the_boot_line_is_one_pretty_line…` (`:1623`) | exactly one pretty `reload: on (N directories)`, never an event | JSON boot prints no line |
| `a_queue_overflow_is_one_all_views_reload` (`:1752`) | policy overflow + floor through the fake watcher | exact clock |
| `the_debounce_windows_are_exact…` (`:1798`) | 49/50, 400/450/500, 249/250 ms | injected clock, zero sleeps |
| `a_boot_report_replaces_a_set_and_arms_nothing` (`:1890`) | a boot batch never arms | 1 s later still nothing |
| `a_forged_source_is_dropped…` (`:1932`) | a datagram claiming another source is dropped and untracked | positive: the descriptor decides |
| `an_invalid_blastfile_keeps_serving…` (`:1975`) | invalid save: one warning, no reload, old pid; valid save: one reload, new order, added route, pre-save keep-alive answers, supervisor/http pids unchanged | both halves |
| `editing_a_preload_module_reloads_the_cycle_once` (`:2088`) | broken module: no reload, old pid; fixed: one cycle, new body | both halves; **no keep-alive (T23 gap)** |
| `an_eperm_watcher_is_one_unavailable…` (`:2137`) | exactly one `EPERM` warning, no reload, zero fanotify fds, three `200`s | fault seam, not a container |
| `a_scheduler_exit_after_a_cycle_is_still_fatal` (`:2206`) | post-cycle scheduler death is fatal and `kill_all`s views | dead pid |
| `learn_skips_the_manifest_cycle…` (`:2243`) | `--learn`: one warning, no reload, old pid; a view save still reloads | both |
| `the_policy_channel_reaches_stage_one_and_no_view_holds_it` (`:2305`) | stage 1 has `--dev-policy-fd`, no view sockets contain its inode, unchanged across a reload | fd table |
| `editing_an_included_php_file…` (`:2598`) | one reload naming both PHP routes, both serve v2, warm compiled artefact no reload | live pair on this host (§4.4) |
| `removing_a_store_is_one_warning…` (`:2750`) | one listener-set warning naming `products→db`, run keeps serving | duplicate assertion (MINOR) |
| `adding_a_store_is_one_warning…` (`:2824`) | the other direction | both |

### 1.3 Epic `# Acceptance` trace

| Epic item | Carrying evidence | Verdict |
|---|---|---|
| 1 children + every command run | 8 child `RESULT.md` files exist (this node's is the 9th) | ✓ |
| 2 all-view save, shared module, outside file, `Blastfile`/preload cycle, invalid save, `--no-reload` | rows 1–3, 7a–7d, 6 | ✓ with **T20/T23 keep-alive gap** (§1.4) |
| 3 PHP included file reloads all views | `editing_an_included_php_file…`; this host ran the live branch (`reloaded (php, /usr/lib/libphp8.4.so + target/php-sapi/libblastit_php84-x86_64-gnu.so): 2 view(s), 1 file(s)`) | ✓ |
| 4 Docker default seccomp: one `dev.reload.unavailable`, `200` throughout | only the `BLASTIT_TEST_RELOAD_FAULT=open` seam; **no container harness** | **gap → `MB.23.11`** |
| 5 zero inotify in binary, wheel, deps, sources | both guard runs + greps (§4.2) | ✓ |
| 6 `check`/production create no group, `seccomp.rs` unchanged | two strace transcripts + `git diff` over the epic (§4.3) | ✓ |
| 7 docs shipped, critics passed, fence executed live | `make docs-test` + `executed=True` (§4.1) | ✓ |
| 8 this audit + 75 % floor + every gap filed | §3, §5 | ✓ |

### 1.4 Unprotected accepted requirements (the gap list)

| Design row | Requirement | Why it is unprotected |
|---|---|---|
| `T17` | a real `check`/production boot holds no fanotify fd, no report socket, no loader hook | no test runs `check`/production in `critical_dev_reload.rs`; this audit's strace proves only the syscalls |
| `T18` (integration half) | a grant edit applies on save (denied → `200`, one `dev.reload`) | no test reads `BLASTIT_TEST_PG_URL` for the reload suite; CI has the service but nothing consumes it |
| `T20` (second half) | a keep-alive opened before an **all-views** `loaded` reload answers after it | the only `keep_alive` use is the `Blastfile` cycle row (`:1980`, `:2063`) |
| `T23` (second half) | the same keep-alive across a `preload` cycle | `editing_a_preload_module…` opens no connection before the save |
| `T31` | a tracked save during the reload cycle fires exactly one further reload | `BLASTIT_TEST_RELOAD_PAUSE` is used by **no test**; the mutation campaign confirms `reconcile -> ()` survives (§3.3) |
| epic item 4 | Docker's default seccomp profile | no `e2e/` reload harness at all |
| `T21` | `reload_all` is idempotent | only the first reload is sent in `pool.rs` |
| `T13` | the `0x45` payload refusal through the **streaming** decoder | `Frame::decode` only (the round trip does use `Decoder`) |

`T10`'s "count to the events pipe's EOF" is not literally implemented; the row uses a 750 ms `SETTLE` **after** a positive control, so a cache-triggered reload that is debounced together with the real edit fails the `files` assertion, and one that arrives later fails `len == 1`. The property holds; the oracle is temporal. MINOR (§2.2).

## 2. Determinism and the dropped behaviour

### 2.1 No surgical-reload assertion survives

`grep -nE "unchanged|untouched|only one|same pid|per-route" crates/blastit/tests/critical_dev_reload.rs` returns the header comment, row 1's comment, and row 1's `"the untouched view still serves its own module"` (`:1103`) — which asserts **content** (`products-1`), not a pid, four lines before the loop that requires **every** old pid to be absent (`:1114`–`:1127`). Row 2 asserts both routes are version 2; row 3 asserts "both views, not one"; row 9 asserts both old pids are gone. There is no per-route map type, no route payload in `Ctl::Reload`, and `RetireReason::Reload` is the scheduler's one word. The comment at `:45` states the rule for the file. **No MAJOR here.**

The scheduler's transaction is route-wide by construction (`reload_retires_every_idle_worker_of_every_route`) and the frame has no route field (`ctl_reload_is_0x45_and_has_no_payload`).

### 2.2 `grep -n "sleep("` — 23 hits, 17 bounded polls + 6 settle windows

`crates/blastit/tests/critical_dev_reload.rs` has 23 `sleep(` calls: **17** are `sleep(Duration::from_millis(N))` inside a `while Instant::now() < deadline` loop whose failure is a panic (`wait_ready`, `wait_reloads`, `wait_marked`, `wait_replaced`, `wait_stopped`, `wait_answers`, `wait_path`, `wait_new_scheduler`, `wait_body_containing`, `wait_unavailable`, `wait_warning`, `wait_exit`, the `/proc` reap loops and `stop`). The other **6** are `sleep(SETTLE)` (750 ms) used for a *negative* assertion (`:1186`, `:1400`, `:1433`, `:1520`, `:1613`, `:2694`). `SETTLE` is documented at `:70`–`:76` as longer than the 500 ms floor and is followed by a positive control in 4 of the 6 rows; the `--no-reload` row (`:1520`) proves the absence of a watcher by fd/mark as well.

**Finding (MINOR):** a fixed 750 ms window is exactly what the node's acceptance excludes ("no fixed sleep deciding a verdict"); on a heavily loaded host a delayed second reload could be missed, and two rows (`:1520`, `:2694`) have no later positive reload control in the same boot. The positive-control design makes it low risk, not a flake source seen in this audit. → `MB.23.13`.

`crates/blastit-sys/tests/critical_fanotify.rs` has 4 `sleep(` calls, all inside bounded `wait_for`/settle helpers (no `SETTLE`-decided verdict).

### 2.3 One save, every view, listener kept

`saving_a_view_reloads_every_view_once_and_the_new_body_is_served` records `before.len() == 2`, saves `views/cart.py`, waits for exactly **one** `dev.reload` whose `routes == ["products", "cart"]` and whose `files == [views/cart.py]`, waits for every old pid to be gone, then asserts each route's answer came from a **live replacement** and that `len() == 1`. The same shape is proven for `models.py` (both routes `VERSION == 2`) and for a file outside the project (both routes `outside-2`). The listener is never closed: the suite picks a free port, and the `Blastfile` row holds a connection opened **before** the save and reads `products-1:1|` from it **after** the cycle (`:2063`). The pre-save keep-alive across the all-views and preload paths is the `T20`/`T23` gap above (`MB.23.10`).

PHP is a live run on this host, not a refusal: `reloaded (php, /usr/lib/libphp8.4.so + target/php-sapi/libblastit_php84-x86_64-gnu.so): 2 view(s), 1 file(s), both routes serve v2; a warm compiled artefact needs no reload`.

### 2.4 The inherited three, and the one unknown

| Inherited item | State | Where |
|---|---|---|
| (a) `policy_out_of_step` is not carried across the exec | **NOT fixed.** `adoption_document` (`supervise.rs:1953`) carries the fd, not the taint; the replacement starts at `false` (`:644`). A timed-out channel looks in step after any cycle. | `MB.23.7` |
| (b1) no idempotent double-reload leg | **NOT fixed.** No test sends two `Ctl::Reload`s; `pool.rs` has two reload tests, neither a repeat. | `MB.23.15` |
| (b2) `0x45` refusals only via `Frame::decode` | **NOT fixed.** `ctl_reload_refuses_a_payload_and_a_non_zero_id` calls `Frame::decode`; the stream test sends only a clean frame. | `MB.23.15` |
| (c) unread `critical_dev_reload` panic (`MB.23.5.2.2` attempt 3) | **NOT reproduced and NOT triaged.** No full log exists; this audit ran the suite 6× (idle, under `make check`, post-mutation) with no failure. | `MB.23.13` |

Also inherited from `MB.23.5.3`'s log: the duplicated `assert_eq!(body.tag, "products-1")` in `removing_a_store…` (two identical lines), and the 90–500 ms ack-latency bound in `supervise::policy_tests` whose own docstring names a test that does not exist (`the_ack_wait_is_bounded_by_the_design_s_deadline`). → `MB.23.13`.

## 3. Mutation results

### 3.1 The campaign, and the cleanup

```console
$ cargo mutants --in-place --file crates/blastit/src/dev_reload.rs --features python \
    --timeout 120 --build-timeout 600 -o /tmp/opencode/mutants-mb236 \
    -- --locked --lib --test critical_dev_reload
Found 180 mutants to test
ok       Unmutated baseline in 16s build + 3s test
...
180 mutants tested in 56m: 22 missed, 123 caught, 35 unviable
```

`--features python` is required (`dev_reload` is a `#[cfg(feature = "python")]` module; the default `blastit` build is the standalone one) and is the same feature set `make mutants-rust PKG=blastit` passes. The test command is scoped to `--lib --test critical_dev_reload` so every mutant is judged by the policy units and the 23 live rows, not by the unrelated PHP/ingress suites. `timeout 0`, `unviable 35` (the usual `Default::default()`-not-`const` and `&&`→`||`-does-not-compile mutants).

Cleanup: the campaign exited 2 (misses, not a crash); `grep -c "changed by cargo-mutants" crates/blastit/src/dev_reload.rs` = 0, `git status --short crates scripts Makefile Cargo.toml Cargo.lock` empty, `make mutants-clean` freed 0 B (scratch already empty), and `pgrep -af "cargo-mutants|cargo mutants|mutants"` found no process. The suite was re-run after restoration: **23 passed; 0 failed** (§4.5).

### 3.2 Result

| Generated | Tested (viable) | Caught | Missed | Unviable | Timeout | **Killed** |
|---|---|---|---|---|---|---|
| 180 | 145 | 123 | 22 | 35 | 0 | **84.8 %** |

### 3.3 The 22 survivors, each classified

| # | Mutant | Why it survived | Verdict |
|---|---|---|---|
| 1–4 | `trigger_mask:129/130/131/132` OR→XOR | Rust precedence makes `a \| b \| (c ^ d) \| e` keep both bits of each XORed pair (`MOVED_TO^MOVED_FROM = 0xc0`, `CREATE^DELETE = 0x300`) | **equivalent ×4** |
| 5 | `trigger_mask:131` OR→AND | drops `FAN_CREATE`; no policy row feeds a synthetic create on a tracked path | **meaningful** → `MB.23.12` |
| 6 | `trigger_mask:132` OR→AND | drops `FAN_DELETE`; no policy row feeds a delete (the live tests delete nothing tracked) | **meaningful** → `MB.23.12` |
| 7 | `TestClock::elapsed` → `Default` | no caller anywhere (`grep -rn '\.elapsed()'` hits only `SystemClock`/`Instant`) | **dead test seam** — delete or stop exporting |
| 8 | `Absorbed::refused -> false` | both call sites assert `!refused()` (a successful absorb); nothing asserts `true` | **weak oracle** → `MB.23.12` |
| 9 | `ReportTable::absorb` `pre_fork` OR-assign → AND-assign | every test reports a path first as pre-fork-true, or never flips it; the "ever pre-fork stays pre-fork" ordering is untested | **meaningful** → `MB.23.12` |
| 10 | `path_refusal` `len > MAX` → `>=` | the unit sends a 4097-byte path (refused either way); a 4096-byte **accepted** path is untested | **boundary** → `MB.23.12` |
| 11 | `cap_reached -> true` | the accessor has no production caller; its only test asserts `true` after a cap | **dead accessor + tautology** |
| 12 | `Policy::marked_dirs -> 1` | the boot-line test asserts only `count >= 1`; the fixture marks one directory | **weak oracle** → `MB.23.12` |
| 13 | `Policy::note_change` `dead OR …` → `dead AND …` | every caller (`observe`, `apply_report`, `reconcile`) returns early when dead, so the changed guard is unreachable | **equivalent by caller guard** |
| 14 | `Policy::reconcile -> ()` | **no test calls the pause seam or the cycle**; `T31` is unwritten | **meaningful, MAJOR** → `MB.23.8` |
| 15 | `reconcile` `mtime <= upper` → `> upper` | no fixture writes a future mtime; the "a future mtime cannot re-arm" half of `T-F5a` is unproven | **meaningful** → `MB.23.8` |
| 16 | `Reload::is_dead -> true` | the accessor is only asserted `true` after a kill, never `false` in a live boot | **weak accessor** → `MB.23.12` |
| 17 | `poll_fds` `len - before` → `+` | every caller ignores the return; the doc promises a mapping nothing uses | **dead return value** (keep for the poll arithmetic, or assert it) |
| 18 | `on_fd` `HUP OR ERR` → XOR | a normal hangup reports one flag; no test sets both | **equivalent under the observed transport** |
| 19–21 | `on_fd` `WouldBlock` guard → `true`/`false`/`!=` | only `Ok(0)` (zero-length datagram) is tested; the idle `EAGAIN` race is untouched | **meaningful** → `MB.23.12` |
| 22 | `Reload::marked_dirs -> 1` | same weak oracle as #12, one layer up | **weak oracle** → `MB.23.12` |

**How the `blastit-sys` pure helpers were treated:** the raw syscall bodies (`fanotify_init`, `fanotify_mark`, `name_to_handle_at`, `read` and the `unsafe` layout casts) cannot be exercised without a real kernel and are outside the floor. Their pure helpers **were** audited by reading and by their own tests, not by a second campaign: `INIT_FLAGS`/`MARK_MASK` are pinned as values (`fanotify::tests::init_flags_and_mark_mask_are_the_measured_values`) and behaviourally per bit (`critical_fanotify::the_frozen_flags_and_mask_are_the_measured_ones` + create/close/rename/delete/mkdir/on-dir), `HandleKey`'s `(fsid, handle_type, bytes)` equality and all three distinctions are a direct unit test (`handle_keys_compare_bytes_and_type`), the errno ladder is enumerated one errno per row (`the_failure_ladder_maps_every_errno_to_its_op`), and `parse_records` has one unit per malformed shape plus "the valid record after the bad one" and the 16 KiB/align assertions. The one helper the policy campaign did reach (`trigger_mask`) is exactly where two survivors were found.

## 4. Gates — commands and tails

All commands ran in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` on `release/0.2.0` (`695e40a`), with `target/debug/blastit` rebuilt from the final tree (`cargo build -p blastit --features python --locked`, so the binary has the same feature set the wheel does). The host was idle-to-moderate (load 3–11 on 8 cores); no failure was attributed to load.

### 4.1 Documentation gates

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.26s
EXIT=0

$ make docs-unit
107 passed, 65 subtests passed in 2.51s
EXIT=0

$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags  4.3s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
...
48 fences: 30 executed, 18 accounted, 0 failed · 38.5s
EXIT=0
```

The `make docs-test` PASS row above is the primary evidence that fence 1 executed and passed. The stock report prints only aggregate counts, so fence 1 was also re-run through a driver that prints each `Outcome` — **run with the freshly built wheel staged in `dist/`** (in a clean shell `dist/` holds the pre-`--no-reload` wheel, where fence 1 reports `ok=False executed=True`):

```console
# run with the freshly built wheel staged in dist/
$ UV_OFFLINE=1 uv run --no-project python /tmp/opencode/exec_report.py docs/reference/cli.md
EXECUTED docs/reference/cli.md fence 1 mode=dev-flags ok=True executed=True seconds=4.4
DETAIL each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
4 fences: 4 executed, 0 accounted, 0 failed · 10.3s
EXIT=0
```

`dev-flags` loops over **every** `$ ` line of the fence (`harness.py:1455`), and `docs/reference/cli.md:20` is `$ blastit dev --no-reload`: it boots, requires the banner naming its port, and stops on `SIGTERM`, or the fence fails. `executed=True` is therefore the `--no-reload` line's verdict, not an adjacent fence's.

### 4.2 The ban guards, on two real files

```console
$ scripts/check-no-inotify.sh target/debug/blastit
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
GUARD_BIN_EXIT=0

$ scripts/check-no-inotify.sh $(ls target/docs-wheel/*/blastit/_blastit*.so | head -1)
ok: no inotify: 1 binary/binaries, no notify/inotify crate, 457 source files (one fanotify group, one Watcher::new)
GUARD_WHEEL_EXIT=0
```

**The wheel-extension path actually checked:** `target/docs-wheel/unpacked/blastit/_blastit.cpython-314-x86_64-linux-gnu.so` (17 313 360 bytes, `readelf --dyn-syms` readable). `make docs-test` itself leaves **only** `target/docs-wheel/blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl`; the `.so` is inside the zip, so the acceptance command finds nothing on a fresh gate. This audit unpacked the wheel with `unzip -q … -d target/docs-wheel/unpacked` and checked the real ELF — the guard was **not** run against a path it did not read. The command defect → `MB.23.14`.

```console
$ cargo tree -p blastit -e normal --locked | grep -Ei '\b(notify|inotify)\b'
(no output; TREE_EXIT=0, 314 dependency lines)

$ grep -rniE "inotify|BLASTIT_DEV_RELOAD" docs/local.md docs/reference/ docs/troubleshooting.md
(no output; exit 1)

$ git diff --quiet -- crates/blastit-sys/src/seccomp.rs Cargo.toml Cargo.lock
FROZEN_EXIT=0
$ git diff --stat e7b9581 HEAD -- Cargo.toml Cargo.lock crates/blastit-sys/src/seccomp.rs
(empty) ; $ git log --oneline e7b9581..HEAD -- those files
(empty — no MB.23 commit touched them)
```

`BLASTIT_DEV_RELOAD` does appear in `docs/design/dev-fanotify-reload.review-consistency.md`, the research note and the design — all excluded from the public site; the three public pages are clean.

### 4.3 Production creates no group

```console
$ strace -f -e trace=fanotify_init,fanotify_mark -o /tmp/opencode/check.strace \
    target/debug/blastit check -- ./scripts/noop.py
1 passed in 0.01s
blastit check: 0 requests, 0 statements, 0/1 routes
blastit check: 1 problem · `--json` for the machine-readable list
CHECK_EXIT=1 (a completeness finding on the trivial suite, not a crash)
FANOTIFY_LINES=0   # 16 trace lines, all process/exit noise

$ strace -f -e trace=fanotify_init,fanotify_mark -o /tmp/opencode/prod-boot.strace \
    timeout -s TERM 20 target/debug/blastit      # production stage 0, fixture with a Blastfile + app
PROD_EXIT=1            # fails closed unprivileged, as designed
FANOTIFY_LINES=0       # 3 trace lines, all exit status
```

The node's literal command (`check -- ./scripts/noop` from the repo root) exits 2 for **two** independent reasons: the root has no `Blastfile`, and `scripts/noop` does not exist anywhere in the tree (`git ls-files | grep noop` empty). The substitution above is a fixture with a `Blastfile`, `app.py`, and `scripts/noop.py` (pytest needs the `.py`); `strace` 6.13 was installed on this host for this audit (it was absent). → `MB.23.14`.

The gate host **does** allow the unprivileged group. A real `blastit dev` boot on this host printed:

```console
  blastit dev · single-uid simulation · 127.0.0.1:18098
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
...
reload: on (1 directories)
```

(`N = 1` because the fixture's only loaded module is `app.py` at the root.) The same fixture with `--no-reload` prints **no** `reload: on` line, only the preload boot line.

### 4.4 The live suites

```console
$ make test-critical
# 47 `test result: ok` lines, 0 failed; then:
Ran 53 tests in 10.840s
OK
EXIT=0                                    # no critical_php_embed probe row tripped this run

$ cargo test -p blastit --locked --features python --test critical_dev_reload php -- --nocapture
reloaded (php, /usr/lib/libphp8.4.so + .../target/php-sapi/libblastit_php84-x86_64-gnu.so): 2 view(s), 1 file(s), both routes serve v2; a warm compiled artefact needs no reload
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 22 filtered out; finished in 1.36s
```

### 4.5 `make check` (the epic gate)

```console
$ make check
MAKE_CHECK_EXIT=0 SECONDS=266
FAIL_LINES=0                 # grep -cE 'FAIL|FAILED' over the whole log
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.19s  (critical_dev_reload)
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.25s  (second target)
Ran 53 tests in 11.830s      # python/tests/critical
48 fences: 30 executed, 18 accounted, 0 failed · 29.9s   # docs-test re-ran the fence
zed-check: OK (42 properties checked)
plan OK
```

### 4.6 `grep` for the determinism rule

```console
$ grep -n "sleep(" crates/blastit/tests/critical_dev_reload.rs
# 23 lines: 17 bounded `sleep(Duration::from_millis(...))` inside deadline loops, 6 `sleep(SETTLE)`
$ grep -n "sleep(" crates/blastit-sys/tests/critical_fanotify.rs
# 4 lines, all inside bounded helpers
```

## 5. Findings and gaps → follow-up nodes

| Node | Severity | Gap | Evidence |
|---|---|---|---|
| `MB.23.7` | **MAJOR** | `policy_out_of_step` is not in the adoption document, so a timeout-tainted channel looks in step after any cycle and the next save can attribute a stale ack to its own document. | §2.4(a); `supervise.rs:514/644/924/1953` |
| `MB.23.8` | **MAJOR** | `T31` is unwritten: no test uses `BLASTIT_TEST_RELOAD_PAUSE`, and `Policy::reconcile -> ()` survives the campaign. | §3.3 #14–15; `supervise.rs:2012`, `dev_reload.rs:1106` |
| `MB.23.9` | **MAJOR** | `T18`'s integration half has no test; CI provides `BLASTIT_TEST_PG_URL` but the reload suite never reads it. | §1.4; `.github/workflows/ci.yml:54-65` |
| `MB.23.10` | **MAJOR** | `T17` (production/`check` fd + hook), `T20` (all-views keep-alive) and `T23` (preload keep-alive) have no protected row. | §1.4; `critical_dev_reload.rs:1980/2063` only |
| `MB.23.11` | **MAJOR** | Epic item 4 (Docker's default seccomp, one warning, `200` throughout) is only faked by `BLASTIT_TEST_RELOAD_FAULT=open`; no `e2e/` harness starts `dev`. | §1.3 item 4; `grep -rn reload e2e/` empty |
| `MB.23.12` | MINOR | Mutation survivors: `FAN_CREATE`/`FAN_DELETE` trigger legs, `pre_fork` ordering, the 4096-byte boundary, the `EAGAIN` arm, `refused()`'s true case, the boot-line count, and the `is_dead`/`cap_reached`/`poll_fds` weak or dead accessors. | §3.3 |
| `MB.23.13` | MINOR + **unknown** | Six `SETTLE`-decided negative verdicts, the 90–500 ms ack bound and its stale docstring, the duplicated `products-1` assertion, and the unread `critical_dev_reload` panic from `MB.23.5.2.2` attempt 3. | §2.2, §2.4(c) |
| `MB.23.14` | MINOR | `scripts/noop` does not exist and `make docs-test` leaves an un-unpacked wheel, so `MB.23.6`'s own commands do not run as written. | §4.2, §4.3 |
| `MB.23.15` | MINOR | `MB.23.5.1`'s two proof gaps: no idempotent double-reload leg; `0x45` payload refusal only via `Frame::decode`. | §2.4(b) |

**Counts:** **0 BLOCKER, 5 MAJOR, 13 MINOR.** The 13 MINORs are: T20 keep-alive, T23 keep-alive, T21 idempotence, T13 streaming refusal, FAN_CREATE/FAN_DELETE legs, pre_fork ordering, the 4096-byte boundary, the EAGAIN arm, the weak/dead accessors (`refused`, `marked_dirs`, `is_dead`, `cap_reached`, `poll_fds`), the six SETTLE windows, the ack bound + duplicate assertion + stale docstring, the unread panic, and the two evidence commands.

**Not filed, deliberately:** `TestClock::elapsed` is dead code (a deletion candidate, not a test gap — no assertion can kill a mutant of an unused accessor); the trigger-mask XOR mutants are Rust-precedence equivalents; `on_fd`'s `HUP^ERR` is equivalent for every flag combination the transport actually reports; `note_change`'s changed guard is unreachable behind three `is_dead` early returns. They are named in §3.3 so the next reader does not re-run them.

## 6. Every command run for this audit

```console
$ make docs-check                      # EXIT 0, "No issues found"
$ make docs-unit                       # EXIT 0, 107 passed, 65 subtests
$ make docs-test                       # EXIT 0, 48 fences: 30 executed, 18 accounted, 0 failed · 38.5s
$ make test-critical                   # EXIT 0, 47 Rust suites ok, Ran 53 tests in 10.840s
$ make check                           # EXIT 0, 266 s, 0 FAIL|FAILED
$ uv run --no-project tools/plan.py validate   # plan OK

$ scripts/check-no-inotify.sh target/debug/blastit                                      # ok
$ scripts/check-no-inotify.sh target/docs-wheel/unpacked/blastit/_blastit.cpython-314-x86_64-linux-gnu.so   # ok
$ cargo tree -p blastit -e normal --locked | grep -Ei '\b(notify|inotify)\b'             # empty
$ grep -rniE "inotify|BLASTIT_DEV_RELOAD" docs/local.md docs/reference/ docs/troubleshooting.md   # empty
$ git diff --quiet -- crates/blastit-sys/src/seccomp.rs Cargo.toml Cargo.lock            # 0
$ git diff --stat e7b9581 HEAD -- crates/blastit-sys/src/seccomp.rs Cargo.toml Cargo.lock # empty
$ strace -f -e trace=fanotify_init,fanotify_mark -o /tmp/opencode/check.strace target/debug/blastit check -- ./scripts/noop.py   # 0 fanotify lines
$ strace -f -e trace=fanotify_init,fanotify_mark -o /tmp/opencode/prod-boot.strace timeout -s TERM 20 target/debug/blastit       # 0 fanotify lines
$ target/debug/blastit dev --port 18098   # reload: on (1 directories)
$ target/debug/blastit dev --no-reload --port 18097   # no reload line

$ cargo test -p blastit --locked --features python --test critical_dev_reload php -- --nocapture   # live PHP pair
$ cargo test -p blastit --locked --features python --lib --test critical_dev_reload                # 289 + 23 passed
$ cargo mutants --in-place --file crates/blastit/src/dev_reload.rs --features python --timeout 120 \
    --build-timeout 600 -o /tmp/opencode/mutants-mb236 -- --locked --lib --test critical_dev_reload
# 180 tested in 56m: 22 missed, 123 caught, 35 unviable, 0 timeout
$ make mutants-clean                   # clean: freed 0 B
$ git status --short crates scripts Makefile Cargo.toml Cargo.lock   # empty
$ pgrep -af "cargo-mutants|cargo mutants|mutants"                   # empty
```

No test was weakened, deleted or `#[ignore]`d. The only files this audit wrote are this report and `plan/nodes/MB/MB.23/MB.23.6/RESULT.md`; `target/docs-wheel/unpacked/` is a generated unpacking of the wheel `make docs-test` built, under the git-ignored `target/`.
