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
#   WHEEL=dist/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl e2e/venv-smoke.sh
#
# `scripts/build-wheel.sh` builds that wheel (universal `py3-none`, with one
# CPython shim per supported minor inside it). The check is Docker-only because
# the layout it has to prove — a venv created by uv from a python-build-standalone
# interpreter, with the server started as root through `sudo` — is exactly the one
# the platform runs.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/.." && pwd)

image=${IMAGE:-modelw/base:2026.07}
python=${PYTHON:-/opt/python/cpython-3.14-linux-x86_64-gnu/bin/python3.14}

wheel=${WHEEL:-}
if [[ -z $wheel ]]; then
    # Default to the universal wheel this repository just built, whatever
    # platform tag it carries; a developer wheel (`cp3NN`) is the fallback.
    wheel=$(ls "$root"/dist/blastit-*-py3-none-*.whl 2>/dev/null | tail -1 || true)
    if [[ -z $wheel ]]; then
        wheel=$(ls "$root"/dist/blastit-*.whl 2>/dev/null | tail -1 || true)
    fi
fi
if [[ -z $wheel || ! -f $wheel ]]; then
    echo "no wheel in $root/dist matching blastit-*-py3-none-*.whl (or blastit-*.whl): build one with scripts/build-wheel.sh, or set WHEEL=<path>" >&2
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
#
# **Not `mktemp -d` under `/tmp`.** `/tmp` is `1777` and owned by root, so
# production `sudo .venv/bin/blastit` refuses a venv it finds there: the child is
# root, and a directory above the venv is writable by another uid. The work
# directory therefore lives under a root-owned `0755` parent outside `/tmp`.
#
# **Created by root, owned by the invoking user.** The parent is `root:root`, so
# the container `user` cannot create anything in it (`mkdir as user: DENIED` —
# the first shape of `MB.49`'s failure). Root makes the run directory with
# `mktemp -d` — not `run-$$`, which a stale, killed run can collide with — and
# hands it to the container user, because every `>"$work/*.log"` in `run.sh` is
# opened by that user's shell. Root's own tree inside it (`$work/app`, after
# `run.sh`'s `chown`) is removed with `sudo` on the way out.
docker run --rm \
    --volume "$(dirname "$wheel"):/wheels:ro" \
    --volume "$here/venv-smoke:/fixture:ro" \
    "$image" \
    bash -euo pipefail -c '
        sudo -n true 2>/dev/null || {
            echo "venv-smoke: IMAGE=$3 must provide sudo with passwordless escalation (modelw/base does); the server leg boots as root" >&2
            exit 1
        }
        parent=/var/lib/blastit-venv-smoke
        sudo --non-interactive mkdir -p "$parent"
        sudo --non-interactive chmod 0755 "$parent"
        sudo --non-interactive chown root:root "$parent"
        work=$(sudo --non-interactive mktemp -d "$parent/run.XXXXXX")
        sudo --non-interactive chown "$(id -u):$(id -g)" "$work"
        cleanup() {
            # `|| echo` keeps a failed `rm` from becoming the exit status: without
            # it a green run under `trap cleanup EXIT` exits 1 with no output.
            # The `INT`/`TERM` traps `exit` through this same `EXIT` trap, so they
            # keep their 130/143 for the same reason.
            sudo --non-interactive rm -rf "$work" \
                || echo "venv-smoke: could not remove $work" >&2
        }
        trap cleanup EXIT
        trap "exit 130" INT
        trap "exit 143" TERM
        bash /fixture/run.sh "/wheels/$(basename "$1")" "$2" "$work"
    ' -- "$wheel" "$python" "$image"
