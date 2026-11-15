---
id: MB.3.58
title: "Every matrix leg must build where it runs, and no leg may pass two platform tags"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [".github/**", "scripts/**", "e2e/**", Makefile, docs/frameworks/php.md, docs/knowledge/LESSONS.md]
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-15T16:38"
updated: "2026-11-15T18:23"
---

# Goal

Make every matrix leg execute our binaries in the environment that built them, and stop passing a
platform tag twice. Both CI failures on the first real matrix run are the same class as the wheel tag:
a property of the build host deciding what a shipped or executed artefact can do.

# Context

The matrix jobs ran for the first time and failed, diagnosably:

1. **Wheels — the tag is supplied twice.** `maturin-action`'s `manylinux: 2_28` already builds inside
   `quay.io/pypa/manylinux_2_28_x86_64`, so also passing `--compatibility manylinux_2_28` gives
   maturin two platform tags:
   `maturin failed: Caused by: Expected only one platform tag but found 2`.
   The `--compatibility` argument must be dropped wherever a `manylinux:` image is used, and kept
   only on the composite-action path that builds on the host. `scripts/check_wheel_tag.py` stays the
   verification: it must still see `cp3xx-cp3xx-manylinux_2_28_<arch>`.
2. **Shim + probe on `debian:bookworm` — a host-built binary in an older image.** The Ubuntu runner
   (glibc 2.39+) builds the test binaries, then executes them inside bookworm (2.36):
   `/lib/x86_64-linux-gnu/libc.so.6: weak version 'GLIBC_2.39' not found (required by target/debug/blastit)`.
   Fix by **building inside the leg image** (a builder stage carrying the Rust toolchain on bookworm or
   trixie), not by raising the leg's base — a leg on a base ≥ the runner proves nothing about older
   distros.

# The rule to apply everywhere

**A container that runs our binary must be the one that built it, or must have a glibc ≥ the
builder's.** Audit every leg, not just the two that failed: the `standalone` jobs (`aarch64-musl` was
still running), every `shim + probe` variant, the musl and aarch64 legs, `rs-test-php-image` (which
already builds in the image), and anything that `docker run`s a binary built on the runner. For each,
say in `RESULT.md` whether it builds inside, and if not, why it is safe.

# Acceptance

```console
$ make check
$ make e2e
$ uv run --no-project tools/plan.py validate
```
- No workflow passes both a `manylinux:` image and `--compatibility`; `check_wheel_tag.py` still
  reports `manylinux_2_28` for a locally built wheel, and a fabricated `2_39` still fails it.
- Every leg that executes our binary in a container builds it there (or its base glibc ≥ the
  builder's, with the reason written down) — the audit table is in `RESULT.md`, one row per leg.
- `make check` exit 0 (including `workflow-check`'s 352 checks and `packaging-gates-test`), and
  `make e2e` exit 0 locally.
- If `scripts/check_workflows.py` can learn this rule, teach it: a step that runs a build-artefact
  inside a `container:`/`docker run` whose image is older than the builder should be refused. Say if
  that is expressible; a rule the auditor cannot see is a rule that will come back.

# Notes

- Do not change the matrix's declared or proven counts; this node fixes *how* the legs are built and
  run, not what they claim.
- A leg that still cannot be built inside its image is a written drop with the reason, not a silent
  pass, and it must not be counted as proved.
