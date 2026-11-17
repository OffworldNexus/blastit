# The `php2shell` fixture's runtime: the wheel this build produced, plus a real
# NTS `libphp` the embedded worker loads. Build context: the repository root
# (the wheel in `dist/` and `e2e/php/**`).
#
# # Why this base and this package (the digest is the pin)
#
# The wheel decides the interpreter (`prebuilt_wheel.py`): `blastit._blastit` is
# not abi3, so the base has to be the CPython the wheel targets, and the digest
# below is the exact `python:3.14-slim-trixie` that produced the run of record.
# Trixie is also the only Debian that ships `libphp8.4-embed` — upstream's
# `php` docker-library images are built without `--enable-embed` and carry no
# `libphp.so` at all (`docs/research/php-embed-sapi.md` §4) — so the runtime is
# the package a host installs with `apt install libphp-embed`
# (`docs/frameworks/php/index.md`), version-pinned with the base.
#
# # What this suite claims, and what it does not
#
# The matrix is neutral because the **surface is absent by construction**: this
# image has no `php-cgi` binary, the worker has no FastCGI parameter stream, no
# argv and no INI channel a request can reach (the C shim builds `$_SERVER` and
# the `ZEND_INI_SYSTEM` blob itself).
#
# It does **not** claim "even an unpatched runtime". `8.4.27-1~deb13u1` is
# Debian's current package, and no distribution ships an unpatched `libphp`
# that an 8.4 shim could load, so that claim is neither made nor testable here.
# What *is* tested is `P4b` in `e2e/php2shell.py`: the same interpreter, the same
# canary file, `php -d auto_prepend_file=…` — its canary is present, so the
# suite's "canary absent" means "the channel is absent", not "the canary is
# broken". Rows 9 and 10 say in their note that on Linux NTS they cannot fail at
# all (no Windows best-fit, no argv).
# The wheel decides the interpreter (`prebuilt_wheel.py`): `blastit._blastit` is
# not abi3, so the base has to be the CPython the wheel targets. Each supported
# minor has its own multi-arch OCI index digest: putting `@sha256:...` on a
# single `FROM python:${PYTHON_VERSION}-slim-trixie@sha256:...` line causes
# Docker to ignore `${PYTHON_VERSION}` completely and pull the digest's minor
# (breaking CI's `cp313` wheel against a hardcoded `3.14` digest). Selecting
# `FROM base-${PYTHON_VERSION}` keeps both digest pins while actually honouring
# `PYTHON_VERSION`.
ARG PYTHON_VERSION=3.14
FROM python:3.13-slim-trixie@sha256:e48f59cdba69a3cbd9ed2cf0f348d90d45a8d350d98134cf1070bc106ee9ed90 AS base-3.13
FROM python:3.14-slim-trixie@sha256:9aaa474097d094040649f2162a627c0049b408f52138d980916788d80f04802f AS base-3.14
FROM base-${PYTHON_VERSION}

ENV DEBIAN_FRONTEND=noninteractive

# `libphp8.4-embed`: the NTS `libphp` the worker embeds.
# `php8.4-dev`: `php-config8.4` and the headers the shim is compiled against.
# `cc`/`libc6-dev`: the shim is C; there is no Rust in this image.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libphp8.4-embed=8.4.27-1~deb13u1 \
      php8.4-dev=8.4.27-1~deb13u1 \
      gcc libc6-dev \
 && rm -rf /var/lib/apt/lists/*

# The wheel this build produced (`scripts/build-wheel.sh`; `e2e/run.py` passes
# the interpreter it targets). Exactly one, like `Dockerfile.prebuilt`: two in
# `dist/` is a mistake to report, not to guess at.
COPY dist/ /tmp/dist/
RUN set -e; \
    wheels="$(ls /tmp/dist/*.whl 2>/dev/null || true)"; \
    count="$(printf '%s\n' "$wheels" | grep -c . || true)"; \
    if [ "$count" != 1 ]; then \
        echo "e2e/php/Dockerfile wants exactly one blastit wheel in dist/, found $count:" >&2; \
        printf '%s\n' "$wheels" >&2; \
        echo "build it with scripts/build-wheel.sh, or keep only the one this image should install" >&2; \
        exit 1; \
    fi; \
    pip install --no-cache-dir "$wheels"; \
    rm -rf /tmp/dist

# The `libblastit_php8X-<arch>-<libc>.so` shim, named exactly as
# `crates/blastit/src/stage0.rs`'s discovery looks for it (`ADR-0010` §4.1), and
# built by the same file the release artifacts use. `BLASTIT_PHP_SHIM_DIR` is
# the `dev`/`check` seam the discovery allows; a deployment takes the shim its
# artifact ships.
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

# The fixture is mounted read-only by the compose service, not baked in: the
# driver copies it to a scratch directory so `--learn` can write, exactly as
# `e2e/static_site/run-dev.sh` does for the static leg.
COPY e2e/php/fixture /fixture
# The one-request probe `e2e/rootless/php_entry_swap.py` drives this image
# with (the swap happens inside the container's own filesystem).
COPY e2e/php/entry_probe.py /opt/e2e/entry_probe.py

# The route's uid: an unprivileged user, because a PHP worker is never root
# (`ADR-0010` §1). `blastit dev` runs as this uid, so the worker does too.
RUN useradd --uid 1000 --create-home --shell /bin/sh you

ENV BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
    BLASTIT_PHP_SHIM_DIR=/opt/blastit/shims \
    PYTHONUNBUFFERED=1

USER 1000:1000
WORKDIR /home/you
# The service keeps the container up and the driver starts the real
# `blastit dev` itself, so the page's command runs in the foreground where its
# exit status is observable (`php2shell.py`).
CMD ["sleep", "infinity"]
