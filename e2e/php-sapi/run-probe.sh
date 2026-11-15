#!/bin/sh
# Runs the probe inside e2e/php-sapi's image: the shim built by `make php-sapi`,
# the host's libphp, and the fixtures. `PHP_MINOR` comes from the image's build
# arg, so a second minor is a build arg and not a rewrite (node MB.3.12).
#
#   run-probe.sh                        # the probe, no seccomp filter
#   run-probe.sh --sandbox              # under the real View { egress: false }
#   run-probe.sh --sandbox --egress allowed   # filter on, INI control removed
#   run-probe.sh --print-tuple          # just `<arch>-<libc>` (the e2e driver)
#
# `--sandbox` is answered here, not by the probe: it runs the probe through
# `php-sapi-seccomp-exec`, which installs the real `blastit-sys` filter and
# execs. Everything else is passed through to the probe.
set -eu

minor="${PHP_MINOR:-8.4}"
digits="$(printf '%s' "$minor" | tr -d '.')"
# The shim's name is the ADR-0010 §4.1 one,
# `libblastit_php<minor>-<arch>-<libc>.so`, and both halves are read from the
# compiler that built it — exactly as `make php-sapi` does
# (`$(PHP_SAPI_CC) -dumpmachine`): a glibc host with a musl loader installed
# still builds glibc. `PHP_SAPI_ARCH`/`PHP_SAPI_LIBC` override the tuple for a
# cross build; a musl tuple with no musl loader here is refused.
cc="${PHP_SAPI_CC:-cc}"
machine="$("$cc" -dumpmachine 2>/dev/null || true)"
arch="${PHP_SAPI_ARCH:-}"
libc="${PHP_SAPI_LIBC:-}"
if [ -z "$machine" ] && [ -z "$arch" ]; then
    echo "run-probe: '$cc' does not answer -dumpmachine; set PHP_SAPI_ARCH and PHP_SAPI_LIBC" >&2
    exit 1
fi
if [ -z "$arch" ]; then arch="${machine%%-*}"; fi
if [ -z "$libc" ]; then
    case "$machine" in
        *musl*) libc=musl ;;
        *gnu*) libc=gnu ;;
        *)
            echo "run-probe: cannot tell the libc of '$cc' ($machine); set PHP_SAPI_LIBC=gnu|musl" >&2
            exit 1
            ;;
    esac
fi
case "$arch" in
    x86_64|aarch64) ;;
    *)
        echo "run-probe: unsupported architecture '$arch' (supported: x86_64, aarch64)" >&2
        exit 1
        ;;
esac
case "$libc" in
    gnu|musl) ;;
    *)
        echo "run-probe: unsupported libc '$libc' (supported: gnu, musl)" >&2
        exit 1
        ;;
esac
if [ "$libc" = musl ] && [ -z "${PHP_SAPI_LIBC:-}" ]; then
    musl_loader=0
    for loader in /lib/ld-musl-*.so.1; do
        if [ -e "$loader" ]; then musl_loader=1; fi
    done
    if [ "$musl_loader" -eq 0 ]; then
        echo "run-probe: '$cc' targets musl but no /lib/ld-musl-*.so.1 is installed" >&2
        exit 1
    fi
fi
shim="target/php-sapi/libblastit_php${digits}-${arch}-${libc}.so"
# The image's own `LIBPHP` (a build arg from the matrix) wins: Ubuntu noble and
# Ubuntu 26.04 install `libphp8.3.so`/`libphp8.5.so` at the same `/usr/lib`
# path Debian trixie uses for 8.4, but a distro that moves it must not need a
# second spelling here. `MB.3.18`.
libphp="${LIBPHP:-/usr/lib/libphp${minor}.so}"

# The e2e driver asks for the same tuple the build derived, rather than
# re-deriving it from `uname`: one derivation, so `readelf` and the probe can
# never look at different names.
if [ "${1:-}" = "--print-tuple" ]; then
    echo "${arch}-${libc}"
    exit 0
fi

sandbox=0
probe_args=""
for arg in "$@"; do
    case "$arg" in
        --sandbox) sandbox=1 ;;
        *) probe_args="$probe_args $arg" ;;
    esac
done

if [ ! -f "$shim" ]; then
    echo "run-probe: $shim is missing (did 'make php-sapi' run in the image?)" >&2
    exit 1
fi
if [ ! -f "$libphp" ]; then
    echo "run-probe: $libphp is missing (is libphp${minor}-embed installed?)" >&2
    exit 1
fi

if [ "$sandbox" -eq 1 ]; then
    exec ./e2e/php-sapi/seccomp-exec/target/release/php-sapi-seccomp-exec \
        ./target/debug/php-sapi-probe "$libphp" "$shim" /fixtures $probe_args
fi

exec ./target/debug/php-sapi-probe "$libphp" "$shim" /fixtures $probe_args
