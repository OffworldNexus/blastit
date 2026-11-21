---
id: MB.2.4.3
title: "critical_static_resolve: pin the open contract and four surviving mutants"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-scheduler/tests/critical_static_resolve.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T23:09"
updated: "2026-11-14T00:02"
---

# Goal

Close the four surviving `static_asset.rs` mutants of the `MB.2.4` audit
(`docs/testing/AUDIT-MB.2.4.md`) in the protected resolve suite: assert the
**effective backend**, prove a **symlink-declared root** opens (design §5.1's one
followed symlink), accept a file **exactly at** `static_max_bytes`, and stop
treating a **space (0x20) as a control byte**.

# Context

`cargo mutants` on `b8e8806` (96 mutants, `-f '**/src/static_asset.rs'`) left these
alive and every one of them is a behaviour a user can hit:

1. `static_asset.rs:302:34` (`backend == Backend::Walk` -> `!=`), `302:51`
   (`||` -> `&&`), `302:54` (delete `!`) and `541:5`
   (`supports_openat2 -> true` / `-> false`) all survive, because
   `every_matrix_row_answers_the_same_on_both_backends` computes `effective` from
   its **own copy** of the predicate and only `println!`s it. A production
   regression that silently takes `Backend::Walk` for every root - losing the
   kernel-enforced `RESOLVE_BENEATH|NO_SYMLINKS|NO_MAGICLINKS` boundary - passes
   the whole suite. The `Walk` half is genuinely exercised; what is missing is
   that the *production* selection is observed.
2. `429:16` (`len > max_bytes` -> `>=` and `==`) survives: the matrix has an
   over-cap row (`/big.bin`) but no **at-cap** row, and `classify`'s at-cap case
   is a pure-function test that never goes through `finish`. A 64 MiB file is
   therefore `404` under `>=` with no test failing.
3. `575:40` (`byte < CONTROL_LIMIT` -> `<=`) survives: a decoded `0x20` becomes
   `StaticRefusal::Control`, so `/my%20file.css` answers `400`. Design §5.1
   refuses *"< 0x20 (NUL and C0)"*; a space is a legal filename byte. No case
   feeds a space to `segments`.
4. `293:26`/`293:46` and the other `|` -> `&` flag degenerations are only
   observable through `O_PATH`/`O_NOFOLLOW` semantics; a symlink-declared root
   (`root = "current/public"`) is documented in §5.1 ("the only place a symlink is
   followed, and only on the operator's own declaration") and has no test at all.
   ADR-0007's consequences paragraph says such a layout "needs the real path in
   `root`" - whichever is true, the suite should state it.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_resolve --locked
```

and, locally reverted afterwards, each of these must fail the suite:

- `let backend = Backend::Walk;` in `open_with` (and `supports_openat2 -> false`);
- `len >= self.max_bytes` in `finish`;
- `byte <= CONTROL_LIMIT` in `segments`.

# Notes

- Prefer **no product change**: `StaticRoot` derives `Debug`, which prints the
  private `backend` field (`format!("{root:?}")` contains `backend: Openat2`).
  Do not add a `#[doc(hidden)] pub fn backend(&self)`: §4.2's surface is fixed.
- Branch on the kernel probe, never `#[ignore]`: when the kernel has no
  `openat2`, assert the `Walk` root is recorded as `Walk` and keep the matrix
  unconditional.
- The at-cap case must not read 64 MiB: add a sparse `cap.bin`
  (`set_len(STATIC_MAX_FILE_BYTES)`, 0 B on disk - verified on the tmpfs `/tmp`) to
  `tests/common/static_fixture.rs`, assert `StaticOutcome::File` and
  `file.len == STATIC_MAX_FILE_BYTES`, then drop the fd without reading. Keep
  `/big.bin` (`+1`) on the `Hidden` side.
- The space case is one table row (`segments("/my file.js") == Ok(["my file.js"])`)
  plus one `%20` target through `head_segments`.
- Do not add `#[ignore]`, do not weaken an existing assertion, and do not edit
  `critical_static_assets.rs` or any product file without the lead.
