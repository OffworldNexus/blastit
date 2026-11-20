#!/usr/bin/env bash
# Proves the wheel refuses — instead of `exec`-looping — when its own `_bin/`
# cannot be repaired (`MB.42.3.3` review, MAJOR 5).
#
#   scripts/wheel_readonly_refusal.sh target/dist
#
# The review reproduced the failure: a read-only `site-packages` makes
# `chmod 0755 blastit/_bin/blastit` fail, `_executable()` says no, `_program()`
# falls through to the pip console script (which is the very process asking),
# and `main()` `exec`s it again: `STILL-LOOPING after 6 execs`, zero output.
#
# The fix is two-sided: the mode repair is limited to this package's own `_bin/`,
# and the console script sets `BLASTIT_WHEEL_EXEC` before it execs, so the
# fallbacks (`<venv>/bin/blastit`, `PATH`) are skipped and the package refuses with
# the sentence that names the fix.
#
# This script installs the wheel into a venv (container 1), then runs the console
# script from that venv **mounted read-only** as an unprivileged user (container
# 2), so no `chmod` can succeed anywhere.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
dist="${1:-target/dist}"
image="${BLASTIT_PYTHON_IMAGE:-python:3.13-slim}"
work=$(mktemp -d "${TMPDIR:-/var/tmp}/blastit-readonly-refusal.XXXXXX")
cleanup() { rm -rf "$work" 2>/dev/null || true; }
trap cleanup EXIT

wheel=$(ls "$dist"/*-py3-none-*.whl 2>/dev/null | head -1)
[ -n "$wheel" ] || { echo "readonly-refusal: no $dist/*-py3-none-*.whl; run make wheel-manylinux" >&2; exit 2; }

echo "readonly-refusal: installing $(basename "$wheel") into a venv"
docker run --rm -v "$(cd "$(dirname "$wheel")" && pwd):/wheels:ro" -v "$work:/work" "$image" sh -euc '
    python3 -m venv /work/venv >/dev/null
    /work/venv/bin/python -m pip install --quiet --no-cache-dir --disable-pip-version-check /wheels/*.whl
    # `pip` before 26 writes `0644` for everything outside `.data/scripts/` (26
    # preserves the wheel mode), so the reproduction forces the case: the wheel
    # `_bin/blastit` is not executable, and the package is then read-only.
    bin=$(find /work/venv -path "*/blastit/_bin/blastit" -type f | head -1)
    [ -n "$bin" ] || { echo "no blastit/_bin/blastit in the venv" >&2; exit 1; }
    chmod 0644 "$bin"
    [ "$(stat -c %a "$bin")" = 644 ] || { echo "chmod 0644 did not take on $bin" >&2; exit 1; }
'

echo "readonly-refusal: running the console script with its package mounted read-only"
start=$(date +%s)
set +e
docker run --rm --user 65534:65534 \
    -v "$work/venv:/work/venv:ro" \
    -e HOME=/tmp \
    "$image" /work/venv/bin/blastit --version > "$work/out.log" 2>&1
status=$?
set -e
seconds=$(( $(date +%s) - start ))
cat "$work/out.log"

if [ "$status" -eq 0 ]; then
    echo "readonly-refusal: FAIL a read-only package still ran" >&2
    exit 1
fi
if [ "$seconds" -gt 20 ]; then
    echo "readonly-refusal: FAIL it took ${seconds}s: a loop was broken only by a timeout" >&2
    exit 1
fi
grep -q 'this blastit package is incomplete' "$work/out.log" || {
    echo "readonly-refusal: FAIL the refusal did not name the incomplete package" >&2
    exit 1
}
grep -q 'BLASTIT_WHEEL_EXEC' "$work/out.log" || {
    echo "readonly-refusal: FAIL the refusal did not name the cycle-breaker" >&2
    exit 1
}
echo "readonly-refusal: ok — refused in ${seconds}s with the reinstall sentence, no loop"
