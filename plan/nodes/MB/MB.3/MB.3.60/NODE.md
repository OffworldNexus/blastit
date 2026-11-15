---
id: MB.3.60
title: "The standalone archive must not need the runner's glibc, and check-exec-env must pull before it reads"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [".github/**", "scripts/**", "e2e/**", Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T18:33"
updated: "2026-11-15T20:22"
---

# Goal

Make the "portable standalone binary" actually portable, and make the guard that proves it unable to
render a missing image as a version. Both are the build-host leak for the third time.

# Context

The `standalone` legs failed:

```text
check-exec-env: FAIL — blastit needs glibc 2.39, debian:trixie-slim has Unable to find image 'debian:trixie-slim' locally
    weak version 'GLIBC_2.39' not found (required by blastit)
       raise the image to glibc >= 2.39 — never the binary's floor
```

1. **The artefact's floor is 2.39 because the runner is Ubuntu 24.04.** A "portable standalone" that
   needs glibc 2.39 runs only on Ubuntu 24.04+/trixie — not what is advertised, and the same class as
   `manylinux_2_39` wheels and host-built test binaries. Build the archive in an **old-glibc builder**
   (a `manylinux_2_28` or `debian:bookworm` stage carrying the Rust toolchain) so its floor drops to
   2.28/2.36, and let `check-exec-env.sh` assert the **actual** floor against each smoke image
   (bookworm and trixie). If 2.28 is impossible with the pinned dependencies, say the floor out loud in
   the docs and in the archive's README rather than implying portability.
2. **`check-exec-env.sh` must `docker pull` before reading an image's glibc.** "Unable to find image …
   locally" was rendered as *the image's version*: a false negative that could equally have been a
   false **pass** on a cached image.

# Acceptance

```console
$ scripts/check-exec-env.sh <archive-binary> <image>      # both directions, both images
$ make check
$ make e2e
```
- Each standalone archive reports its real floor — `readelf --version-info <bin> | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -1` — and the x86_64-gnu archive's floor is **≤ the oldest smoke image's glibc** (bookworm 2.36), or the docs state the real floor.
- `check-exec-env.sh` pulls the image first (or fails loudly saying it could not), and can no longer read "no such image" as a version. Prove it: point it at a nonsense image tag and show it refuses instead of reporting a version.
- Both smoke images (bookworm, trixie) pass for the archive that is meant to run there; a musl archive needs no glibc at all — say what it needs instead (`apk add libgcc`, already known).
- `make check` exit 0 (the workflow auditor's rules still hold), `make e2e` exit 0.
- Report the floor per archive in `RESULT.md`, plus which legs only CI can prove.

# Notes

- This is the third instance of the same class (wheels `manylinux_2_39`, host-built test binaries in
  bookworm, now the standalone floor). If the auditor can learn "an artefact built on the runner and
  shipped to older systems must be built in the oldest image it claims", teach it — a rule it cannot
  see comes back a fourth time.
- Do not change the matrix's declared/proven counts; this node fixes how the artefact is built and how
  the guard reads an image.
