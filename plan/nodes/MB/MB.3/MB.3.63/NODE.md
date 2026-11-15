---
id: MB.3.63
title: "Make the standalone build container own its caches (read-only CARGO_HOME in manylinux)"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths: ["scripts/**", ".github/**", Makefile]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T21:06"
updated: "2026-11-15T21:06"
---

# Goal

Make the standalone legs' build container own what it writes. The manylinux build fails because
`CARGO_HOME`/`CARGO_TARGET_DIR` are not writable by the container user — the same rule as "a container
that runs our binary must have built it", one turn further: a container that builds must own its
caches.

# Context

CI on `71dcaf8`:

```text
error: failed to download `aho-corasick v1.1.5`
failed to create directory `/tmp/cargo-home/registry/src/...`
Read-only file system (os error 30)
```

`scripts/build-standalone-in-image.sh` mounts the host Rust toolchain read-only and the manylinux
container's `/tmp` (or the mounted `CARGO_HOME`) is not writable by the user it runs as. Mount a
**writable** `CARGO_HOME` and `CARGO_TARGET_DIR` the container owns (a tmpfs, or a directory created
inside the container and `chown`ed), and pass `--user "$(id -u):$(id -g)"` consistently so the same
identity writes and reads. Check the musl legs too — they were still running when this failed and hit
the same shape.

# Acceptance

```console
$ scripts/build-standalone-in-image.sh --image quay.io/pypa/manylinux_2_28_x86_64 --max-glibc 2_28
$ scripts/build-standalone-in-image.sh --image <musl builder> --max-glibc <floor>
$ make check
```
- Both the gnu and the musl in-image builds complete **from a cold cache** (clear the registry dir
  first, or build with a fresh `CARGO_HOME`) inside the container, as the same uid that then reads
  the artefact, and the produced binary still passes `--max-glibc` and the runtime check on
  bookworm/trixie (or alpine + `apk add libgcc` for musl).
- Nothing is written back into the repository by the container; `git status` after a build shows no
  stray artefacts outside the intended `--out`.
- `make check` exit 0 (workflow-check's rules still hold), and the auditor does not need a new rule —
  or, if this is expressible, it gains one ("a build container's cache paths must be writable by the
  uid it runs as") with a self-test.
- Paste the cold-cache build tail for each arch you can run here; say what only CI can prove.

# Notes

- This is the fourth instance of "the environment that builds is not the environment we assumed";
  if the auditor can see it, teach it.
