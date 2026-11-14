---
id: MB.3.13.1
title: "MB.3.13 part 1: PhpEmbedRuntime worker loop over view.chan: route_uid, EmbeddedView filter, max_requests recycling"
kind: task
status: active
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-view/src/**"
  - "crates/blastit-view/Cargo.toml"
  - "crates/blastit-view/tests/**"
  - "crates/blastit/src/**"
  - crates/blastit/Cargo.toml
  - "crates/blastit-php-sapi/src/**"
  - Cargo.lock
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T17:04"
updated: "2026-11-14T18:31"
---

# Goal

PhpEmbedRuntime worker loop over view.chan: route_uid, EmbeddedView filter, max_requests recycling

# Context

- Design of record: `docs/design/backend-runtimes.md`, `ADR-0010` (`MB.3.13` part 1).
- The worker is an ordinary view worker: `UID_BASE + 1000 + i`, a private runtime dir with
  only granted proxy sockets, `max_requests` recycling. The PHP-specific parts are the
  `dlopen` of the host NTS `libphp.so`, the matching shim from `MB.3.12`, and the vtable.
- The boundary is the **seccomp filter**; the INI policy is defence in depth (see the egress
  doctrine in `MB.3.13`'s parent body).
- Never lazy: a boot-time failure is `exit 1` with an actionable message, never a
  first-request fault.

# Acceptance

```console
$ cargo test -p blastit --locked
$ make check
```
- The rule this part owns has a denied case and an accepted twin, and the failure is a boot
  refusal rather than a degraded request.
- No `libphp` symbol is reachable outside the view worker.
- No backend-kind `match` in dispatch.
