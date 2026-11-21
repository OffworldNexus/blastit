# Testability review (second pass) — `docs/design/dev-fanotify-reload.md` (MB.23.2)

Lens: verification and oracles. Judged the revised §7's 33 rows against §4's frozen seams, the four accepted DX pages, `.opencode/skills/testing/SKILL.md`, `tools/critical_guard.py`, the Makefile's critical tier, and the code the rows name. Every first-pass finding in §9 was checked in the text, not taken on trust.

## Findings

### F1 · MAJOR (docs/design/dev-fanotify-reload.md:823, :536-545)
T33's pid oracle is impossible. It requires "the supervisor pid changed while the `http`/dbproxy pids did not", but §4.6 step 6 "execve[s] the stage-1 program prefix" — `execve` preserves the PID. On a correct reload the supervisor's pid is unchanged, so T33 fails on the very implementation it tests (or an implementer "fixes" it by forking a new stage 1, which contradicts the adopted `http`/dbproxy state). Fix: assert an adoption witness instead — the new image emits no second `boot`, the old scheduler pid is gone, and `http`/dbproxy pids and `/proc/<pid>/stat` start times are unchanged — or freeze one observable supervisor generation counter.

### F2 · MAJOR (docs/design/dev-fanotify-reload.md:804, :174-179)
T14 counts the wrong thing. "Exactly one `anon_inode:fanotify` link" in the supervisor cannot be true: §4.1 keeps two descriptors to the one group (the `FAN_CLOEXEC` group fd plus the `F_DUPFD_CLOEXEC` poll clone), so `/proc/<pid>/fd` shows two symlinks on a correct run. The oracle must `stat` each fd and assert one distinct group inode across the three saves (two fds, same inode, no accumulation), not one link.

### F3 · MAJOR (docs/design/dev-fanotify-reload.md:797, :410-416)
T7's floor expectation contradicts the frozen formula. "Two windows 400 ms apart fire once" — `deadline = max(last_fire + 500 ms, min(last + 50 ms, first + 250 ms))` *delays* the second fire to `last_fire + 500` (t = 550); it never merges it. So the 400 ms case fires twice (at 50 and 550), exactly like the "500 ms apart" case, and T7's "fire once" is unsatisfiable. Either specify coalescing/suppression inside the floor (§4.5's bullets do not) or change the row to assert the second fire at +500 ms. As written R7/R12's only floor oracle cannot pass.

### F4 · MAJOR (docs/design/dev-fanotify-reload.md:820, :339-340, :435-438)
T30 has no channel through the frozen seam. `ReportTable::on_datagram(...) -> (Report, Option<PathBuf>)` with `Report::{Applied, Dropped}` cannot distinguish `boot` from `resync`, and returns at most one path for a ≤64-path batch; §4.5's `Policy` owns the window but exposes only `reports()`. So "a resync arms the window, the post-reload `boot` batch does not" is inexpressible, and a resync whose meaning is "treat every path you already have from this source as changed" has nowhere to hand its set. Fix: return a report kind (`Boot | Resync | Append`) plus the changed paths, and feed the window from `Policy::reports`.

### F5 · MAJOR (docs/design/dev-fanotify-reload.md:821, :494-574)
T31 is neither deterministic nor specified. The window "after `Ctl::Reload` but before the exec" has no observable barrier: `dev.reload` is emitted at step 2, before the step-3 `Ctl::Reload`, so the test races the exec and is flaky by construction. Worse, §4.5/§4.6 define no disposition for a fire while `Supervisor::reloading` is set — queue it, drop it, send a second `Ctl::Reload`, or fold it into the pending cycle? T31 asserts "exactly one further `dev.reload`" against an unwritten rule. Name the state-machine arm and a deterministic sync point (or move the row to a scripted-clock unit test) before claiming the row.

### F6 · MAJOR (docs/design/dev-fanotify-reload.md:822, :701)
T32's third case lives in the wrong file. The 8192-path / 4 MiB per-source collector and its `resync` are sender-side (§4.10 "paths per source, per request"; the Python hook / `blastit-view`), not in `crates/blastit/src/dev_reload.rs`, so the named unit test cannot reach it. Split the row: the union cap (16385) and the marks cap (8193) stay in `dev_reload.rs`; the collector cap belongs beside T24 in `runtime.rs`/`loaded.rs`.

### F7 · MAJOR (docs/design/dev-fanotify-reload.md:718, :805, :141)
The `Drop::Missing` existence check — the bound the security review's F1 was resolved with — has no oracle. §5 S1 says "A reported path must exist as a file or directory when it is first reported" and attributes it to "§7 T14, T15", but neither row mentions existence; R14's coverage line (:826) omits it too. Delete the single `symlink_metadata` and replay a forged path naming a nonexistent file, and every one of the 33 rows stays green. Add to T15: a reported path that does not exist is dropped (`Drop::Missing`), counted once, adds no mark and fires nothing.

### F8 · MAJOR (docs/design/dev-fanotify-reload.md:137, :496-501, :826-828)
R10's `--learn` clause has no row. "Under `--learn` the manifest cycle is skipped with one warning (the tally must survive)" is asserted by nothing; the coverage line maps R10→T22/T23/T33, none of which passes `--learn`. A regression that re-execs on a `Blastfile` save under `--learn` and discards the one-pass tally passes all 33 rows and silently breaks `docs/local.md`'s "every view and statement" promise. Add `learn_save_keeps_the_tally` (dev `--learn`, edit `Blastfile`, no exec, tally written on exit, one warning) or extend T33 with a `--learn` half.

### F9 · MAJOR (docs/design/dev-fanotify-reload.md:816)
T26 now fails (not skips) without `libphp`, as demanded — but the design never says where that failure is allowed. It is placed in `crates/blastit/tests/critical_loaded_files.rs`, which `make check` runs through `rs-test-critical` over every `crates/*/tests/critical_*.rs` on an offline host with no `libphp` (Makefile:262-295); the repo's precedent is the opposite — `critical_php_embed` takes a refusal branch in `make check`, and `php-sapi.yml:183-185`'s guard fails the job if that branch is taken, with `make rs-test-php` running the real pair. "(PHP-enabled target)" names no feature, cfg or target, so the row either breaks the unprivileged gate on every stock host or is never built where it claims to run. Put the PHP half in `make rs-test-php`'s suite (or a named cfg-gated test) and say which.

### F10 · MINOR (docs/design/dev-fanotify-reload.md:794)
T4's `handle_type` half is unconstructible. Two tmpfs directories cannot produce equal handle bytes under two `handle_type` values, and `HandleKey` is not in the public frozen surface, so the `critical_fanotify.rs` integration test can only exercise the bytes-inequality half behaviourally. Move the key assertions into a `fanotify.rs` unit test (where the private key is visible) or freeze a probe accessor.

### F11 · MINOR (docs/design/dev-fanotify-reload.md:795)
T5 has no valid-record case. All five inputs are malformed, so R4's positive rule (`hdr.len - 20 - handle_bytes`, 4-byte padding, stop at the first NUL) is exercised only indirectly by a real-kernel row. Add one crafted `DFID_NAME` record with a padded name and assert the joined dir/name plus `skipped == 0`; an off-by-`handle_bytes` slice then fails at the seam, not only when a host happens to fire.

### F12 · MINOR (docs/design/dev-fanotify-reload.md:802)
T12's "a non-UTF-8 path never reaches the type" is not enforceable as written: `DevReload.files` is `Vec<PathBuf>` (:665), which holds non-UTF-8, and the drop happens at the sender codec. State what the assertion really is — `encode_json` of an event holding a non-UTF-8 `PathBuf` yields valid UTF-8 JSON and names `files` in `truncated`, or the field is `Vec<String>`.

### F13 · MINOR (docs/design/dev-fanotify-reload.md:808)
T18 does not name its PostgreSQL upstream. The critical tier is explicitly offline (Makefile:295), so a live `DATABASE_URL` makes the row environment-dependent. Say it uses the in-repo wire stub (the `pgstub` harness) and assert the denied→allowed transition against that stub.

### F14 · MINOR (docs/design/dev-fanotify-reload.md:587, :798)
§4.7's mid-run latch has only half an oracle. T8 asserts `Effect::Stopped` and "no retry" in the unit, but the supervisor's observable half — one `manifest.warning` with the errno, both fanotify fds closed, no deadline, and a later save producing no reload and no second warning — is observed nowhere. T16 is the natural home (it already counts boot-failure warnings).

## Verdict

The revision genuinely closed the first pass's structural holes: the frozen accessors and mask equality plus per-bit twins (T1), the `parse_records` seam with size/align assertions (T5), the guarded overlay leg (T3), tmpfs T4 with the xfs observation moved to `MB.23.6`, the GIL unit plus behavioural T24, the EOF-counted differential T10, the whole-entry `MAX_EVENT_BYTES` rule and worst-case row (T12), and T17's check-host assertions. Strong rows needing no change: T1, T3, T6, T8, T9, T10, T11, T13, T15 (cap and forged-source halves), T16, T17, T19, T20, T21, T22, T23, T24, T25, T27, T28, T29, and T32's first two cases.

What remains is in the rows the rewrite added or asserted: three rows cannot pass on a correct implementation (T7, T14, T33), two added rows have no executable oracle (T30, T31), one row tests code its file does not own (T32's collector), T26 has no legal host under the offline gate, and two requirement clauses have no row at all (R10's `--learn` half, R14/S1's `Drop::Missing`). No BLOCKER: every §5 security claim has a row (T3, T15, T16, T17) and none of these is a CVE-critical claim. Not ready for acceptance.

## Resolution

Final check against the edited document; the second-pass paragraph above is historical. Every finding I raised is now real in the text:

- T-F1 (T33, :888): supervisor pid **unchanged** under `execve`, scheduler pid changed, `http`/dbproxy pids unchanged.
- T-F2 (T14, :869): exactly **two** `anon_inode:fanotify` links, the old generation gone.
- T-F3 (T7, :862): two windows 400 ms apart fire twice, the second at the 500 ms floor.
- T-F4 (T30, :885; §4.5 :449-480): `Policy::apply_report(report, paths, now)` is frozen, `paths` is the source's whole set, and the policy test calls it after `on_datagram`.
- T-F5 (T31, :886; §4.6 step 5 :581-585): `BLASTIT_TEST_RELOAD_PAUSE` gives a deterministic post-scheduler-stop window; T31 saves there and requires exactly one further `dev.reload` naming the file, so deleting `reconcile` is red.
- T-F6 (T32, :887): the collector cap in `loaded.rs`, the union/marks in `dev_reload.rs`.
- T-F7 (T26, :881; §8 4b :954-956): `PHP_SUITE_ARGS` gains `--test critical_loaded_files`, the image records a third executable (`$# -ne 2` → `3`), and the no-pair line is refused.
- T-F8 (T34, :889; T11, :866): `--learn` and `Drop::Missing` both have rows.
- T-M1/T-M2 (T4, :859; T5, :860): the pure `HandleKey` unit plus the tmpfs leg; a valid record after a malformed one.
- T-M3 (:723): `DevReload.files` is `Vec<String>`.
- T-M4 (T18, :873): CI's `BLASTIT_TEST_PG_URL` `test` job plus the offline unit half.
- T-M5 (T16, :874; §4.6 step 5 :585-588): the second debug-only seam, `BLASTIT_TEST_RELOAD_FAULT=read|mark`, makes the **real** watcher's first `read`/`mark_dir` return `EIO` once; both names sit in `scripts/check-no-test-seam.sh` beside `BLASTIT_TEST_SWAP_POINT` and are compiled out of release builds. T16 therefore drives the real supervisor to the mid-run latch it asserts — one `manifest.warning`, both fanotify fds closed, a later save silent — while T8 keeps the in-process `Effect::Stopped` half. The named integration file now has a fault source for the supervisor state.

No BLOCKER or MAJOR remains, and the amendments introduce no new one.

VERDICT: PASS
