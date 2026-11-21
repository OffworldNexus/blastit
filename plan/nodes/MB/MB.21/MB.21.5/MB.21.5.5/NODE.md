---
id: MB.21.5.5
title: "Close the purity gate's nested-use evasion (R24) and prove it in both directions"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: [scripts/check_manifest_purity.py, scripts/test_manifest_purity.py]
attempts: 4
max_attempts: 4
escalation: none
created: "2026-11-16T23:03"
updated: "2026-11-17T05:39"
---

# Goal

Make R24's purity gate a gate: `scripts/check_manifest_purity.py` currently matches one literal
(`NEEDLE = "std::env"`), so `use std::{env};` plus `env::var("HOME")` in
`crates/blastit-manifest/src/**` passes. Measured in `docs/testing/AUDIT-MB.21.md` §5.1 with a scratch
tree under `/tmp/opencode/mb2153-purity` (exit 0 with the evasion, exit 1 on the direct spelling).

# Context

- Design `docs/design/blastfile-dsl.md` §17.2 / R20 / R24: a Blastfile *names* variables and never
  reads them, so a `std::env` read in `blastit-manifest` is a build failure.
- The gate is one of `make check`'s lines (`make manifest-purity`); a rule a one-line edit evades is
  documentation, not a gate.
- The self-test (`scripts/test_manifest_purity.py`) proves the marker's own behaviour; it must gain the
  evasion case in both directions.
- Filed by `MB.21.5.3`'s mutation audit; the tested tree's purity is currently true (16 files, no
  literal), so this hardens the gate rather than fixing an existing violation.

# Acceptance

```console
$ UV_OFFLINE=1 uv run --no-project python scripts/test_manifest_purity.py        # green, with the new fixture
$ UV_OFFLINE=1 uv run --no-project python scripts/check_manifest_purity.py       # green on the real tree
```

1. The needle catches, at minimum, `std::env::…`, `env::var|vars|var_os|args|current_dir|…` and
   `use std::…{…env…}` — the three patterns are in the audit report and can be copied.
2. `scripts/test_manifest_purity.py` gains a negative fixture holding exactly the evasion text
   (`use std::{env};` + `env::var("HOME")`) and asserts exit 1, plus a positive fixture asserting the
   marker alone still passes (a rule that never fires is not a gate either).
3. No false positive on the 16 real source files, and no doc-comment exception is needed (grep the
   tree for `env::` first).
4. `make manifest-purity` and `make check` stay green.

# Notes

- `NEEDLE`'s message ("a Blastfile names variables and never reads them") should stay verbatim in the
  refusal so the report and the gate agree.
- A stronger alternative is clippy's `disallowed-methods` for the `std::env` functions (resolves the
  path through any import), but it is a `clippy.toml` change with workspace-wide blast radius: do it
  only if the script patterns leave a hole. Say which you chose in `RESULT.md`.
