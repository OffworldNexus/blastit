---
id: MB.9.2.1.3
title: "Re-export the boot surface and wire roles::scheduler to run_with - design MB.9.2a part 3"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.9.2.1.2]
paths: ["crates/blastit-scheduler/src/lib.rs", crates/blastit/src/roles.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T10:51"
updated: "2026-11-14T10:51"
---

# Goal

Expose the scheduler's new boot surface and wire the composition root: `lib.rs` gains the
`pub use` re-exports of `builtin_registry`/`run_with` and `RunError`/`SchedulerError`, and
`crates/blastit/src/roles.rs` calls `run_with(builtin_registry()?, …)` and prints a
`RunError::Boot` before returning `1` (design leaf `MB.9.2a`, part 3).

# Context

- Design of record: `docs/design/php-fastcgi.md` §4.2 (`run_with`, `RunError`,
  `builtin_registry`, the `roles::scheduler` composition root) and §8's `MB.9.2.1.3` row;
  §8.2's `MB.9.2.1` row: "`.3` only re-exports `builtin_registry` from `lib.rs` and adds
  the `roles::scheduler` `RunError` print".
- Files (the whole leaf, 2): `crates/blastit-scheduler/src/lib.rs`,
  `crates/blastit/src/roles.rs`.
- What lands:
  - `lib.rs`: `pub use crate::scheduler::{builtin_registry, run_with};` and
    `pub use crate::error::{RunError, SchedulerError};` — **re-exports only**. The
    definitions live in `MB.9.2.1.2`'s `scheduler.rs`/`error.rs`; do not move or
    duplicate them, and do not declare `mod upstream;` again (`.2` owns the crate root).
  - `roles.rs`: the `scheduler` role calls `run_with(builtin_registry()?, …)`, and on
    `RunError::Boot(e)` writes `eprintln!("{e}")` (captured by init's stderr pipe as
    `app.output` in production, printed on fd 2 in `dev`) and returns `1`.
- No `Cargo.toml`, no `Cargo.lock`: this leaf adds no dependency and no member.

# Acceptance

```console
$ cargo test -p blastit --locked
$ grep -q 'pub use crate::scheduler::{builtin_registry, run_with};' crates/blastit-scheduler/src/lib.rs && echo "boot surface re-exported"
$ grep -q 'RunError' crates/blastit/src/roles.rs && echo "composition root prints Boot"
$ make check
```

# Notes

- Keep this leaf tiny on purpose: it is the seam that lets `MB.9.2.2`/`MB.9.3` register
  builders without touching `roles.rs` again.
- `crates/blastit/src/roles.rs` is also read by `MB.9.2.1.1`'s `stage1.rs` work; the two
  do not overlap (`stage1.rs` vs `roles.rs`).
- Do not add a second `BootError` print: `MB.3.6.3`'s spawn work must not re-add one.
