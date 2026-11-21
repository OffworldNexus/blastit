---
id: MB.36.2
title: "Extract crates/blastit-view-static: openat2 root resolution, MIME/ETag/immutable/SPA-answer responder, and the three protected static suites, with no behaviour change"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.36.1, MB.33, MB.23.5.2]
paths:
  - "crates/blastit-view-static/**"
  - "crates/blastit-view/src/path.rs"
  - "crates/blastit-view/src/lib.rs"
  - "crates/blastit-view/Cargo.toml"
  - "crates/blastit-scheduler/src/lib.rs"
  - "crates/blastit-scheduler/src/config.rs"
  - "crates/blastit-scheduler/src/error.rs"
  - "crates/blastit-scheduler/src/rewrite.rs"
  - "crates/blastit-scheduler/src/scheduler.rs"
  - "crates/blastit-scheduler/src/path.rs"
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/src/static_response.rs"
  - "crates/blastit-scheduler/tests/critical_static_assets.rs"
  - "crates/blastit-scheduler/tests/critical_static_resolve.rs"
  - "crates/blastit-scheduler/tests/critical_static_entry.rs"
  - "crates/blastit-scheduler/tests/critical_rewrite.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
  - "crates/blastit-scheduler/tests/static_feature_off.rs"
  - "crates/blastit-scheduler/Cargo.toml"
  - crates/blastit/Cargo.toml
  - crates/blastit/src/stage1.rs
  - Cargo.lock
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T14:07"
updated: "2026-11-18T13:21"
---

# Goal

Create `crates/blastit-view-static` and move the whole static view kind into it:
`openat2` root resolution, the `O_NOFOLLOW` per-segment walk, the `.php`-source refusal, the
closed MIME table, `ETag`/`Last-Modified`/`304`/`HEAD`, the `immutable` cache rule and the SPA catch-all answer.
`crates/blastit-scheduler` keeps dispatch and becomes a consumer.

**Behaviour-preserving move.** Not one status code, header, or precedence rule changes.

# Context

- Design of record: `MB.36.1`'s `docs/design/view-crates-architecture.md` (crate graph,
  decision 3 "one percent-decoder", decision 4 "static is not a process") and `ADR-0016`.
- Files today: `crates/blastit-scheduler/src/static_asset.rs` (800 lines, `AnswerKind`,
  `StaticFile`, `StaticMeta`, `StaticRoot`, `head_segments`, `same_inode`,
  `STATIC_MAX_FILE_BYTES`, `is_php_source`), `static_response.rs` (333 lines,
  `StaticBody`, `StaticResponse`). Callers: `scheduler.rs` (`use crate::static_asset::{…}`,
  `static_response::{StaticBody, StaticResponse}`), `rewrite.rs` (`StaticFile`,
  `StaticOutcome`), `config.rs` (`STATIC_MAX_FILE_BYTES`), `lib.rs` (`pub mod`).
- The invariant list this move must not dent: a dirfd per root opened once and
  `openat2(STATIC_RESOLVE_FLAGS)` per candidate; probe `O_PATH` then read `O_RDONLY` with the
  `(st_dev, st_ino)` identity check; never `canonicalize()` + string prefix; a final segment
  ending `.php`/`.phtml`/`.phar` answers `404` **before any descriptor opens**; the cache rule
  is keyed on the **answering** file (an SPA answer is never `immutable`); no response
  buffers a whole file.
- Protected suites: `crates/blastit-scheduler/tests/critical_static_assets.rs`,
  `critical_static_resolve.rs`, `critical_static_entry.rs` (and `critical_static_cli.rs` in
  `crates/blastit/tests/`, which is CLI-level and **stays**). Move the three into the new
  crate per the design's relocation table unless the design keeps one in the scheduler for a
  stated reason; keep every case, and list the old glob in `paths` so
  `tools/critical_guard.py` accepts the move.
- `[dev-dependencies]` of the new crate: whatever the moved tests need (`rustix`,
  `tokio`), nothing language-related. No `pyo3`, no `blastit-php-sapi` — the new crate is
  reachable from the standalone binary.

# Acceptance

```console
$ cargo test -p blastit-view-static --locked
$ cargo test -p blastit-scheduler --locked
$ cargo test -p blastit --test critical_static_cli --locked
$ make critical-guard NODE=MB.36.2
$ test -d crates/blastit-view-static && echo "crate exists"
$ test -z "$(ls crates/blastit-scheduler/src/static_*.rs 2>/dev/null)" && echo "static left the scheduler"
$ test -z "$(grep -rn 'openat2\|ResolveFlags' crates/blastit-scheduler/src)" && echo "no resolver left behind"
$ cargo tree -p blastit-view-static -e normal
$ make check
```
- The three protected suites pass **with every case they had**, in their new home; `git diff`
  shows moves, not deletions (a removed `#[test]` fails the leaf).
- `cargo tree -p blastit-view-static` contains no `pyo3` and no `blastit-php-sapi`, and
  `blastit-scheduler` depends on `blastit-view-static` (not the reverse).
- `RESULT.md` pastes the tail of `make check` and the `critical-guard` verdict.

# Notes

- Do not touch `blastit-scheduler/src/{ctl,scheduler,task,fair}.rs` beyond the `use` lines and
  re-exports the move requires.
- If the design keeps `path.rs` in the scheduler, the new crate needs the decoder passed in or
  a move to whichever crate the design named — follow the design; a second
  `percent_decode` implementation is a security defect, not a style choice.
- `Cargo.lock` gains one workspace member here; no other leaf may add dependencies while this
  one is active.
