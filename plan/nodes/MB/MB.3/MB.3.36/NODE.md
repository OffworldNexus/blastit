---
id: MB.3.36
title: "Stage-1 probe child: load the shim and prove the minor before the listener binds (ADR-0010 s3)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.13.3, MB.3.13.4]
paths:
  - crates/blastit/src/php_view.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/tests/critical_php_embed.rs
  - docs/knowledge/LESSONS.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T00:59"
updated: "2026-11-15T01:38"
---

## Goal

Prove the shim before the listener: fork one contained probe child per
`[route.php]` route **before** `bind_tcp`, run `blastit_php_worker_init` +
the ABI/minor check there, and `exit 1` with ADR-0010 §4.1's sentence if the
shim cannot load or does not match the library — as `ADR-0010` §3 requires.

## Context

- `MB.3.13.3` wired the switch end-to-end and `MB.3.13.4`'s protected suite
  caught the remaining gap: `boot_bindings` re-opens and pins `libphp`/shim
  and verifies `(st_dev, st_ino)`, but nothing loads the shim at boot, so a
  shim whose `blastit_php_minor()` disagrees with the library is refused by
  the **worker at the first request** (a 500), not before the listener.
  `ADR-0010` §3: "forks one contained probe child per `[route.php]` route
  **before** `bind_tcp(0.0.0.0:$PORT)` … running `blastit_php_worker_init` +
  `blastit_php_worker_shutdown` (`BLASTIT_PROBE_TIMEOUT_MS` default `5000`
  -> `kill(-pgid, SIGKILL)`)".
- `.13.4` already has the `Some(code)` arm that asserts exactly the
  boot-refusal shape, so closing this is a contained change plus a one-line
  flip in the test.

## Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
$ cargo test -p blastit --locked
$ make check
$ uv run --no-project tools/plan.py validate
```

- With a shim stub that reports the wrong minor, a boot exits `1` **before**
  any listener binds (assert nothing is listening and no `ready`/`spawned`
  event), naming the route, `(PHP X.Y, <arch>-<libc>)` and
  `BLASTIT_LIBPHP`.
- A probe child never becomes a served worker: it runs with its own `-pgid`,
  the route uid, `SeccompRole::EmbeddedView { egress }`, and is killed at
  `BLASTIT_PROBE_TIMEOUT_MS` (a stub whose `worker_init` hangs must not hang
  the boot; prove the timeout path).
- The correct pair still boots and serves `200` (the accepted twin); the
  existing `critical_php_backend` / `critical_php_embed` suites stay green
  with the `Some(code)` arm tightened to the boot-refusal shape.
- `make check` green; `critical_guard --node MB.3.36` covers the test edit.

## Notes

- Keep `dlopen` inside the probe child, never in stage 0/1 proper: the
  probe is a `fork` + `drop_privileges` + seccomp, then load.
- The inode pin and `O_NOFOLLOW` re-open stay as they are; this node adds the
  load-and-match step, not a second verification path.
