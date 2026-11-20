#!/usr/bin/env python3
"""The `0.2.x` Linux packaging matrix: the one place a leg is declared (MB.3.18).

Every job that builds a distribution artifact — the wheels, the standalone
archives and the PHP SAPI shims — takes its list of legs from here, and every
document that states a count is checked against here. A leg is added (or
re-enabled) once, in `LEGS`, and appears in all of them; nothing is copied per
job. `ADR-0010` §4 is the table of record for what the artifacts contain; this
module is the table of record for *which* targets are attempted, and for what
has actually been built rather than merely declared.

Three things make this more than a matrix constant:

1. **Provenance is data, not prose.** `PROVEN` lists the artifacts that were
   built *and* exercised (a wheel installed and served, an archive executed, a
   shim `dlopen`ed and probed), with the node that did it. `--summary` prints
   declared next to proved, separately, because they are different claims: a
   declared leg is one CI builds on the release PR, a proved one is one whose
   artifact has been run.

2. **A count in a document is a claim, and claims are checked — but only in a
   document a reader can reach.** `--check` reads every page the site publishes
   *and* the two internal records it pins (the ADR is the table of record a
   reviewer reads), refusing any `N wheels/archives/shims/lsp assets` above the
   declared matrix and any `N proven …` above `PROVEN`. The site's own
   `exclude_docs` list (`zensical.toml`) decides what a page is: a design
   *review* that quotes a count is working material, not a promise, and letting
   it red `make check` is how the gate dies on an untracked file (`MB.35.5`).
   Hits in the two records are listed as notes, so the report says which
   document states what.

3. **A dropped leg keeps its reason.** `enabled: False` carries
   `dropped_because` in one sentence, and `--summary` quotes it, so the honest
   count is computed from what is enabled and the reason does not live in a
   commit message.

Usage (stdlib only; `make platform-matrix-check` runs the first):

    scripts/platform_matrix.py --check
    scripts/platform_matrix.py --matrix wheels      # GitHub Actions matrix JSON
    scripts/platform_matrix.py --matrix binaries
    scripts/platform_matrix.py --matrix shims
    scripts/platform_matrix.py --matrix py-shims   # the 8 per-minor CPython shims
    scripts/platform_matrix.py --matrix images
    scripts/platform_matrix.py --matrix lsp         # editor binaries (five targets)
    scripts/platform_matrix.py --matrix packages    # the 4 .deb/.rpm legs
    scripts/platform_matrix.py --matrix install     # the 6 container install legs
    scripts/platform_matrix.py --summary            # Markdown, for a RESULT
    scripts/platform_matrix.py --count shims --libc gnu --arch x86_64
    scripts/platform_matrix.py --count lsp
    scripts/platform_matrix.py --leg lsp --target x86_64-unknown-linux-gnu
    scripts/platform_matrix.py --leg shims --minor 8.3 --libc gnu --arch x86_64
    scripts/platform_matrix.py --leg install --name debian-13
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# --- the matrix -------------------------------------------------------------

# `runs_on` is the native runner for the arch, never an emulator: `ADR-0010` §4.2
# forbids cross-compiling a shim against a foreign `php_config.h`, and an
# emulated runner would make the probe evidence about qemu's syscalls.
ARCHES = {
    "x86_64": {"runs_on": "ubuntu-24.04", "manylinux_arch": "x86_64"},
    "aarch64": {"runs_on": "ubuntu-24.04-arm", "manylinux_arch": "aarch64"},
}

LIBCS = {
    "gnu": {
        "wheel_tag": "manylinux_2_28",
        "manylinux": "2_28",
        # The glibc floor the gnu artifacts are allowed to need, in the
        # `<major>_<minor>` spelling `check_wheel_tag.py --max-glibc` and
        # `build-standalone.sh --max-glibc` both take. These two artifact
        # families share it on purpose: a wheel tagged `manylinux_2_28` and an
        # archive that needs glibc 2.39 are two different promises about one
        # program, and only the lower one is true.
        "glibc_floor": "2_28",
        # The ceiling a **package** bundle may measure. It is not the newest base a
        # PHP leg builds on: a package is a promise about other machines, so the
        # number is the oldest target the release claims — RHEL/Rocky 9 (2.34),
        # Debian 12 (2.36) and Ubuntu 22.04 (2.35) all satisfy it. `build-{deb,rpm}.sh
        # --max-glibc` takes this value and refuses a bundle above it.
        #
        # A shim that measures higher is **rebuilt on an older base**, never
        # accommodated by dropping a distribution: Ondřej Surý publishes PHP 8.3,
        # 8.4, 8.5 and 8.6 for `bookworm` (glibc 2.36, the repository the 8.6 gnu
        # leg already uses), and `manylinux_2_28` is the floor for `blastit`,
        # `blastit-lsp` and the Python shims. The measured needs of the shipped
        # shims are far below this ceiling today (the PHP shims are 2.17), so the
        # declared `Depends:`/`Requires:` is the measured number, and this is only
        # the gate that keeps it there.
        "shim_floor": "2_34",
        # Where a glibc artifact is **built**: the image whose glibc *is* the
        # floor, so the floor is a property of the build rather than of whichever
        # runner happened to pick up the job (`MB.3.60`: `ubuntu-24.04` gave
        # every archive `GLIBC_2.39`, which then refused to start on Debian 12).
        # One image per arch, because a gcc on native hardware is the only
        # compiler that proves anything.
        "builder_image": "quay.io/pypa/manylinux_2_28_{arch}",
        # Every image the archive is *executed* in, oldest first: Debian 12
        # (glibc 2.36) and 13 (2.41), the two glibc distros whose libc the docs
        # are about. A tag added here is a claim CI then executes.
        #
        # The builder image is checked separately (`check-exec-env.sh` is handed
        # `builder_image` as well): it compiled the binary, so it must run it, but
        # it cannot be the *execution* proof here because a manylinux image ships
        # `/usr/bin/python3` for wheels and `--require-no-python` would be about
        # the image instead of the artifact.
        "smoke_images": "debian:bookworm-slim,debian:trixie-slim",
    },
    "musl": {
        "wheel_tag": "musllinux_1_2",
        "musllinux": "1_2",
        # No glibc at all: musl does not version its symbols, and
        # `libc.musl-<arch>.so.1` is ABI-stable across musl 1.x, so there is no
        # floor to state. The runtime package a dynamic musl Rust binary needs is
        # `libgcc` (`apk add libgcc`), which the archive README says.
        "glibc_floor": "",
        "builder_image": "alpine:3.21",
        "smoke_images": "alpine:3.21",
    },
}

# The CPython minors one `py3-none` wheel is *smoked* in (design §6.1): the
# artifact itself serves every CPython `>=3.12`, so three interpreted legs per
# wheel is the promise, and one `cpNN` wheel per minor would be a second matrix.
SMOKE_PYTHONS = ("3.12", "3.13", "3.14")

# The CPython minors a **shim** exists for (`MB.42.2`, design §1): a WSGI view
# `dlopen`s its own minor's shim, so `3.13` and `3.14` are two artifacts per
# `(arch, libc)`, never one.
PY_MINORS = {"3.14": "314", "3.13": "313"}

# The two native package formats (`MB.42.3.1`), one artifact family each. The
# `pattern` is the filename `scripts/build-{deb,rpm}.sh` writes, with the placeholders
# a caller substitutes; `image` is where `scripts/test_packages.sh` installs it.
PACKAGE_FORMATS = {
    "deb": {
        "arch": {"x86_64": "amd64", "aarch64": "arm64"},
        "pattern": "blastit_<version>_{package_arch}.deb",
        "image": "debian:trixie",
    },
    "rpm": {
        "arch": {"x86_64": "x86_64", "aarch64": "aarch64"},
        "pattern": "blastit-<version>-<release>.{package_arch}.rpm",
        "image": "rockylinux:9",
    },
}

# The container install matrix (`MB.42.3.4`): every distribution the signed
# repositories advertise, and the package a machine of that distribution
# installs. One leg per distribution — a machine has one package manager and
# installs one of the four packages — and `format` says which. `modes` is what
# `scripts/test_packages.sh` drives in that image: every leg installs the package
# and serves a static route, and only `debian:trixie` also runs the WSGI gate,
# because the other five ship no CPython 3.13/3.14 (Rocky 9 has 3.9, Ubuntu 22.04
# has 3.10) and a WSGI leg there would test an interpreter the product never
# claims. `installer` is the package manager the image has.
#
# `packages.yml` and `nightly.yml` take this list with `--matrix install`, and a
# literal `debian:...`/`fedora:...` in either workflow is refused by
# `scripts/check_workflows.py`: the second list is the one that goes stale when a
# distribution is added or dropped. `install_legs()` expands each entry over the
# two architectures the release ships, so an `aarch64` package is installed
# natively too rather than being a promise.
INSTALL_LEGS = (
    {
        "name": "debian-12",
        "distro": "debian",
        "release": "12",
        "image": "debian:bookworm-slim",
        "format": "deb",
        "installer": "apt",
        "modes": "install static",
    },
    {
        "name": "debian-13",
        "distro": "debian",
        "release": "13",
        "image": "debian:trixie",
        "format": "deb",
        "installer": "apt",
        # The one leg that also runs the WSGI gate: Debian 13 ships the
        # `python3-pytest` + `libpython3.13` the packaged shim needs, so the
        # package's own `python/blastit` tree is imported by a real gate.
        "modes": "install static wsgi",
    },
    {
        "name": "ubuntu-22.04",
        "distro": "ubuntu",
        "release": "22.04",
        "image": "ubuntu:22.04",
        "format": "deb",
        "installer": "apt",
        "modes": "install static",
    },
    {
        "name": "ubuntu-24.04",
        "distro": "ubuntu",
        "release": "24.04",
        "image": "ubuntu:24.04",
        "format": "deb",
        "installer": "apt",
        "modes": "install static",
    },
    {
        "name": "rocky-9",
        "distro": "rocky",
        "release": "9",
        "image": "rockylinux:9",
        "format": "rpm",
        "installer": "dnf",
        "modes": "install static",
    },
    {
        "name": "fedora",
        "distro": "fedora",
        "release": "latest",
        "image": "fedora:latest",
        "format": "rpm",
        "installer": "dnf",
        "modes": "install static",
    },
)

# The distro that carries the headers *and* the embedded SAPI for each
# (minor, libc). Every entry is a signed distro package on a distro that ships
# that minor: no single distro ships 8.3, 8.4, 8.5 and 8.6, and Alpine and Debian
# disagree about both the package names (`php86-dev` vs `php8.6-dev`) and where
# the library lives (`/usr/lib/php86/libphp.so` vs `/usr/lib/libphp8.6.so`).
#
# `extra_repo` is the repository a leg needs *before* its packages exist, as a
# name this file declares and the consumers switch on (`MB.42.5`): no leg
# hard-codes a keyring or an `edge/testing` line in the Dockerfile, the workflow
# or the probe driver. `""` is a stock image's own repositories.
#
# `suites` is whether this leg also compiles and runs the two protected suites
# (`critical_php_backend`, `critical_php_embed`) **in its own image**
# (`MB.3.58`). A leg whose distro cannot carry them says why in
# `suites_because`, and the reason is part of the matrix rather than a step
# condition buried in a workflow: "the shim builds and probes here, but the
# worker-level suites cannot run" is a claim a reader has to be able to see.
PHP_BUILDS = {
    ("8.6", "gnu"): {
        # `dpkg -L libphp8.6-embed` in `debian:bookworm-slim` (glibc 2.36, the
        # lowest floor of any PHP leg) with Ondřej Surý's signed repository:
        # the package is 8.6.0 and the NTS library is `/usr/lib/libphp8.6.so`.
        "base": "debian:bookworm-slim",
        "extra_repo": "sury",
        "dev": "php8.6-dev",
        "embed": "libphp8.6-embed",
        "curl": "php8.6-curl",
        "libphp": "/usr/lib/libphp8.6.so",
        "suites": True,
    },
    ("8.6", "musl"): {
        # `apk info -L php86-embed` in `alpine:edge` after appending
        # `edge/testing`: `/usr/lib/php86/libphp.so`. The shim, the probe and —
        # since `MB.42.10` gave the handoff a musl-declared `execveat` — the
        # worker-level suites all build and run here.
        "base": "alpine:edge",
        "extra_repo": "alpine-edge-testing",
        "dev": "php86-dev",
        "embed": "php86-embed",
        "curl": "php86-curl",
        "libphp": "/usr/lib/php86/libphp.so",
        "suites": True,
    },
    ("8.5", "gnu"): {
        # `dpkg -L libphp8.5-embed` in `ubuntu:26.04`: `/usr/lib/libphp8.5.so`.
        "base": "ubuntu:26.04",
        "dev": "php8.5-dev",
        "embed": "libphp8.5-embed",
        "curl": "php8.5-curl",
        "libphp": "/usr/lib/libphp8.5.so",
        "suites": True,
    },
    ("8.3", "gnu"): {
        "base": "ubuntu:24.04",
        "dev": "php8.3-dev",
        "embed": "libphp8.3-embed",
        "curl": "php8.3-curl",
        "libphp": "/usr/lib/libphp8.3.so",
        "suites": True,
    },
    ("8.4", "gnu"): {
        "base": "debian:trixie-slim",
        "dev": "php8.4-dev",
        "embed": "libphp8.4-embed",
        "curl": "php8.4-curl",
        "libphp": "/usr/lib/libphp8.4.so",
        "suites": True,
    },
    ("8.5", "musl"): {
        # `apk info -L php85-embed` in `alpine:3.23`: `/usr/lib/php85/libphp.so`.
        "base": "alpine:3.23",
        "dev": "php85-dev",
        "embed": "php85-embed",
        "curl": "php85-curl",
        "libphp": "/usr/lib/php85/libphp.so",
        "suites": True,
    },
    ("8.3", "musl"): {
        "base": "alpine:3.21",
        "dev": "php83-dev",
        "embed": "php83-embed",
        "curl": "php83-curl",
        "libphp": "/usr/lib/libphp83.so",
        # `MB.3.59`: the boot resolves 8.3 from Alpine's packed name, so the
        # worker-level suites run in the image that built the shim.
        "suites": True,
    },
    ("8.4", "musl"): {
        "base": "alpine:3.21",
        "dev": "php84-dev",
        "embed": "php84-embed",
        "curl": "php84-curl",
        "libphp": "/usr/lib/php84/libphp.so",
        # `MB.3.59`: the boot resolves 8.4 from the per-minor directory, so the
        # worker-level suites run in the image that built the shim.
        "suites": True,
    },
}

PHP_MINORS = {"8.6": "86", "8.5": "85", "8.4": "84", "8.3": "83"}

# The repository setup a leg can declare in `extra_repo`. The Dockerfile
# (`e2e/php-sapi/Dockerfile`) performs the named setup before installing the
# four packages, so a new leg's repository is one word here and no consumer
# grows a second list (`MB.42.5`):
#
#   sury                  Ondřej Surý's signed APT repository for Debian
#                         (the key at `packages.sury.org/php/apt.gpg`, the
#                         `bookworm main` suite)
#   alpine-edge-testing   `https://dl-cdn.alpinelinux.org/alpine/edge/testing`
#                         appended to `/etc/apk/repositories`
EXTRA_REPOS = ("", "sury", "alpine-edge-testing")

# A leg is `(arch, libc)`. `enabled` is the switch that turns a target into an
# artifact; a disabled leg must say why in one sentence.
LEGS = (
    {"id": "x86_64-gnu", "arch": "x86_64", "libc": "gnu", "enabled": True},
    {"id": "x86_64-musl", "arch": "x86_64", "libc": "musl", "enabled": True},
    {"id": "aarch64-gnu", "arch": "aarch64", "libc": "gnu", "enabled": True},
    {"id": "aarch64-musl", "arch": "aarch64", "libc": "musl", "enabled": True},
)

# Built **and** exercised. A key that is absent is declared and unbuilt: CI
# builds it on the release PR, and until it has run the artifact is a promise,
# not evidence. The value names the node that ran it, because "proven" without a
# who/when is the claim this file exists to keep honest.
PROVEN: dict[tuple, str] = {
    # `MB.3.12` built and probed 8.4 on x86_64-gnu; `MB.3.18` did the rest.
    ("shims", "8.6", "x86_64", "gnu"): "MB.42.5",
    ("shims", "8.6", "x86_64", "musl"): "MB.42.5",
    ("shims", "8.4", "x86_64", "gnu"): "MB.3.12",
    ("shims", "8.3", "x86_64", "gnu"): "MB.3.18",
    ("shims", "8.5", "x86_64", "gnu"): "MB.3.65",
    ("shims", "8.4", "x86_64", "musl"): "MB.3.18",
    ("shims", "8.3", "x86_64", "musl"): "MB.3.18",
    ("shims", "8.5", "x86_64", "musl"): "MB.3.65",
    # `MB.42.3.3` built the `py3-none-manylinux_2_28_x86_64` wheel, repacked it
    # from a `maturin build --bindings bin` wheel (4 PHP + 2 Python shims under
    # `blastit/_bin/`) and installed it in `python:3.12-slim`, `3.13-slim` and
    # `3.14-slim` — one artifact, three interpreters, `blastit --version`/`check`/
    # `fold` and `blastit-lsp --help` in each. The four other legs are built by
    # the release workflow on their native runners and stay unproven until it runs.
    ("wheels", "x86_64", "gnu"): "MB.42.3.3",
    # `MB.42.3.3` ran `make py-sapi PY_SAPI_REQUIRE_ALL=1` for both minors and
    # asserted the artifact contract (`nm -D` exports, `DT_SONAME`,
    # `UND Py_InitializeEx`, zero `NEEDED libpython`). The `musl` and `aarch64`
    # legs are built and asserted by `.github/workflows/py-sapi.yml` on their own
    # runners; until that run they are declared, unproven.
    ("py_shims", "3.13", "x86_64", "gnu"): "MB.42.3.3",
    ("py_shims", "3.14", "x86_64", "gnu"): "MB.42.3.3",
    ("archives", "x86_64", "gnu"): "MB.3.18",
    ("archives", "x86_64", "musl"): "MB.3.18",
    # `MB.42.3.1` built both archives from one manylinux+distro-shim bundle and
    # installed them in `debian:trixie` (WSGI leg) and `rockylinux:9` (static leg).
    ("packages", "x86_64", "deb"): "MB.42.3.1",
    ("packages", "x86_64", "rpm"): "MB.42.3.1",
    # `MB.35.5` built the `manylinux_2_28` archive, unpacked it and drove a real
    # `initialize → didOpen → shutdown → exit` session with `scripts/lsp_smoke.py`
    # (`0` stderr bytes). The four other legs are built by the first CI run on
    # their native runners; until then they are states, not proofs (§14.2).
    ("lsp", "x86_64-unknown-linux-gnu"): "MB.35.5",
}

# --- what each built shim leg actually reached (`MB.3.58` review) -------------
#
# `PROVEN` says an artifact was "built and exercised"; this says what *exercised*
# reached, because "6 built & exercised" reads as six working shims and four of
# them are not usable for `[route.php]` today. One row per built leg; an absent
# key is a leg that has not been built at all (declared, unbuilt), and the
# aarch64 legs are therefore absent.
#
#   built    the image produced `libblastit_php<minor>-<arch>-<libc>.so`; nothing
#            ran it
#   probed   `php-sapi-probe` served real requests through `libphp` + the shim
#            (the shim-level proof `PROVEN` records), but `blastit`'s own boot
#            did not serve with the distro's own package path
#   served   a `[route.php]` request went through `blastit`'s worker with the
#            distro's own package path — what the install docs tell a user to do
#   refused  a boot refused the pair; the sentence and its cause are measured
#
# `served` is the support statement: 6 of the 12 declared shims are served today
# (12 once the aarch64 legs run), so the docs may say "PHP 8.3/8.4/8.5 anywhere".
SHIM_REACHES: dict[tuple[str, str, str], tuple[str, str]] = {
    ("8.6", "gnu", "x86_64"): (
        "served",
        "measured by MB.42.5 in the `debian:bookworm-slim` image with Ondřej Surý's "
        "signed repository: with `BLASTIT_LIBPHP` unset the boot reaches "
        "`/usr/lib/libphp8.6.so` and serves `GET / → app 200`; `critical_php_backend` "
        "and `critical_php_embed` pass in the same image",
    ),
    ("8.6", "musl", "x86_64"): (
        "served",
        "measured by MB.42.10 in the `alpine:edge` image with `edge/testing` "
        "appended: with `BLASTIT_LIBPHP` unset the boot reaches "
        "`/usr/lib/php86/libphp.so` through the per-minor directory and serves "
        "`GET / → 200`; `critical_php_backend` 9/9, `critical_php_embed` 29/29 and "
        "`critical_file_sink` 8/8 pass in the same image (MB.42.5's `suites: False` "
        "was only the handoff's gnu-only `libc::execveat`, fixed by MB.42.10), and "
        "the unprivileged `PR_SET_DUMPABLE(0)` witness denies 2 roles",
    ),
    ("8.5", "gnu", "x86_64"): (
        "served",
        "measured by MB.3.65 in the `ubuntu:26.04` image: with `BLASTIT_LIBPHP` "
        "unset the boot reaches `/usr/lib/libphp8.5.so` and serves `GET / → app "
        "200`; `critical_php_backend` and `critical_php_embed` pass in the same image",
    ),
    ("8.4", "gnu", "x86_64"): (
        "served",
        "MB.3.12's protected suite, re-measured by MB.3.58 in the `debian:trixie` "
        "image: `GET / → app 200` through the worker (5 + 21 tests)",
    ),
    ("8.3", "gnu", "x86_64"): (
        "served",
        "MB.3.58 in the `ubuntu:24.04` image: `critical_php_backend` 5/5 and "
        "`critical_php_embed` 21/21, `served: HTTP/1.1 200 OK`",
    ),
    ("8.5", "musl", "x86_64"): (
        "served",
        "measured by MB.3.65 in the `alpine:3.23` image: with `BLASTIT_LIBPHP` "
        "unset the boot reaches `/usr/lib/php85/libphp.so` through the per-minor "
        "directory and serves `GET / → app 200`; both protected suites pass in the "
        "same image",
    ),
    ("8.4", "musl", "x86_64"): (
        "served",
        "measured by MB.3.59 in the `alpine:3.21` image: with `BLASTIT_LIBPHP` "
        "unset the boot reaches `/usr/lib/php84/libphp.so` through the per-minor "
        "directory and serves `GET / → app 200`; `critical_php_backend` and "
        "`critical_php_embed` pass in the same image",
    ),
    ("8.3", "musl", "x86_64"): (
        "served",
        "measured by MB.3.59 in the `alpine:3.21` image: with `BLASTIT_LIBPHP` "
        "unset the boot reaches `/usr/lib/libphp83.so` through the packed name "
        "and serves `GET / → app 200`; both protected suites pass in the same "
        "image",
    ),
}
REACHES = ("built", "probed", "served", "refused")

# The editor binaries (`MB.35`, `ADR-0010` §4, design §8.3a/§14). One leg per
# target, and the four fields a workflow and a consumer both need: `target` (the
# Rust triple), `runs_on` (the **native** runner), `archive` (the exact asset
# name) and `executable` (what the Zed extension relaunches). Everything else —
# the format, the Python a Windows leg spells `python`, the builder image — is
# derived below, so a leg is declared once.
#
# `runs_on` is never an emulator: an artifact that was never executed is a claim,
# not a proof (`MB.35.1`), and `macos-15-intel` is the only `cargo test`-grade
# evidence for the x86_64 Apple leg (cross-plus-Rosetta is not).
LSP_LEGS = (
    {
        "target": "x86_64-unknown-linux-gnu",
        "runs_on": "ubuntu-24.04",
        "platform": "linux",
        "enabled": True,
    },
    {
        "target": "aarch64-unknown-linux-gnu",
        "runs_on": "ubuntu-24.04-arm",
        "platform": "linux",
        "enabled": True,
    },
    {
        "target": "aarch64-apple-darwin",
        "runs_on": "macos-latest",
        "platform": "macos",
        "enabled": True,
    },
    {
        "target": "x86_64-apple-darwin",
        "runs_on": "macos-15-intel",
        "platform": "macos",
        "enabled": True,
    },
    {
        "target": "x86_64-pc-windows-msvc",
        "runs_on": "windows-latest",
        "platform": "windows",
        "enabled": True,
    },
)

# The archive format and the executable name per OS: `.tar.gz` on Unix, `.zip` on
# Windows (the extension's `Zip` extractor, §8.3a), and the `exe` suffix only
# where Windows needs it.
LSP_FORMATS = {"linux": "tar.gz", "macos": "tar.gz", "windows": "zip"}
LSP_EXECUTABLES = {
    "linux": "blastit-lsp",
    "macos": "blastit-lsp",
    "windows": "blastit-lsp.exe",
}

# Consumer images (`ADR-0010` §4.3), one tag per libc.
IMAGES = (
    {"tag": "ghcr.io/offworldnexus/blastit:php8.4", "minor": "8.4", "libc": "gnu"},
    {
        "tag": "ghcr.io/offworldnexus/blastit:php8.4-alpine",
        "minor": "8.4",
        "libc": "musl",
    },
)

# --- derived views ----------------------------------------------------------


def enabled_legs() -> list[dict]:
    return [leg for leg in LEGS if leg["enabled"]]


def wheel_legs() -> list[dict]:
    """One `py3-none` wheel per enabled (arch, libc) (design §6.1, `MB.42.3.3`).

    A wheel carries **no extension**: `maturin build --bindings bin` puts the
    universal binary in it and `scripts/repack_universal_wheel.py` retags it
    `py3-none-<platform>`, so four artifacts serve CPython 3.12, 3.13 and 3.14.
    The keys are the ones the two consumers actually read: `wheel_tag` is the
    platform half of the file name (`scripts/check_wheel_tag.py`),
    `smoke_pythons`/`runtime_images` are the interpreters one wheel is installed
    into (`e2e/prebuilt_wheel.py` takes the default from `E2E_PYTHON_VERSION`).
    """
    legs = []
    for leg in enabled_legs():
        libc = LIBCS[leg["libc"]]
        tag = f"{libc['wheel_tag']}_{leg['arch']}"
        legs.append(
            {
                "name": leg["id"],
                "arch": leg["arch"],
                "libc": leg["libc"],
                # PEP 425's two non-platform halves, in one field: `pip` accepts
                # this wheel on any CPython 3, and it is how the filename reads.
                "python_tag": "py3-none",
                "wheel_tag": tag,
                "wheel": f"blastit-<version>-py3-none-{tag}.whl",
                "smoke_pythons": ",".join(SMOKE_PYTHONS),
                "runtime_images": ",".join(
                    f"python:{minor}-{'alpine' if leg['libc'] == 'musl' else 'slim'}"
                    for minor in SMOKE_PYTHONS
                ),
                "runs_on": ARCHES[leg["arch"]]["runs_on"],
                "manylinux": libc.get("manylinux", ""),
                "musllinux": libc.get("musllinux", ""),
                # The glibc floor every embedded ELF must satisfy
                # (`scripts/check-portable-elf.sh --max-glibc`, `MB.42.3.1`'s gate).
                "glibc_floor": libc["glibc_floor"],
            }
        )
    return legs


def binary_legs() -> list[dict]:
    """One archive per enabled (arch, libc)."""
    legs = []
    for leg in enabled_legs():
        libc = LIBCS[leg["libc"]]
        legs.append(
            {
                "name": leg["id"],
                "arch": leg["arch"],
                "libc": leg["libc"],
                "target": f"{leg['arch']}-unknown-linux-{leg['libc']}",
                "archive": f"blastit-{leg['arch']}-unknown-linux-{leg['libc']}.tar.gz",
                # Where the archive is built, and the first image it is executed
                # in: the container that compiled it is the only one that may
                # run it on faith (`MB.3.60`).
                "builder_image": libc["builder_image"].format(arch=leg["arch"]),
                # `<major>_<minor>` for glibc, empty for musl (no floor exists).
                # `build-standalone.sh --max-glibc` refuses a binary above it.
                "glibc_floor": libc["glibc_floor"],
                # Every image the archive is executed in, oldest first.
                "smoke_images": libc["smoke_images"].format(arch=leg["arch"]),
                "runs_on": ARCHES[leg["arch"]]["runs_on"],
            }
        )
    return legs


def package_legs() -> list[dict]:
    """One `.deb` and one `.rpm` per enabled gnu arch (`MB.42.3.1`).

    A package is built from **one** §6.1 bundle: `blastit`, `blastit-lsp` and the
    `gnu` Python shims come from the manylinux builder (`glibc_floor`, `2_28`), and
    the four PHP shims come from their distro legs (`shim_floor`, `2_41`). Both
    numbers are here because they are different, and the packaging scripts must be
    handed the *bundle's* ceiling — `--max-glibc 2_41` — not the binaries' floor.
    """
    return [
        {
            "name": f"{leg['arch']}-{fmt}",
            "arch": leg["arch"],
            "format": fmt,
            "libc": "gnu",
            "package_arch": spec["arch"][leg["arch"]],
            "pattern": spec["pattern"],
            "image": spec["image"],
            "builder_image": LIBCS["gnu"]["builder_image"].format(arch=leg["arch"]),
            # `<major>_<minor>`: what `build-{deb,rpm}.sh --max-glibc` is given.
            "bundle_ceiling": LIBCS["gnu"]["shim_floor"],
            "binary_floor": LIBCS["gnu"]["glibc_floor"],
            "runs_on": ARCHES[leg["arch"]]["runs_on"],
        }
        for leg in enabled_legs()
        if leg["libc"] == "gnu"
        for fmt, spec in PACKAGE_FORMATS.items()
    ]


def install_legs() -> list[dict]:
    """One container install leg per (distribution, architecture).

    `INSTALL_LEGS` declares the six distributions once; this expands each over
    the two architectures the release ships, because a `.deb`/`.rpm` for
    `aarch64` that was never installed is a promise, not a package — and the
    `aarch64` legs run on the native `ubuntu-24.04-arm` runner rather than under
    emulation. `scripts/install_matrix.sh --leg <name>` reads one row through
    `--leg install --name <name>`, and both package workflows iterate the whole
    table as a GitHub matrix, so "which distributions does this release support"
    is answered here and executed before anything is published.
    """
    return [
        {
            "name": f"{leg['name']}-{arch}",
            "distro": leg["distro"],
            "release": leg["release"],
            "image": leg["image"],
            "format": leg["format"],
            "installer": leg["installer"],
            "modes": leg["modes"],
            "arch": arch,
            # The filename half `build-{deb,rpm}.sh` writes for this arch, so a
            # leg picks its package by the table rather than by a glob.
            "package_arch": PACKAGE_FORMATS[leg["format"]]["arch"][arch],
            "runs_on": ARCHES[arch]["runs_on"],
        }
        for leg in INSTALL_LEGS
        for arch in ARCHES
    ]


def shim_floor() -> str:
    """The declared ceiling for the distro-built PHP shims, `<major>_<minor>`.

    `scripts/build-{deb,rpm}.sh --max-glibc` and the `packages` Make target take it
    from here so the number is stated once, beside the matrix legs that produce it.
    """
    return LIBCS["gnu"]["shim_floor"]


def shim_legs() -> list[dict]:
    """One shim per (minor, arch, libc), named `ADR-0010` §4.1's way."""
    return [
        {
            "name": f"{leg['id']}-php{digits}",
            "arch": leg["arch"],
            "libc": leg["libc"],
            "minor": minor,
            "digits": digits,
            "shim": shim_name(minor, leg["arch"], leg["libc"]),
            "base": PHP_BUILDS[(minor, leg["libc"])]["base"],
            "dev": PHP_BUILDS[(minor, leg["libc"])]["dev"],
            "embed": PHP_BUILDS[(minor, leg["libc"])]["embed"],
            "curl": PHP_BUILDS[(minor, leg["libc"])]["curl"],
            "libphp": PHP_BUILDS[(minor, leg["libc"])]["libphp"],
            # The repository this leg needs before its packages exist: named,
            # never performed here. `""` is a stock image's own repositories.
            "extra_repo": PHP_BUILDS[(minor, leg["libc"])].get("extra_repo", ""),
            # A JSON string, not a bool: GitHub compares `matrix.suites` to
            # `'true'`, and the workflow auditor stringifies every matrix value,
            # so one spelling has to work for both.
            "suites": "true" if PHP_BUILDS[(minor, leg["libc"])]["suites"] else "false",
            # `built|probed|served|refused`, empty when this leg has never been
            # built — the support statement a reader needs before choosing a
            # minor (`MB.3.58` review).
            "reaches": SHIM_REACHES.get((minor, leg["libc"], leg["arch"]), ("",))[0],
            "runs_on": ARCHES[leg["arch"]]["runs_on"],
        }
        for leg in enabled_legs()
        for minor, digits in PHP_MINORS.items()
    ]


def shim_name(minor: str, arch: str, libc: str) -> str:
    """`libblastit_php84-x86_64-gnu.so` (`ADR-0010` §4.1)."""
    return f"libblastit_php{PHP_MINORS[minor]}-{arch}-{libc}.so"


# The image a CPython shim of each libc is built in. gnu builds on the runner
# itself (its `uv python` ships the shared `libpython` the `dlopen` probe needs);
# musl must build in Alpine, because no `--target` makes a glibc linker emit a
# musl binary — and the resulting `NEEDED libc.musl-<arch>.so.1` is asserted.
PY_SHIM_BUILDER_IMAGES = {"gnu": "", "musl": "alpine:3.21"}


def py_shim_legs() -> list[dict]:
    """One CPython shim per (minor, arch, libc) — `8` legs (`MB.42.2`, §1).

    The shim is `libblastit_py3<minor>-<arch>-<libc>.so`: a `cdylib` that exports
    the frozen ABI (`blastit_py_abi_version`, `blastit_py_minor`,
    `blastit_py_abi_v1`), carries `DT_SONAME` equal to its file name, leaves
    `Py_InitializeEx` **undefined** (resolved by the host interpreter at
    `dlopen`) and links zero `libpython`. Those four properties are what
    `.github/workflows/py-sapi.yml` asserts on the built artifact, and they are
    named here so the workflow, the `.deb`/`.rpm` bundle check and this matrix
    cannot disagree about them.
    """
    return [
        {
            "name": f"{leg['id']}-py{digits}",
            "arch": leg["arch"],
            "libc": leg["libc"],
            "minor": minor,
            "digits": digits,
            "shim": py_shim_name(minor, leg["arch"], leg["libc"]),
            # The exports `nm -D --defined-only` must carry, and the one symbol
            # that must stay undefined (the host interpreter's).
            "exports": "blastit_py_abi_version,blastit_py_minor,blastit_py_abi_v1",
            "undefined": "Py_InitializeEx",
            # `make py-sapi` needs an interpreter of exactly this minor: it
            # validates the pair and finds that minor's shared libpython for the
            # `dlopen` probe (a shim never links it).
            "python": minor,
            # Where this shim can actually be **linked** for its libc
            # (`MB.42.3.3` review): a musl shim built on a glibc runner was
            # linked against `libc.so.6` and every assertion still passed, so the
            # leg names the image its `NEEDED` line is a property of.
            "builder_image": PY_SHIM_BUILDER_IMAGES[leg["libc"]],
            "runs_on": ARCHES[leg["arch"]]["runs_on"],
        }
        for leg in enabled_legs()
        for minor, digits in PY_MINORS.items()
    ]


def py_shim_name(minor: str, arch: str, libc: str) -> str:
    """`libblastit_py313-x86_64-gnu.so` (design §1, `MB.42.2`)."""
    return f"libblastit_py{PY_MINORS[minor]}-{arch}-{libc}.so"


def lsp_legs() -> list[dict]:
    """One editor archive per enabled target (`ADR-0010` §4, design §14.1 R74).

    The archive name is the contract §8.3a states and `MB.33`'s resolution reads:
    `blastit-lsp-<target>.tar.gz` on Linux and macOS, `.zip` on Windows, one
    top-level `blastit-lsp-<target>/` directory holding the executable, `LICENSE`
    and an in-archive `SHA256SUMS`.

    Every field a producer or a consumer needs is **derived here**: `archive`,
    `dir`, `executable`, `ext`, the Python a leg spells out loud, the builder
    image and floor, and `ci_command` — the exact command line that proves the
    leg. `scripts/build-lsp.sh` and both workflows take the names from these
    fields, so a leg added to `LSP_LEGS` reaches the build, the unpack, the
    smoke and the release without a second edit, and no other file has to know
    the five target triples.

    A Linux leg carries the builder image whose glibc *is* its floor and the
    floor itself, because those are the two numbers the shipping workflow must
    name and the guard refuses a step that does not (`MB.3.60`'s class). The
    three other legs build on the runner's own toolchain and claim no glibc.
    """
    legs = []
    for leg in LSP_LEGS:
        if not leg["enabled"]:
            continue
        target = leg["target"]
        arch = target.split("-")[0]
        platform = leg["platform"]
        linux = platform == "linux"
        ext = LSP_FORMATS[platform]
        executable = LSP_EXECUTABLES[platform]
        directory = f"blastit-lsp-{target}"
        archive = f"{directory}.{ext}"
        builder_image = LIBCS["gnu"]["builder_image"].format(arch=arch) if linux else ""
        glibc_floor = LIBCS["gnu"]["glibc_floor"] if linux else ""
        # The Python a leg spells out loud: `python3` on Unix, `python` on
        # Windows. It runs `scripts/lsp_smoke.py`, which is stdlib only — the
        # artifact itself needs no interpreter at all.
        python = "python" if platform == "windows" else "python3"
        # The SHA-256 verifier the leg's OS has: macOS ships `shasum` (Perl) and
        # no coreutils, every other runner has `sha256sum`. `shasum -c` defaults
        # to SHA-1, so `-a 256` is part of the command, not a detail.
        sha_verify = (
            "shasum -a 256 -c SHA256SUMS"
            if platform == "macos"
            else "sha256sum -c SHA256SUMS"
        )
        # The exact command line the leg's CI job runs, printed by `--summary`
        # so a "NOT RUN" row carries the command that would run it rather than a
        # promise that one exists. Built from this leg's own fields, so it cannot
        # drift from them.
        build = [
            "bash",
            "scripts/build-lsp.sh",
            "--target",
            target,
        ]
        if linux:
            build += ["--image", builder_image, "--max-glibc", glibc_floor]
        build += [
            "--archive",
            archive,
            "--executable",
            executable,
            "--dir",
            directory,
            "--out",
            "dist",
        ]
        unpack = (
            f"{python} -m zipfile -e dist/{archive} dist/smoke-{target}"
            if ext == "zip"
            else f"tar -xzf dist/{archive} -C dist/smoke-{target}"
        )
        legs.append(
            {
                "name": target,
                "target": target,
                "arch": arch,
                "runs_on": leg["runs_on"],
                "platform": platform,
                "archive": archive,
                "dir": directory,
                "ext": ext,
                "executable": executable,
                "python": python,
                "sha_verify": sha_verify,
                "builder_image": builder_image,
                "glibc_floor": glibc_floor,
                "ci_command": (
                    " ".join(build)
                    + f" && {unpack}"
                    + f" && ( cd dist/smoke-{target}/{directory} && {sha_verify} )"
                    + f" && {python} scripts/lsp_smoke.py --binary"
                    + f" dist/smoke-{target}/{directory}/{executable}"
                ),
            }
        )
    return legs


def proved(family: str) -> list[tuple]:
    return sorted(key for key in PROVEN if key[0] == family)


def proven_count(family: str) -> int:
    return len(proved(family))


def reached(reaches: str) -> list[dict]:
    """The shim legs that reached `reaches` (the support statement)."""
    return [leg for leg in shim_legs() if leg["reaches"] == reaches]


def reaches_counts() -> dict[str, int]:
    return {name: len(reached(name)) for name in REACHES} | {
        "unbuilt": len(shim_legs()) - len(reached_any())
    }


def reached_any() -> list[dict]:
    return [leg for leg in shim_legs() if leg["reaches"]]


def reaches_sentence() -> str:
    """The one sentence `--check`, `--summary` and the release notes print.

    `MB.3.58` review: the gate and the summary disagreed in wording, so a number a
    reader quoted could be one no gate printed. One function, one string — the
    counts and the arm-runner projection included.
    """
    counts = reaches_counts()
    total = len(shim_legs())
    prospective = counts["served"] + len(
        [
            leg
            for leg in shim_legs()
            # Unbuilt, but its (minor, libc) is served on x86_64: the arm runners
            # will add it, so the sentence says what the number becomes.
            if not leg["reaches"] and PHP_BUILDS[(leg["minor"], leg["libc"])]["suites"]
        ]
    )
    sentence = (
        f"served {counts['served']} of {total} shims, "
        f"{counts['probed']} probed, {counts['refused']} refused, "
        f"{counts['unbuilt']} unbuilt"
    )
    if prospective > counts["served"]:
        sentence += f"; {prospective} served once the aarch64 legs run"
    return sentence


# --- the check --------------------------------------------------------------


# Every page that states a count. `docs/` is scanned whole — hand-picking six
# pages is how the gate went vacuous the first time (the ADR and the design doc
# are the two documents a reviewer reads, and both were outside the list) — plus
# `README.md`, which is the wheel's long description.
#
# The two internal records get a `note` line either way, so `--check` says which
# document states what instead of leaving it to be discovered; the published
# pages are checked the same way.
def _pages() -> list[tuple[str, Path]]:
    """Every Markdown page the repository ships, plus the README.

    All of them, not a hand-picked few: the first version of this check looked at
    six pages and missed the two documents a reviewer reads (`ADR-0010` §4 and
    `backend-runtimes.md` §4.7, both of whose counts are backticked — see `MARK`),
    which made the gate vacuous exactly where it mattered. A page added under
    `docs/` is scanned the day it lands.
    """
    pages = [("README.md", REPO / "README.md")]
    for path in sorted((REPO / "docs").rglob("*.md")):
        pages.append((path.relative_to(REPO).as_posix(), path))
    return pages


INTERNAL_DOCS = frozenset(
    {
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "docs/design/backend-runtimes.md",
    }
)

# The site's exclusion list, read from the site's own configuration. A gate that
# scans a document nobody can reach is a gate that fails on working material: a
# design *review* quoting "9 archives" red `make check` once (`MB.35.5`, the
# untracked `portable-lsp-arch.review-implementability.md`), because `--check`
# walked `docs/design/`. `zensical.toml` already says which directories are not
# pages, so that one list decides here too — two lists is how they disagree. The
# fallback is the list as written when this was added, for a fixture whose copy
# of the site config is missing.
SITE_EXCLUSIONS_FALLBACK: tuple[str, ...] = (
    "/archive/",
    "/spikes/",
    "/VISION.md",
    "/design/",
    "/decisions/",
    "/research/",
    "/knowledge/",
    "/testing/",
)

# A review of a document is never a promise: it quotes the numbers it is arguing
# about, including the ones it says are wrong. Skipped wherever it lives, so
# moving one out of `docs/design/` cannot re-arm it.
REVIEW_FILE = re.compile(r"\.review[-.]")


def site_exclusions(repo: Path = REPO) -> tuple[str, ...]:
    """`zensical.toml`'s `exclude_docs` lines (GitHub-style docs ignore patterns)."""
    config = repo / "zensical.toml"
    try:
        with config.open("rb") as handle:
            document = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return SITE_EXCLUSIONS_FALLBACK
    raw = (document.get("project") or {}).get("exclude_docs") or ""
    patterns = tuple(
        line.strip()
        for line in str(raw).splitlines()
        if line.strip() and not line.strip().startswith("#")
    )
    return patterns or SITE_EXCLUSIONS_FALLBACK


def is_page(name: str, patterns: tuple[str, ...]) -> bool:
    """Whether a repo-relative Markdown path is a document a reader can reach.

    `README.md` is a page (it is the wheel's long description). The two
    `INTERNAL_DOCS` are checked even though the site excludes them: they are the
    records the pins below exist for, and they are named, not discovered.
    """
    if name in INTERNAL_DOCS:
        return True
    if REVIEW_FILE.search(name):
        return False
    if not name.startswith("docs/"):
        return True
    site_path = name[len("docs") :]
    for pattern in patterns:
        if pattern.endswith("/"):
            if site_path.startswith(pattern):
                return False
        elif site_path == pattern:
            return False
    return True


# `N shims`, "`8` wheels", "**4** proven PHP shims": every live count in the ADR
# and the design doc is backticked, so the digit is never bare and a pattern that
# insists on `\d+` matches nothing. `MARK` is the wrapper the Markdown actually
# uses (backticks, bold, italics), and the qualifier allows the words that sit
# between the number and the noun ("native PHP", "standalone", "proven").
MARK = r"[`*_]*"
# The words that sit between a number and its noun, plus one inline code span:
# `ADR-0009` says "up to `3` proven `libblastit_php{82,83,84}-…so` shims", where
# the artifact name separates the count from the noun. `universal` is there for
# the `py3-none` family's spelling ("`4` universal wheels").
QUALIFIER = r"(?:proven\s+|PHP\s+|native\s+|standalone\s+|universal\s+|`[^`\n]*`\s+)*"
# `python shims?` comes first: the per-minor CPython family is a different count
# from the PHP shims it is always written next to (`MB.42.3.3`), and a regex that
# read "8 Python shims" as the PHP family would compare it to the wrong number.
CLAIM = re.compile(
    r"(?<![A-Za-z0-9._-])"
    + MARK
    + r"(?P<count>\d+)"
    + MARK
    + r"\s+"
    + QUALIFIER
    + r"(?P<family>python\s+shims?|shims?|wheels?|archives?|tarballs?|lsp assets?)",
    re.IGNORECASE,
)
PROVEN_CLAIM = re.compile(
    r"(?<![A-Za-z0-9._-])"
    + MARK
    + r"(?P<count>\d+)"
    + MARK
    + r"\s+proven\s+"
    + QUALIFIER
    + r"(?P<family>python\s+shims?|shims?|wheels?|archives?|tarballs?|lsp assets?)",
    re.IGNORECASE,
)

# A count a page states **above** the matrix, with the node that lowers it.
#
# Empty since `MB.42.4`: `MB.42.3.3` replaced eight `cp3NN` wheels with four
# `py3-none` ones, and the two records that stated the old number (`ADR-0010` §4
# and `backend-runtimes.md` §4.7) are now rewritten to the `4`-wheel matrix. The
# mechanism stays because the next matrix change needs it — a page that over-claims
# is recorded here with the node that fixes it, and an entry is checked in **both**
# directions: if the page stops making the claim, `--check` refuses the entry, so
# the list cannot outlive its reason.
STALE_CLAIMS: dict[tuple[str, str], str] = {}

# The lines the gate must keep matching, quoted from the two records it exists to
# guard. If a Markdown edit (or a regex change) makes the scanner miss them,
# `--check` says so instead of quietly checking nothing — a gate that matches no
# document is worse than no gate, because it reports `ok`.
PINNED: tuple[tuple[str, str, str], ...] = (
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`4` wheels",
        "declared wheel count",
    ),
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`4` tarballs",
        "declared archive count",
    ),
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`12` shims",
        "declared shim count",
    ),
    # `MB.35.5` / `R74`: the editor family's one numeric claim. Without this pin a
    # regex edit could stop seeing it and the `lsp` count would be checked against
    # nothing, which is the vacuous-gate failure the module's header names.
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`5` lsp assets",
        "declared lsp count",
    ),
    (
        "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md",
        "`3` shims",
        "per-tuple shim count (8.3/8.4/8.5)",
    ),
    (
        "docs/design/backend-runtimes.md",
        "`4` wheels",
        "declared wheel count",
    ),
    (
        "docs/design/backend-runtimes.md",
        "`16` shims",
        "declared shim count",
    ),
    (
        "docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md",
        "`3` proven",
        "ADR-0009's per-target proven shim count",
    ),
)


def declared_of(family: str) -> int:
    """How many artifacts of this family the matrix declares."""
    family = family.lower().rstrip("s")
    return {
        "shim": len(shim_legs()),
        "wheel": len(wheel_legs()),
        "archive": len(binary_legs()),
        "tarball": len(binary_legs()),
        # `family.lower().rstrip("s")` of the page noun `lsp assets` (`R74`): the
        # family key is `lsp asset`, or `--count lsp` would `KeyError`.
        "lsp asset": len(lsp_legs()),
        # `.deb` + `.rpm` per gnu arch (`MB.42.3.1`).
        "package": len(package_legs()),
        # The container install matrix (`MB.42.3.4`): one leg per
        # (distribution, architecture) the release claims.
        "install": len(install_legs()),
        # The per-minor CPython shims (`MB.42.3.3`). Spelled as its own noun so a
        # page saying "`8` Python shims" is checked against this family and not
        # against the PHP one it sits beside.
        "python shim": len(py_shim_legs()),
        # `images` is not a count any page states yet; 0 refuses the claim.
        "image": 0,
    }[family]


def proven_of(family: str) -> int:
    family = family.lower().rstrip("s")
    return {
        "shim": proven_count("shims"),
        "wheel": proven_count("wheels"),
        "archive": proven_count("archives"),
        "tarball": proven_count("archives"),
        "lsp asset": proven_count("lsp"),
        "package": proven_count("packages"),
        "python shim": proven_count("py_shims"),
        # No install leg is "proved" by `PROVEN`: a container install runs in CI
        # on every release, so there is no artifact to keep a proof of. A page may
        # therefore not claim a proven install.
        "install": 0,
        "image": 0,
    }[family]


def check(problems: list[str], notes: list[str]) -> None:
    ids = [leg["id"] for leg in LEGS]
    if len(set(ids)) != len(ids):
        problems.append(f"duplicate leg id in LEGS: {ids}")

    for leg in LEGS:
        if leg["arch"] not in ARCHES:
            problems.append(f"{leg['id']}: unknown arch {leg['arch']!r}")
        if leg["libc"] not in LIBCS:
            problems.append(f"{leg['id']}: unknown libc {leg['libc']!r}")
        if not leg["enabled"] and not leg.get("dropped_because"):
            problems.append(
                f"{leg['id']}: disabled with no reason: the matrix records why a "
                "leg is not attempted, or `--summary` cannot be honest"
            )
        if leg["enabled"] and leg.get("dropped_because"):
            problems.append(f"{leg['id']}: enabled and dropped at the same time")

    # Every (arch, libc) pair is declared, enabled or dropped: a target that
    # silently vanishes from LEGS is the failure this file exists to prevent.
    declared = {(leg["arch"], leg["libc"]) for leg in LEGS}
    for arch in ARCHES:
        for libc in LIBCS:
            if (arch, libc) not in declared:
                problems.append(
                    f"{arch}-{libc} is not declared: declare it enabled or dropped "
                    "with a reason, never leave it out"
                )

    for (minor, libc), spec in PHP_BUILDS.items():
        if minor not in PHP_MINORS:
            problems.append(f"PHP_BUILDS names an unknown minor {minor!r}")
        for key in ("base", "dev", "embed", "curl", "libphp"):
            if not spec.get(key):
                problems.append(
                    f"PHP {minor} on {libc}: no {key}: the leg cannot be built"
                )
        # `MB.42.5`: the repository a leg needs is a matrix fact with a fixed
        # vocabulary, so the Dockerfile's switch and this table cannot disagree
        # about how a leg's packages come to exist.
        if spec.get("extra_repo", "") not in EXTRA_REPOS:
            problems.append(
                f"PHP {minor} on {libc}: extra_repo {spec.get('extra_repo')!r} is not one "
                f"of {EXTRA_REPOS}: the Dockerfile would install no repository"
            )
        # `MB.3.58`: whether the worker-level suites run in this leg's image is a
        # matrix fact, so a leg that cannot carry them says so where the leg is
        # declared. A silent `False` and a `True` that carries a reason are both
        # refusals: the reason is either needed or it is stale.
        if not isinstance(spec.get("suites"), bool):
            problems.append(
                f"PHP {minor} on {libc}: `suites` must be True or False, not "
                f"{spec.get('suites')!r}: the workflow picks a matrix value, not a step condition"
            )
        if not spec.get("suites") and not spec.get("suites_because"):
            problems.append(
                f"PHP {minor} on {libc}: the protected suites do not run here and no "
                "`suites_because` says why: a dropped proof keeps its reason, or it is "
                "read as a pass"
            )
        if spec.get("suites") and spec.get("suites_because"):
            problems.append(
                f"PHP {minor} on {libc}: `suites` is True and `suites_because` says why "
                "it is not: the reason is stale"
            )
    for minor in PHP_MINORS:
        for leg in LEGS:
            if (minor, leg["libc"]) not in PHP_BUILDS:
                problems.append(
                    f"PHP {minor} on {leg['libc']}: no build record, so {leg['id']} "
                    "cannot produce that shim"
                )

    for leg in shim_legs():
        expected = shim_name(leg["minor"], leg["arch"], leg["libc"])
        if leg["shim"] != expected:
            problems.append(f"{leg['name']}: shim {leg['shim']} != {expected}")
        if not re.fullmatch(
            r"libblastit_php8\d-(x86_64|aarch64)-(gnu|musl)\.so", leg["shim"]
        ):
            problems.append(f"{leg['name']}: {leg['shim']} is not the §4.1 name")

    # The per-minor CPython shims (`MB.42.3.3`): one name per (minor, arch, libc),
    # and the name is the `DT_SONAME` the workflow asserts — an artifact whose
    # soname and file name disagree cannot be found by the loader the docs name.
    py_shims = [leg["shim"] for leg in py_shim_legs()]
    if len(set(py_shims)) != len(py_shims):
        problems.append(f"py_shim_legs(): duplicate shim name in {py_shims}")
    for leg in py_shim_legs():
        expected = py_shim_name(leg["minor"], leg["arch"], leg["libc"])
        if leg["shim"] != expected:
            problems.append(f"{leg['name']}: shim {leg['shim']} != {expected}")
        if not re.fullmatch(
            r"libblastit_py3(13|14)-(x86_64|aarch64)-(gnu|musl)\.so", leg["shim"]
        ):
            problems.append(f"{leg['name']}: {leg['shim']} is not the §1 name")
        # A musl shim cannot be linked on a glibc runner (`MB.42.3.3` review):
        # without its image the artifact is named musl and needs libc.so.6.
        if leg["libc"] == "musl" and not leg["builder_image"]:
            problems.append(
                f"{leg['name']}: a musl shim names no builder_image, so it would be "
                "linked on a glibc runner and only *named* musl"
            )
        if leg["libc"] == "gnu" and leg["builder_image"]:
            problems.append(
                f"{leg['name']}: a gnu shim is built on the runner (its own "
                "libpython is the probe's); naming a builder_image is stale"
            )

    # The editor family (`MB.35.5`, `R74`): five legs, one asset name each, and a
    # format that is Windows' `.zip` and Unix's `.tar.gz` — the two the
    # extension's extractor and §8.3a's table state. A dropped leg keeps its
    # reason, exactly like the serving legs.
    seen_targets: list[str] = []
    for leg in LSP_LEGS:
        target = leg.get("target", "")
        where = f"LSP_LEGS {target or '<no target>'}"
        if not re.fullmatch(
            r"(?:x86_64|aarch64)-(?:unknown-linux-gnu|apple-darwin|pc-windows-msvc)",
            target,
        ):
            problems.append(f"{where}: not one of the five targets §8.3a names")
        if target in seen_targets:
            problems.append(f"{where}: declared twice: a target is one leg")
        seen_targets.append(target)
        platform = leg.get("platform", "")
        if platform not in LSP_FORMATS:
            problems.append(f"{where}: unknown platform {platform!r}")
            continue
        if not leg.get("runs_on"):
            problems.append(
                f"{where}: no native runner: an unpackaged leg proves nothing"
            )
        if not leg.get("enabled") and not leg.get("dropped_because"):
            problems.append(
                f"{where}: disabled with no reason: `--summary` cannot say which "
                "targets have no language server without it"
            )
        if leg.get("enabled") and leg.get("dropped_because"):
            problems.append(f"{where}: enabled and dropped at the same time")
    for leg in lsp_legs():
        if (
            leg["archive"]
            != f"blastit-lsp-{leg['target']}.{LSP_FORMATS[leg['platform']]}"
        ):
            problems.append(
                f"{leg['name']}: {leg['archive']} is not the §8.3a asset name"
            )
        if leg["executable"] != LSP_EXECUTABLES[leg["platform"]]:
            problems.append(
                f"{leg['name']}: {leg['executable']} is not the executable the "
                "extension relaunches"
            )
        if leg["platform"] == "linux":
            if not leg["builder_image"] or not leg["glibc_floor"]:
                problems.append(
                    f"{leg['name']}: a Linux leg names no builder_image/glibc_floor, "
                    "so nothing builds it inside the glibc it claims"
                )
        elif leg["builder_image"] or leg["glibc_floor"]:
            problems.append(
                f"{leg['name']}: a non-Linux leg claims a glibc floor: only the "
                "Linux assets promise one"
            )

    # `SHIM_REACHES` is the support statement (`MB.3.58` review): a row must name
    # a leg this matrix builds, say what it reached, and say how that was
    # measured. A `served` row without a CI suite would be a claim no leg
    # re-proves, and a leg with suites must be `served` — otherwise CI runs the
    # suites for a leg whose own row says the boot refuses.
    for (minor, libc, arch), (value, because) in SHIM_REACHES.items():
        where = f"SHIM_REACHES {minor}/{libc}/{arch}"
        if minor not in PHP_MINORS:
            problems.append(f"{where}: unknown PHP minor")
        if libc not in LIBCS:
            problems.append(f"{where}: unknown libc")
        if arch not in ARCHES:
            problems.append(f"{where}: unknown arch")
        if value not in REACHES:
            problems.append(f"{where}: {value!r} is not one of {REACHES}")
        if not because:
            problems.append(f"{where}: no reason recorded for {value!r}")
        if value == "served" and not PHP_BUILDS.get((minor, libc), {}).get("suites"):
            problems.append(
                f"{where}: claims `served` for a leg whose `suites` is false, so no CI "
                "leg re-proves it; run the suites there or record `probed`"
            )
    for leg in shim_legs():
        spec = PHP_BUILDS[(leg["minor"], leg["libc"])]
        if spec["suites"] and leg["arch"] == "x86_64" and leg["reaches"] != "served":
            problems.append(
                f"{leg['name']}: `suites` is true, so CI runs the worker-level suites "
                f"here, but `SHIM_REACHES` says {leg['reaches'] or 'nothing'}: the two "
                "cannot both be right"
            )

    # Every declared artifact has a place in PROVEN or it is a promise; every
    # PROVEN key names an artifact the matrix actually builds.
    declared_keys = (
        {("shims", leg["minor"], leg["arch"], leg["libc"]) for leg in shim_legs()}
        | {
            ("py_shims", leg["minor"], leg["arch"], leg["libc"])
            for leg in py_shim_legs()
        }
        | {("wheels", leg["arch"], leg["libc"]) for leg in wheel_legs()}
        | {("archives", leg["arch"], leg["libc"]) for leg in binary_legs()}
        | {("lsp", leg["target"]) for leg in lsp_legs()}
        | {("packages", leg["arch"], leg["format"]) for leg in package_legs()}
    )
    for key in PROVEN:
        if key not in declared_keys:
            problems.append(
                f"PROVEN names {key}, which this matrix does not build: a proof of "
                "something no artifact exists for is not evidence"
            )

    for image in IMAGES:
        if not image.get("tag"):
            problems.append("an image entry has no tag")

    # The container install matrix (`MB.42.3.4`): every distribution the release
    # claims is installed before a package is published, so a leg must name a
    # package this matrix builds and the package manager that distribution has.
    seen_install: list[str] = []
    built_formats = {leg["format"] for leg in package_legs()}
    for leg in install_legs():
        where = f"INSTALL_LEGS {leg['name']}"
        if leg["name"] in seen_install:
            problems.append(
                f"{where}: declared twice: one (distribution, arch) is one leg"
            )
        seen_install.append(leg["name"])
        if leg["format"] not in built_formats:
            problems.append(
                f"{where}: installs a {leg['format']!r} package, which this matrix does "
                f"not build ({sorted(built_formats)}): a leg nothing produces never runs"
            )
        expected_installer = {"deb": "apt", "rpm": "dnf"}.get(leg["format"], "")
        if leg["installer"] != expected_installer:
            problems.append(
                f"{where}: a {leg['format']} package is installed with {leg['installer']!r}, "
                f"not {expected_installer!r}: the image would have no such command"
            )
        modes = leg["modes"].split()
        for mode in modes:
            if mode not in ("install", "static", "wsgi"):
                problems.append(
                    f"{where}: unknown smoke mode {mode!r} (install, static, wsgi)"
                )
        if "install" not in modes:
            problems.append(
                f"{where}: never installs the package: the leg proves nothing"
            )
        if "wsgi" in modes and leg["image"] != "debian:trixie":
            problems.append(
                f"{where}: runs the WSGI gate in {leg['image']}, which ships no CPython "
                "3.13/3.14 (the `debian:trixie` leg is the one image that does), so the "
                "leg would import through an interpreter the packaged shim never binds to"
            )

    # The pinned lines first: if the scanner no longer sees them, every count
    # below is being read from a document it does not understand, and an `ok`
    # would be a lie.
    texts: dict[str, str] = {}
    for name, target in _pages():
        if not target.exists():
            problems.append(f"{name}: a page named here is missing")
            continue
        texts[name] = target.read_text(encoding="utf-8")
    for name, needle, what in PINNED:
        text = texts.get(name)
        if text is None:
            # A pin whose document moved is a pin that checks nothing.
            problems.append(
                f"PINNED names {name}, which this repository does not have: a count "
                "gate that pins a missing document is not a gate"
            )
            continue

        # Markdown wrappers are compared away on both sides: the regex may consume
        # the opening backtick or the `@` before it, and a pin should fail on the
        # *count* disappearing, not on which character swallowed the wrapper.
        def bare(chunk: str) -> str:
            return chunk.replace("`", "").replace("*", "")

        if not any(
            bare(needle) in bare(match.group(0)) for match in CLAIM.finditer(text)
        ):
            problems.append(
                f"{name}: the scanner no longer matches {needle!r} ({what}): a "
                "count gate that sees no count is not a gate"
            )

    # The proven-count rule, applied to every page that states a count — never to
    # a document `zensical.toml` keeps out of the site (`MB.35.5`), except the two
    # records pinned above.
    patterns = site_exclusions()
    stale_used: set[tuple[str, str]] = set()
    for name, text in texts.items():
        if not is_page(name, patterns):
            continue
        internal = name in INTERNAL_DOCS
        label = "note" if internal else "claim"
        for match in CLAIM.finditer(text):
            count = int(match.group("count"))
            if count == 0:
                # "zero shims in the wheel" is a true statement about a family
                # that does not travel there; it is not a claim about the matrix.
                continue
            family = match.group("family")
            ceiling = declared_of(family)
            line = text[: match.start()].count("\n") + 1
            if count > ceiling:
                claim = f"{count} {family}"
                stale = STALE_CLAIMS.get((name, claim))
                if stale is not None:
                    # A recorded over-claim: the page states an older matrix and
                    # the entry names the node that rewrites it. Printed, never
                    # silent, and refused below if it stops being true.
                    stale_used.add((name, claim))
                    notes.append(
                        f"{name}:{line}: states {claim}, the matrix declares "
                        f"{ceiling} — stale claim ({stale})"
                    )
                    continue
                problems.append(
                    f"{name}:{line}: claims {count} {family}, the matrix declares "
                    f"{ceiling}: a page may not promise a leg that is not attempted"
                )
            notes.append(
                f"{label} {name}:{line}: {count} {family} "
                f"(matrix {ceiling}, proved {proven_of(family)})"
            )
        for match in PROVEN_CLAIM.finditer(text):
            count = int(match.group("count"))
            if count == 0:
                continue
            family = match.group("family")
            ceiling = proven_of(family)
            line = text[: match.start()].count("\n") + 1
            if count > ceiling:
                problems.append(
                    f"{name}:{line}: claims {count} proven {family}, "
                    f"{ceiling} are built and exercised (PROVEN)"
                )
            notes.append(
                f"{label} {name}:{line}: {count} proven {family} (proved {ceiling})"
            )

    # The stale-claim list is checked in the other direction too: an entry whose
    # page no longer makes the claim is a permission nobody needs, and the next
    # over-claim would be admitted under it.
    for (name, claim), owner in STALE_CLAIMS.items():
        if (name, claim) not in stale_used:
            problems.append(
                f"STALE_CLAIMS: {name} no longer states {claim!r} above the matrix: "
                f"delete this entry ({owner}), because a stale-claim permission that "
                "outlives its claim admits the next over-claim silently"
            )


# --- the output -------------------------------------------------------------

# One sentence for every leg whose smoke has not run on its own runner. It is the
# wording `MB.35.5`'s RESULT and the evidence table must use, defined here so the
# summary and the record cannot disagree about what "unproven" means.
UNPROVEN = "declared, unproven — NOT RUN (no CI runner or cross toolchain on this host)"


def summary(family: str = "") -> str:
    """The Markdown report, whole or for one family (`--summary [FAMILY]`).

    `--summary lsp` prints the editor family's row, its five artifacts (with the
    exact command CI runs for each) and the note that defines "proved"; `R74` and
    the review command name it that way, so the option takes a family rather than
    making a reader grep one table out of all of them.
    """
    only = family

    def wants(name: str) -> bool:
        return not only or only == name

    lines = [
        "| family | declared | built & exercised | dropped (reason) |",
        "|---|---|---|---|",
    ]
    dropped = [leg for leg in LEGS if not leg["enabled"]]
    reason = (
        "; ".join(f"{leg['id']}: {leg['dropped_because']}" for leg in dropped)
        if dropped
        else "—"
    )
    lsp_dropped = [leg for leg in LSP_LEGS if not leg["enabled"]]
    lsp_reason = (
        "; ".join(f"{leg['target']}: {leg['dropped_because']}" for leg in lsp_dropped)
        if lsp_dropped
        else "—"
    )
    for name, key, count in (
        ("wheels", "wheels", len(wheel_legs())),
        ("standalone archives", "archives", len(binary_legs())),
        ("PHP shims", "shims", len(shim_legs())),
        # The per-minor CPython family (`MB.42.3.3`): four wheels, but eight
        # shims, because a WSGI view `dlopen`s its own minor's shim.
        ("CPython shims (per minor)", "py_shims", len(py_shim_legs())),
        # The editor family is counted on its own noun: an LSP archive is never an
        # "archive" here, because that count is the standalone family's (`R74`).
        ("lsp assets (editor)", "lsp", len(lsp_legs())),
        ("native packages (.deb/.rpm)", "packages", len(package_legs())),
        # The container install matrix (`MB.42.3.4`): every distribution the
        # repositories advertise, on both architectures.
        ("container install legs", "install", len(install_legs())),
    ):
        if (
            only
            and {"archives": "binaries", "py_shims": "py-shims"}.get(key, key) != only
        ):
            continue
        lines.append(
            f"| {name} | {count} | {proven_count(key)} of {count} | "
            f"{lsp_reason if key == 'lsp' else reason} |"
        )
    lines.append("")
    lines.append(
        "Declared is what CI builds on the release PR; built & exercised is what "
        "has been run (PROVEN)."
    )
    lines.append("")
    lines.append("| artifact | leg | status |")
    lines.append("|---|---|---|")
    if wants("wheels"):
        for leg in wheel_legs():
            node = PROVEN.get(("wheels", leg["arch"], leg["libc"]))
            lines.append(
                f"| `{leg['wheel']}` (smoked in `{leg['smoke_pythons']}`) | "
                f"{leg['runs_on']} | {'proved by ' + node if node else 'declared, unbuilt'} |"
            )
    if wants("py-shims"):
        for leg in py_shim_legs():
            node = PROVEN.get(("py_shims", leg["minor"], leg["arch"], leg["libc"]))
            lines.append(
                f"| `{leg['shim']}` | CPython {leg['minor']} on {leg['runs_on']} | "
                f"{'proved by ' + node if node else 'declared, unbuilt'} |"
            )
    if wants("binaries"):
        for leg in binary_legs():
            node = PROVEN.get(("archives", leg["arch"], leg["libc"]))
            lines.append(
                f"| `{leg['archive']}` | {leg['runs_on']} | "
                f"{'proved by ' + node if node else 'declared, unbuilt'} |"
            )
    if wants("shims"):
        for leg in shim_legs():
            node = PROVEN.get(("shims", leg["minor"], leg["arch"], leg["libc"]))
            lines.append(
                f"| `{leg['shim']}` | {leg['base']} ({leg['runs_on']}) | "
                f"{'proved by ' + node if node else 'declared, unbuilt'} |"
            )
    if wants("images"):
        for image in IMAGES:
            lines.append(
                f"| `{image['tag']}` | PHP {image['minor']} on {image['libc']} "
                "| declared (a consumer image, not a counted family) |"
            )
    if wants("lsp"):
        for leg in lsp_legs():
            node = PROVEN.get(("lsp", leg["target"]))
            status = f"proved by {node}" if node else UNPROVEN
            lines.append(
                f"| `{leg['archive']}` | {leg['runs_on']} | "
                f"{status}; the command CI runs is `{leg['ci_command']}` |"
            )
    if wants("packages"):
        for leg in package_legs():
            node = PROVEN.get(("packages", leg["arch"], leg["format"]))
            lines.append(
                f"| `{leg['pattern']}` ({leg['format']}) | {leg['image']} on "
                f"{leg['runs_on']} | {'proved by ' + node if node else 'declared, unbuilt'}; "
                f"bundle ceiling glibc {leg['bundle_ceiling']}, binaries {leg['binary_floor']} |"
            )
    if wants("install"):
        for leg in install_legs():
            lines.append(
                f"| `{leg['image']}` ({leg['format']}, {leg['package_arch']}) | "
                f"{leg['runs_on']} | {', '.join(leg['modes'].split())} under "
                f"{leg['installer']}; runs before anything is published |"
            )
    lines.append("")
    if wants("lsp"):
        lines.append(
            "An lsp leg is proved only after `scripts/lsp_smoke.py` drove a real "
            "session against **the archive it publishes** on that leg's native runner; "
            "a built-but-unsmoked leg is declared, unproven, and carries the command "
            "that would prove it."
        )
        lines.append("")
    if not wants("shims"):
        return "\n".join(lines).rstrip("\n")
    lines.append(
        "The two protected PHP suites (`MB.3.39`) run **in the leg's own image**, "
        "so a leg may only claim them if that image builds them (`MB.3.58`):"
    )
    lines.append("")
    lines.append("| (minor, libc) | suites in its image |")
    lines.append("|---|---|")
    for minor in PHP_MINORS:
        for libc in LIBCS:
            spec = PHP_BUILDS[(minor, libc)]
            cell = "yes" if spec["suites"] else f"no — {spec['suites_because']}"
            lines.append(f"| PHP {minor}, {libc} | {cell} |")
    lines.append("")
    lines.append("## Which PHP a user can actually run (measured, `SHIM_REACHES`)")
    lines.append("")
    lines.append(f"reaches: {reaches_sentence()}.")
    lines.append("")
    lines.append(
        "`PROVEN` says an artefact was built and exercised; this table says what the "
        "boot did with it, because the two are different claims."
    )
    lines.append("")
    lines.append("| shim leg | reaches | why (measured) |")
    lines.append("|---|---|---|")
    for leg in shim_legs():
        value, because = SHIM_REACHES.get(
            (leg["minor"], leg["libc"], leg["arch"]),
            ("unbuilt", "declared, never built"),
        )
        lines.append(f"| `{leg['shim']}` | {value} | {because} |")
    lines.append("")
    lines.append("| PHP minor | glibc (Debian/Ubuntu) | musl (Alpine) |")
    lines.append("|---|---|---|")
    for minor in PHP_MINORS:
        cells = [
            SHIM_REACHES.get((minor, libc, "x86_64"), ("unbuilt", ""))[0]
            for libc in LIBCS
        ]
        lines.append(f"| {minor} | {cells[0]} | {cells[1]} |")
    return "\n".join(lines)


def _filtered_legs(
    family: str,
    *,
    minor: str = "",
    python: str = "",
    libc: str = "",
    arch: str = "",
    target: str = "",
    name: str = "",
) -> list[dict]:
    legs = {
        "wheels": wheel_legs,
        "binaries": binary_legs,
        "shims": shim_legs,
        "py-shims": py_shim_legs,
        "images": lambda: [dict(item) for item in IMAGES],
        "lsp": lsp_legs,
        "packages": package_legs,
        "install": install_legs,
    }[family]()
    if name:
        legs = [leg for leg in legs if leg.get("name") == name]
    if minor:
        legs = [leg for leg in legs if leg.get("minor") == minor]
    if python:
        legs = [leg for leg in legs if leg.get("python") == python]
    if libc:
        legs = [leg for leg in legs if leg.get("libc") == libc]
    if arch:
        legs = [leg for leg in legs if leg.get("arch") == arch]
    if target:
        legs = [leg for leg in legs if leg.get("target") == target]
    return legs


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="validate the matrix and every stated count",
    )
    parser.add_argument(
        "--matrix",
        choices=(
            "wheels",
            "binaries",
            "shims",
            "py-shims",
            "images",
            "lsp",
            "packages",
            "install",
        ),
    )
    parser.add_argument(
        "--count",
        choices=(
            "wheels",
            "binaries",
            "shims",
            "py-shims",
            "images",
            "lsp",
            "packages",
            "install",
        ),
        help="print how many enabled legs match the filters (exits 0 even when 0)",
    )
    parser.add_argument(
        "--summary",
        nargs="?",
        const="",
        default=None,
        choices=(
            "",
            "wheels",
            "binaries",
            "shims",
            "py-shims",
            "images",
            "lsp",
            "packages",
            "install",
        ),
        help="print the Markdown report, or one family's section (`--summary lsp`)",
    )
    parser.add_argument(
        "--leg",
        choices=(
            "wheels",
            "binaries",
            "shims",
            "py-shims",
            "lsp",
            "packages",
            "install",
        ),
    )
    parser.add_argument(
        "--shim-floor",
        action="store_true",
        help="print the declared PHP shim glibc ceiling (`2_41`) for `--max-glibc`",
    )
    parser.add_argument(
        "--minor",
        default="",
        help="with shims: the PHP minor (8.3-8.6); with py-shims: the CPython minor (3.13, 3.14)",
    )
    parser.add_argument(
        "--python",
        default="",
        help="with py-shims: filter by CPython minor; a wheel serves every CPython >=3.12",
    )
    parser.add_argument(
        "--libc",
        default="",
        choices=("", "gnu", "musl"),
        help="filter to this libc (gnu or musl)",
    )
    parser.add_argument(
        "--arch",
        default="",
        choices=("", "x86_64", "aarch64"),
        help="filter to this arch (x86_64 or aarch64)",
    )
    parser.add_argument(
        "--target",
        default="",
        choices=("", *(leg["target"] for leg in LSP_LEGS)),
        help="with --leg/--count/--matrix lsp: filter by Rust triple",
    )
    parser.add_argument(
        "--name",
        default="",
        help="filter by leg name (e.g. `--leg install --name debian-13-x86_64`)",
    )
    parser.add_argument(
        "--quiet", action="store_true", help="--check without the per-document notes"
    )
    args = parser.parse_args(argv)

    problems: list[str] = []
    notes: list[str] = []
    if args.check:
        check(problems, notes)
    if notes and not args.quiet:
        for note in notes:
            print(f"platform-matrix: {note}")
    if problems:
        for problem in problems:
            print(f"platform-matrix: {problem}", file=sys.stderr)
        return 1

    if args.shim_floor:
        print(shim_floor())

    if args.matrix:
        legs = _filtered_legs(
            args.matrix,
            minor=args.minor,
            python=args.python,
            libc=args.libc,
            arch=args.arch,
            target=args.target,
            name=args.name,
        )
        print(json.dumps({"include": legs}, indent=None, separators=(",", ":")))
    if args.count:
        legs = _filtered_legs(
            args.count,
            minor=args.minor,
            python=args.python,
            libc=args.libc,
            arch=args.arch,
            target=args.target,
            name=args.name,
        )
        print(len(legs))
    if args.leg:
        legs = _filtered_legs(
            args.leg,
            minor=args.minor,
            python=args.python,
            libc=args.libc,
            arch=args.arch,
            target=args.target,
            name=args.name,
        )
        if len(legs) == 0:
            print(
                f"platform-matrix: --leg {args.leg} matched 0 enabled legs "
                f"(minor={args.minor!r}, python={args.python!r}, libc={args.libc!r}, arch={args.arch!r})",
                file=sys.stderr,
            )
            return 2
        if len(legs) > 1:
            missing: list[str] = []
            if args.leg in ("shims", "py-shims") and not args.minor:
                missing.append("--minor")
            if args.leg == "lsp" and not args.target:
                missing.append("--target")
            if args.leg != "lsp" and not args.libc:
                missing.append("--libc")
            if args.leg != "lsp" and not args.arch:
                missing.append("--arch")
            hint = (
                f"pass {', '.join(missing)} (or use --count {args.leg})"
                if missing
                else f"use --count {args.leg} or --matrix {args.leg}"
            )
            print(
                "platform-matrix: --leg matched "
                f"{len(legs)} legs ({[leg.get('name') for leg in legs]}): {hint}",
                file=sys.stderr,
            )
            return 2
        print("\n".join(f"{key}={value}" for key, value in legs[0].items()))
    if args.summary is not None:
        print(summary(args.summary))
    if args.check and not (args.matrix or args.count or args.leg or args.summary):
        print(
            f"platform-matrix: ok — {len(wheel_legs())} wheels, "
            f"{len(binary_legs())} archives, {len(shim_legs())} shims, "
            f"{len(py_shim_legs())} python shims, {len(lsp_legs())} lsp assets declared; "
            f"built and exercised: {proven_count('wheels')}/{len(wheel_legs())} wheels, "
            f"{proven_count('archives')}/{len(binary_legs())} archives, "
            f"{proven_count('shims')}/{len(shim_legs())} shims, "
            f"{proven_count('py_shims')}/{len(py_shim_legs())} python shims, "
            f"{proven_count('lsp')}/{len(lsp_legs())} lsp assets; "
            f"{len(package_legs())} packages installed in {len(install_legs())} "
            f"container legs; "
            f"reaches: {reaches_sentence()}; "
            f"{len([leg for leg in LEGS if not leg['enabled']])} leg(s) dropped"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
