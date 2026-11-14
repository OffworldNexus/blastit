#!/bin/sh
# Runs the probe inside e2e/php-sapi's image: the shim built by `make php-sapi`,
# the host's libphp, and the fixtures. `PHP_MINOR` comes from the image's build
# arg, so a second minor is a build arg and not a rewrite (node MB.3.12).
#
#   run-probe.sh                        # the probe, no seccomp filter
#   run-probe.sh --sandbox              # under the real View { egress: false }
#   run-probe.sh --sandbox --egress allowed   # filter on, INI control removed
#
# `--sandbox` is answered here, not by the probe: it runs the probe through
# `php-sapi-seccomp-exec`, which installs the real `blastit-sys` filter and
# execs. Everything else is passed through to the probe.
set -eu

minor="${PHP_MINOR:-8.4}"
digits="$(printf '%s' "$minor" | tr -d '.')"
shim="target/php-sapi/libblastit_php${digits}.so"
libphp="/usr/lib/libphp${minor}.so"

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
