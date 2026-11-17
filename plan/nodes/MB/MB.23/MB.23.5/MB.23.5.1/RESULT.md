# RESULT — MB.23.5.1

## STATUS

done

## Summary

Landed design §8 commit **5a**. Verified `6d07742`'s frame/scheduler half rather than
rewriting it: `TypeCode::Reload = 0x45`, `Ctl::Reload` (no route, empty payload),
`Frame::decode`'s ctl arm, `UnixCtl::take_frame`/`finish_frame`, `supervise::on_ctl`'s
init-sender arm and `Scheduler::reload()`'s one all-views transaction were already
correct. Changed only what the node requires:

- `RetireReason::Reload` (`as_str() == "reload"`) replaces the scheduler's stringly
  `RetireReason::Other("reload")` in all four reload sites (`reload`, stale marking,
  stale-at-birth on `SPAWNED`, stale at `READY`); the `RELOAD` const is gone.
- `ReloadReason` (closed `loaded|preload|blastfile|overflow`), `DevReload`
  (`routes`, `files`, `reason`) and `DevReloadUnavailable` (`errno`, `filesystem`) in
  `blastit-events`, with `EventKind::{DevReload,DevReloadUnavailable}`,
  `info`/`warn` levels, JSON encode + decode, pretty lines and the whole-entry array
  truncation arm (files tail first, then routes; `truncated` names the array;
  `MAX_EVENT_BYTES`/2048 untouched).
- `ARCHITECTURE.md`: §3 `ctl.chan` carries row + one `RELOAD` rule bullet, §3.1 frame
  table row (`RELOAD | 0x45 | empty | init → scheduler`), §8.1 `process.retired` reason
  list gains `reload`.

No dependency/feature/workspace change; `Cargo.toml`/`Cargo.lock` byte-identical. No
watcher, no CLI, no `dev_reload`, no supervisor edit (MB.23.5.2). No route payload and no
per-route selection anywhere.

## Evidence

### `cargo test -p blastit-frame --locked ctl`

```console
$ cargo test -p blastit-frame --locked ctl
running 4 tests
test frame::tests::ctl_reload_is_0x45_and_has_no_payload ... ok
test frame::tests::ctl_reload_refuses_a_payload_and_a_non_zero_id ... ok
test decoder::tests::ctl_messages_round_trip_through_the_decoder ... ok
test tests::encoded_len_rejects_exactly_what_encode_rejects ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 69 filtered out; finished in 0.00s
```

Pins: `0x45` ↔ `Ctl::Reload` through `TypeCode::from_code`/`name`/`is_ctl`, `Frame`'s
ctl arm, the real `Decoder` stream (id 0, header-only payload), a payload-carrying `0x45`
refused by `reader.finish()`, and a non-zero id refused before any byte is written.

### `cargo test -p blastit-scheduler --locked reload`

```console
$ cargo test -p blastit-scheduler --locked reload
test ctl::tests::reload_is_accepted_without_a_descriptor_and_refused_with_one ... ok
test a_spawn_sent_before_a_reload_is_retired_on_arrival ... ok
test reload_retires_every_idle_worker_of_every_route ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 49 filtered out
...
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 18 filtered out
```

The transaction is pinned by `tests/pool.rs` (outside this node's paths, unchanged): one
`Ctl::Reload` retires route 0's idle worker (`(pid, "reload")`), leaves route 1's busy
worker running until its `DONE`, never dispatches a second request to it, and retires a
`SPAWNED` born before the reload at once. The new `ctl` test asserts a descriptor next to
`RELOAD` is a protocol error naming `RELOAD`.

### `cargo test -p blastit-events --locked`

```console
$ cargo test -p blastit-events --locked
test result: ok. 91 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.03s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

New: `dev.reload` shape/key order + round trip; `dev.reload.unavailable` shape; unknown
reload reason/missing array is not an event; worst-case `64 × 4096` files + `64 × 256`
routes encodes `<= 2048` with whole tail entries and `truncated: ["files", ...]`; a
65-route reload keeps its first entries and names `routes`; `process.retired` `reload`
round-trips typed while `Other("oom")` still round-trips; pretty prints
`-  routes=3  reason=loaded  files=2` and §4.7's sentence; every named `RetireReason` is
typed and distinct.

### `cargo clippy -p blastit-frame -p blastit-scheduler -p blastit-events --all-targets --locked -- -D warnings`

```console
$ cargo clippy -p blastit-frame -p blastit-scheduler -p blastit-events --all-targets --locked -- -D warnings
    Checking blastit-scheduler v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.93s
EXIT=0
```

### `cargo fmt --all -- --check`

```console
$ cargo fmt --all -- --check
FMT_OK           # EXIT=0
$ git diff --quiet -- Cargo.toml Cargo.lock
LOCK_IDENTICAL   # EXIT=0
```

### `make check` — one known pre-existing failure only

`make check` stops at `docs-test` (11th of 20 targets), so it was rerun as
`make -k check` to prove the remaining nine targets: only `docs-test` fails.

```console
FAIL  docs/reference/cli.md  fence 1  dev-flags             0.3s  blastit dev exited before ready; stdout:

stderr:
blastit dev: unknown argument `--no-reload`
...
48 fences: 30 executed, 18 accounted, 1 failed · 29.3s
make: *** [Makefile:365: docs-test] Error 1
```

Everything else ran and passed (guard, critical-guard, rs-fmt, rs-lint, rs-doc,
rs-test/test-critical, py-lint, docs-unit, docs-check, rs-no-python, seam-check,
platform-matrix-check, workflow-check, packaging-gates-test, doc-tone, manifest-purity,
zed-check, `plan OK`). The one failure is `docs/reference/cli.md`'s `--no-reload` fence,
delivered by MB.23.5.2; nothing was added to any allowlist.

### `uv run --no-project tools/critical_guard.py --base 6d07742 --node MB.23.5.1`

```console
$ uv run --no-project tools/critical_guard.py --base 6d07742 --node MB.23.5.1
critical-guard: 6d07742..working tree, 11 changed file(s)
node MB.23.5.1: paths [...]
  no change to a protected critical test
EXIT=0
```

(11 changed files = my 8 scoped files + the lead's pre-existing `plan/` edits in the
worktree: `plan/JOURNAL.md`, `plan/nodes/.../MB.23.5/{LOG.md,NODE.md}` and the new child
node dirs. I did not touch `plan/`.)

## Files

- `crates/blastit-frame/src/frame.rs` — tests only (T13 sites; frame vocabulary untouched)
- `crates/blastit-scheduler/src/ctl.rs` — descriptor-refusal test only
- `crates/blastit-scheduler/src/scheduler.rs` — `RetireReason::Reload` at the four reload
  sites, `RELOAD` const removed, doc list
- `crates/blastit-events/src/lib.rs` — `ReloadReason`, `DevReload`,
  `DevReloadUnavailable`, `RetireReason::Reload`, two `EventKind`s + names/levels
- `crates/blastit-events/src/json.rs` — encode/decode, whole-entry array truncation
- `crates/blastit-events/src/pretty.rs` — both pretty forms
- `ARCHITECTURE.md` — §3 row + rule, §3.1 frame row, §8.1 reason
- `crates/blastit-frame/src/ctl.rs` — verified, unchanged

## Follow-ups

- `tests/pool.rs` (outside paths) already pins the all-views transaction with pid sets and
  none of its assertions pin a pid unchanged across a reload; nothing to delete.
- `crates/blastit-sys/src/testutil.rs` still carries its deliberately non-exhaustive
  `0x43, 0x44, 0x45` list; untouched and green.
- MB.23.5.2 closes the one `docs/reference/cli.md` fence by landing `--no-reload`.
