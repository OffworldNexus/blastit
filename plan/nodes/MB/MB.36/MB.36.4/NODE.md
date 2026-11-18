---
id: MB.36.4
title: "Move immortal preload and the zygote fork into blastit-view-python behind the design's spawn-context seam, with no cycle back into blastit"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.36.3]
paths:
  - "crates/blastit-view-python/**"
  - "crates/blastit-view/src/spawn.rs"
  - "crates/blastit-view/src/lib.rs"
  - "crates/blastit-view/Cargo.toml"
  - "crates/blastit-sys/**"
  - crates/blastit/src/child.rs
  - crates/blastit/src/forking.rs
  - crates/blastit/src/immortal.rs
  - crates/blastit/src/zygote.rs
  - crates/blastit/src/py.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/supervise.rs
  - crates/blastit/src/config.rs
  - crates/blastit/src/lib.rs
  - crates/blastit/Cargo.toml
  - Cargo.lock
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T14:07"
updated: "2026-11-18T15:14"
---

# Goal

Move the CPython preload and view fork into `crates/blastit-view-python`: `immortal.rs`
(1400 lines: the decision, the immortalisation pass, its stats and the version marker),
`zygote.rs` (281 lines: `fork_view`, `ViewSpawn`, `ForkedView`, `set_view_environment`,
`traceback_text`) and the interpreter token seam (`crates/blastit/src/py.rs`).

`crates/blastit` keeps stage 1's *call* to them and nothing else. **Behaviour-preserving
move** — same fork protocol, same GIL discipline, same `/proc/self/…` accounting.

# Context

- Design of record: `MB.36.1`'s `view-crates-architecture.md`, seam decisions 1 (the child
  sequence's home) and 2 (the spawn-context struct), plus `ADR-0016`.
- Read `ARCHITECTURE.md` §9.3 before touching the fork: `PyOS_BeforeFork` /
  `PyOS_AfterFork_Parent` / `PyOS_AfterFork_Child` run **with the GIL held**, the child
  inherits the GIL, and the only thing it may do with it is hand it to `serve`. `immortal`
  decides per interpreter version whether the pass runs; both the decision and its
  deviation note are part of the contract, not an implementation detail.
- The seam this leaf must respect: `blastit-view-python` may **not** depend on `blastit`.
  `zygote` currently takes `blastit_scheduler::RouteInfo` and `crate::config::Config`, and
  `immortal` takes `config::Immortalize`, while `config.rs` reads
  `crate::immortal::{marker, supported}`. Use the struct the design named in decision 2 and
  map `Config → that struct` **at the call site in `blastit`**, so the dependency arrow stays
  one-way. If the design put the child sequence in `blastit-sys` (decision 1), import it from
  there; do not reach back into `crate::child`.
- Call sites: `stage1.rs` (`immortal::apply`, `immortal::decide`, `Outcome`, `Stats`,
  `zygote::traceback_text`), `supervise.rs` (`zygote::fork_view`, `zygote::ViewSpawn`,
  `From<zygote::ForkedView> for Forked`), `config.rs` (`immortal::marker`,
  `immortal::supported`), `lib.rs` (`mod immortal; mod zygote;`), and `py.rs`'s
  `Python`/`no_interpreter` token.
- Feature rule: everything here is `#[cfg(feature = "python")]`. The standalone build must
  still compile with the crate absent — `py.rs`'s phantom token stays in `blastit` (it is the
  no-interpreter token the *composition root* carries), while the real `pyo3::Python`
  re-export moves with the crate. Keep that distinction explicit in the crate docs.

# Acceptance

```console
$ cargo test -p blastit-view-python --locked
$ cargo test -p blastit --lib --locked
$ cargo test -p blastit --locked
$ make critical-guard NODE=MB.36.4
$ test ! -e crates/blastit/src/immortal.rs && test ! -e crates/blastit/src/zygote.rs && echo "fork left the composition root"
$ test -z "$(grep -rn 'mod immortal\|mod zygote' crates/blastit/src)" && echo "no orphan modules"
$ grep -q 'blastit_view_python::zygote' crates/blastit/src/supervise.rs && echo "supervisor calls the crate"
$ cargo test -p blastit --test critical_dev_and_fail_closed --locked
$ make check
```
- `cargo tree -p blastit-view-python -e normal` shows **no** dependency on `blastit`, and
  `cargo test -p blastit --lib` still builds the standalone configuration (no `python`).
- The fork's seams stay testable: whatever test walked the immortal decision or the fork
  today either moved with the code or is re-created in
  `crates/blastit-view-python/tests/`, with the same assertions (a deleted assertion fails
  the leaf).
- `RESULT.md` pastes the `git diff --stat -M` rename summary, `make check`, and the
  `cargo tree` proof of the one-way arrow.

# Notes

- The stage-1 order is security-relevant: secrets are swapped by `execve` **before** stage 1;
  nothing in this move may reorder the fork relative to that barrier, `harden_child`,
  `install_filter` or `set_dumpable(false)`.
- `set_view_environment` (`HOME`/`TMPDIR` under the view dir) is part of the child sequence,
  not of the scheduler: keep it next to the fork it serves.
- Do not "simplify" the GIL hand-off while moving: a `Python::attach`/`detach` edit is a
  different node with its own critical test.
