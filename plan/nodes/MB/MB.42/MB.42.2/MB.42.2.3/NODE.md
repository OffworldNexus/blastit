---
id: MB.42.2.3
title: "Safe local CLI handoff (cli/handoff.rs): zero-TOCTOU venv/vendor/node_modules execveat"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.2.2]
paths: ["crates/**", python/blastit/__main__.py, python/blastit/_blastit.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T17:18"
updated: "2026-11-19T21:30"
---

# Goal
A global `blastit` running `dev`/`check`/`test`/`fmt`/`lsp` silently hands off to the project's own `blastit`, with zero TOCTOU between the safety check and the `execve` — `docs/design/universal-distribution.md` §4, `ADR-0017` Decision 5.

# Exact Deliverables
1. **`crates/blastit/src/cli/handoff.rs` (new, `<500` LOC)**, called from `cli::main` **only** for `Command::{Dev, Check, Test, Fmt, Lsp}` — never `Serve`/`--preset`, `--stage=1`, `--role=*`, `--internal-json`, `--help`/`help`, `--version`; skipped when `BLASTIT_HANDOFF_DONE` is non-empty, or `BLASTIT_NO_LOCAL_HANDOFF` is set and neither `""` nor `"0"`.
2. **Candidate order from `cwd`**: (1) `$VIRTUAL_ENV/bin/blastit` when `VIRTUAL_ENV` is non-empty (explicit — no `./.venv` fallback), else `<cwd>/.venv/bin/blastit`; (2) `<cwd>/vendor/bin/blastit`; (3) `<cwd>/node_modules/.bin/blastit`.
3. **Zero-TOCTOU verification** per candidate: `canonicalize` -> `(parent, file_name)`; `open(parent, O_RDONLY|O_DIRECTORY|O_CLOEXEC|O_NOFOLLOW)`; `openat(parent_fd, file_name, O_RDONLY|O_CLOEXEC|O_NOFOLLOW)`; require regular file, `st_mode & 0o111 != 0`, `st_uid in {geteuid(), 0}`, and `st_mode & 0o022 == 0` on `bin_fd`, `parent_fd` **and every ancestor to `/`** (root-owned sticky dirs `st_uid == 0 && st_mode & 0o1000` allowed when the immediate child is owned by `geteuid()` or `0`).
4. **Failure policy**: explicit `$VIRTUAL_ENV` candidate -> `exit 1` with `blastit <cmd>: refusing local handoff to <path>: <reason>; set BLASTIT_NO_LOCAL_HANDOFF=1 to use global blastit` on `stderr`; auto-discovered `.venv`/`vendor`/`node_modules` candidate -> one `stderr` warning `blastit <cmd>: skipping unsafe local binary <path> (<reason>)` and **continue probing** (a `0775` CI checkout or third-party `node_modules` must never brick `blastit dev`).
5. **Self-handoff loop**: if `(bin_stat.st_dev, bin_stat.st_ino) == stat("/proc/self/exe")`, stop probing and `Ok(())`.
6. **Exec**: export `BLASTIT_HANDOFF_DONE=1`; read 4 magic bytes from `bin_fd`; `\x7fELF` -> `execveat(bin_fd, c"", argv, envp, AT_EMPTY_PATH)`; `#!` -> `execve(canonical_path, …)` while keeping `parent_fd` + `bin_fd` (`O_CLOEXEC`) open across the call so `binfmt_script` preserves `$0`/`__file__` and the inode cannot be recycled. Silent (zero bytes) on valid handoff.

# Acceptance
- `make fast-check`
- New `crates/blastit/tests/critical_handoff.rs` over **real** temp trees (no mocks): valid `.venv`/`vendor`/`node_modules` shim hands off (`BLASTIT_HANDOFF_DONE` seen by the child, child's argv preserved); a self-referencing binary is refused and recovers; a `0777` and a `0775` binary are refused; a binary owned by another uid is refused; a `VIRTUAL_ENV` candidate that fails safety exits `1` with the exact sentence while an unsafe auto-discovered candidate only warns and the next candidate still runs; production `blastit` (`Serve`, `--stage=1`, `--role=dbproxy`, `--internal-json`, `--help`, `--version`) never hands off; `BLASTIT_NO_LOCAL_HANDOFF=1` and `BLASTIT_HANDOFF_DONE=1` both disable it.
- `cargo test -p blastit handoff` plus one real `blastit dev` run inside a temp project with a `.venv/bin/blastit` wrapper printing its marker.
- Manual: `strace -f -e trace=execve,execveat target/debug/blastit dev` shows one `execveat` and no `open` between the check and the `execve` on the candidate path.

# Notes
- **1 Cargo writer**; no `Cargo.lock` churn expected (no new deps) but the node still owns `crates/**` edits serially.
- Never write `/home/remy/.local`, `/usr/bin/blastit`, or the host's PHP/Python packages — every fixture lives under `CARGO_TARGET_TMPDIR`/`target/tmp` (not `/tmp`, whose `1777` ancestors make a "safe tree" fixture refuse its own good case; see `LESSONS.md`).
- `execveat`/`execve` are `unsafe`: confine them to one small module with a SAFETY comment; no `unwrap()` on `st_*` reads.
- No `Mock*Server`, no `scripts/check_*.py` guard, no env-var test hooks (`#[cfg(test)]` seams are forbidden).
- Do not touch `crates/*/tests/critical_*.rs`; out-of-scope finds to `## Follow-ups`.
