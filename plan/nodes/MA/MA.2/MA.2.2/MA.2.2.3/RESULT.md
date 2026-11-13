# RESULT: MA.2.2.3 — HTTP early upload abort and scheduler ingress enforcement

STATUS: complete

## What changed

### `crates/blastit-http` (R13, design §4.2 D.1)
- `Stream` gained `pending: Option<Frame>` plus `try_peek_response`, `wait_response`,
  and `wait_upload` (`UploadWait::{Response, Credit, Closed}`). `Stream::recv`
  drains `pending` first, so a `RespHead` seen during an upload is never lost.
- `Conn` stops reading the client socket the moment a response frame arrives while
  streaming a fixed-length or chunked body, or while waiting for request-body
  credit (`BodyOutcome::Early`, `BodyFill::Responded`, `CreditWait::Responded`).
- An early answer is relayed with `Connection: close` forced (the view's own
  `Connection` header is dropped, `may_keep_alive` is forced off), then the write
  half is shut down so the client sees EOF immediately instead of waiting for
  `body_timeout`. No `CANCEL` is sent: the scheduler already answered.
- `Content-Length > BLASTIT_MAX_BODY_BYTES` still refuses before any stream is
  opened; `Dechunker` still caps decoded chunked bytes. Both are now protected by
  the new critical suite.

### `crates/blastit-scheduler` (R12, R14..R16, R26, design §4.2 D.2)
- `routes_from_manifest` now carries `Route::ingress()` into `RouteInfo`
  (previously it was left `None` and nothing enforced the manifest policy).
- Every routed request is planned from the head with
  `blastit_ingress::plan_body` (`BodyPlan::{StreamPassthrough, AllowEmptyOnly,
  Buffer}`):
  - `raw` streams with zero scheduler buffer;
  - a route with no ingress (or `body = "none"`) is `AllowEmptyOnly`: a positive
    `Content-Length` is `413` at the head, and the first non-empty `REQ_BODY` is
    `413` (an already-dispatched view is `CANCEL`led);
  - `json`/`form` buffer up to `BLASTIT_MAX_BODY_BYTES`, are re-encoded with
    `transcode_buffered` at `REQ_END`, and the view gets
    `apply_transcoded_headers` output (one canonical `Content-Length`, one
    canonical `Content-Type`, no `Transfer-Encoding`/`Content-Encoding`/`Expect`).
    Buffered requests grant consumed credit back to http so an upload may exceed
    one window before dispatch.
- `SchedulerConfig` gained `learn_mode` (default `false`) and `max_body_bytes`
  (default 100 MiB).
- `Scheduler` gained `learn_fallback_busy: bool` + `learn_fallback_queue:
  VecDeque<u32>`: with `learn_mode`, an unrouted request is not `404`ed; it is
  served by the first route's view one at a time (bounded by `queue_size` /
  `queue_timeout`, `503` on overflow/timeout), attributed without a route name.
  Unrouted bodies are `StreamPassthrough` so `--learn` never short-circuits them.
- `SchedulerConfig::default()` remains usable everywhere via `..default()`.

### Tests
- New protected suite `crates/blastit-http/tests/critical_early_abort.rs`.
- New protected suite `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs`.
- Standard scheduler tests adapted to the new fail-closed default: the shared
  `Options::new()` manifest and the four manifests that upload onto them declare
  `[route.ingress] body = "raw"`, preserving the old pass-through semantics of
  those tests (the new critical suite covers the no-ingress `413`).

## Acceptance evidence

```console
$ cargo test -p blastit-http --test critical_early_abort --locked
running 5 tests
test a_chunked_body_over_the_cap_is_413_and_closed ... ok
test a_normal_upload_still_relays_intact ... ok
test a_declared_body_over_the_cap_is_refused_before_it_is_read ... ok
test a_scripted_ten_mib_upload_stops_at_the_early_413 ... ok
test a_chunked_upload_stops_at_the_early_413 ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
```

```console
$ cargo test -p blastit-scheduler --test critical_ingress_enforcement --locked
running 7 tests
test routes_from_manifest_carries_the_declared_ingress ... ok
test a_non_empty_body_on_an_ingress_less_route_is_413 ... ok
test a_non_empty_body_on_a_none_route_is_413 ... ok
test a_chunked_json_body_at_the_cap_is_accepted ... ok
test a_valid_chunked_json_body_reaches_the_view_canonically ... ok
test raw_streams_chunk_one_to_the_view_before_chunk_two ... ok
test concurrent_unrouted_learn_requests_queue_behind_one_slot ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

```console
$ cargo fmt --check
fmt rc=0
```

```console
$ CARGO_NET_OFFLINE=true make check
...
cargo test -p blastit-dbproxy --test critical_learn_and_redaction
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.00s
cargo test -p blastit-http --test critical_early_abort
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
cargo test -p blastit-ingress --test critical_ingress
test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
cargo test -p blastit-manifest --test critical_manifest_011
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
cargo test -p blastit-manifest --test critical_manifest_edit
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
cargo test -p blastit-scheduler --test critical_ingress_enforcement
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
cargo test -p blastit-sqlpolicy --test critical_literals
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
uvx --offline ruff check tools
All checks passed!
uvx --offline ruff format --check tools
3 files already formatted
uvx --offline zensical build --strict
Build finished in 0.19s
uv run --no-project tools/plan.py validate
plan OK
```

`make check` exited `0` (71 `test result: ok` lines in the run, zero failures).

## Notes / bounds

- The upload loop can already have read up to one `INITIAL_WINDOW` (1 MiB) of body
  frames into the channel before the `RespHead` is observed by the connection
  task; after it is observed, no further client socket read happens and the
  connection closes. The protected test asserts the early answer closes within
  5 s (a regression would wait for the 600 s body deadline after the window is
  spent), that the client cannot upload 10 MiB, and that no body is forwarded
  beyond one window.
- `Request.bytes_in` stays the wire byte count; the canonical re-encoding does
  not rewrite it.

## Follow-ups

1. **Scheduler-side `ctl_fd` JSON-line client (`push_routed` / `push_unrouted` /
   `pop` / `set_db`)** is not wired here. It needs a `SchedulerConfig` seam for
   the dbproxy peer descriptor/path, which is created by stage-1/dev plumbing
   outside this node's `paths`; `blastit-dbproxy` (MA.2.2.4) already owns the
   server end (`check_ctl_fd`, 4096 B line, 5 s deadline). The scheduler's
   `learn_fallback_busy` slot is the scheduling half of that contract.
2. `learn_mode` and `max_body_bytes` must be populated by stage 1 from `--learn`
   and `BLASTIT_MAX_BODY_BYTES`; both default safely (`false`, 100 MiB).
3. `cargo fmt` rewrites the whole workspace. While implementing in parallel with
   MA.2.2.2/MA.2.2.4 it also formats their in-progress files; prefer
   `rustfmt <paths>` in parallel waves.
