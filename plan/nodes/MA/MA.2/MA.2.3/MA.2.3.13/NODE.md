---
id: MA.2.3.13
title: Make make e2e work on any CPython (prebuilt Dockerfile wheel glob)
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/**", "scripts/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T22:27"
updated: "2026-11-12T22:57"
---

# Goal

`make e2e` must work on any supported Python (this host builds a `cp314` wheel), not only where the interpreter happens to be CPython 3.13: the prebuilt-image Dockerfiles hardcode `cp313` in their `COPY` glob, so the build dies with `lstat /dist: no such file or directory` and a contributor cannot run the e2e suite at all.

# Context

- Observed at the MA.2.3/MA.2 gate: `make e2e` -> `scripts/build-wheel.sh` produced `dist/blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl`, then `Dockerfile.prebuilt:8` `COPY dist/blastit-*-cp313-*.whl /tmp/dist/` matched nothing and the build failed.
- `e2e/Dockerfile.prebuilt` and `e2e/rootless/Dockerfile.prebuilt` both hardcode `cp313` in the `COPY` glob and in the "exactly one wheel" error message.
- The e2e suite is the only proof of real multi-UID/rootless isolation, so it must run on the developer's machine as well as in CI (DX-first: local e2e must not require a specific interpreter).
- Skills `python-standards`, `testing`, `security-invariants`.

# Acceptance

```console
$ rm -rf dist && make e2e
# exit 0: Docker app + sqlalchemy legs and the rootless capability leg pass
```

- Both prebuilt Dockerfiles select the single `blastit-*` wheel in `dist/` whatever its CPython ABI tag (`cp3*.whl`), keep the "exactly one wheel, here is what I found" error, and fail loudly if zero or two wheels are present.
- A stale wheel from another interpreter in `dist/` must not silently mix: the count check stays authoritative.
- `make check` stays green (no Rust/Python product change).
- Paste the real `make e2e` tails (both legs) in RESULT.md, plus one line showing the wheel tag your host produced.

# Notes

- Touch only `e2e/**` (and `scripts/` only if the wheel staging genuinely needs it). Never weaken an isolation assertion to get green.
- Docker is available on this host; run the suite once for real, do not narrate.
