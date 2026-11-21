---
id: MB.2.5.2
title: "Dispatch static answers and pump the body through the scheduler - design MB.2.5b"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.2.4.2, MB.2.5.1]
paths: ["crates/blastit-scheduler/src/scheduler.rs", "crates/blastit-scheduler/tests/critical_rewrite.rs"]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-13T20:52"
updated: "2026-11-13T23:01"
---

# Goal

Answer a static route from the scheduler itself: open the roots at boot, admit a static request without a view, and pump the file body on the existing `RawFrame` credit/window path with a bounded reader pool. This is the design's `MB.2.5b`.

# Context

- `docs/design/static-and-spa.md` §4.6 in full: `admit_ready`'s static branch before `can_dispatch`; `Request.static_target`/`static_body`/`pumping`; `Scheduler.static_roots: Vec<Option<Arc<StaticRoot>>>` opened by `run` and failing as `SchedulerError::Protocol("static root <root>: <source>")`; the `static_in` `mpsc` channel and its `select!` arm; `static_reads`; `can_dispatch_static()` = `slots < max_workers`; `dispatch_static` setting `slots`/`view = None`/`dispatched_at`/`deadline` exactly as `dispatch` does; both `dispatch_queued` walks and `Scheduler::start` treating such a request as dispatchable and **never** calling `try_spawn`; the same queue policy, `queue_timeout` and `503` + `Retry-After`; the `pump_static` steps 1–3 (zero-length → `RESP_END` and release **before** any read); "a parked read is bounded, not detached" (slot/`pumping`/`static_reads` released by the read result, never by the timeout; `expire_requests` and `client_cancel` release a view-less request exactly like a dispatched one); `besides`, `RespAbort { reason: "static: read failed" }` on a read error or short read, no truncated body under a promised `Content-Length`, `pread` of at most `min(remaining, credit, max_payload, STATIC_CHUNK_BYTES)`.
- §4.4: resolve/`decide` runs **before** `plan_request_body`, and a selected static target never buffers or transcodes. §4.3 steps 1–2 explain why the health path and non-static routes are untouched; the `request` event keeps its `route` and `rule`. §7.2 (`R6`, `R8`, `R12` → `critical_rewrite.rs`).
- Consumes `static_asset.rs` (`MB.2.4.1`), `static_response.rs` (`MB.2.4.2`), `rewrite::decide` + `RouteInfo::with_static` (`MB.2.5.1`) and `SchedulerConfig::{manifest_dir, static_max_bytes}` (`MB.2.4.1`).
- Out of scope: `crates/blastit/**` boot/CLI/`check` wiring and the banner (`MB.2.6`), `e2e/**` (`MB.2.7`), the docs leaves. Keep the diff localized to dispatch, the pump and release; everything that can be pure already lives in `rewrite.rs`.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_rewrite --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.2.5.2
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
```

# Notes

- Append the flow half to the existing protected `tests/critical_rewrite.rs` (design §8 gives that file to `MB.2.5.1` and `MB.2.5.2`; this leaf runs strictly after `MB.2.5.1`, so never edit it concurrently). Cases per §7.2: static answer with no view fork, `HEAD` with no `RESP_BODY`, one `request` event with `route`/`rule`/`outcome == "ok"` and an unchanged `log.spawns`, the route's script never seeing `REQ_HEAD`, `503` + `Retry-After` at `max_workers` live static streams, two concurrent static streams interleaving, a client that stops granting credit aborted at `request_timeout`, the slot released exactly once on every path (including timeout and client cancel), `can_dispatch_static` false at the bound, and the fd count not growing while `max_workers` streams are parked yet a further stream still completes.
- Budget: ≤ 3 files, ≤ 600 changed lines. Reuse the existing batching/credit path: no second write loop, no per-request thread, no new dependency, no `Cargo.toml` edit.
- If the pump needs a change in a file outside `paths`, stop and open a Follow-up rather than widening the diff.
