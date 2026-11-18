---
id: MB.26.29
title: "Prove the static open flags: O_DIRECTORY at the root, O_NONBLOCK against a FIFO swap, CLOEXEC across execve"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-view-static/tests/**", "crates/blastit-view-static/src/static_asset.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T18:27"
updated: "2026-11-18T18:27"
---

# Goal

Prove the three static open-flag cells `AUDIT-MB.36.md` §4.3 lists as unaccounted, each with the
behaviour the flag exists for:

1. `StaticRoot::open_with`'s `O_DIRECTORY`: a root that is a **regular file** is refused at open
   (today dropping `O_DIRECTORY` makes it open and every request `404`, so the boot's documented
   refusal disappears);
2. `finish`'s `O_NONBLOCK` **and `O_NOCTTY`**: the probe→read swap leg swaps the leaf to a
   **FIFO**, and the request is `Hidden` without blocking; a leaf that is a terminal is refused or
   opened without becoming the worker's controlling tty (today the swap leg swaps to a
   directory/symlink, so neither flag is exercised — `461:59` drops `NONBLOCK`, `461:78` drops
   both);
3. `CLOEXEC` on the root, probe and read descriptors: a child `execve`'d while a request is open
   does not inherit them (or, if the design says that cannot be proved, the cell is recorded as
   accepted-untestable with the reason).

# Context

`make mutants-rust PKG=blastit-view-static` left `265:26`, `265:46`, `461:41`, `461:59`, `461:78` — the
`|→&` cells that empty the flag set — unaccounted. `585:34`, `586:58`, `623:22` and `611:24` are
equivalent (disjoint bits, and the `fstat` type check that follows makes an empty probe set
indistinguishable). The audit's earlier "`a | b == a ^ b`" sentence justified only `|→^`; this node
closes the `&` half.

# Acceptance

```console
$ cargo test -p blastit-view-static --locked --test critical_static_resolve
$ make mutants-rust PKG=blastit-view-static
```
- the four `&` cells are killed, or the ones judged untestable are recorded in
  `AUDIT-MB.36.md` §4.2 with an argument that covers `&` (emptying the set), not only `^`;
- the file-root refusal is a case in the suite that owns the boot (`critical_static_resolve` or
  `critical_static_cli`), never a new duplicate of the resolve matrix.

# Notes

`crates/blastit-view-static/tests/common/static_fixture.rs` already builds a FIFO (`fifo`) and the
swap hook; extend it rather than adding a second fixture tree. `StaticRoot::open_with` is
`#[doc(hidden)]` but is the boot's own call (`static_asset.rs:265`).

# An added case (the `MB.36` audit's own re-derivation)

- `586:58` (`probe & DIRECTORY`) empties the **initial** `openat(dirfd, ".", …)`; `walk` fstats
  only the next descriptor (`static_asset.rs:597`), so the call loses `O_PATH`/`O_NOFOLLOW`/
  `O_CLOEXEC` and opens `.` `O_RDONLY`. A root mode `0o111` is served today (`O_PATH` needs no
  read permission) and answers `EACCES`/`404` under the mutant: root `0o111`, `Backend::Walk`,
  `GET /` → **200**, beside the `Openat2` leg (both backends).
