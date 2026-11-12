---
id: MA.2.2.3
title: "Leaf 6: HTTP early upload abort and scheduler ingress enforcement"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MA.2.2.1]
paths:
  - "crates/blastit-http/src/**"
  - "crates/blastit-http/tests/**"
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T13:12"
---

# Goal

Enforce the `0.1.1` body gate on the hot path: `blastit-http` must stop reading an upload the moment the scheduler answers early (`400`/`404`/`413`/`503`/`504`) and close the connection, while `blastit-scheduler` plans bodies with `blastit-ingress` (`Raw` zero-buffer, `Json`/`Form` transcode + header scrub), rejects undeclared/`none` bodies with `413`, and serialises `--learn` unrouted requests behind one busy slot.

# Context

- Normative source: `docs/design/ma-lean-dx.md` §4.2 D.1/D.2, §4.3, §7.2 row `R12..R13, R16`, §8 Leaf 6; `ADR-0006`; requirements `R12`, `R13`, `R14..R16` (runtime wiring), `R26` (learn fallback queue).
- `blastit_http::chan::Stream` gains `pending: Option<Frame>`; `try_peek_response` pulls credit frames into credit and stores `RespHead`; `Stream::recv` drains `pending` first so `relay()` never loses the head. On early response: stop socket reads (via `Stream.pending`), append `("Connection", "close")`, flush, close.
- `Scheduler` owns `learn_fallback_busy: bool` + `learn_fallback_queue: VecDeque<u32>`; `RouteInfo` stays a pure value. Share the JSON-line `ctl_fd` protocol with `_check_pytest.py` (`push_routed`, `push_unrouted`, `pop`, `set_db`; 4096 B/line, 5 s deadline, `502` on error).
- Load skills `rust-standards`, `security-invariants`, `testing`.

# Acceptance

```console
$ cargo test -p blastit-http --test critical_early_abort --locked
$ cargo test -p blastit-scheduler --test critical_ingress_enforcement --locked
$ make check
```

- `crates/blastit-http/tests/critical_early_abort.rs` is **new**: a scripted `10 MiB` upload that panics if read after `RespHead(413)` sees the socket closed immediately, `CL: cap + 1` -> upfront `413`, chunked `cap + 1` -> `413` + `Connection: close`, and a normal upload still relays intact.
- `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` is **new**: `Raw` chunk 1 reaches the view before chunk 2 is sent (0 B buffered), chunked body `== cap` passes, valid chunked `json` reaches the view with `HTTP_TRANSFER_ENCODING` absent, non-empty body on an ingress-less or `none` route -> `413`, and concurrent unrouted `--learn` requests queue behind `learn_fallback_busy`.
- Tests use the existing in-memory/socketpair harnesses; no Docker, no wheel build.

# Notes

- ~420 changed lines across two crates; stays parallel with `MA.2.2.4` (dbproxy only).
- `RouteInfo::with_ingress` and `..SchedulerConfig::default()` already exist from `MA.2.1.1`.
- Do not implement audit findings or `check.rs` here (Leaf `MA.2.3.4`).
