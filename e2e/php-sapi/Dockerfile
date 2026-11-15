# The real-load acceptance for crates/blastit-php-sapi (node MB.3.12), one leg
# per (PHP minor, libc) (node MB.3.18).
#
# The base image and the four package names are build args because **no single
# distro ships all three minors, and Alpine and Debian disagree about the
# names**: Debian bookworm is PHP 8.2, Ubuntu noble is 8.3, Debian trixie and
# Alpine 3.21 are 8.4, Alpine 3.20 is 8.2 — each with its own signed
# `libphpX.Y-embed` (Debian/Ubuntu: `/usr/lib/libphp8.2.so`) or `phpXY-embed`
# (Alpine: `/usr/lib/libphp82.so` for 8.2/8.3, `/usr/lib/php84/libphp.so` for 8.4).
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

ENV DEBIAN_FRONTEND=noninteractive \
    PHP_MINOR=${PHP_MINOR} \
    LIBPHP=${LIBPHP} \
    PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# One layer, two package managers. The glibc legs are Debian/Ubuntu (`apt-get`,
# `libc6-dev`); the musl legs are Alpine (`apk`, `musl-dev`, `pkgconf`). Detected
# rather than declared so a leg cannot name the wrong one and still build.
RUN set -eux; \
    if command -v apt-get >/dev/null; then \
        apt-get update; \
        apt-get install -y --no-install-recommends \
            "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
            gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils; \
        rm -rf /var/lib/apt/lists/*; \
    else \
        apk add --no-cache \
            "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
            gcc make pkgconf musl-dev binutils ca-certificates curl bash tar xz; \
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

# The e2e-only seccomp harness (built with the probe above) links the real
# `blastit-sys` filter and execs the probe under it. A standalone package (its own
# `[workspace]`), so it never touches the Blastit workspace or its lock file.
# `php84-curl` above is what `egress.php` calls: curl opens its own socket and
# never consults `allow_url_fopen`, so it is the honest second target for the filter.

CMD ["sh", "/src/e2e/php-sapi/run-probe.sh"]
