---
id: MB.36
title: "View crates architecture: blastit-view-{python,php,static} composed by Cargo features over view.chan, replacing the Upstream/WorkerRuntime trait soup (mandatory for 0.2.0)"
kind: epic
status: done
priority: 1
depends_on: []
paths:
  - "crates/**"
  - ARCHITECTURE.md
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - "scripts/**"
  - "e2e/**"
  - "docs/testing/**"
  - "docs/design/view-crates-architecture*.md"
  - "docs/decisions/ADR-0016-view-crates-over-wire.md"
  - "docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md"
  - docs/decisions/README.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T14:07"
updated: "2026-11-18T19:57"
---

# Goal

Replace the `Upstream` / `WorkerRuntime` trait soup with the architecture the network
protocol already implies: **one crate per view kind**, composed by Cargo features, with the
`view.chan` wire as the only interface between a view process and the rest of Blastit.

- `crates/blastit-frame` — raw wire frames (unchanged).
- `crates/blastit-view` — **shared `view.chan` transport only**: framing, body credit, the
  loaded-file datagram, errors. No interpreter, no SAPI, no worker trait.
- `crates/blastit-view-python` — the whole WSGI worker plus the `immortal` preload and
  `zygote` fork, compiled only with the `python` feature.
- `crates/blastit-view-php` — the whole embedded-PHP worker plus `libphp` discovery,
  entry pinning and the probe child, compiled only with the `php` feature.
- `crates/blastit-view-static` — safe static root resolution and the static responder.
- `crates/blastit` — thin CLI / stage 0 / stage 1 / supervisor that wires whichever
  `blastit-view-*` crates the features enabled.

After this epic a new backend is a new crate (`blastit-view-node` in `0.2.2`,
`blastit-view-jvm` in `0.2.3`) whose innards are the implementer's business; no sibling
language crate, no dispatch branch, and no shared trait has to change.

# Context

Owner mandate (2026-11-17), inside `0.2.0`:

> "you never took a step back to make a proper architecture … instead of suggesting to go
> with traits, the interface is kind of already defined by the network protocol between the
> view and the rest … maybe we have blastit-view-{python,php,static,...} crates, which get
> compiled depending on said languages being selected in the features, and then the innards
> of each view process is up to the implementer — from what I'm gathering that's de facto
> what you did but without splitting into different crates."

**Honest starting point (measured in this worktree).**

| Path | Lines | What it is today |
|---|---|---|
| `crates/blastit-view/src/php.rs` | 2558 | PHP worker inside the *Python* crate |
| `crates/blastit/src/{immortal,zygote}.rs` | 1400 + 281 | preload/immortalisation and the CPython view fork, in the composition root |
| `crates/blastit-view/src/{environ,input,request,response}.rs` | 2161 | WSGI half, feature-gated inside `blastit-view` |
| `crates/blastit-view/src/lib.rs` | 1234 | the WSGI `serve` loop, inside the crate every backend depends on |
| `crates/blastit-view/src/runtime.rs` | 1088 | `WorkerRuntime` trait + `OutputStream`/`RequestIo` + Python `LoadedTracker` |
| `crates/blastit/src/stage0.rs` | 6306 | PHP discovery, pinning, probe and every other boot gate |
| `crates/blastit/src/php_view.rs` | 1726 | PHP fork child + `PinnedEntryRuntime` |
| `crates/blastit-scheduler/src/static_{asset,response}.rs` | 1133 | static serving inside the scheduler |

The wire is already the interface: `READY` / `DONE`, muxed `req_id` frames, lazy body credit,
`CANCEL`/`ABORT`, `RESP_END_COST`, and the `ADR-0014` loaded-file datagram. Nothing in that
list is Python-specific, so nothing in that list belongs to a language crate.

**This epic supersedes, it does not re-litigate, the trait epics.** `MB.9` (and every
descendant: `MB.9.1`…`MB.9.5`) and `MB.3.24` are marked `superseded` by `planner` in the same
change that creates this epic. `MB.3.23`'s `WorkerRuntime` trait (already `done`, and its code
is shipping) is **deleted** by `MB.36.5`, because a trait whose only implementations are the
two crates you also own is a crate boundary that was never drawn. `ADR-0016` records the
decision and amends `ADR-0009` Decision 1 (and the trait half of Decision 2) while keeping
Decision 2's discovery/pinning, Decision 3's sandbox bijection, Decision 4's CPU accounting and
Decision 5's distribution rules **verbatim**.

**Non-negotiables carried into every child.**

1. `crates/blastit-view` has **zero** `pyo3`, zero `blastit-php-sapi`, and no `runtime.rs`
   trait — not even in `[dev-dependencies]` (a dev-dep is how `pyo3` comes back).
2. The standalone `blastit` binary keeps **no `DT_NEEDED` on `libpython` and none on
   `libphp`**; the wheel keeps `libpython` and still no `libphp` (`make rs-no-python`,
   `scripts/check-portable-elf.sh`, `scripts/build-standalone.sh`).
3. Every security invariant of `ADR-0010`/`ADR-0009` survives the move unchanged: the stage-0
   `execve` secret barrier, `SeccompRole::{View,EmbeddedView}` and the order
   `drop_privileges → harden_child → install_filter → set_dumpable(false)`, PHP
   `(st_dev, st_ino)` entry + library pinning with `BLASTIT_LIBPHP`, `openat2`-only static
   resolution with the one percent-decoder, `.php`/`.phtml`/`.phar` never served, and the
   `view.chan` credit window.
4. **Zero behaviour change.** No CLI flag, no `Blastfile` key, no documented command, no
   status code moves. Every user page keeps printing what it printed before; `MB.36.7` proves
   it. This is why the epic has no separate user-docs leaf: the developer experience is
   *invariance*, and the design records the invariance list instead of restating the guides.

**Ordering.** `MB.36.1` (design + ADR) has no dependencies and runs immediately, in parallel,
on disjoint `docs/design/view-crates-architecture*` + `docs/decisions/ADR-0016-*` paths. Every
implementation leaf is chained and waits for `MB.33` (the last in-flight feature node that
must land before the crate freeze) and `MB.23.5.2` (which still edits
`stage0.rs`/`supervise.rs`/`zygote.rs`) **before moving files across crates**. Leaves run
strictly one after another: each ends with a green workspace and a single Cargo.lock owner.

# Acceptance

```console
$ cargo test -p blastit-view --locked
$ cargo test -p blastit-view-python --locked
$ cargo test -p blastit-view-php --locked
$ cargo test -p blastit-view-static --locked
$ uv run --no-project tools/critical_guard.py --base develop \
    --node MB.36.2 --node MB.36.3 --node MB.36.4 --node MB.36.5
$ test -d crates/blastit-view-python && test -d crates/blastit-view-php && test -d crates/blastit-view-static && echo "three view crates exist"
$ test ! -e crates/blastit-view/src/runtime.rs && echo "WorkerRuntime trait deleted"
$ test -z "$(grep -rn 'pyo3' crates/blastit-view/Cargo.toml)" && echo "transport crate links no interpreter"
$ test -z "$(grep -rn 'blastit_php_sapi' crates/blastit-view/src)" && echo "transport crate knows no SAPI"
$ grep -q 'ADR-0016' docs/decisions/README.md && echo "ADR indexed"
$ make rs-no-python
$ make check
$ make e2e
```
- Every leaf's `RESULT.md` pastes command + output tail; the `MB.36.6`/`MB.36.7` evidence is
  the standalone, wheel and Docker legs, not a claim.

# Notes

- `Cargo.toml`/`Cargo.lock` edits belong to **one leaf at a time** (the chain above); never
  let two leaves add dependencies in parallel.
- `crates/*/tests/critical_*.rs` moves are legal only because each leaf lists the old glob in
  its `paths`; `make critical-guard NODE=MB.36.<leaf>` is part of every implementation
  acceptance, run **before** that leaf's commit (the Makefile's `BASE` default is `HEAD`, so the
  diff is exactly that leaf's deletions). The branch-wide proof is the `--node` list above: the
  epic's own `NODE=MB.36` is vacuous, because `paths` is `crates/**` and every protected path
  matches. Deleting a case instead of relocating it is a failed leaf.
- `docs/knowledge/LESSONS.md` records the feature-unification trap (`cargo build --workspace`
  links `libpython` through `blastit-py`); the design must state the exact per-artifact build
  commands rather than leaving `--workspace` available to a reader.
