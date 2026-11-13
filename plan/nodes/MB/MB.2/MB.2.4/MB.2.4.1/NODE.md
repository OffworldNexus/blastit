---
id: MB.2.4.1
title: "Resolve request paths beneath a static root (openat2, refusal matrix, TOCTOU) - design MB.2.4a"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.1, MB.2.3]
paths:
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/tests/critical_static_resolve.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-13T20:52"
updated: "2026-11-13T22:01"
---

# Goal

Resolve one decoded request path to a regular file **strictly beneath** a declared static root, with the refusal matrix and the TOCTOU-safe open as the whole contract. This is the resolver half of the design's `MB.2.4a`: no response headers, no scheduler wiring.

# Context

- `docs/design/static-and-spa.md` §4.2 (the exact types and their fields — `MAX_STATIC_PATH_BYTES`, `MAX_SEGMENT_BYTES`, `STATIC_MAX_FILE_BYTES`, `StaticRefusal`, `segments`, `classify`, `StaticError`, `ImmutableGlob`, `StaticRoot::{open,open_with,resolve}`, `Backend`, `StaticOutcome`, `AnswerKind`, `StaticFile`, `StaticMeta`), §5.1 (the boundary: `O_PATH|O_DIRECTORY|O_CLOEXEC` dirfd on `manifest_dir.join(root)`, `openat2` with `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS`, the read re-open `O_RDONLY|O_CLOEXEC|O_NONBLOCK|O_NOCTTY` plus the `(st_dev, st_ino)` identity check, the `ENOSYS`/`EINVAL` per-segment `O_NOFOLLOW` walk, the **total** errno table, one decode/no normalization), §5.2 rows 1–26 (the refusals and hidden cases this leaf owns), §4.1 `StaticPolicy` (already parsed by `MB.2.3`), §4.3 steps 3–11 (the order `resolve` implements, step 1–2 are the caller's), §4.7 (`manifest_dir`, `static_max_bytes`), §8 (`MB.2.4a` row).
- New `crates/blastit-scheduler/src/static_asset.rs`, declared `pub mod static_asset;` in `lib.rs`. `rustix` (`fs` feature) is already a dependency: no `Cargo.toml` or `Cargo.lock` edit. **Never** `canonicalize()` + string prefix as the boundary.
- `config.rs`: `SchedulerConfig` gains `manifest_dir: PathBuf` and `static_max_bytes: u64` (`STATIC_MAX_FILE_BYTES`, no env var, no manifest key — §4.6 Budgets), with `Default` values; `MB.2.6` fills `manifest_dir` from stage 1.
- Mark `StaticRoot`'s fields `pub(crate)` (and any accessor the sibling module needs): `static_response.rs` in `MB.2.4.2` adds `impl StaticRoot { respond }` from another module and must read the immutable globs and the size cap.
- Fixtures: create `crates/blastit-scheduler/tests/common/static_fixture.rs` with §5.2's **S**, **F**, **N** and **D** exactly (`index.html`, `docs/index.html`, `assets/app.4f1c2a9b7d3e.js`, `.well-known/acme-challenge/token`, `.env`, `.git/config`, `link -> /etc/passwd`, `link2 -> ../outside`, `fifo`, `sock`, `mode000`, `big.bin` over the cap). Include it with `#[path = "common/static_fixture.rs"] mod static_fixture;` — do **not** edit `tests/common/mod.rs`, which every scheduler suite shares. Build everything in a tempdir and run unprivileged.
- Out of scope: `respond`/ETag/headers/`304` (`MB.2.4.2`), `rewrite.rs`/`decide` (`MB.2.5.1`), `scheduler.rs` (`MB.2.5.2`), `path.rs`.

# Acceptance

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --lib --locked
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --test critical_static_resolve --locked
$ make rs-lint
$ make critical-guard BASE=develop NODE=MB.2.4.1
$ CARGO_NET_OFFLINE=true cargo test -p blastit-scheduler --locked
```

# Notes

- New protected suite `tests/critical_static_resolve.rs`: `segments` over §5.2 rows 1–13 and the empty/`.`/trailing-slash cases (`/a//b`, `/x.js/`, double-encoded inert); `classify` tabled over `RegularFile`/`Directory`/`Fifo`/`Socket`/`CharacterDevice`/`Symlink` with the `_` arm the total case; every §5.1 errno row; `resolve` outcomes for exact file, directory index, SPA index, `SpaLost`, `Missing`, `Hidden`, `Refused`; the §4.5 `immutable` glob vectors; both backends through `#[doc(hidden)] pub open_with`; a swap attempt (replace the file between the probe open and the read open) proving the `(st_dev, st_ino)` identity check, and a record of which backend the kernel exercised.
- Budget: ≤ 5 files, ≤ 600 changed lines; `static_asset.rs` stays ≤ 400 production lines.
- Rows 15–17/22–26 are refusals *and* positives for the responder; keep their observable status out of this suite (that is `critical_static_assets.rs` in `MB.2.4.2`) — here assert `StaticOutcome`/`StaticRefusal` values.
