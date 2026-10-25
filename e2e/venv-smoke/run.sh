#!/usr/bin/env bash
# What `e2e/venv-smoke.sh` runs inside the container: the pilot's layout.
#
# Creates a virtualenv with uv, installs the wheel into it, starts the server
# through the venv's console script — `sudo .venv/bin/blastit`, because init has
# to be root to bind the view sockets, chown them and setuid (§4) — and checks
# that a request is served by that venv's interpreter. The venv's own python is
# the runner here, which is the whole point of the test.
#
# Arguments: <wheel> <interpreter> <work directory>
set -euo pipefail

wheel=$1
interpreter=$2
work=$3

mkdir -p "$work"
# The venv's directory has to stay traversable by the uids blastit drops to:
# init `execve`s `_bin/blastit-dbproxy` *after* `setuid(B+3)` (§4), and every
# view imports the app as uid B+1000+i. `mktemp -d` makes a 0700 directory,
# which is the one realistic layout that would hide the wheel's own binary from
# the sandbox — and /app, /srv/app or a project checkout are all 0755.
chmod 0755 "$work"
cd "$work"
cp /fixture/wsgi.py /fixture/blastit.toml /fixture/check.py .

uv venv "$work/.venv" --python "$interpreter"
# The app's dependency, stood in for by a module that is in the venv's
# site-packages and nowhere else. A real project would `uv add` something here.
site_packages=$(ls -d "$work"/.venv/lib/python*/site-packages)
printf 'VALUE = "blastit-venv-smoke"\n' >"$site_packages/venv_marker.py"

uv pip install --python "$work/.venv/bin/python" "$wheel"
"$work/.venv/bin/python" -c 'import blastit, blastit._blastit'
echo "--- the wheel's dbproxy binary, as installed"
ls -l "$site_packages/blastit/_bin/blastit-dbproxy"

# The environment the platform gives PID 1 (§8.2). `DATABASE_URL` is a
# credential for dbproxy — the app never sees it — and it is never dialled: the
# test only needs dbproxy to start from the wheel's own binary.
sudo env \
    PORT=8080 \
    BLASTIT_APP=wsgi:application \
    BLASTIT_MANIFEST="$work/blastit.toml" \
    BLASTIT_LOG_FORMAT=json \
    PYTHONPATH="$work" \
    DATABASE_URL="postgresql://demo:not-a-real-password@db.invalid:5432/demo?sslmode=disable" \
    "$work/.venv/bin/blastit" >"$work/blastit.log" 2>&1 &
server=$!

cleanup() {
    sudo kill "$server" 2>/dev/null || true
    wait "$server" 2>/dev/null || true
}
trap cleanup EXIT

if ! "$work/.venv/bin/python" check.py \
    --base-url http://127.0.0.1:8080 \
    --prefix "$work/.venv"; then
    echo "--- blastit.log"
    cat "$work/blastit.log"
    exit 1
fi

status=0
checked() {
    local pattern=$1 what=$2
    if grep -q "$pattern" "$work/blastit.log"; then
        echo "  ok   $what"
    else
        echo "  FAIL $what"
        status=1
    fi
}
checked '"event":"boot"' "the event stream carries boot"
checked '"event":"ready"' "the event stream carries ready"
checked '"stage":"stage0"' "boot is stage 0"
checked '"stage":"serving"' "ready is serving"

echo "--- blastit.log"
cat "$work/blastit.log"
exit "$status"
