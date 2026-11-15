#!/usr/bin/env bash
# Takes the shim one leg just built out of its container, and refuses anything
# that is not the file that leg was supposed to produce (`MB.3.18`).
#
#   scripts/collect-shim.sh IMAGE OUTDIR --minor 8.3 [--libc gnu] [--arch x86_64]
#
# The name is not guessed here: `ADR-0010` §4.1 fixes it as
# `libblastit_php<major><minor>-<arch>-<libc>.so`, and
# `scripts/platform_matrix.py` is the one place that spells it for a leg.
# `php_sapi_probe.py` proves the shim loads and serves before this runs, so what
# this copies is a built *and* probed artifact, not a compile.
#
# The image is read, never modified: `make php-sapi-e2e` leaves it behind
# (`--keep`) with the shim at `/src/target/php-sapi`, which is where the probe
# loaded it from.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

if [ "$#" -lt 2 ]; then
    echo "usage: $0 IMAGE OUTDIR --minor X.Y [--libc gnu|musl]" >&2
    exit 2
fi
image=$1
out=$2
shift 2

minor=""
libc="gnu"
arch=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --minor) minor=$2; shift 2 ;;
        --libc) libc=$2; shift 2 ;;
        --arch) arch=$2; shift 2 ;;
        *) echo "usage: $0 IMAGE OUTDIR --minor X.Y" >&2; exit 2 ;;
    esac
done
if [ -z "$minor" ]; then
    echo "error: --minor is required: it decides which shim this leg owes" >&2
    exit 2
fi

# The arch is the leg's own: `(8.3, musl)` exists on x86_64 *and* aarch64, and
# taking the first match would let a muscle-memory run copy the wrong artifact.
if [ -z "$arch" ]; then
    echo "error: --arch is required: (PHP $minor, $libc) exists on x86_64 and aarch64" >&2
    exit 2
fi
spec=$(cd "$root" && uv run --no-project scripts/platform_matrix.py --leg shims --minor "$minor" --libc "$libc" --arch "$arch")
expected=$(printf '%s\n' "$spec" | sed -n 's/^shim=//p')
if [ -z "$expected" ]; then
    echo "error: the matrix has no enabled shim leg for PHP $minor on $libc" >&2
    exit 1
fi

mkdir -p "$out"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/blastit-shim.XXXXXX")
container=$(docker create "$image")
trap 'docker rm -f "$container" >/dev/null 2>&1 || true; rm -rf "$scratch"' EXIT
docker cp "$container:/src/target/php-sapi/." "$scratch/"

if [ ! -f "$scratch/$expected" ]; then
    echo "error: $image has no $expected; it built:" >&2
    ls -l "$scratch" >&2
    echo "  the leg and the artifact name disagree — check PHP_MINORS and cc -dumpmachine" >&2
    exit 1
fi

# A second shim in the image would be an artifact nobody probed on this leg.
for shim in "$scratch"/libblastit_php*.so; do
    [ -f "$shim" ] || continue
    if [ "$(basename "$shim")" != "$expected" ]; then
        echo "error: $image also produced $(basename "$shim"), which this leg did not probe" >&2
        exit 1
    fi
done

install -m 0755 "$scratch/$expected" "$out/$expected"
"$root/scripts/check-portable-elf.sh" "$out/$expected"
echo "collect-shim: $out/$expected ($(stat -c %s "$out/$expected") bytes)"
