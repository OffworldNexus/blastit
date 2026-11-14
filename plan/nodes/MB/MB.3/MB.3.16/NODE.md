---
id: MB.3.16
title: "Refuse (or prove) blastit check on an all-static Python-less manifest, pretty and --json"
kind: task
status: ready
owner: implementer
priority: 3
depends_on: [MB.3.11]
paths:
  - crates/blastit/src/check.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/tests/no_python_build.rs
  - crates/blastit/tests/check_capability.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T15:08"
updated: "2026-11-14T15:08"
---

# Goal

`blastit check` must refuse an **all-static, Python-less** manifest instead of pretending to
run a suite it cannot run.

# Context

- `MB.3.11` follow-up: in a `python`-less build, `check` on a manifest whose routes are all
  `[route.static]` is not refused. `check`'s job is to exercise routes through the app; with
  no interpreter and no PHP worker there is nothing to exercise, so the honest answer is a
  refusal naming why, or the all-static path that is genuinely supported if one exists.
- Preserve the protected `critical_static_cli` behaviour (`MIXED` fixture and its
  "no interpreter to run pytest on" message) — that suite is unmodified and authoritative
  for the app-less all-static CLI path. Read it before deciding the new refusal is distinct.
- `--json` must carry the same decision as pretty output.

# Acceptance

```console
$ cargo test -p blastit --test critical_static_cli --locked     # unmodified, green
$ cargo test -p blastit --locked
$ make check
```
- A python-less build + all-static manifest: one refusal, exit code and message asserted,
  naming the missing interpreter and the package, and `--json` carries a machine-readable
  reason that a reader can act on.
- The accepted twin: the same manifest in the `python` build keeps its current behaviour
  (whatever `critical_static_cli` pins), unchanged.
- No `#[cfg]` silently changes an exit code without a test for that configuration.

# Notes

- If the correct outcome is "all-static check is supported and must keep working", then the
  node becomes the test that pins it and says so in the RESULT — do not invent a refusal.
