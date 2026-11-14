---
id: MB.3.11
title: "Feature-gate pyo3 (feature=python); decouple RawFrame transport and forking from the GIL"
kind: task
status: active
owner: implementer
priority: 1
depends_on: [MB.3.5.1]
paths:
  - Cargo.toml
  - Cargo.lock
  - crates/blastit/Cargo.toml
  - "crates/blastit/src/**"
  - "crates/blastit-view/Cargo.toml"
  - "crates/blastit-view/src/**"
  - Makefile
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T14:39"
updated: "2026-11-14T14:39"
---

# Goal

Feature-gate PyO3 behind `feature = "python"` so a standalone/PHP-only build carries
**zero `libpython`**, and decouple the `RawFrame` worker transport and `forking` from the
GIL when `python` is disabled or the manifest declares no `app`.

# Context

- ADR-0010 §4: the standalone archive ships `php` with `python` **off** — no `libpython`,
  no `libphp`; the PyPI wheel ships `python, php`. Today `blastit` and `blastit-view` link
  PyO3 unconditionally, so a Python-free binary is impossible.
- The blocker is not PyO3 alone: `forking` calls `PyOS_BeforeFork`/`PyOS_AfterFork_*`
  because the GIL is held for the whole `Supervisor::run(py)` loop, and `view.chan`'s
  `RawFrame` transport is entangled with the same loop. Both must work with no
  interpreter present.
- The rule from `security-invariants`: no behaviour change on the python-enabled path.
  Every existing protected suite must pass unmodified, and the python-off build must be
  compiled *and* tested in `make check` (at least `cargo check --no-default-features
  --features php` plus the view/forking unit tests that need no interpreter).

# Acceptance

```console
$ cargo check -p blastit --no-default-features --features php --locked
$ cargo check -p blastit-view --no-default-features --locked
$ cargo test -p blastit --locked            # python on, unchanged
$ cargo test -p blastit-view --locked
$ make check
```
- `cargo tree -p blastit --no-default-features --features php -e normal | grep -c pyo3`
  is `0`; with `--features python` it is non-zero.
- `cargo metadata`/`cargo tree` shows `libpython` is not linked in the python-off build
  (`ldd` on the produced binary in a `cargo build` smoke step or an equivalent check).
- `forking` has one call site for the GIL hooks, behind `#[cfg(feature = "python")]`, with
  a comment proving the hook pair still matches on the python path.
- No `#[cfg]` disables a security check: the stage-0 env strip, the seccomp roles and the
  view isolation are compiled in both builds.

# Notes

- This leaf owns the workspace `Cargo.toml` and `Cargo.lock` edits; nobody else may touch
  them in parallel.
- Do not add `libphp` linking of any kind here — the shim is `MB.3.12` and is `dlopen`ed
  in the worker only.
