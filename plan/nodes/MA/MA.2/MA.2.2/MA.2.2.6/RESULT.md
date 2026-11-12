# RESULT — MA.2.2.6

**Status:** done

**Changed:** `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` (test-only; +2 tests,
module doc line; no production code, no manifest/Cargo change, no `#[ignore]`).

- `a_buffered_body_over_the_cap_is_refused_before_req_end` — chunked `json` `POST /json/`,
  `cfg.max_body_bytes = 9`, one 10-byte valid JSON frame (`{"a":1234}`), **no** `REQ_END`:
  `413` + terminating frame within a paused-clock `timeout(5 s)`, `queue_timeout = 3600 s` so
  only the test's own bound can end the wait; the view saw no `REQ_BODY` and no `REQ_HEAD`.
- `a_full_learn_fallback_queue_answers_503_instead_of_growing` — `learn_mode`, `queue_size = 1`,
  `Script::Hold`; first unrouted request takes the slot (barrier: `request_ids(0).len() == 1`),
  second fills the one waiting place, third (frames are ordered on one `http.chan`, so no sleep is
  needed) gets `503` + `body = "queue full\n"` + `Retry-After: 3600` via `awaited_response` under
  `timeout(5 s)`; the view never saw it. Releasing the first serves the second, and only then.
- Untouched: the at-the-cap accept twin, the one-slot queueing test, and the other five cases.

## Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_ingress_enforcement --locked
running 9 tests
test routes_from_manifest_carries_the_declared_ingress ... ok
test a_non_empty_body_on_an_ingress_less_route_is_413 ... ok
test a_buffered_body_over_the_cap_is_refused_before_req_end ... ok
test a_non_empty_body_on_a_none_route_is_413 ... ok
test a_chunked_json_body_at_the_cap_is_accepted ... ok
test a_valid_chunked_json_body_reaches_the_view_canonically ... ok
test a_full_learn_fallback_queue_answers_503_instead_of_growing ... ok
test concurrent_unrouted_learn_requests_queue_behind_one_slot ... ok
test raw_streams_chunk_one_to_the_view_before_chunk_two ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Crate-wide (no regression from the edited file):

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
test result: ok. 40 / 11 / 9 / 9 / 17 / 6 / 13 / 18 / 14 / 14 / 1 passed; 0 failed; 0 ignored
```

## Mutation proof (scratch copy, never the tree)

`rsync` of the sources (no `target/`, no `.git/`) to `/tmp/opencode/blastit-ma226-scratch`,
`CARGO_TARGET_DIR=/tmp/opencode/blastit-ma226-target`, one anchor-checked `sed` per run.

```console
# pristine scratch tip: both new tests green
running 9 tests ... test result: ok. 9 passed; 0 failed; 0 ignored

# sc.buffer_cap_off: scheduler.rs:1359 `if total > self.cfg.max_body_bytes` -> `if false && ...`
test a_buffered_body_over_the_cap_is_refused_before_req_end ... FAILED
  panicked at critical_ingress_enforcement.rs:274: no 413 before REQ_END: the per-frame body
  cap was not enforced: Elapsed(())
test result: FAILED. 0 passed; 1 failed; 0 ignored

# sc.learn_queue_overflow_off: scheduler.rs:1029 `if self.learn_fallback_queue.len() >= ...` -> `if false && ...`
test a_full_learn_fallback_queue_answers_503_instead_of_growing ... FAILED
  panicked at critical_ingress_enforcement.rs:456: the third unrouted --learn request was queued
  instead of refused: Elapsed(())
test result: FAILED. 0 passed; 1 failed; 0 ignored
```

Both killing failures are the intended ones (a missing response under the mutant), not a compile
or harness error; the scratch source was restored afterwards, the working tree was never mutated.

## Other gates

```console
$ CARGO_NET_OFFLINE=true cargo clippy -p blastit-scheduler --tests --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s)
$ cargo fmt --check -p blastit-scheduler          # exit 0
$ uv run --no-project tools/critical_guard.py --base HEAD --node MA.2.2.6
  M  crates/blastit-scheduler/tests/critical_ingress_enforcement.rs  [authorized by crates/blastit-scheduler/tests/critical_ingress_enforcement.rs]
```

`make check` is **red in this worktree for reasons outside this node**: it stops at `rs-fmt`
with diffs in `crates/blastit-manifest/tests/critical_manifest_011.rs`,
`critical_manifest_edit.rs` and `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs`,
i.e. the two sibling pins (MA.2.2.5/7/8) editing their protected files concurrently in the same
checkout. `cargo fmt --check -p blastit-scheduler` is clean, and the crate's tests and clippy are
green as shown above.

## Follow-ups

- **`make check` must be re-run by the lead after the three MA.2.2 test pins land** (rs-fmt sees
  the sibling `crates/blastit-manifest/tests/*` and `crates/blastit-dbproxy/tests/*` edits as
  unformatted). Nothing to fix in this node's file.
- **Paused-clock test gotcha (not added to LESSONS: `docs/**` is outside this node's paths):**
  under `tokio::time::pause()`, `Http::response()` spin-waits with `yield_now`, so the runtime is
  never idle and the clock never auto-advances — a `tokio::time::timeout` wrapped around it never
  fires. Wrap `Http::awaited_response()` instead, which parks on the socket and lets the paused
  clock advance to the timeout. `Http::awaited_response` is at `tests/common/mod.rs:463`.
- No production-code defect was found; the two mutant branches behave exactly as `AUDIT-MA.2.2`
  G2 describes.
