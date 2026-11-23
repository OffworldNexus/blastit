# MB.74 — one bounded dnf retry helper for every image that installs build tooling

## Changed paths

New:

- `scripts/dnf-retry.sh`

Modified:

- `scripts/build-package-bundle.sh` (bundle assembly, dnf branch)
- `scripts/build-standalone-in-image.sh` (inner standalone build, dnf branch)
- `scripts/build-lsp.sh` (standalone LSP build, dnf branch — publish path,
  `publish-pypi.yml:483`)
- `scripts/build-rpm.sh` (container RPM build)
- `scripts/check-bundle-layout.sh` (RPM-set unpack fallback, `bash -euc` —
  `packages.yml:365-368`)
- `scripts/build-wheel.sh` (manylinux builder image provisioning)

## Helper contract (`scripts/dnf-retry.sh`)

- `dnf_install_retry <pkg>...`; sourceable and runnable directly.
- At most 3 attempts of `dnf install -y --setopt=install_weak_deps=False "$@"`,
  stderr folded into the captured output so the last failure can be shown.
- Between failed attempts: `dnf clean all` (a failing clean is a warning, not
  fatal) then `sleep 2`.
- Each dnf status is captured explicitly (`output=... || status=$?`), so
  `set -e` cannot abort between attempts; no `|| true` on the install; no
  pipeline is relied on.
- A **sourced** library changes no caller option (no file-scope `set -u`); the
  direct-run path sets `set -euo pipefail` itself. Correct under both
  `set -euo pipefail` and `bash -euc` because no pipefail-only construct is
  used and no variable is read unset (`${BASH_VERSION:-}`, `${BASH_SOURCE[0]:-}`,
  `${0:-}`).
- `sh scripts/dnf-retry.sh ...` is refused with
  `dnf-retry.sh must run under bash` and exit 1 instead of silently exiting 0.
- Final failure prints the last dnf output and a message naming the metadata
  error class (`Failed to download metadata for repo ...` = mirror mid-publish,
  re-run; otherwise the package or repo is genuinely wrong), then returns dnf's
  own status. No package names -> 2; no dnf -> 1.
- Only transport is retried: a bogus package still fails after the 3 attempts.

## Call sites (six, one implementation)

- `build-package-bundle.sh`: `. /src/scripts/dnf-retry.sh` then
  `dnf_install_retry clang binutils tar gzip git >/dev/null` inside the existing
  `bash -euo pipefail -c` body; `command -v dnf` guard kept.
- `build-standalone-in-image.sh`: same source and package list in the
  `elif command -v dnf` branch; `apk`/`apt-get` untouched.
- `build-lsp.sh`: `. /src/scripts/dnf-retry.sh` then
  `dnf_install_retry clang binutils tar gzip libcap >/dev/null` (its docker run
  already mounts `$root:/src`); `apt-get`/`apk` untouched.
- `build-rpm.sh`: extra read-only file mount at
  `/usr/local/lib/blastit/dnf-retry.sh`, then `.` and
  `dnf_install_retry rpm-build`; `command -v rpmbuild` and the `setpriv`/uid
  error context untouched. Proven under `bash -euc`.
- `check-bundle-layout.sh`: same single-file mount, `.` then
  `dnf_install_retry rpm cpio >/dev/null`; `command -v rpm2cpio/cpio` guard
  untouched; proven under `bash -euc` in `rockylinux:9`.
- `build-wheel.sh`: hand-written retry removed; `docker build -f - "$builder_ctx"`
  with a throwaway context at `$scratch/builder-context` (removed by the existing
  `restore_readme` EXIT trap), `COPY dnf-retry.sh ...`, then
  `RUN … dnf_install_retry clang && command -v clang >/dev/null && { dnf clean all || true; }`
  — post-condition restored, and only the cleanup stays tolerant. Image-inspect
  guard and tag unchanged.

## Proofs

### `bash -n` (seven changed scripts)

```
$ for f in scripts/dnf-retry.sh scripts/build-package-bundle.sh \
      scripts/build-standalone-in-image.sh scripts/build-rpm.sh scripts/build-wheel.sh \
      scripts/build-lsp.sh scripts/check-bundle-layout.sh; do bash -n "$f" && echo "OK: $f"; done
OK: scripts/dnf-retry.sh
OK: scripts/build-package-bundle.sh
OK: scripts/build-standalone-in-image.sh
OK: scripts/build-rpm.sh
OK: scripts/build-wheel.sh
OK: scripts/build-lsp.sh
OK: scripts/check-bundle-layout.sh
```

### Negative — bogus package, real dnf, manylinux_2_28_x86_64 (exit 1)

```
$ docker run --rm -v "$PWD/scripts/dnf-retry.sh:/helper.sh:ro" \
    quay.io/pypa/manylinux_2_28_x86_64 bash -euo pipefail -c \
    ". /helper.sh; dnf_install_retry blastit-no-such-package-xyz"
dnf_install_retry: dnf install failed after 3 attempts for: blastit-no-such-package-xyz
  last dnf output:
AlmaLinux 8 - BaseOS                             23 MB/s | 120 MB     00:05
AlmaLinux 8 - AppStream                          18 MB/s |  36 MB     00:02
...
No match for argument: blastit-no-such-package-xyz
Error: Unable to find a match: blastit-no-such-package-xyz
  If the output names `Failed to download metadata for repo ...`, the mirror was
  mid-publish and a re-run should pass. Otherwise the package name or the
  repository is genuinely wrong and this failure is the answer.
exit=1
```

### Positive — real toolchain install through the helper (exit 0)

```
$ docker run --rm -v "$PWD/scripts/dnf-retry.sh:/helper.sh:ro" \
    quay.io/pypa/manylinux_2_28_x86_64 bash -euo pipefail -c \
    ". /helper.sh; dnf_install_retry clang binutils tar gzip git >/dev/null; echo helper-ok; \
     command -v clang tar gzip git; clang --version | head -1; git --version"
helper-ok
/usr/bin/clang
/usr/bin/tar
/usr/bin/gzip
/usr/local/bin/git
clang version 22.1.8 ( 22.1.8-2.module_el8.10.0+4359+f07421b9)
git version 2.56.1
exit=0
```

### New site 1 — `build-lsp.sh` shape (its `-v $root:/src -w /src`)

```
$ docker run --rm -v "$PWD:/src" -w /src quay.io/pypa/manylinux_2_28_x86_64 \
    bash -euo pipefail -c ". /src/scripts/dnf-retry.sh; echo lsp-helper-ok; \
    command -v dnf_install_retry; test -f /src/scripts/dnf-retry.sh"
lsp-helper-ok
dnf_install_retry
exit=0
```

### New site 2 — `check-bundle-layout.sh` single-file mount, `bash -euc`, real install

```
$ docker run --rm --volume "$PWD/scripts/dnf-retry.sh:/usr/local/lib/blastit/dnf-retry.sh:ro" \
    rockylinux:9 bash -euc ". /usr/local/lib/blastit/dnf-retry.sh; \
    dnf_install_retry rpm cpio >/dev/null; echo layout-helper-ok; command -v rpm2cpio cpio"
layout-helper-ok
/usr/bin/rpm2cpio
/usr/bin/cpio
exit=0
```

### `bash -euc` failure shape, `sh` refusal, and no option leak

```
$ docker run --rm --entrypoint bash -v "$PWD/scripts/dnf-retry.sh:/helper.sh:ro" \
    quay.io/pypa/manylinux_2_28_x86_64 -euc '<stub dnf exiting 7>; . /helper.sh; dnf_install_retry clang'
dnf_install_retry: attempt 1/3 failed (status 7) for: clang
...
dnf_install_retry: dnf install failed after 3 attempts for: clang
exit=7

$ sh scripts/dnf-retry.sh clang
dnf-retry.sh must run under bash
sh-exit=1

$ bash -c '. scripts/dnf-retry.sh; set -o | grep nounset; command -v dnf_install_retry'
nounset         off
dnf_install_retry
```

### `docker build -f -` shape (build-wheel.sh) works

```
$ docker build -t blastit-mb74-selftest -f - <temp context with dnf-retry.sh> <<DOCKER
FROM quay.io/pypa/manylinux_2_28_x86_64
COPY dnf-retry.sh /usr/local/lib/blastit/dnf-retry.sh
RUN bash -euo pipefail -c ". /usr/local/lib/blastit/dnf-retry.sh && command -v dnf_install_retry"
DOCKER
#7 0.796 dnf_install_retry
build-exit=0
```

### One implementation

```
$ grep -rn "dnf_install_retry()\|for attempt in" scripts/*.sh
scripts/dnf-retry.sh:25:dnf_install_retry() {
```

Six call sites source it: `build-lsp.sh` 265/266, `build-package-bundle.sh`
182/183, `build-rpm.sh` 407/409, `build-standalone-in-image.sh` 237/238,
`check-bundle-layout.sh` 384/386, `build-wheel.sh` 370.

## Follow-ups

- None outstanding for `build-lsp.sh` / `check-bundle-layout.sh`: both are on
  the helper now.
- `scripts/build-standalone.sh:484` still shows `dnf install python3-libs` in a
  user-facing hint string (not executed), so it needs no retry.
- `build-wheel.sh` keeps a warm `blastit-manylinux-2_28:<arch>` image without
  rebuilding it, so an image provisioned before this change keeps its old
  Dockerfile; it already carries clang, and the next rebuild uses the helper.
