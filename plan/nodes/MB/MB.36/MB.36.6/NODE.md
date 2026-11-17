---
id: MB.36.6
title: "Finish the Cargo feature matrix (default php+static, wheel python+php+static), teach the guards the new members, and rewrite ARCHITECTURE.md's crate map while proving no libpython/libphp DT_NEEDED"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.36.5]
paths:
  - crates/blastit/Cargo.toml
  - "crates/blastit-py/Cargo.toml"
  - Cargo.toml
  - Cargo.lock
  - Makefile
  - "scripts/check-portable-elf.sh"
  - "scripts/build-standalone.sh"
  - ARCHITECTURE.md
  - docs/knowledge/LESSONS.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T14:07"
updated: "2026-11-17T14:07"
---

# Goal

Make the new crate graph the *only* thing the build can produce: finish the Cargo feature
wiring, teach every guard about the new members, and bring `ARCHITECTURE.md` back in line with
the code a reader will find.

This is the leaf that converts "the crates exist" into "the standalone binary, the wheel and
`make check` all build the right subset".

# Context

- Design of record: `MB.36.1`'s feature matrix (decision 6) and `ADR-0016`.
- Feature wiring on `crates/blastit`:
  `default = ["php", "static"]` (the standalone archive), `python = ["dep:pyo3",
  "dep:blastit-view-python"]`, `php = ["dep:blastit-view-php"]`,
  `static = ["dep:blastit-view-static"]`. `crates/blastit-py` keeps
  `features = ["python"]` on `blastit`, which is additive on top of the defaults and therefore
  yields the wheel's `python, php, static`. Remove every `blastit-view/python` and
  `blastit-view/php` feature edge that `MB.36.3`/`MB.36.5` left behind.
- `Makefile`'s `rs-no-python` still says `cargo check -p blastit-view --no-default-features`;
  point it at the configuration that actually carries the risk now (`-p blastit-view` with
  no features, plus the standalone `-p blastit` build it already does). Keep
  `scripts/check-portable-elf.sh` and `scripts/check-no-inotify.sh` unchanged in intent.
- `ARCHITECTURE.md` claims to update, at minimum: §2 components (add the crate map: crate →
  what it owns → the feature that compiles it), §2.4 (static now resolves in
  `blastit-view-static`, the scheduler only dispatches), §2.5 (the WSGI worker is
  `blastit-view-python`, the transport is `blastit-view`, there is no `WorkerRuntime`),
  §2.6 (the PHP worker, discovery, pinning and probe live in `blastit-view-php`), §3.1
  (framing/credit primitives stay in `blastit-view`), §9.3 (the fork lives in
  `blastit-view-python`; the GIL protocol is unchanged). Keep the existing section numbers:
  nodes and ADRs cite them.
- Also sweep the same names in `docs/knowledge/LESSONS.md` (the PyO3/feature-unification
  bullets name `blastit-view`) and `Cargo.lock`'s member list.

# Acceptance

```console
$ grep -q '^default = \["php", "static"\]' crates/blastit/Cargo.toml && echo "standalone default"
$ grep -q 'python = \["dep:pyo3", "dep:blastit-view-python"\]' crates/blastit/Cargo.toml && echo "python feature"
$ test -z "$(grep -rn 'blastit-view/python\|blastit-view/php' crates/*/Cargo.toml)" && echo "stale feature edges gone"
$ cargo build -p blastit --locked
$ cargo check -p blastit-py --locked
$ make rs-no-python
$ make standalone
$ make platform-matrix-check
$ grep -q 'blastit-view-python' ARCHITECTURE.md && grep -q 'blastit-view-php' ARCHITECTURE.md && grep -q 'blastit-view-static' ARCHITECTURE.md && echo "architecture names the crates"
$ test -z "$(grep -rn 'WorkerRuntime' ARCHITECTURE.md docs/)" && echo "no trait claim left"
$ make check
```
- `make rs-no-python` and `make standalone` prove the shipped artifact: no `libpython`, no
  `libphp` `DT_NEEDED`, no `RUNPATH`, no test-only boot seam, no inotify symbol.
- `make platform-matrix-check` proves the declared artifact matrix still describes the build.
- `RESULT.md` pastes `make check`, `make rs-no-python`, `make standalone`,
  `make platform-matrix-check` tails and the `cargo tree -p blastit --features …` lines for
  both configurations.

# Notes

- Never build the workspace with `--workspace` for an artifact claim: `blastit-py` turns
  `python` on and every member inherits it (`docs/knowledge/LESSONS.md`). State the exact
  command in the README/architecture text so the next reader does not repeat the mistake.
- `ARCHITECTURE.md` is excluded from the public site, so `make docs-check` does not cover it;
  it is checked by `make doc-tone` and by the grep guards above.
- If a feature combination turns out not to compile (e.g. `static` alone), fix the feature
  edges rather than adding a `#[cfg]` in a language crate.
