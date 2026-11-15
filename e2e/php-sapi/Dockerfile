# The real-load acceptance for crates/blastit-php-sapi (node MB.3.12), one leg
# per (PHP minor, libc) (node MB.3.18).
#
# The base image and the four package names are build args because **no single
# distro ships all three minors, and Alpine and Debian disagree about the
# names**: Debian trixie is PHP 8.4, Ubuntu noble is 8.3, Ubuntu 26.04 and
# Alpine 3.23 are 8.5, Alpine 3.21 is 8.4/8.3 — each with its own signed
# `libphpX.Y-embed` (Debian/Ubuntu: `/usr/lib/libphp8.5.so`) or `phpXY-embed`
# (Alpine: `/usr/lib/libphp83.so` for 8.3, `/usr/lib/php8X/libphp.so` for 8.4
# and 8.5).
# They come from `scripts/platform_matrix.py` (the one place a leg is declared),
# never from a second list here.
#
# `debian:trixie-slim`, not `php:8.4-cli`: upstream's docker-library images are
# built without `--enable-embed`, so they carry no `libphp.so` at all
# (docs/research/php-embed-sapi.md §4). Debian ships NTS libphp8.4 + the exact
# headers `php8.4-dev` provides, which is what a Blastit host installs too; the
# Alpine legs are the musl half of the same rule (`ADR-0010` §4).
#
# The Rust toolchain is here only for the probe binary; the shim itself is C,
# built by `make php-sapi` against the distro headers.
#
#   docker build -f e2e/php-sapi/Dockerfile --build-arg PHP_MINOR=8.4 -t blastit-php-sapi-e2e .
#   docker run --rm blastit-php-sapi-e2e            # one JSON line on stdout
#   docker run --rm blastit-php-sapi-e2e readelf -d target/php-sapi/libblastit_php84-x86_64-gnu.so
# (`-aarch64-…` on an arm64 image: `make php-sapi` names the shim for the
# image's own `cc -dumpmachine`, see `run-probe.sh`.)
#
# The context is the repository root (`Dockerfile.dockerignore` beside this file
# keeps the host's `target/`, `site/` and `plan/` out of it); `make php-sapi`
# then has the workspace it needs.
#
# `make php-sapi-e2e PHP_MINOR=8.3` drives all of it (e2e/php_sapi_probe.py),
# which reads this base/packages/libphp tuple from the matrix.
ARG BASE=debian:trixie-slim
FROM ${BASE}

ARG PHP_MINOR=8.4
ARG PHP_DEV=php8.4-dev
ARG PHP_EMBED=libphp8.4-embed
ARG PHP_CURL=php8.4-curl
ARG LIBPHP=/usr/lib/libphp8.4.so
ARG RUST_TOOLCHAIN=stable
# Whether this image compiles the two protected suites (`MB.3.39`), driven by the
# leg's `suites` field in `scripts/platform_matrix.py` (`e2e/php_sapi_probe.py`
# passes it). 0 keeps a ~7-minute `blastit` build — and a leg that can never
# serve, because its `libphp` exports no `php_version` — out of the image
# entirely (`MB.3.58` review). The shim and the probe are built either way: they
# are what the leg proves.
ARG PHP_SAPI_SUITES=1

ENV DEBIAN_FRONTEND=noninteractive \
    PHP_MINOR=${PHP_MINOR} \
    LIBPHP=${LIBPHP} \
    PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# One layer, two package managers. The glibc legs are Debian/Ubuntu (`apt-get`,
# `libc6-dev`); the musl legs are Alpine (`apk`, `musl-dev`, `pkgconf`). Detected
# rather than declared so a leg cannot name the wrong one and still build.
#
# `clang`/`libclang-dev` are on the glibc branch, and Alpine's `clang`/
# `llvm-dev`/`clang19-libclang` on the musl branch: the two protected suites
# (`MB.3.39`) are compiled in this image too, and `pg_query`'s bindgen needs
# libclang by name. The Alpine legs now carry the suites as well (`MB.3.59`):
# once the boot serves with Alpine's own package names, the worker-level proof
# has to run where the shim is built, exactly like the glibc legs.
RUN set -eux; \
    if command -v apt-get >/dev/null; then \
        apt-get update; \
        apt-get install -y --no-install-recommends \
            "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
            gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils \
            clang libclang-dev; \
        rm -rf /var/lib/apt/lists/*; \
    else \
        apk add --no-cache \
            "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
            gcc make pkgconf musl-dev binutils ca-certificates curl bash tar xz; \
        if [ "${PHP_SAPI_SUITES:-1}" = "1" ]; then \
            apk add --no-cache clang llvm-dev clang19-libclang; \
        fi; \
    fi

# rustup on both libcs: it ships a musl `rustup-init`, and the workspace's MSRV
# (1.88) is newer than Alpine's packaged cargo, so `apk add rust` is not an
# option. `cargo` already on PATH (a `rust:` base image) is kept.
RUN if ! command -v cargo >/dev/null; then \
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | bash -s -- -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}"; \
    fi \
    && cargo --version

WORKDIR /src
# The probe's fixtures at a stable path, independent of where the worker's
# `view_tmp` is: `open_basedir` admits exactly this directory and the view's own
# temp directory.
COPY e2e/php-sapi/fixtures /fixtures
COPY . /src

# A musl build defaults to static-pie, and a static musl binary cannot `dlopen`
# at all: musl answers `Dynamic loading not supported`, which is exactly the call
# this probe exists to make (`dlopen(libphp)`, then `dlopen(shim)`). So the musl
# legs ask rustc for a *dynamic* musl binary; the glibc legs are dynamic already.
# The same flag is what `scripts/build-standalone.sh` sets for a musl artifact,
# where the assertion is that `readelf` shows an interpreter.
RUN extra=""; \
    if ls /lib/ld-musl-*.so.1 >/dev/null 2>&1; then extra="-C target-feature=-crt-static"; fi; \
    echo "probe RUSTFLAGS=$extra"; \
    make php-sapi PHP_MINORS="${PHP_MINOR}" \
 && RUSTFLAGS="$extra" cargo build --locked -p blastit-php-sapi --features probe --bin php-sapi-probe \
 && RUSTFLAGS="$extra" cargo build --release --manifest-path e2e/php-sapi/seccomp-exec/Cargo.toml

# The two protected suites (`MB.3.39`) are compiled **here**, and the binaries are
# recorded where `make rs-test-php-image` runs them, because a container may only
# execute a binary it built itself (`MB.3.58`): the runner is Ubuntu 24.04 (glibc
# 2.39) and this leg is `debian:bookworm` (2.36), so a runner-built test binary
# never even starts here —
#
#   /lib/x86_64-linux-gnu/libc.so.6: weak version 'GLIBC_2.39' not found
#   (required by target/debug/blastit)
#
# — and raising the leg's base would prove nothing about the distros the shim is
# shipped for. The musl legs compile and run the same two suites inside
# `alpine` (`MB.3.59`): `BLASTIT_LIBPHP` in this image names the distro's own
# package path, so the served half of both suites is re-proved where the musl
# shim is built.
#
# `PHP_SAPI_SUITES=0` (`matrix.suites == "false"`) skips the build altogether:
# a leg whose pair the boot refuses cannot serve, so its suites would only take
# their refusal branch.
#
# A musl Rust binary defaults to static-pie, and a static musl binary cannot
# `dlopen`: the test binaries load `libphp` for real, so they need the same
# `-C target-feature=-crt-static` the probe above is built with.
#
# `bash -euo pipefail` rather than the default `sh`: without `pipefail` a failed
# `cargo test` would be masked by the `sed` that follows it in the pipe, and this
# RUN would report a green leg with no binaries.
RUN bash -euo pipefail -c '\
        if [ "${PHP_SAPI_SUITES:-1}" != "1" ]; then \
            echo "PHP_SAPI_SUITES=${PHP_SAPI_SUITES}: this leg does not build the protected suites (MB.3.58)"; \
            : > /src/target/php-suite-executables; \
            exit 0; \
        fi; \
        extra=""; \
        if ls /lib/ld-musl-*.so.1 >/dev/null 2>&1; then \
            extra="-C target-feature=-crt-static"; \
            export LIBCLANG_PATH=/usr/lib/llvm19/lib; \
        fi; \
        RUSTFLAGS="$extra" cargo test --locked -p blastit --test critical_php_backend --test critical_php_embed --no-run \
            2>&1 | tee /dev/stderr \
            | sed -n "s/^[[:space:]]*Executable [^(]*(\(.*\))\$/\\1/p" \
            > /src/target/php-suite-executables; \
        count=$(wc -l < /src/target/php-suite-executables); \
        if [ "$count" -ne 2 ]; then \
            echo "expected 2 suite executables, recorded $count:" >&2; \
            cat /src/target/php-suite-executables >&2; \
            exit 1; \
        fi' \
 && chmod -R a+rX /src/target/php-suite-executables /src/target/php-sapi

# What `make rs-test-php-image` runs the suites against: this image's own pair —
# the NTS `libphp` its package installed and the shim `make php-sapi` left at
# `/src/target/php-sapi`. They are the same two files the probe just loaded, so a
# leg cannot prove one pair and run the suites against another.
ENV BLASTIT_LIBPHP=${LIBPHP} \
    BLASTIT_PHP_SHIM_DIR=/src/target/php-sapi

# The e2e-only seccomp harness (built with the probe above) links the real
# `blastit-sys` filter and execs the probe under it. A standalone package (its own
# `[workspace]`), so it never touches the Blastit workspace or its lock file.
# `php84-curl` above is what `egress.php` calls: curl opens its own socket and
# never consults `allow_url_fopen`, so it is the honest second target for the filter.

CMD ["sh", "/src/e2e/php-sapi/run-probe.sh"]
