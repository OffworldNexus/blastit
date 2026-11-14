# The real-load acceptance for crates/blastit-php-sapi (node MB.3.12).
#
# `debian:trixie-slim`, not `php:8.4-cli`: upstream's docker-library images are
# built without `--enable-embed`, so they carry no `libphp.so` at all
# (docs/research/php-embed-sapi.md §4). Debian ships NTS libphp8.4 + the exact
# headers `php8.4-dev` provides, which is what a Blastit host installs too.
#
# The Rust toolchain is here only for the probe binary; the shim itself is C,
# built by `make php-sapi` against the distro headers.
#
#   docker build -f e2e/php-sapi/Dockerfile --build-arg PHP_MINOR=8.4 -t blastit-php-sapi-e2e .
#   docker run --rm blastit-php-sapi-e2e            # one JSON line on stdout
#   docker run --rm blastit-php-sapi-e2e readelf -d target/php-sapi/libblastit_php84.so
#
# The context is the repository root (`Dockerfile.dockerignore` beside this file
# keeps the host's `target/`, `site/` and `plan/` out of it); `make php-sapi`
# then has the workspace it needs.
#
# `make php-sapi-e2e` drives all of it (e2e/php_sapi_probe.py).
FROM debian:trixie-slim

ARG PHP_MINOR=8.4
ARG RUST_TOOLCHAIN=stable

ENV DEBIAN_FRONTEND=noninteractive \
    PHP_MINOR=${PHP_MINOR} \
    PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      "php${PHP_MINOR}-dev" "libphp${PHP_MINOR}-embed" "php${PHP_MINOR}-curl" \
      gcc make pkg-config libc6-dev binutils ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}"

WORKDIR /src
# The probe's fixtures at a stable path, independent of where the worker's
# `view_tmp` is: `open_basedir` admits exactly this directory and the view's own
# temp directory.
COPY e2e/php-sapi/fixtures /fixtures
COPY . /src

# The shim and then the probe. `make php-sapi` prints one line per minor: the
# one it built and the two this image has no toolchain for.
RUN make php-sapi PHP_MINORS="${PHP_MINOR}" \
 && cargo build --locked -p blastit-php-sapi --features probe --bin php-sapi-probe

# The e2e-only seccomp harness: it links the real `blastit-sys` filter and execs
# the probe under it. A standalone package (its own `[workspace]`), so it never
# touches the Blastit workspace or its lock file. `php8.4-curl` above is what
# `egress.php` calls: curl opens its own socket and never consults
# `allow_url_fopen`, so it is the honest second target for the filter.
RUN cargo build --release --manifest-path e2e/php-sapi/seccomp-exec/Cargo.toml

CMD ["sh", "/src/e2e/php-sapi/run-probe.sh"]
