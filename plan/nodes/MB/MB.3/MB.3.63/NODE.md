---
id: MB.3.63
title: "Make the standalone build container own its caches (read-only CARGO_HOME in manylinux)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["scripts/**", ".github/**", Makefile]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-15T21:06"
updated: "2026-11-15T22:52"
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

# Owner addition (2026-11-15): the `aarch64-musl` red is the judge, not the artifact

The CI line, verbatim (`check-exec-env.sh:254`, the step's **first** check — the inner Alpine
runtime run never executes, because of `set -e`):

```text
check-exec-env: FAIL — blastit needs glibc 2.0, and alpine:3.21 provides musl
  a glibc ELF cannot start where there is no glibc: the loader is missing, not older
```

Three seconds earlier, the same job's builder printed:

```text
ok   /src/target/release/blastit: NEEDED libgcc_s.so.1 libc.musl-aarch64.so.1; no RPATH/RUNPATH
build-standalone: dynamic musl: /lib/ld-musl-aarch64.so.1
build-standalone: 4921908 bytes -> dist/blastit-aarch64-unknown-linux-musl.tar.gz
```

So the packaged binary is provably musl, and the check on that same file calls it a glibc ELF
needing **2.0** — a floor no aarch64 glibc binary has (2.17 is the architectural minimum), so it is
the parser's answer for something it misread. `2.0` can only come from a literal
`GLIBC_<major>.<minor>` in `readelf --version-info`, and there is no default in the script that
produces it. The **x86_64-musl leg passes the identical check**, which points at the arch.

What this node must do about it:

1. **Do not change the artifact to satisfy the check.** The artifact is provably musl. A "fix" that
   makes a musl binary pass a glibc check is the wrong direction — say so in RESULT.md.
2. **Make the leg print the evidence before it judges**: `readelf -h` (e_machine + INTERP) and the
   raw `readelf --version-info` for the *extracted* `blastit`, into the job log. One run turns the
   hypothesis into a fact, and a future failure names its own reason instead of a bare verdict.
3. **Reproduce locally by parsing an aarch64 musl binary on x86_64** — the shape most likely to be
   wrong is a host `readelf` reading a foreign-arch object, and it needs no arm runner. If
   `--version-info` cannot read it, the script's own header says that is `exit 2` "cannot decide",
   **not** `exit 1` "too old". A confident wrong answer is exactly the class `MB.3.60` fixed for
   image pulls.
4. If it turns out to be a genuine `GLIBC_2.0` need in the file, then the **builder's** ELF check is
   the liar, and `MB.3.60`'s "every leg names its builder and floor" rule is what caught it — say
   that plainly instead of patching either check to agree.

`aarch64-musl` stays honest: it is CI-only until the leg is green, and no RESULT may call it proved
before that.
