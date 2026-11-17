---
id: MB.23.5.1
title: "Ctl::Reload (0x45, no route payload) in the frame and scheduler, the all-views retire transaction, and the two dev events"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-frame/src/frame.rs"
  - "crates/blastit-frame/src/ctl.rs"
  - "crates/blastit-scheduler/src/ctl.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-events/src/lib.rs"
  - "crates/blastit-events/src/json.rs"
  - "crates/blastit-events/src/pretty.rs"
  - ARCHITECTURE.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T12:37"
updated: "2026-11-17T13:08"
---

# Goal

Land design §8's commit **5a** as one green, self-contained leaf: the `RELOAD` control
frame (`0x45`, no payload) at every receiver site, the scheduler's one all-views retire
transaction, the reason word `reload`, the two dev events in JSON + pretty, and the three
`ARCHITECTURE.md` rows. No supervisor, no watcher, no CLI and no `dev_reload` module — this
leaf must compile and pass on its own.

# Context

- Read `docs/design/dev-fanotify-reload.md` §4.6 (the two reload transactions), §4.9 (the
  observable surface, both encodings) and §8's 5a line; §3.1's frame table is the wire
  contract. Ground truth for the kernel side is
  `docs/research/2026-11-16-dev-fanotify-reload.md`; nothing here re-derives it.
- **Verify, do not rewrite, what `6d07742` already landed:** `TypeCode::Reload = 0x45`
  with `from_code`/`name`/`is_ctl`, `Ctl::Reload` with `type_code`/`write_payload`/
  `decode`, `Frame::decode`'s ctl arm, `supervise::on_ctl`'s "init is the sender" arm, and
  `Scheduler::reload()` (the `reload_epoch`, `stale: Option<RetireReason>`,
  `Ctl::Reload => self.reload()`). Missing today: the `blastit-events` half below and the
  `ARCHITECTURE.md` rows. Prove the existing half with the T13/T21 assertions; rewrite only
  what those assertions prove wrong.
- `Ctl::Reload` carries **no route payload** and retires **every** active view's idle
  workers together with `RetireReason::Reload`. init never `SIGKILL`s a view mid-request —
  the scheduler keeps that decision (`stuck_threads` stays the only thing that can end a
  request that never returns). The `:8080` listener and the proxies are never retired or
  re-bound. No per-route selection, no route map, no "reload one view" path.
- The reason set grows by exactly one word, `reload`: `RetireReason::Reload` with
  `as_str() == "reload"` (§4.9 names the typed variant), and the scheduler's
  `RetireReason::Other("reload")` is replaced by it. On the wire a `RETIRE` still carries
  `reason string32`, so `Ctl::Retire`'s encoding is unchanged.
- Events, field names frozen by `MB.23.1` (which already published
  `docs/reference/log-events.md` — do not edit that page): `dev.reload` (`info`: `routes`
  = every route serving after the reload, in manifest order; `files` = absolute paths,
  sorted; `reason` ∈ `loaded|preload|blastfile|overflow`); `dev.reload.unavailable`
  (`warn`: `errno` symbolic, `filesystem`); `reload` added to `process.retired`'s reason
  list.
- `DevReload`/`DevReloadUnavailable`/`ReloadReason` are plain field holders like the
  existing events; `EventKind::DevReload` is `info`, `EventKind::DevReloadUnavailable` is
  `warn`. Both arrays are cut from the tail, whole entries only, adding the array's name to
  `truncated`, so `encoded_len <= MAX_EVENT_BYTES` (2048) by construction (§4.9, §4.10).
  `json.rs` writes both arrays with `push_str_array`; `pretty.rs` prints
  `-  routes=3  reason=loaded  files=2`, and the unavailable arm is §4.7's
  `reload disabled: … (<ERRNO> on <filesystem>); serving without live reload` sentence.
- `ARCHITECTURE.md` gains three rows only: §3's channel table + rule (`RELOAD`,
  init → scheduler, empty, one all-views transaction), §3.1's frame table
  (`RELOAD | 0x45 | empty | init → scheduler`), §8.1's `reload` reason. One line each, no
  new prose section.

# Acceptance

```console
$ cargo test -p blastit-frame --locked ctl
$ cargo test -p blastit-scheduler --locked reload
$ cargo clippy -p blastit-frame -p blastit-scheduler -p blastit-events --all-targets --locked -- -D warnings
$ make check
$ uv run --no-project tools/critical_guard.py --base 6d07742 --node MB.23.5.1
```

1. **Every receiver site (T13).** `0x45` encodes with `id == 0` and an empty payload; a
   payload-carrying `0x45` is refused by `reader.finish()`; a descriptor next to it is
   refused by `finish_frame`; and each of `TypeCode::from_code`, `TypeCode::name`,
   `TypeCode::is_ctl`, `Frame::decode`'s ctl arm, `Ctl::type_code`, `Ctl::write_payload`,
   `Ctl::decode` and `supervise::on_ctl`'s init-sender arm has its arm — the four
   catch-all sites must be asserted by a test, not by the compiler.
2. **The transaction (T21).** `Ctl::Reload` reaches `Scheduler::reload()` and not the
   protocol-error catch-all; idle (or not-yet-ready) views retire with `reload`; a
   mid-request view is **not** retired until `DONE` and is never killed; no dispatch
   reaches a stale view; a `SPAWNED` that arrives after the reload is retired at once;
   queued requests are served by the fresh view; `reload()` is idempotent.
3. **No surgical restart.** No test, old or new, pins one route's pid unchanged across a
   reload or asserts "only one view restarted"; such an assertion is a bug and is deleted.
4. **The reason word.** `RetireReason::Reload.as_str() == "reload"`, the JSON/pretty
   `process.retired` encoding is unchanged apart from the new value, and
   `RetireReason::Other` still round-trips its free-form word.
5. **The two events (both encodings).** `dev.reload` is `info` with `routes`/`files`/
   `reason` in §4.9's order and `ReloadReason::as_str()` over the closed four-value set;
   `dev.reload.unavailable` is `warn` with symbolic `errno` + `filesystem`; `pretty.rs`
   prints `-  routes=3  reason=loaded  files=2` and the §4.7 sentence.
6. **The 2048-byte bound (T12).** A worst-case `DevReload` (64 × 4096-byte files,
   64 × 256-byte routes) encodes to `<= MAX_EVENT_BYTES`, the tail is cut whole, and
   `truncated` names `files`/`routes` respectively; a 65-route event keeps its first
   entries and names the truncation.
7. **`make check`** is green except the one known pre-existing failure:
   `docs/reference/cli.md`'s fence 1 fails because the page already documents
   `blastit dev --no-reload` and the flag lands in `MB.23.5.2`. Record that exact verdict
   tail in `RESULT.md`; add nothing else to any allowlist.

# Notes

- `Cargo.toml` and `Cargo.lock` stay **byte-identical**: no new dependency, no feature, no
  workspace member. Prove it with `git diff --quiet -- Cargo.toml Cargo.lock`.
- No new knob, flag or config key anywhere; `--no-reload` is `MB.23.5.2`'s and this leaf
  adds no CLI surface at all.
- Never a second watcher and never a second event loop: nothing in this leaf starts
  watching anything.
- Production and `check` never construct `dev_reload`; that module does not exist yet and
  this leaf must not create it.
- `crates/blastit-sys/src/seccomp.rs` is untouched; the frame/scheduler changes are policy
  plumbing only, and `blastit-frame`'s crate layering is unchanged (no new dependency
  edges).
- The scheduler change is one transaction, not a selection algorithm: keep `reload()` as
  it is unless a T21 assertion fails, and do not reintroduce a route map or a reload epoch
  rewrite.
- `crates/blastit-sys/src/testutil.rs` enumerates every type code (`0x43, 0x44, 0x45`) — a
  deliberately non-exhaustive list; it is not in this node's paths, so leave it alone and
  note a follow-up only if a test there turns red.
- `make check` here is expected to show exactly the one documented failure. Do not "fix"
  it here: `MB.23.5.2` closes it by landing the flag. Paste the tail into `RESULT.md` so
  the next leaf can prove the delta.
- Keep the diff to these 8 files. If an event needs a helper outside them, record a
  `## Follow-ups` item instead of widening the leaf.
- Do not edit `docs/reference/log-events.md` (`MB.23.1` shipped it) and do not edit
  `docs/design/dev-fanotify-reload.md` (only `MB.23.5.2` corrects §4.2's two sentences).
- The array-truncation arm is the only edit to `json.rs`; keep the documented
  `MAX_EVENT_BYTES` and existing string-cut behaviour untouched, since four shipped events
  depend on it.
- Every new assertion runs under `make check` (unprivileged, offline) — no kernel, no
  Docker, no service, and zero `sleep`-based verdicts.
