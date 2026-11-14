---
id: MB.3.27
title: "Scope the WSGI-bound protected suites to feature=python (default build made cargo test -p blastit red)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.17]
paths:
  - crates/blastit/tests/critical_dev_and_fail_closed.rs
  - crates/blastit/tests/critical_config_and_presets.rs
  - crates/blastit/tests/critical_static_cli.rs
  - crates/blastit/tests/no_python_build.rs
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T18:40"
updated: "2026-11-14T17:43"
---

# Goal

`MB.17` made the standalone build the default, so plain `cargo test -p blastit` is now red in
three protected suites that assert WSGI behaviour. The capability gate is correct; the suites
are not scoped to the configuration they need.

# Context

- Red under the default: `critical_dev_and_fail_closed` (16 assertions),
  `critical_config_and_presets` (1), `critical_static_cli` (1). `make check` stays green
  because `rs-no-python` runs the interpreter-free subset by design — so the red only bites a
  developer running the obvious command. That is a DX defect, not a test defect.
- **This is an authorized protected-suite edit, not a weakening**: a file-level
  `#![cfg(feature = "python")]` mirroring `no_python_build.rs`'s
  `#![cfg(not(feature = "python"))]`, so each suite runs exactly where its contract exists.
  **No assertion may be added, removed, relaxed or reordered.**
- `tools/critical_guard.py --node MB.3.27` is the authorizer; paste its verdict.

# Acceptance

```console
$ cargo test -p blastit --locked                        # default: green, the three suites absent
$ cargo test -p blastit --locked --features python      # all three run, assertions intact
$ make check
```
- `git diff` on the three files shows **only** the crate-level cfg attribute (plus at most a
  one-line comment naming `MB.17`); every test and assertion is byte-identical.
- `make check` and `rs-no-python` green.
- Each file's comment says why the cfg exists, so a later reader does not read it as a
  disabled suite.

# Notes

- If a suite mixes WSGI-bound and capability-agnostic assertions, split by attribute rather
  than gating the whole file, and say which in the RESULT.
