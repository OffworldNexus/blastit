#!/usr/bin/env bash
# e2e/venv-smoke.sh — the wheel, installed into a project virtualenv, in Docker.
#
# This is the pilot's layout (modelw/base: Debian trixie, CPython 3.14 built by
# python-build-standalone under /opt/python, the unprivileged `user` with
# passwordless sudo), and it checks the promise `pip install blastit` makes: the
# server runs on the interpreter of the environment it was installed into, so
# the application's dependencies are importable, and the credentials-holding role
# runs from the Python-free binary the wheel ships.
#
#   WHEEL=dist/blastit-0.1.0-cp314-cp314-manylinux_2_34_x86_64.whl e2e/venv-smoke.sh
#
# `scripts/build-wheel.sh` (against CPython 3.14) builds that wheel. The check is
# Docker-only because the layout it has to prove — a venv created by uv from a
# python-build-standalone interpreter, with the server started as root through
# `sudo` — is exactly the one the platform runs.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/.." && pwd)

image=${IMAGE:-modelw/base:2026.07}
python=${PYTHON:-/opt/python/cpython-3.14-linux-x86_64-gnu/bin/python3.14}

wheel=${WHEEL:-}
if [[ -z $wheel ]]; then
    # Default to the wheel this repository just built, whichever patch version
    # of CPython 3.14 it was built for.
    wheel=$(ls "$root"/dist/blastit-*cp314*.whl 2>/dev/null | tail -1 || true)
fi
if [[ -z $wheel || ! -f $wheel ]]; then
    echo "no wheel: build one with scripts/build-wheel.sh (CPython 3.14), or set WHEEL=" >&2
    exit 1
fi
wheel=$(cd "$(dirname "$wheel")" && pwd)/$(basename "$wheel")

echo "image:  $image"
echo "wheel:  $wheel"
echo "python: $python"

# The container keeps the image's own `user` (root is only used by `sudo` for
# the server, as PID 1 is root on the platform), and gets two read-only mounts:
# the wheel, and this directory's fixtures. The work directory is writable and
# outside both.
docker run --rm \
    --volume "$(dirname "$wheel"):/wheels:ro" \
    --volume "$here/venv-smoke:/fixture:ro" \
    "$image" \
    bash -euo pipefail -c '
        work=$(mktemp -d)
        bash /fixture/run.sh "/wheels/$(basename "$1")" "$2" "$work"
    ' -- "$wheel" "$python"
