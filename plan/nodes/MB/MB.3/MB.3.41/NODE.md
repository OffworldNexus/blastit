---
id: MB.3.41
title: "A completed 5xx must say why it failed: contained bailout vs the app's own 500, and the scheduler's 502/504 must not pay for queue wait"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.38]
paths:
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-view/src/**"
  - "crates/blastit-php-sapi/src/**"
  - "crates/blastit-frame/src/**"
  - "crates/blastit-events/src/**"
  - ARCHITECTURE.md
  - crates/blastit/tests/critical_php_embed.rs
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T02:02"
updated: "2026-11-15T04:06"
---

## Goal

Make a completed `5xx` truthful about *why* it failed, so `outcome: "view_died"`
means what the events crate says it means, and a `5xx` charge never punishes a
request the scheduler itself failed.

## Context

- `MB.3.38` charged a contained PHP bailout at least its wall time and, to keep
  it out of `outcome: "ok"`, mapped **every** completed `5xx` to `view_died`.
  A script-set `http_response_code(500)` (PHP), a Django `500`, a Node or JVM
  `500` therefore now reads as "view process died mid-request, 502"
  (`crates/blastit-events/src/lib.rs` gloss), which widens a user-visible
  outcome beyond `MB.3.13`.
- Contained-vs-script is not on the wire today: the shim knows `contained = 1`
  and the frame carries only the status, so the scheduler cannot tell a
  bailout from an app's own `500`.
- `ARCHITECTURE.md`'s `cost_ns` clamp / `502` charge paragraphs are now false
  for `5xx` and must state the crash rule.

## Acceptance

```console
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit --locked
$ make check
$ uv run --no-project tools/plan.py validate
```

- An app's own `500` is distinguishable from a contained bailout in the
  `request` event (either a distinct outcome/field the shim sets on the wire,
  or a documented rule that survives an adversarial reading). Say which and
  why in `RESULT.md`; do not relabel both as `view_died`.
- A scheduler-generated `502`/`504` keeps its dispatch→completion charge and
  its own outcome (no queue-wait bill), pinned by a test.
- `ARCHITECTURE.md` says what the code does for `cost_ns` on a `5xx`, a
  timeout and a crash-without-report, and no paragraph claims the old clamp.
- `make check` green; protected tests untouched unless `critical_guard
  --node MB.3.41` authorizes it.

## Notes

- Prefer the smallest wire change that makes the distinction real: one flag on
  the completion frame beats inferring intent from a status code.
- `docs/design/backend-runtimes.md` R19 is the design of record; if the rule
  needs amending, record the amendment rather than a second rule.
