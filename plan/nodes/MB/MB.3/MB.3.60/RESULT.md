# RESULT — MB.3.60

The "portable" standalone archive now has a floor that a build *decided* instead
of a floor the runner happened to have, and `check-exec-env.sh` can no longer read
a missing image as a version. Both defects are fixed, both are proven locally in
both directions, and the workflow auditor learned the class so it does not return
a fourth time.

Base: `755281c` on `release/0.2.0`. No commit, no plan-status change.

---

## 1. The floor per archive (the number, not the claim)

Measured with the exact command the node names:

```console
$ readelf --version-info <bin> | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -1
```

| archive | built in | **floor** | runs on (proved) | only CI can prove |
|---|---|---|---|---|
| `blastit-x86_64-unknown-linux-gnu.tar.gz` | `quay.io/pypa/manylinux_2_28_x86_64` | **glibc 2.28** | manylinux_2_28 (2.28), `debian:bookworm-slim` (2.36), `debian:trixie-slim` (2.41) | — (proved here) |
| `blastit-x86_64-unknown-linux-musl.tar.gz` | `alpine:3.21` | **no glibc at all** (`NEEDED libgcc_s.so.1 libc.musl-x86_64.so.1`) | `alpine:3.21` + `apk add libgcc` | — (proved here) |
| `blastit-aarch64-unknown-linux-gnu.tar.gz` | `quay.io/pypa/manylinux_2_28_aarch64` | expected 2.28 | — | the whole leg: no arm64 host here |
| `blastit-aarch64-unknown-linux-musl.tar.gz` | `alpine:3.21` (arm64) | no glibc | — | the whole leg |

**Before this node the x86_64-gnu archive was glibc 2.39** — the `ubuntu-24.04`
runner's own libc, so it refused to start on Debian 12 (2.36), RHEL 9 (2.34) and
Ubuntu 22.04 (2.35). Same measurement, host build, for comparison:

```console
$ scripts/build-standalone.sh --max-glibc 2_28 --out /tmp/opencode/mb360/hostdist
ok   .../target/release/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
build-standalone: glibc floor 2.39 (readelf --version-info)
error: .../target/release/blastit needs glibc 2.39, above the 2.28 this archive must run on
  build it inside a container with glibc <= 2.28
  (scripts/build-standalone-in-image.sh --image quay.io/pypa/manylinux_2_28_$(uname -m))
  or raise --max-glibc and say so in the archive's docs — never lower it silently
exit=1
```

and the container build with the same flag:

```console
$ make standalone-glibc
build-standalone: cargo build --release --no-default-features --features php
ok   /src/target/standalone-2_28/release/blastit: NEEDED libgcc_s.so.1 libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
build-standalone: glibc floor 2.28 (readelf --version-info)
build-standalone: floor 2.28 <= --max-glibc 2.28
build-standalone: 3 shim(s) for x86_64-gnu from /src/target/php-sapi
build-standalone: 4981518 bytes -> dist/blastit-x86_64-unknown-linux-gnu.tar.gz
```

2.28 is not a hopeful number: it is `manylinux_2_28`'s own glibc, the same family
the wheels are tagged `manylinux_2_28` in, so **one floor now covers both artifact
families**. It is achieved with the pinned dependencies unchanged (no `Cargo.*`
edit was needed).

The floor is also written **into the archive**, next to the binary, because a
release note is not something a server can act on:

```console
$ tar -xzOf dist/blastit-x86_64-unknown-linux-gnu.tar.gz blastit-x86_64-unknown-linux-gnu/README.md
## What it needs at run time

**glibc 2.28 or newer.** That is the floor measured in this binary:

    readelf --version-info blastit | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -1

Check a host with `getconf GNU_LIBC_VERSION` before deploying. …

Each shim is `dlopen`ed, so the loader applies its version needs as well: a
shim's floor is the PHP distribution's, not this archive's.

- `libblastit_php82-x86_64-gnu.so` — glibc 2.17 or newer
- `libblastit_php83-x86_64-gnu.so` — glibc 2.17 or newer
- `libblastit_php84-x86_64-gnu.so` — glibc 2.17 or newer

## How it was built

Built inside `quay.io/pypa/manylinux_2_28_x86_64`. …
```

The musl README says what a musl archive needs instead, since "no glibc" is not
"nothing":

```console
$ tar -xzOf dist/blastit-x86_64-unknown-linux-musl.tar.gz blastit-x86_64-unknown-linux-musl/README.md | sed -n '/What it needs/,/How it was/p'
**No glibc requirement** — this is a musl binary, which does not use glibc at all.
It does need `libgcc_s.so.1`, which Alpine ships in the `libgcc` package:

    apk add libgcc

The binary is dynamically linked on purpose: a static musl build cannot `dlopen`,
and `dlopen(libphp)` is how `[route.php]` works.
```

## 2. Both smoke images pass, for the archive meant to run there

The gnu archive inside `debian:bookworm-slim` (2.36) and `debian:trixie-slim`
(2.41), plus the builder, exactly as the `binaries_gnu` job now runs it:

```console
$ (# same step as .github/workflows/publish-pypi.yml "Prove the shipped archive in every image it claims")
check-exec-env: ok — blastit needs glibc 2.28, quay.io/pypa/manylinux_2_28_x86_64 provides 2.28
check-exec-env: ok — blastit needs glibc 2.28, debian:bookworm-slim provides 2.36
check-exec-env: ok — blastit needs glibc 2.28, debian:trixie-slim provides 2.41
::group::debian:bookworm-slim
host: no CPython on PATH
ok   200  /                      Cache-Control: no-cache
ok   404  /mb17-does-not-exist
ok   exit 1 for a WSGI manifest (capability gate, names 'uv add blastit')
standalone-runtime-check: 3/3 passed without touching an interpreter
::group::debian:trixie-slim
… same 3/3 …
```

The musl archive in the image it claims (`alpine:3.21`, with only `libgcc` added —
the runtime package `ADR-0010` §4 already documented):

```console
$ scripts/check-exec-env.sh --image alpine:3.21 --binary /tmp/artifact-musl/…/blastit
check-exec-env: ok — blastit requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
$ docker run --rm … alpine:3.21 sh -c 'apk add --no-cache bash libgcc …; bash /src/scripts/standalone-runtime-check.sh --require-no-python …'
ok   200  /                      Cache-Control: no-cache
ok   404  /mb17-does-not-exist
ok   exit 1 for a WSGI manifest (capability gate, names 'uv add blastit')
standalone-runtime-check: 3/3 passed without touching an interpreter
```

## 3. `check-exec-env.sh` pulls before it reads — and refuses when it cannot

`docker pull` runs first; a pull failure, a `docker run` failure, and an answer
that is not `musl` or `<major>.<minor>` are all **refusals**, never comparisons.
The three proofs, plus the two directions of the comparison itself:

```console
# 1. A tag that does not exist: the exact CI message that used to be read as a version.
$ scripts/check-exec-env.sh --image debian:definitely-not-a-tag-mb360 --binary target/release/blastit
check-exec-env: FAIL — cannot pull image 'debian:definitely-not-a-tag-mb360', so its C library version is unknown:
  Error response from daemon: failed to resolve reference "docker.io/library/debian:definitely-not-a-tag-mb360": … not found
  an image this check cannot obtain has no glibc to compare: reporting a version here
  would be a guess. Fix the tag or the registry access, or pre-pull 'debian:definitely-not-a-tag-mb360'
exit=2

# 2. An image that pulls but cannot be inspected (no shell) is refused too, not "unknown".
$ scripts/check-exec-env.sh --image gcr.io/distroless/static-debian12:latest --binary …
check-exec-env: FAIL — cannot inspect image 'gcr.io/distroless/static-debian12:latest' (docker run exited 127), so its C library version is unknown:
  docker: … exec: "sh": executable file not found in $PATH
exit=2

# 3. The comparison still says both numbers, in both directions (2.39 binary).
$ scripts/check-exec-env.sh --image debian:trixie-slim --binary target/release/blastit
check-exec-env: ok — blastit needs glibc 2.39, debian:trixie-slim provides 2.41
exit=0
$ scripts/check-exec-env.sh --image debian:bookworm-slim --binary target/release/blastit
check-exec-env: FAIL — blastit needs glibc 2.39, debian:bookworm-slim has 2.36
  running it there fails at execve with:
    weak version 'GLIBC_2.39' not found (required by blastit)
  fix: build the binary inside debian:bookworm-slim (or a builder no older than it), or
       raise the image to glibc >= 2.39 — never the binary's floor
exit=1

# 4. A glibc binary in a musl image is still refused by name.
$ scripts/check-exec-env.sh --image alpine:3.21 --binary target/release/blastit
check-exec-env: FAIL — blastit needs glibc 2.39, and alpine:3.21 provides musl
exit=1
```

Before the fix the same first command reported
`image debian:trixie-slim has Unable to find image 'debian:trixie-slim' locally`
— i.e. docker's error text sat in the version slot, which is a false negative in
CI and would have been a false **pass** for any image that happened to be cached.

## 4. Systemic: the auditor learned the class (rule 15)

`scripts/check_workflows.py` now refuses, in `publish-pypi.yml`, a step that:

1. runs `scripts/build-standalone.sh` **on the runner** (no `docker run` in that
   step), i.e. compiles against the runner's C library; and
2. builds for a `-gnu` leg without naming that leg's `builder_image` **and**
   `glibc_floor` from `scripts/platform_matrix.py`, i.e. without letting
   `--max-glibc` assert the floor the matrix declares.

The rule is matrix-aware: it expands `${{ matrix.* }}` per concrete leg from
`platform_matrix.py`, so changing the declared floor without changing the build
fails the audit. It is a rule the workflow *executes* (`--max-glibc` is what
fails the build) with the audit making sure it is there — the same shape as
rule 14 for wheels and runner-built test binaries.

Two checked-in negative self-tests (`make packaging-gates-test`, 29 cases now,
14 in the workflow group) prove it cannot go vacuous:

* `/standalone-built-on-runner` — the pre-node shape restored, expects
  `runs scripts/build-standalone.sh on the runner`;
* `/standalone-without-floor` — a container build that drops
  `--max-glibc "${{ matrix.glibc_floor }}"`, expects `never passes it as`.

**What the rule cannot see** (stated in the module docstring): whether the named
image's glibc really equals the declared floor — that is `build-standalone.sh
--max-glibc` at build time and `check-exec-env.sh` at run time, both of which
read the finished binary — and a build driven from Python inside `e2e/`.

## 5. Acceptance commands

```console
$ make rs-no-python
…
ok   /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
default build: 0 pyo3 dependencies (no libpython linked)
real	0m1.530s
```

```console
$ make check
…
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims; …0 leg(s) dropped
workflow-check: ok — 364 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 29 cases passed (workflow=14, matrix=11, wheel-tag=4)
plan OK
make check exit=0
```

(The counts are unchanged: 8 wheels, 4 archives, 12 shims — this node changed how
one artifact is built and how a guard reads an image, not the matrix. Re-run once
after the last edit to `scripts/build-standalone.sh`: same tail, exit 0.)

```console
$ make e2e
scripts/build-wheel.sh --manylinux 2_28
…
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
…
PASS: 0 failure(s)          # e2e/run.py
PASS: 0 failure(s)          # e2e/rootless/run.py --local
  ok   a restart serves the page as it is now
  ok   and the restarted boot serves it (200)
  ok   still 0 decoy opens (0)
make e2e exit=0
```

## 6. Files changed

| file | change |
|---|---|
| `scripts/build-standalone-in-image.sh` (new) | builds the gnu archive inside `--image`, mounts the host Rust toolchain, redirects `CARGO_TARGET_DIR` per floor, requires `--max-glibc` |
| `scripts/build-standalone.sh` | honours `CARGO_TARGET_DIR`; `--max-glibc` measures and refuses; `--builder-image`; writes `README.md` into the archive with the binary's floor **and each shim's** |
| `scripts/check-exec-env.sh` | pulls first (or fails), accepts repeated `--image`, refuses any answer that is not `musl`/`<major>.<minor>`, never mixes docker's stderr into the version |
| `scripts/platform_matrix.py` | binary legs carry `builder_image`, `glibc_floor`, `smoke_images` (`runtime_image` stays for wheels) |
| `.github/workflows/publish-pypi.yml` | `binaries_gnu` builds in `matrix.builder_image` with `--max-glibc matrix.glibc_floor` and proves the archive in every `smoke_images` entry; `binaries_musl` names `matrix.builder_image` and records it in the archive README |
| `scripts/check_workflows.py` | rule 15 + docstring |
| `scripts/test_packaging_gates.py` | two negative self-tests for rule 15 |
| `Makefile` | `standalone-glibc` (the shipped way, one command locally), comment fix |
| `docs/knowledge/LESSONS.md` | one entry: `CARGO_TARGET_DIR` vs a relative `target/` path silently packaging the host binary; manylinux images carry `/usr/bin/python3` |

## 7. What only CI can prove

* **The aarch64 legs** (`standalone (aarch64-gnu)`, `standalone (aarch64-musl)`):
  no arm64 host here, so `quay.io/pypa/manylinux_2_28_aarch64` on
  `ubuntu-24.04-arm` and Alpine-on-arm are untested. The wrapper refuses a
  cross-arch container (`container_arch != arch`), so a wrong runner fails loudly
  instead of emulating.
* **The shim-carrying archive**: CI downloads the per-distro shims, so the
  `--require-shims N` path with three real shims runs there; locally the archive
  carried the three host-built shims (`glibc 2.17` each, measured into the README).
* **The real `dtolnay/rust-toolchain` mount + `Swatinem/rust-cache`** interaction
  (local runs mount `~/.rustup` the same way, but not the cache action).
* **`make e2e`'s Docker suite** is local (below); the *publish* pipeline itself
  only runs on the release PR.

## 8. Follow-ups (outside this node's `paths`)

* `docs/decisions/ADR-0010-…md:41` still lists the standalone archive's runtime
  requirement as **"Nothing for static"**. It should read "glibc 2.28+ (measured;
  `manylinux_2_28`), musl: `libgcc`", matching the archive README this node emits.
* `docs/getting-started.md:15` offers `cargo build -p blastit --locked` as the
  standalone story without a floor; if a user-facing page ever publishes the
  tarballs, it should name glibc 2.28 (and `apk add libgcc` for musl).
* The writer of this node edited `scripts/build-standalone.sh` while a container
  was executing it, and bash resumed at a stale offset (`line 116: args[@]+}:
  command not found`). Worth a LESSONS line if it ever bites CI: never edit a
  script a long-running container is mid-way through.
