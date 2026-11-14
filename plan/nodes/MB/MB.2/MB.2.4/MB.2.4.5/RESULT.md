# RESULT MB.2.4.5

**Status: done.** The three audit gaps (G7, G8, G9) plus the lead's zero-size-read
addition now each have a protected oracle that fails without the behaviour, and the
suite grew from 25 to 32 tests. **Two production changes were necessary** (both in
`crates/blastit-scheduler/src/scheduler.rs`, both loud below): the static buffered
`Forward` really did admit one more half-body than every other route (G9), and a
zero-size read really could make the pump loop until `request_timeout` instead of
aborting. Everything else was pinned, not changed.

## Files

| File | Change |
|---|---|
| `crates/blastit-scheduler/tests/critical_rewrite.rs` | +7 flow tests, 2 harness helpers (`one_blocking_thread`, `BlockingPool`), one `STATIC_CHUNK_BYTES` import |
| `crates/blastit-scheduler/src/scheduler.rs` | **production:** `forward_static` counts itself against the half-body bound; `pump_static` aborts on a zero-byte read |
| `docs/knowledge/LESSONS.md` | one bullet (how to hold a `spawn_blocking` read deterministically) |

No existing assertion was weakened or deleted, no `#[ignore]`, `tests/common/mod.rs`
needed no change (`Http::send` already carries a client `Frame::Cancel`).

## Gap table — gap → test → what fails without the behaviour

| Gap | Test(s) added | Proof it fails without the behaviour (run, output tail) |
|---|---|---|
| **G7a** client cancel on a parked-on-credit static stream: `client_gone`, one release, next request admitted | `a_client_that_goes_away_mid_body_releases_its_slot_once` | dropping `release_static_slot`+`release` from `client_cancel` → `panicked …: the slot and the descriptor went back with the cancel: left: 2, right: 1` |
| **G7b** a cancel while a `pread` is still parked keeps the slot *and* the descriptor until the result returns | `a_cancel_while_a_read_is_parked_keeps_the_slot_until_the_read_returns` | deleting the `static_read_parked` guard in `client_cancel` → `panicked …: the slot is still the parked read's: [1, 2]` |
| **G8a** `REQ_END` while queued must not re-enqueue (one FIFO entry) | `a_buffered_forward_that_ends_while_queued_keeps_one_queue_entry` | deleting `if queued { return Ok(()) }` in `request_end` → the third request gets `503` (`the request behind it was admitted: left: 503, right: 200`) |
| **G8b** the queued request keeps its `queued_since`, so `expire_queue` still fires | `a_buffered_forward_that_ends_while_queued_still_expires_on_queue_timeout` | deleting the `!` in `if !queued { … queued_since = None }` → the waiter is never answered (`no response for stream 2`, ~1 s, forever) |
| **G9** buffered `Forward` counts itself, exactly like `begin_raw` | `a_buffered_forward_counts_itself_against_the_app_paths_bound` (+ twin leg at `queue_size = 2`) | reverting the `forward_static` ordering → `the static path at queue_size = 1: left: 200, right: 503` while the app path (same config) is `503` |
| **lead addition (d)** a zero-size read aborts, and the read bound is on the wire | `the_pump_reads_the_documented_chunk`, `a_zero_byte_read_aborts_at_once_and_never_waits_for_the_timeout` | `STATIC_CHUNK_BYTES = 0` in a detached worktree: **with** the guard the suite fails in **0.13 s (13 failed)**; **without** it the pinning test runs past 60 s and the 180 s cap (`EXIT: 124`) — the TIMEOUT the lead reported. Dropping `bytes.len() == read.want` from `on_static_read` makes the zero-byte-read test hang past 60 s instead of aborting |

Pairs (testing §3): G7a/G7b are the two arms of the same transition (no read vs a
parked read); G8a/G8b are the two halves of one queue entry (kept once / still
mortal); G9 pairs the refusing `queue_size = 1` row with the admitting
`queue_size = 2` row, and each row is asserted against the app route with the same
config. `the_pump_reads_the_documented_chunk`'s twin is the abort test: a normal read
completes, a zero-byte one aborts.

## Production changes (justified, in scope: `scheduler.rs`)

### 1. `forward_static` — the half-body bound (G9, a real off-by-one)

`begin_raw` inserts the request with `body_ready = false` **before** it asks
`buffering_count()`, so the request counts itself (`queue_size = N` ⇒ `N - 1` others).
`forward_static` checked while `body_ready` was still `true` and cleared it afterwards,
admitting `N` — one more than any other route, contradicting its own comment
("the app path's own bound") and design §4.6. Fixed by clearing `body_ready` before the
check:

```diff
+            if let Some(request) = self.requests.get_mut(&stream_id) {
+                request.body_ready = false;
+            }
             if self.buffering_count() >= self.cfg.queue_size {
                 return self.reject(… 503 "queue full" …);
             }
             if let Some(request) = self.requests.get_mut(&stream_id) {
-                request.body_ready = false;
                 request.queued_since = Some(Instant::now());
             }
```

Before/after (the test that drove it):

```console
# without the change
test a_buffered_forward_counts_itself_against_the_app_paths_bound ... FAILED
assertion `left == right` failed: the static path at queue_size = 1: …
  left: 200
 right: 503
# with it
test a_buffered_forward_counts_itself_against_the_app_paths_bound ... ok
```

No other route changes behaviour: `queue_size = 8`/`4` tests keeps admitting one
buffered forward, and the streaming static queue bound (`admit_static`) is untouched.

### 2. `pump_static` — a zero-size read fails closed (lead's addition)

`want = min(remaining, credit, max_payload, STATIC_CHUNK_BYTES)` can only be `0` if a
bound is degenerate (`STATIC_CHUNK_BYTES = 0`), and `on_static_read` treats
`bytes.len() == read.want` as success, so a zero-byte read pushed an empty
`RESP_BODY`, advanced nothing, and asked again — burning the slot and CPU until
`request_timeout` answered a stream that had already failed. Now:

```diff
+        if want == 0 {
+            // A read of nothing can never advance the offset … Fail closed, on
+            // the same rule as a short read (design §4.6).
+            return self.abort_static(stream_id);
+        }
```

`abort_static` is the existing short-read path (`RespAbort { reason: "static: read
failed" }`, slot released once). Before/after with the constant forced to `0` in a
detached worktree at 69504ed + this branch's files:

```console
# with the guard
test the_pump_reads_the_documented_chunk ... FAILED
thread '…' panicked at critical_rewrite.rs:2173:5:
the response ends after [] body frames
test result: FAILED. 19 passed; 13 failed; … finished in 0.13s
# without it (guard removed, same mutant)
test the_pump_reads_the_documented_chunk has been running for over 60 seconds
EXIT: 124          # killed by `timeout 180` — the 90 s cargo-mutants TIMEOUT
```

The mutant moves back from TIMEOUT to CAUGHT, and it is `RespAbort`, not a park.

## Acceptance evidence

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked
running 32 tests
… (all 32 ok, including the 7 new ones)
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s
```

Run 7× (parallel), 7× with `-- --test-threads=1`, and the gated test 8× alone: 32/32
every time (no flakes; the gated rows are paced by the test, not by wall time).

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
test result: ok. 46 passed … (static_resolve)   ok. 11 … (static_assets)
ok. 9 …   ok. 32 (critical_rewrite)   ok. 14 …   ok. 18 …   ok. 9 …   ok. 17 …
ok. 6 …   ok. 2 …   ok. 13 …   ok. 18 …   ok. 14 …   ok. 14 …   ok. 1 (doc-test)
```

```console
$ CARGO_NET_OFFLINE=true cargo clippy -p blastit-scheduler --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.54s

$ cargo fmt --all -- --check
FMT_CLEAN
```

```console
$ uv run --no-project tools/critical_guard.py --base 69504ed --node MB.2.4.5
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [authorized by …]
  M  crates/blastit-scheduler/tests/critical_static_assets.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 1 protected critical test(s) changed …
```

The single violation is **not mine**: `MB.2.4.4`'s implementer is editing
`critical_static_assets.rs` in the same working tree (its mtime, 23:46, predates every
edit of this node). Authorizing both live nodes gives a clean pass:

```console
$ uv run --no-project tools/critical_guard.py --base 69504ed --node MB.2.4.5 --node MB.2.4.4
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.2.4.5, MB.2.4.4
```

(Note: `critical_guard` must run with `PYTHONHOME` unset; the `RUST_ENV` export that
`cargo` needs here breaks `uv`'s managed interpreter — `Failed to import encodings`.)

## How the deterministic parked read works (the one non-obvious piece)

A 64 KiB `pread` from a sparse tmpfs file is microseconds, so "the slot is still the
read's" has no stable window under the default 512-thread blocking pool. The two gated
tests build their own runtime —
`new_current_thread().enable_all().max_blocking_threads(1)` — and `BlockingPool::occupy`
takes the single blocking thread with a `std::sync::mpsc::recv()` (signalling "started"
over a oneshot) before the pump spawns its read. The read then cannot run until the test
releases the gate; `BlockingPool::drop` releases it, so a failing assertion can never
park the runtime. Paused clock + `advance` keep the timeout arms out of the way.
Logged in `docs/knowledge/LESSONS.md`.

## Follow-ups (not done here)

1. `/var/tmp/mb245-target` (the mutant worktree's scratch `CARGO_TARGET_DIR`) was left
   behind: the shell tool refuses `rm -rf` outside the workspace. Disk-backed, safe to
   delete (`rm -rf /var/tmp/mb245-target`); the worktree itself was removed.
2. No crate-level `cargo-mutants` campaign was run (a sibling runs Docker for MB.2.7):
   the two new production lines were proven by hand-mutations instead (each one's
   failing test is in the gap table). A short `-f '**/src/scheduler.rs'` campaign after
   this lands would confirm the ratio.
3. `crate/blastit-scheduler/tests/critical_static_assets.rs` is dirty from `MB.2.4.4`;
   nothing in this node touched it (`cargo fmt --all` did not rewrite it — mtime
   unchanged).
4. `on_static_read`'s `Ok(bytes) if … bytes.len() == read.want` still trusts
   `want`; a future mutant of `want` itself (e.g. `min` → `max`) is covered only by the
   chunk test's frame sizes. If a campaign misses there, add a `want` assertion next to
   it.
