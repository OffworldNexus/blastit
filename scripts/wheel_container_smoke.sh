#!/usr/bin/env bash
# Installs one built `py3-none` wheel in **every** CPython the matrix smokes it
# in, and drives it there (node MB.42.3.3, acceptance 1).
#
#   scripts/wheel_container_smoke.sh dist
#
# The images are `scripts/platform_matrix.py --leg wheels`'s `runtime_images`
# (`python:3.12-slim`, `3.13-slim`, `3.14-slim` for a glibc leg; the `-alpine`
# images for a musl one), so the list is declared once — with the leg — and this
# script cannot claim to have tested a minor the matrix does not.
#
# Per image: `scripts/wheel-smoke.sh` installs the wheel, proves the console
# scripts, `--version`, the embedded binary and a served 200/404, then
# `scripts/wheel-drive.sh` drives `blastit check`, `blastit-manifest fold` and a
# real `blastit-lsp` session. One wheel, three interpreters, one promise
# (design §6.1: `py3-none-*` serves every CPython `>=3.12`).
#
# Nothing is installed on this host: the container is the environment.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
dist="${1:-dist}"

wheels=("$dist"/blastit-*-py3-none-*.whl)
if [ ! -e "${wheels[0]}" ]; then
    echo "wheel-container-smoke: no $dist/blastit-*-py3-none-*.whl; run \`make wheel\` or \`make wheel-manylinux\`" >&2
    exit 2
fi
if [ "${#wheels[@]}" -ne 1 ]; then
    echo "wheel-container-smoke: $dist holds ${#wheels[@]} py3-none wheels; exactly one is smoked per run" >&2
    printf '  %s\n' "${wheels[@]}" >&2
    exit 2
fi
wheels_dir=$(cd "$(dirname "${wheels[0]}")" && pwd)
wheel=$(basename "${wheels[0]}")
tag=$(basename "$wheel" | sed -n 's/^blastit-.*-py3-none-\(.*\)\.whl$/\1/p')
case "$tag" in
    manylinux_*_x86_64 | musllinux_*_x86_64 | linux_x86_64) arch=x86_64 ;;
    manylinux_*_aarch64 | musllinux_*_aarch64 | linux_aarch64) arch=aarch64 ;;
    *)
        echo "wheel-container-smoke: $tag is not a platform tag this matrix knows" >&2
        exit 2
        ;;
esac
case "$tag" in
    musllinux_*) libc=musl ;;
    manylinux_* | linux_*) libc=gnu ;;
    *)
        echo "wheel-container-smoke: $tag names no libc" >&2
        exit 2
        ;;
esac
images=$(uv run --no-project scripts/platform_matrix.py --leg wheels --arch "$arch" --libc "$libc" |
    sed -n 's/^runtime_images=//p')
if [ -z "$images" ]; then
    echo "wheel-container-smoke: platform_matrix.py named no runtime image for $arch-$libc" >&2
    exit 1
fi
echo "wheel-container-smoke: $wheel in $images"

old_ifs=$IFS
IFS=,
for image in $images; do
    IFS=$old_ifs
    echo "wheel-container-smoke: --- $image"
    docker run --rm \
        -v "$root:/src:ro" \
        -v "$wheels_dir:/wheels:ro" \
        "$image" \
        sh -euc '
            # A musl leg runs a `python:<minor>-alpine` image, which has no bash:
            # both smoke scripts are bash (`MB.42.3.3` review: the musl wheel was
            # never actually installed anywhere).
            # A musl host needs `bash` for these scripts and `libgcc` for the
            # `_Unwind_*` symbols a Rust binary uses — the same `apk add libgcc`
            # the standalone archive README names (`MB.42.3.3` review: the musl
            # wheel had never been installed anywhere, and `blastit-lsp` died on
            # `symbol not found: _Unwind_RaiseException`).
            if command -v apk >/dev/null 2>&1; then
                apk add --no-cache bash libgcc >/dev/null
            fi
            python3 -m pip install --quiet --no-cache-dir --disable-pip-version-check \
                /wheels/*.whl pytest
            bash /src/scripts/wheel-smoke.sh /wheels/*.whl
            bash /src/scripts/wheel-drive.sh --repo /src
        '
done
IFS=$old_ifs
echo "wheel-container-smoke: ok — $wheel installed, smoked and driven in $images"
