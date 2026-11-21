---
id: MB.23.12
title: "Kill the dev_reload mutation survivors: create/delete trigger legs, pre_fork ordering, path boundary, EAGAIN, weak oracles"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: [MB.23.6]
paths: [crates/blastit/src/dev_reload.rs, crates/blastit/tests/critical_dev_reload.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T20:59"
updated: "2026-11-17T21:09"
---

# Goal

Kill the meaningful survivors of the `dev_reload` mutation campaign and make the weak oracles say what they claim.

# Context

`docs/testing/AUDIT-MB.23.md` §3: 180 mutants, 123 caught, 22 missed, 35 unviable (84.8 %). The meaningful misses are: `FAN_CREATE` and `FAN_DELETE` dropped from `trigger_mask` (no policy leg for a created/deleted tracked file); `entry.pre_fork |= pre_fork` → `&=` (no test reports the same path post-fork then pre-fork); `bytes.len() > MAX_PATH_BYTES` → `>=` (no 4096-byte accepted path); the `WouldBlock` arm of `Reload::on_fd` (only `Ok(0)` is tested); `Absorbed::refused()` is only ever asserted false; `marked_dirs()` may lie (`-> 1`) because the boot-line test asserts only `>= 1`.

# Acceptance

- A created and a deleted tracked file each produce exactly one `dev.reload` naming the file (and deleting `FAN_CREATE`/`FAN_DELETE` from `trigger_mask` makes those legs red).
- Reporting the same path first as post-fork and then as pre-fork keeps the cycle bit (and `|=`→`&=` is red).
- A 4096-byte absolute path is accepted, 4097 is `Drop::TooLong`.
- `on_fd` on a non-blocking empty socket returns no effect and does not revoke the source (EAGAIN), distinct from a zero-length datagram.
- `absorb` with a relative path asserts `refused() == true`; the boot line asserts the exact marked-directory count on a fixture with more than one marked directory.
- `cargo mutants --in-place --file crates/blastit/src/dev_reload.rs --features python -- --locked --lib --test critical_dev_reload` re-run: every survivor above is either caught or recorded as equivalent with a sentence.
- `make check`.

# Notes

Found by `docs/testing/AUDIT-MB.23.md` (MINOR, eight test gaps). Do not weaken an existing assertion; add the missing positive/negative twin.
