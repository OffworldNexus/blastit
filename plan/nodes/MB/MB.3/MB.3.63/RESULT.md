# Result MB.3.63

## Summary

Three release-blocking reds, three defects, none of them "the artefact is wrong".

**(1) The manylinux build container wrote through a read-only mount.** `scripts/build-standalone-in-image.sh`
gave cargo `CARGO_HOME=/tmp/cargo-home` and then *symlinked* `/host-cargo/registry` and
`/host-cargo/git` into it, where `/host-cargo` is the host cache mounted `:ro`. Cargo extracts
`registry/src/<crate>` and `git/checkouts/<dep>` **into** `CARGO_HOME`, so the first extraction
through the link is a write to a read-only mount: `Read-only file system (os error 30)`. It passed
review because the author's cache was warm — the crates were already extracted on the host, so
nothing wrote through the link. Reproduced here with a host cache that has `cache`/`index` but no
`src`/`checkouts` (below). The fix: `CARGO_HOME=/tmp/build-cargo-home` is a **writable** directory
**seeded by copying** the read-only mount, root installs the C toolchain and then `setpriv`s to
`HOST_UID`/`HOST_GID`, so **one identity** writes and reads the cache, `CARGO_TARGET_DIR` and
`--out`; the `chown -R` at exit is gone, and both bind-mounted directories are created by the
invoking user (a directory an older root-run build left behind is refused with the fix named).
`scripts/build-wheel.sh` had the same two `ln -s` lines and got the same copy seed (that script
still runs as root and chowns at exit — only the read-only write is fixed there, see Follow-ups).

**(2) The `aarch64-musl` red was the judge, not the artifact** (owner addition). The artifact is
provably musl (`NEEDED libc.musl-aarch64.so.1`, `INTERP /lib/ld-musl-aarch64.so.1`) and stays musl:
**nothing in this node changes it, and a musl binary must never be made to satisfy a glibc check.**
The bug is in `scripts/check-exec-env.sh`: `.gnu.version_r` lists the version nodes of *every*
needed library, and Alpine's **aarch64 `libgcc_s`** declares a node spelled `GLIBC_2.0` — so the
parser read "needs glibc 2.0", a floor no aarch64 glibc has (2.17 is the minimum), and refused the
one image that can run the file. The x86_64 leg passes because its `libgcc_s` needs only `GCC_*`
nodes: the arch was the variable, exactly as suspected. Reproduced **on x86_64 with the shipped
`aarch64-unknown-linux-musl` wheel** (before/after below, no arm runner needed). The fix reads the
libc from the ELF itself (`INTERP`, which `readelf` prints as an absolute path, and
`NEEDED libc.*`): when it says musl, the `GLIBC_*` names are printed as ignored and never compared;
when the readings contradict each other (a musl loader beside `NEEDED libc.so.6`, **or** a glibc
loader beside `NEEDED libc.musl-*` — derived from `INTERP`/`NEEDED` alone, before any version name)
the verdict is **2 "cannot decide"**, never 1 "too old" and never a false "links musl" pass. The
musl leg now also prints `readelf -h`/`-l`/`-d`/`--version-info` for the extracted binary **before**
it judges.

Review round 1 found three defects in that judge — the absolute-`INTERP` match never fired, the
reverse contradiction passed as "links musl", and the name-free forward contradiction passed as
"static". All three are fixed and each is now a case that was shown red before the fix and green
after it (Acceptance 4b).

**(3) The rule, so it cannot come back.** `scripts/check_workflows.py` gained rule 16 ("no symlink
under a container `CARGO_HOME`") with its own self-test, plus a negative fixture in
`scripts/test_packaging_gates.py`; `make check` runs both.

The honest limits: **both aarch64 legs are CI-only and unproved here.** `aarch64-musl`'s inner
Alpine runtime proof never executed in CI (`set -e` stopped at the judge), so that artifact's
runtime behaviour on aarch64 is still **unproven**. `aarch64-gnu` was never reproduced (needs an
arm64 host; the gnu path is proved on x86_64 only).

## Evidence

### Diagnosis, before any change: the extraction path is the read-only mount

Host cache copied without `registry/src` and without `git/checkouts`, mounted at `/host-cargo:ro`,
then the *previous* script's exact setup:

```console
$ docker run --rm -e CARGO_HOME=/tmp/cargo-home -v /tmp/opencode/cold-cargo:/host-cargo:ro \
    quay.io/pypa/manylinux_2_28_x86_64 bash -c 'mkdir -p /tmp/cargo-home;
      ln -s /host-cargo/registry /tmp/cargo-home/registry;
      ln -s /host-cargo/git /tmp/cargo-home/git; cargo fetch --locked'
Caused by:
  unable to update https://github.com/OffworldNexus/pregresql?rev=e00e747...
Caused by:
  failed to create directory `/tmp/cargo-home/git/checkouts/pregresql-ea3e5675c7c168e0`
Caused by:
  Read-only file system (os error 30)
```

With the *warm* host cache (which this checkout has) the same command succeeds and writes nothing —
which is why the defect survived review.

### Acceptance 1 — `scripts/build-standalone-in-image.sh --image quay.io/pypa/manylinux_2_28_x86_64 --max-glibc 2_28`, cold cache

`CARGO_HOME=/tmp/opencode/cold-gnu-cargo` was an **empty** directory (so the build downloads and
extracts everything itself) and `target/standalone-2_28` was removed first:

```console
$ CARGO_HOME=/tmp/opencode/cold-gnu-cargo scripts/build-standalone-in-image.sh \
      --image quay.io/pypa/manylinux_2_28_x86_64 --max-glibc 2_28
build-standalone-in-image: x86_64-unknown-linux-gnu inside quay.io/pypa/manylinux_2_28_x86_64 (floor 2_28)
cargo 1.100.0 (3acc79c0c 2026-11-05)
build-standalone-in-image: building as uid 1000:1000, cache /tmp/build-cargo-home
build-standalone-in-image: glibc glibc 2.28
build-standalone: cargo build --release --no-default-features --features php
    Updating crates.io index
    Updating git repository `https://github.com/OffworldNexus/pregresql`
 Downloading crates ...
  Downloaded pbkdf2 v0.12.2
...
    Finished `release` profile [optimized] target(s) in 1m 31s
ok   /src/target/standalone-2_28/release/blastit: NEEDED libgcc_s.so.1 libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
build-standalone: glibc floor 2.28 (readelf --version-info)
build-standalone: floor 2.28 <= --max-glibc 2.28
build-standalone: 0 pyo3 dependencies (no libpython linked)
build-standalone: 3 shim(s) for x86_64-gnu from /src/target/php-sapi
build-standalone: 4981344 bytes -> dist/blastit-x86_64-unknown-linux-gnu.tar.gz
build-standalone: checksum -> dist/SHA256SUMS
EXIT=0
```

170 crates downloaded and extracted inside the container (`grep -c Downloaded` = 170) — a genuine
cold fetch, not a pass on the host's warm `src/`. `--max-glibc` still measures and refuses, and the
packaged artefact is the one this build produced (`/src/target/standalone-2_28/...`, not a host
`target/release`).

The gnu archive then runs where it claims to, on the same numbers:

```console
$ scripts/check-exec-env.sh --image quay.io/pypa/manylinux_2_28_x86_64 \
      --image debian:bookworm-slim --image debian:trixie-slim --binary .../blastit
check-exec-env: ok — blastit needs glibc 2.28, quay.io/pypa/manylinux_2_28_x86_64 provides 2.28
check-exec-env: ok — blastit needs glibc 2.28, debian:bookworm-slim provides 2.36
check-exec-env: ok — blastit needs glibc 2.28, debian:trixie-slim provides 2.41
$ docker run --rm debian:bookworm-slim ... standalone-runtime-check.sh --require-no-python .../blastit
standalone-runtime-check: 3/3 passed without touching an interpreter
$ docker run --rm debian:trixie-slim ... standalone-runtime-check.sh --require-no-python .../blastit
standalone-runtime-check: 3/3 passed without touching an interpreter
```

Nothing written back into the repository — only `--out` and `target/<subdir>`, both git-ignored:

```console
$ git status --short
 M .github/workflows/publish-pypi.yml
 M plan/BUDGET.md
 M plan/JOURNAL.md
 M plan/nodes/MB/MB.3/MB.3.62/LOG.md
 M plan/nodes/MB/MB.3/MB.3.63/LOG.md
 M plan/nodes/MB/MB.3/MB.3.63/NODE.md
 M plan/nodes/MB/MB.3/MB.3.65/LOG.md
 M plan/nodes/MB/MB.3/MB.3.65/NODE.md
 M scripts/build-standalone-in-image.sh
 M scripts/build-wheel.sh
 M scripts/check-exec-env.sh
 M scripts/check_workflows.py
 M scripts/test_packaging_gates.py
?? plan/nodes/MB/MB.20/
?? plan/nodes/MB/MB.3/MB.3.63/RESULT.md
```

(every `plan/**` entry above is another node's work in this shared worktree, except this
`RESULT.md`; `dist/` and `target/` are git-ignored, so the build left no stray file.)

### Acceptance 2 — the musl equivalent runnable here (`alpine:3.21`, x86_64), cold

The workflow's own block, run verbatim (fresh container ⇒ cold cargo, rustup inside Alpine as the
leg does):

```console
$ docker run --rm -e HOST_UID -e HOST_GID -e TARGET=x86_64-unknown-linux-musl -e OWED=0 \
    -e BUILDER_IMAGE=alpine:3.21 -v "$PWD:/src" -w /src alpine:3.21 sh -eu -c '...'
    Updating crates.io index
  Downloaded pg_query v6.2.1
...
    Finished `release` profile [optimized] target(s) in 1m 16s
ok   /src/target/release/blastit: NEEDED libgcc_s.so.1 libc.musl-x86_64.so.1; no RPATH/RUNPATH
build-standalone: no glibc floor (musl); needs libgcc_s.so.1 (apk add libgcc)
build-standalone: dynamic musl: /lib/ld-musl-x86_64.so.1
build-standalone: 5147183 bytes -> dist/blastit-x86_64-unknown-linux-musl.tar.gz
BUILD_EXIT=0

# the two steps CI runs after it, on the extracted archive:
$ scripts/check-exec-env.sh --image alpine:3.21 --binary .../blastit
check-exec-env: ok — blastit requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
$ docker run --rm alpine:3.21 sh -c 'apk add bash libgcc; bash scripts/standalone-runtime-check.sh ...'
host: no CPython on PATH
ok   200  /                      Cache-Control: no-cache
ok   2/2 static responses without touching an interpreter
ok   exit 1 for a WSGI manifest (capability gate, names 'uv add blastit')
standalone-runtime-check: 3/3 passed without touching an interpreter
```

The musl build path itself is sound on x86_64; that is why `standalone (x86_64-musl)` is green — and
it says nothing about aarch64, which is CI-only.

### Acceptance 3 — the `aarch64-musl` judge, reproduced on x86_64 (before/after)

Fixture: the shipped `0.2.0` `musllinux_1_2_aarch64` wheel's extension module (an aarch64 musl Rust
artefact, unpacked from `wheel-cp314-aarch64-musl`). Its own ELF: `Machine: AArch64`,
`NEEDED libgcc_s-69c45f16.so.1`, `NEEDED libc.musl-aarch64.so.1`, and

```console
$ readelf --version-info .../_blastit.cpython-314-aarch64-linux-musl.so
Version needs section '.gnu.version_r' contains 1 entry:
  000000: Version: 1  File: libgcc_s-69c45f16.so.1  Cnt: 4
  0x0010:   Name: GCC_3.0 ...
  0x0020:   Name: GLIBC_2.0 ...        <-- Alpine aarch64 libgcc_s's version script
```

Before (`git show HEAD:scripts/check-exec-env.sh`), verbatim the CI sentence:

```console
$ /tmp/opencode/check-exec-env.before.sh --image alpine:3.21 --binary .../_blastit.cpython-314-aarch64-linux-musl.so
check-exec-env: FAIL — _blastit.cpython-314-aarch64-linux-musl.so needs glibc 2.0, and alpine:3.21 provides musl
  a glibc ELF cannot start where there is no glibc: the loader is missing, not older
exit=1
```

After:

```console
$ scripts/check-exec-env.sh --image alpine:3.21 --binary .../_blastit.cpython-314-aarch64-linux-musl.so
check-exec-env: _blastit...so links musl (NEEDED libc.musl-aarch64.so.1), so GLIBC_2.0 in .gnu.version_r is not a glibc requirement
  those names are the version script of a library that is not glibc (libgcc_s/libunwind); machine AArch64
check-exec-env: ok — ... requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
exit=0
```

The gnu path is untouched (`2.28` still compared and still refused in a musl image, with the
binary's own machine/interpreter/`NEEDED` now printed), and an unparseable file is still exit 2:

```console
$ scripts/check-exec-env.sh --image debian:bookworm-slim --binary .../blastit-x86_64-unknown-linux-gnu/blastit
check-exec-env: ok — blastit needs glibc 2.28, debian:bookworm-slim provides 2.36
$ scripts/check-exec-env.sh --image alpine:3.21 --binary .../blastit-x86_64-unknown-linux-gnu/blastit
check-exec-env: FAIL — blastit needs glibc 2.28, and alpine:3.21 provides musl
  the binary's own ELF: machine Advanced Micro Devices X86-64, interpreter /lib64/ld-linux-x86-64.so.2, NEEDED: ...
exit=1
$ scripts/check-exec-env.sh --image alpine:3.21 --binary README.md
check-exec-env: FAIL — cannot read symbol versions from 'README.md', so its glibc requirement is unknown:
  readelf: Error: Not an ELF file - it has the wrong magic bytes at the start
exit=2
```

### Acceptance 4 — the rule and its self-tests (`make check` runs both)

Rule 16 scans the repository's shell and workflow text for a symlink under a `CARGO_HOME` a container
is given. It found the same defect a second time in `scripts/build-wheel.sh` (now also seeded by
copying, which is the only change made there):

```console
$ UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py
workflow-check: rule 15 self-test ok — `make standalone` is a host build
workflow-check: rule 16 self-test ok — a CARGO_HOME symlink is a read-only write
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)

$ UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 37 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=7)
```

The seven `exec-env` cases pin: unparseable file ⇒ exit **2** (not 1); musl artefact carrying
`GLIBC_2.0` ⇒ pass, naming what it ignored; the same name on a glibc ELF ⇒ still compared, still
exit 1 in a musl image; and the four contradiction shapes below. They are not vacuous: run against
the pre-fix judge they fail with the CI sentence —

```console
$ cp /tmp/opencode/check-exec-env.before.sh scripts/check-exec-env.sh && uv run ... test_packaging_gates.py
AssertionError: a musl artifact whose version names include GLIBC_2.0 must pass in a musl image,
not be refused as glibc (MB.3.63); got 1
check-exec-env: FAIL — blastit needs glibc 2.0, and alpine:3.21 provides musl
```

and the workflow-fixture case 15 mutates `build-standalone-in-image.sh` back to the symlink and
requires the auditor to reject it (`returncode == 1 and "symlinks /host-cargo/registry" in stdout`).

### Acceptance 4b — the reviewer's adversarial shapes (attempt 2)

The first version of this judge got three shapes wrong, and all three are now encoded as cases:
`readelf` prints the interpreter as an **absolute** path (`/lib/ld-musl-x86_64.so.1`), so a bare
`ld-musl-*` case never fired; and the contradiction was gated on a non-empty `GLIBC_*` name, so two
impossible files passed. The contradiction is now derived from `elf_interp` + `elf_needed` alone,
before the `required` guard, and exits **2 in both directions**.

Red, against the code as reviewed (`got N` is the test's own message):

```console
$ uv run --no-project python - <<'PY'   # the new cases, driven by the test's own fixtures
(a) interp-only musl (expect 0): got 1
    check-exec-env: FAIL — blastit needs glibc 2.0, and alpine:3.21 provides musl
      the binary's own ELF: machine AArch64, interpreter /lib/ld-musl-x86_64.so.1, NEEDED: libgcc_s.so.1
(b) reverse contradiction: glibc interp + NEEDED libc.musl-* (expect 2): got 0
    check-exec-env: blastit links musl (NEEDED libc.musl-aarch64.so.1), so GLIBC_2.0 ... (a false pass)
(c) musl interp + NEEDED libc.so.6, no GLIBC names (expect 2): got 0
    check-exec-env: ok — blastit requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
```

Green after the fix:

```console
(a) interp-only musl (expect 0): got 0
    check-exec-env: blastit links musl (interpreter /lib/ld-musl-x86_64.so.1), so GLIBC_2.0 in .gnu.version_r is not a glibc requirement
    check-exec-env: ok — blastit requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
(b) reverse contradiction: glibc interp + NEEDED libc.musl-* (expect 2): got 2
    check-exec-env: FAIL — blastit contradicts itself: a glibc interpreter (/lib64/ld-linux-x86-64.so.2) beside NEEDED libc.musl-*
(c) musl interp + NEEDED libc.so.6, no GLIBC names (expect 2): got 2
    check-exec-env: FAIL — blastit contradicts itself: a musl interpreter (/lib/ld-musl-aarch64.so.1) beside NEEDED libc.so.6
(d) forward contradiction with names (expect 2): got 2
    check-exec-env: FAIL — blastit contradicts itself: NEEDED libc.so.6 and NEEDED libc.musl-* in one file
(e) glibc ELF, GLIBC_2.0 names (expect 1 in a musl image): got 1
    check-exec-env: FAIL — blastit needs glibc 2.0, and alpine:3.21 provides musl
      the binary's own ELF: machine AArch64, interpreter /lib64/ld-linux-x86-64.so.2, NEEDED: libgcc_s.so.1 libc.so.6
```

The reviewer's shapes, on real files here:

```console
$ scripts/check-exec-env.sh --image alpine:3.21 --binary .../blastit-x86_64-unknown-linux-musl/blastit
check-exec-env: ok — blastit requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
exit=0
$ scripts/check-exec-env.sh --image quay.io/pypa/manylinux_2_28_x86_64 --binary /usr/bin/rg
check-exec-env: FAIL — rg needs glibc 2.39, quay.io/pypa/manylinux_2_28_x86_64 has 2.28
  running it there fails at execve with: weak version 'GLIBC_2.39' not found (required by rg)
exit=1
$ scripts/check-exec-env.sh --image alpine:3.21 --binary README.md
check-exec-env: FAIL — cannot read symbol versions from 'README.md', ... readelf: Error: Not an ELF file
exit=2
$ scripts/check-exec-env.sh --image alpine:3.21 --binary /tmp/opencode/not-an-elf.png
exit=2
$ scripts/check-exec-env.sh --image alpine:3.21 --binary .../_blastit.cpython-314-aarch64-linux-musl.so
check-exec-env: ... links musl (NEEDED libc.musl-aarch64.so.1), so GLIBC_2.0 ... is not a glibc requirement
exit=0
```

### Acceptance 4c — `critical_guard.py --base 637aa97 --node MB.3.63`

```console
$ uv run --no-project tools/critical_guard.py --base 637aa97 --node MB.3.63
critical-guard: 637aa97..working tree, 13 changed file(s)
node MB.3.63: paths ['scripts/**', '.github/**', 'Makefile']
  no change to a protected critical test
exit=0
```

### Acceptance 5 — `make check` (re-run on the final tree, after the attempt-2 judge fix)

```console
$ make check
...
UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py
workflow-check: rule 15 self-test ok — `make standalone` is a host build
workflow-check: rule 16 self-test ok — a CARGO_HOME symlink is a read-only write
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 37 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=7)
uv run --no-project tools/plan.py validate
plan OK
CHECK_EXIT=0
```

(earlier in the same run: `guard: OK (3 staged/untracked file(s) checked)`,
`critical-guard: HEAD..working tree, 13 changed file(s) / no change to a protected critical test`,
`cargo fmt --all -- --check` clean, `clippy` zero warnings, workspace `cargo test` and the Python
suites green, `docs-unit: 83 passed, 65 subtests passed`, `rs-no-python` and
`platform-matrix-check` green. The `docs-assets` `retired_php_vocabulary` finding the brief warned
about did **not** appear (its brief was reworded at source), and nothing under `plan/` was touched
here.)

## Files

- `scripts/build-standalone-in-image.sh` — writable `CARGO_HOME` seeded by copying, `setpriv` to
  `HOST_UID`/`HOST_GID` before the first cargo, `chown`-at-exit trap removed, bind-mounted dirs
  created by the caller with a refusal that names the fix.
- `scripts/build-wheel.sh` — same two `ln -s` lines replaced by the copy seed (no uid change).
- `scripts/check-exec-env.sh` — the ELF's own libc decides (absolute `INTERP` matched as
  `*/ld-musl-*`, `NEEDED libc.musl-*`); `GLIBC_*` names from a non-glibc version script are reported
  and ignored; contradictory `INTERP`/`NEEDED` evidence is exit 2 in **both** directions, before any
  version name is believed; refusals print machine / interpreter / `NEEDED`.
- `scripts/check_workflows.py` — rule 16 + `self_test_rule16`, both run in `make check`.
- `scripts/test_packaging_gates.py` — 7 `exec-env` cases (fake `readelf`/`docker` answering with the
  recorded aarch64 transcript: unparseable, musl-with-GLIBC-names, glibc-with-those-names,
  interp-only musl, and the two directions of contradiction plus the name-free one) and
  workflow-fixture case 15 for rule 16.
- `.github/workflows/publish-pypi.yml` — the musl leg prints `readelf -h` / `-l` (INTERP) / `-d`
  (`NEEDED`) / the `Version needs` section for the extracted binary before `check-exec-env.sh`.
- No `Makefile` change: `make check` already runs `workflow-check` and `packaging-gates-test`.

## Follow-ups

- **`aarch64-gnu` and `aarch64-musl` remain CI-only.** `aarch64-gnu` was not reproduced (needs an
  arm64 host; an emulated C compiler is not evidence). `aarch64-musl`'s *judge* is fixed and its
  reproduction is on x86_64, but the leg's inner Alpine runtime check still never ran there, so the
  artifact's runtime behaviour on aarch64 is **unproven**; the next CI run either turns green or
  prints the ELF evidence that explains why not.
- `scripts/build-wheel.sh` still runs its container as root and `chown -R`s at exit; only the
  read-only-cache write is fixed there. Converting it to one `setpriv` uid is the same change as
  `build-standalone-in-image.sh` and wants its own acceptance (two Pythons × two arches).
- **The `binaries_musl` job still runs as root** (the reviewer's structural note): its inline
  `docker run … alpine` step in `.github/workflows/publish-pypi.yml` runs cargo as root and
  `chown -R`s `/src/target` and `/src/dist` at exit — the exact identity shape this node removed
  from the gnu script. It is not a two-line change: the host toolchain is glibc and cannot run
  build scripts inside Alpine, so one script for both paths needs either a musl-hosted toolchain
  per image or a `rustup` inside a single-uid container.
- `scripts/check-exec-env.sh` still passes a **musl** binary in a **glibc** image (documented as
  "the age of the image's libc cannot refuse it"), which is a *missing* refusal rather than a false
  one. Out of this node's blast radius (no leg does it).
- Two `docs/knowledge/LESSONS.md` entries belong here but `docs/**` is outside this node's paths:
  (a) cargo extracts `registry/src/<crate>` and `git/checkouts/<dep>` **into** `CARGO_HOME`, so
  symlinking a read-only cache into it fails only on a cold cache (`Read-only file system (os error
  30)`) and passes on a warm one; (b) `Name: GLIBC_*.` in `.gnu.version_r` names the version script
  of *any* needed library — Alpine's aarch64 `libgcc_s` spells a node `GLIBC_2.0`, so a musl
  artifact can read as needing glibc 2.0; read `INTERP`/`NEEDED libc.*` instead.
