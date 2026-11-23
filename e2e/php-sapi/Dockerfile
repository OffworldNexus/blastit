# The real-load acceptance for crates/blastit-php-sapi (node MB.3.12), one leg
# per (PHP minor, libc) (node MB.3.18).
#
# The base image and the four package names are build args because **no single
# distro ships all four minors, and Alpine and Debian disagree about the
# names**: Debian trixie is PHP 8.4, Ubuntu noble is 8.3, Ubuntu 26.04 and
# Alpine 3.23 are 8.5, `debian:bookworm-slim` + the Surý repository and
# `alpine:edge` + `edge/testing` are 8.6 — each with its own signed
# `libphpX.Y-embed` (Debian/Ubuntu: `/usr/lib/libphp8.6.so`) or `phpXY-embed`
# (Alpine: `/usr/lib/libphp83.so` for 8.3, `/usr/lib/php8X/libphp.so` for 8.4,
# 8.5 and 8.6).
# They come from `scripts/platform_matrix.py` (the one place a leg is declared),
# never from a second list here. `EXTRA_REPO` is that leg's declarative
# repository (`sury` / `alpine-edge-testing`): this file performs the setup by
# name, so no workflow and no driver carries a keyring or a repository line.
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
#
# Every network fetch below is bounded (`MB.76`): `APT_OPTS`, `APK_OPTS`,
# `CURL_OPTS` and `RUSTUP_MAX_RETRIES` carry the deadlines, so one stalled
# mirror or rustup download fails in seconds instead of eating the job's whole
# `PHP_SAPI_TIMEOUT`. They are ARGs (not literals) so `e2e/php_sapi_probe.py`
# passes them explicitly and a test can shrink them.
ARG BASE=debian:trixie-slim
FROM ${BASE}

ARG PHP_MINOR=8.4
ARG PHP_DEV=php8.4-dev
ARG PHP_EMBED=libphp8.4-embed
ARG PHP_CURL=php8.4-curl
ARG LIBPHP=/usr/lib/libphp8.4.so
# The repository this leg needs before its packages exist, as the name
# `scripts/platform_matrix.py` declares (`""` for a stock image):
#   sury                  pinned PHP 8.6 payload source (Debian bookworm): its
#                         mirror's index objects are unreliable, so it is
#                         deliberately never added as an apt source
#   alpine-edge-testing   `edge/testing` appended to `/etc/apk/repositories`
ARG EXTRA_REPO=
ARG RUST_TOOLCHAIN=stable
# Whether this image compiles the protected suites (`MB.3.39`, `MB.23.4.2`),
# driven by the leg's `suites` field in `scripts/platform_matrix.py`
# (`e2e/php_sapi_probe.py` passes it). 0 keeps a ~7-minute `blastit` build — and
# a leg that can never serve, because its `libphp` exports no `php_version` —
# out of the image entirely (`MB.3.58` review). The shim and the probe are built
# either way: they are what the leg proves.
ARG PHP_SAPI_SUITES=1

# The bounded-acquire settings (`MB.76`). `Acquire::ForceIPv4=true` also skips a
# blackholed v6 path, which is the usual CI stall: a v6 route that never answers
# makes `apt-get update` hang with no deadline at all, and one such hang used the
# entire 1800 s `PHP_SAPI_TIMEOUT` inside `docker build`.
ARG APT_OPTS="-o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 -o Acquire::Retries=3 -o Acquire::ForceIPv4=true"
ARG APK_OPTS="--timeout 60"
ARG CURL_OPTS="--connect-timeout 15 --max-time 300 --retry 3 --retry-delay 2 --retry-connrefused"
ARG RUSTUP_MAX_RETRIES=3

ENV DEBIAN_FRONTEND=noninteractive \
    PHP_MINOR=${PHP_MINOR} \
    LIBPHP=${LIBPHP} \
    EXTRA_REPO=${EXTRA_REPO} \
    PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# One layer, two package managers. The glibc legs are Debian/Ubuntu (`apt-get`,
# `libc6-dev`); the musl legs are Alpine (`apk`, `musl-dev`, `pkgconf`). Detected
# rather than declared so a leg cannot name the wrong one and still build.
#
# The extra repository (Alpine) or the pinned payload fetch (sury) comes first,
# because the PHP packages do not exist in a stock `debian:bookworm-slim`
# (8.6 is Ondřej Surý's) or a stock `alpine:edge` (8.6 is in `edge/testing`).
# `EXTRA_REPO` is that leg's name from the matrix, so the setup lives here once
# instead of in every workflow and driver.
#
# `sury` is deliberately **not** an apt source: its mirror serves
# `main/binary-<arch>/Packages*` objects that disagree with its own signed
# `InRelease` (measured at 2026-11-21T14:17:55Z, arm64 served `Packages` 2258246
# vs 2257457 signed, `.gz` 523272 vs 525237, `.xz` 294520 vs 296088; amd64
# disagreed in the other direction), and that held for over an hour, so no retry
# window spans it. Its `by-hash/` answers 404, so apt has no content-addressed
# escape either. The `.deb` payloads are immutable and hash-verifiable, so this
# installs seven pinned payloads per architecture and lets `sha256sum -c` fail
# the leg closed: the pins were verified against the signature-checked index of
# 2026-11-21T14:17:55Z, and are bumped when PHP 8.6 moves. Nothing is installed
# `--allow-unauthenticated`.
#
# `clang`/`libclang-dev` are on the glibc branch, and Alpine's `clang`/
# `llvm-dev` plus whichever `clang<version>-libclang` this rolling image ships
# on the musl branch (named, never guessed: `alpine:edge` moved past
# `clang19-libclang`): the protected suites (`MB.3.39`) are compiled in this
# image too, and `pg_query`'s bindgen needs libclang by name. The Alpine legs
# now carry the suites as well (`MB.3.59`): once the boot serves with Alpine's
# own package names, the worker-level proof has to run where the shim is built,
# exactly like the glibc legs.
RUN set -eux; \
    if command -v apt-get >/dev/null; then \
        apt-get ${APT_OPTS} update; \
        apt-get ${APT_OPTS} install -y --no-install-recommends ca-certificates curl; \
        if [ "${EXTRA_REPO}" = "sury" ]; then \
            mkdir -p /tmp/sury-debs; \
        elif [ -n "${EXTRA_REPO}" ]; then \
            echo "unknown EXTRA_REPO '${EXTRA_REPO}' for this image" >&2; \
            exit 1; \
        fi; \
        # This re-reads the distro's indexes only (sury's payloads are installed
        # from pins verified against a signature-checked index, never from an apt source), and a distro mirror
        # can still serve an index object its own signed `InRelease` does not match
        # while it publishes — Debian and Ubuntu alike — and no compression choice
        # wins when the variants are stale. Hence three spaced attempts (20s of
        # sleeps): plain, the gz indexes, then plain after a longer pause, which
        # covers a publisher that rotates within that window. The third failure
        # fails the leg closed and says so in the log, so a repeat reads as
        # upstream rather than "rerun". Nothing sets a global
        # `CompressionTypes::Order` (that would hide real index mismatches).
        # `MB.76` bounds each attempt (`APT_OPTS`) but leaves this ladder exactly
        # as `MB.62` wrote it: a bounded attempt reaches the next rung in ~20 s
        # instead of holding the build for the job's whole timeout.
        if ! apt-get ${APT_OPTS} update; then \
            echo "apt-get update failed: the mirror is probably publishing indexes; retrying with the gz variant" >&2; \
            sleep 5; \
            if ! apt-get ${APT_OPTS} -o Acquire::CompressionTypes::Order::=gz update; then \
                echo "apt-get update failed again; retrying plainly after a 15s pause" >&2; \
                sleep 15; \
                if ! apt-get ${APT_OPTS} update; then \
                    echo "apt-get update failed on all three spaced attempts: the mirror was unreachable within the per-attempt deadline (a stall) or is serving indexes its signed InRelease does not match — investigate, do not merely rerun" >&2; \
                    exit 1; \
                fi; \
            fi; \
        fi; \
        if [ "${EXTRA_REPO}" = "sury" ]; then \
            case "$(dpkg --print-architecture)" in \
                amd64) set -- \
                    4017e18448b109f640161eacfbd9e2c14f5ccd43fee0711f4f752b1faa39a6b9 pool/main/p/php8.6/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    6e729a360ac400a3be48e084389cc66c9e04af7cec30d9b1a7a69336ab029383 pool/main/p/php-defaults/php-common_102+debsuryorg12.1+3_all.deb \
                    6bbe1c4d2af3a29fb818d38989d866a5805a987511b5f6da857c0fe6a141b1d6 pool/main/p/php8.6/php8.6-cli_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    c1deb64ad9f064b5f06b1ebc8eefd9cb7c4b93407f35709b620b2b770059d941 pool/main/p/php8.6/php8.6-common_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    ff98afc81bcbbf4395ccfec15d1990ac52e9a4dcbddc4fa0315440840d8d4843 pool/main/p/php8.6/php8.6-curl_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    dc84a0d58ada1a6eed84e6c02a711e17766f774d6425aa1db691198b1ad3675c pool/main/p/php8.6/php8.6-dev_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    076bd388dc546c64b44fa47da7e75c79855b4377dea42af02563ebca0e570ce9 pool/main/p/php8.6/php8.6-readline_8.6.0-3+debsuryorg12.1+1_amd64.deb ;; \
                arm64) set -- \
                    05c7df15aee65495629856b6a357bddc241cc858df20fe0fd1ad6b0a8e901150 pool/main/p/php8.6/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    6e729a360ac400a3be48e084389cc66c9e04af7cec30d9b1a7a69336ab029383 pool/main/p/php-defaults/php-common_102+debsuryorg12.1+3_all.deb \
                    114b0a361209e8cc2649fa096580e3ff645f8500dff373117b6d9cb0d99984af pool/main/p/php8.6/php8.6-cli_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    ab4d4939ec7609ade3d653f5cb2a79fb0876e9b5f927c91f551ee09b4d10e141 pool/main/p/php8.6/php8.6-common_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    5e65f362112e9a17c1413df38fa0b871f07c77b911aeb4bcb04cd74f9ba76330 pool/main/p/php8.6/php8.6-curl_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    5b26ce094929289b721533075ccd46a4035d425aef5f82213bd54f214210f917 pool/main/p/php8.6/php8.6-dev_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    a1a18d293f22f03a7c8f76417cb272c05f2b0e7250ac5f64877230f59b5318db pool/main/p/php8.6/php8.6-readline_8.6.0-3+debsuryorg12.1+1_arm64.deb ;; \
                *) echo "no pinned PHP 8.6 payloads for this architecture" >&2; exit 1 ;; \
            esac; \
            [ "${PHP_MINOR}" = "8.6" ] || { echo "the pinned sury payloads are PHP 8.6 only: bump the pins when PHP ${PHP_MINOR} is published" >&2; exit 1; }; \
            while [ "$#" -gt 0 ]; do \
                sha=$1; path=$2; shift 2; \
                file=$(basename "$path"); \
                curl -fsSL ${CURL_OPTS} "https://packages.sury.org/php/$path" -o "/tmp/sury-debs/$file"; \
                printf '%s  /tmp/sury-debs/%s\n' "$sha" "$file" >> /tmp/sury-SUMS; \
            done; \
            sha256sum -c /tmp/sury-SUMS; \
            apt-get ${APT_OPTS} install -y --no-install-recommends /tmp/sury-debs/*.deb \
                gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils \
                clang libclang-dev; \
            rm -rf /tmp/sury-*; \
        else \
            apt-get ${APT_OPTS} install -y --no-install-recommends \
                "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
                gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils \
                clang libclang-dev; \
        fi; \
        rm -rf /var/lib/apt/lists/*; \
    else \
        if [ "${EXTRA_REPO}" = "alpine-edge-testing" ]; then \
            echo "https://dl-cdn.alpinelinux.org/alpine/edge/testing" \
                >> /etc/apk/repositories; \
        elif [ -n "${EXTRA_REPO}" ]; then \
            echo "unknown EXTRA_REPO '${EXTRA_REPO}' for this image" >&2; \
            exit 1; \
        fi; \
        apk update ${APK_OPTS}; \
        apk add --no-cache ${APK_OPTS} \
            "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
            gcc make pkgconf musl-dev binutils ca-certificates curl bash tar xz; \
        if [ "${PHP_SAPI_SUITES:-1}" = "1" ]; then \
            apk add --no-cache ${APK_OPTS} clang llvm-dev; \
            libclang=$(apk search -q 'clang*-libclang' \
                | grep -xE 'clang[0-9]+-libclang' | sort -V | tail -n 1); \
            [ -n "$libclang" ] || { \
                echo "no clang<version>-libclang package on this image (bindgen needs it)" >&2; \
                exit 1; \
            }; \
            apk add --no-cache ${APK_OPTS} "$libclang"; \
        fi; \
    fi

# rustup on both libcs: it ships a musl `rustup-init`, and the workspace's MSRV
# (1.88) is newer than Alpine's packaged cargo, so `apk add rust` is not an
# option. `cargo` already on PATH (a `rust:` base image) is kept.
#
# `-f -o` then run the script, never `curl … | bash` (`MB.76`): piping a failed
# download into a shell executes an empty script and exits 0, so a dead mirror
# produced a leg whose `cargo --version` at the end of this RUN was the only
# failure — pointing at the toolchain instead of the fetch. `RUSTUP_MAX_RETRIES`
# bounds rustup's own toolchain downloads the way `CURL_OPTS` bounds ours, and
# `timeout 600` bounds *one* transfer: retries alone leave a connection that is
# established and then goes silent (a CDN edge that accepts and stops, a proxy
# that dribbles) with no deadline at all.
RUN if ! command -v cargo >/dev/null; then \
        curl --proto '=https' --tlsv1.2 -fsSL ${CURL_OPTS} -o /tmp/rustup-init.sh \
            https://sh.rustup.rs \
            || { echo "could not download https://sh.rustup.rs (bounded by: ${CURL_OPTS})" >&2; exit 1; }; \
        RUSTUP_MAX_RETRIES="${RUSTUP_MAX_RETRIES}" \
            timeout 600 sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}" \
            || { echo "rustup-init did not install the ${RUST_TOOLCHAIN} toolchain: the download from https://static.rust-lang.org stalled or hit the 600s deadline (RUSTUP_MAX_RETRIES=${RUSTUP_MAX_RETRIES})" >&2; exit 1; }; \
        rm -f /tmp/rustup-init.sh; \
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

# The protected suites (`MB.3.39`, `MB.23.4.2`) are compiled **here**, and the
# binaries are recorded where `make rs-test-php-image` runs them, because a
# container may only execute a binary it built itself (`MB.3.58`): the runner is
# Ubuntu 24.04 (glibc 2.39) and this leg is `debian:bookworm` (2.36), so a
# runner-built test binary never even starts here —
#
#   /lib/x86_64-linux-gnu/libc.so.6: weak version 'GLIBC_2.39' not found
#   (required by target/debug/blastit)
#
# — and raising the leg's base would prove nothing about the distros the shim is
# shipped for. The musl legs compile and run the same PHP legs inside
# `alpine` (`MB.3.59`): `BLASTIT_LIBPHP` in this image names the distro's own
# package path, so the served half of all of them is re-proved where the musl
# shim is built.
#
# `PHP_SAPI_SUITES=0` (`matrix.suites == "false"`) skips the build altogether:
# a leg whose pair the boot refuses cannot serve, so its legs would only take
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
            echo "PHP_SAPI_SUITES=${PHP_SAPI_SUITES}: this leg does not build the PHP legs (MB.3.58)"; \
            : > /src/target/php-suite-executables; \
            exit 0; \
        fi; \
        extra=""; \
        if ls /lib/ld-musl-*.so.1 >/dev/null 2>&1; then \
            extra="-C target-feature=-crt-static"; \
            clang_lib=$(find /usr/lib -name libclang.so -print -quit); \
            if [ -z "$clang_lib" ]; then \
                echo "no libclang.so in this image: the pg_query bindgen cannot build" >&2; \
                exit 1; \
            fi; \
            export LIBCLANG_PATH=$(dirname "$clang_lib"); \
        fi; \
        RUSTFLAGS="$extra" cargo test --locked -p blastit --test e2e_dev --no-default-features --features php,static --no-run \
            2>&1 | tee /dev/stderr \
            | sed -n "s/^[[:space:]]*Executable [^(]*(\(.*\))\$/\\1/p" \
            > /src/target/php-suite-executables; \
        count=$(wc -l < /src/target/php-suite-executables); \
        if [ "$count" -ne 1 ]; then \
            echo "expected 1 suite executable (e2e_dev), recorded $count:" >&2; \
            cat /src/target/php-suite-executables >&2; \
            exit 1; \
        fi' \
 && chmod -R a+rX /src/target/php-suite-executables /src/target/php-sapi

# What `make rs-test-php-image` runs the PHP legs against: this image's own pair —
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
