# The PHP leg's runtime: a bare Debian trixie with the distro's NTS `libphp`
# and PDO's PostgreSQL driver. Build context: the repository root.
#
# No CPython wheel and no Rust toolchain: the binary under test is the one the
# driver mounts from `target/release/blastit` (or `E2E_BLASTIT`), so this image
# proves the shipped artifact against the distribution library rather than a
# rebuilt copy. `python3` is present only because the `php2shell` driver runs
# inside this container (where `127.0.0.1:8080` is verbatim and libphp lives).
#
# Debian trixie is the first Debian that ships `libphp8.4-embed`; the upstream
# `php` docker-library images are built without `--enable-embed`.
FROM debian:trixie-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libphp8.4-embed \
      php8.4-cli \
      php8.4-pgsql \
      php8.4-dev \
      gcc libc6-dev \
      python3 \
      ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# The shim `blastit` loads beside the binary, built from the same C the release
# artifacts use (no Rust here). `php-config8.4 --includes` carries the headers
# the shim needs, and `-Wall -Wextra -Werror` keeps our file honest.
COPY crates/blastit-php-sapi/c /opt/blastit-src/c
RUN set -eux; \
    arch="$(cc -dumpmachine)"; arch="${arch%%-*}"; \
    libc=gnu; \
    case "$arch" in x86_64|aarch64) ;; *) echo "unsupported architecture $arch" >&2; exit 1 ;; esac; \
    mkdir -p /opt/blastit/shims; \
    cc -O2 -D_GNU_SOURCE -fPIC -shared -Wall -Wextra -Werror \
        -Wl,-soname,"libblastit_php84-${arch}-${libc}.so" \
        -I/opt/blastit-src/c/include \
        $(php-config8.4 --includes) \
        -o "/opt/blastit/shims/libblastit_php84-${arch}-${libc}.so" \
        /opt/blastit-src/c/blastit_php_sapi.c; \
    rm -rf /opt/blastit-src; \
    ls -l /opt/blastit/shims/ /usr/lib/libphp8.4.so

# The fixture, the probe matrix and the driver that replays it.
COPY e2e/apps/php /fixture
COPY e2e/php2shell.py /opt/e2e/php2shell.py
COPY docs/frameworks/php /opt/e2e/php-docs
COPY e2e/apps/php/probes.md /opt/e2e/probes.md

# A PHP worker is never root (`ADR-0010` §1), so the image has the route's uid.
RUN useradd --uid 1000 --create-home --shell /bin/sh you

ENV BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
    BLASTIT_PHP_SHIM_DIR=/opt/blastit/shims \
    PYTHONUNBUFFERED=1

USER 1000:1000
WORKDIR /home/you
CMD ["sleep", "infinity"]
