#!/usr/bin/env bash
# What `e2e/venv-smoke.sh` runs inside the container: the pilot's layout.
#
# Creates a virtualenv with uv, installs the wheel into it, starts the server
# through the venv's console script — `sudo .venv/bin/blastit`, because init has
# to be root to bind the view sockets, chown them and setuid (§4) — and checks
# that a request is served by that venv's interpreter. The venv's own python is
# the runner here, which is the whole point of the test.
#
# It also checks the two things the wheel could get wrong and nothing else would
# notice:
#
# * importing `blastit` starts no thread — stage 0 forks from inside a live
#   interpreter, and §2.1's single-threaded-at-fork invariant is what makes that
#   safe (§9.3);
# * a dbproxy that dies before it reports ready fails the boot, with its own
#   words in the event stream rather than a hang.
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
dbproxy=$site_packages/blastit/_bin/blastit-dbproxy

uv pip install --python "$work/.venv/bin/python" "$wheel"

status=0
checked() {
    local pattern=$1 what=$2
    if grep -q "$pattern" "$3"; then
        echo "  ok   $what"
    else
        echo "  FAIL $what"
        status=1
    fi
}

# `active_count() == 1`: the console script's interpreter, and nothing blastit
# started behind its back. Stage 0 forks dbproxy from this process (§4), so a
# thread here would be a thread in the child.
"$work/.venv/bin/python" - <<'PY'
import threading

import blastit
import blastit._blastit

assert threading.active_count() == 1, f"importing blastit started a thread: {threading.enumerate()}"
print(f"  ok   importing blastit starts no thread ({threading.active_count()} thread)")
PY

echo "--- the wheel's dbproxy binary, as installed"
ls -l "$dbproxy"

# The environment the platform gives PID 1 (§8.2). `DATABASE_URL` is a
# credential for dbproxy — the app never sees it — and it is never dialled: the
# test only needs dbproxy to start from the wheel's own binary.
#
# No `PYTHONPATH`: `cp /fixture/wsgi.py .` puts the app in the working directory
# and never installs it, so `wsgi:application` importing at all is the check that
# blastit makes the directory it was started in importable (§5.3) — the way
# gunicorn, uwsgi and granian do. `sudo` keeps that directory.
run_server() {
    sudo env \
        PORT=8080 \
        BLASTIT_APP=wsgi:application \
        BLASTIT_MANIFEST="$work/blastit.toml" \
        BLASTIT_LOG_FORMAT=json \
        DATABASE_URL="postgresql://demo:not-a-real-password@db.invalid:5432/demo?sslmode=disable" \
        "$work/.venv/bin/blastit" "$@"
}

run_server >"$work/blastit.log" 2>&1 &
server=$!
cleanup() {
    sudo kill "$server" 2>/dev/null || true
    wait "$server" 2>/dev/null || true
    # The stub the failure scenario installs, if that run did not restore it.
    if [ -f "$work/blastit-dbproxy.real" ]; then
        mv "$work/blastit-dbproxy.real" "$dbproxy"
    fi
}
trap cleanup EXIT

if ! "$work/.venv/bin/python" check.py \
    --base-url http://127.0.0.1:8080 \
    --prefix "$work/.venv" \
    --cwd "$work"; then
    echo "--- blastit.log"
    cat "$work/blastit.log"
    exit 1
fi

checked '"event":"boot"' "the event stream carries boot" "$work/blastit.log"
checked '"event":"ready"' "the event stream carries ready" "$work/blastit.log"
checked '"stage":"stage0"' "boot is stage 0" "$work/blastit.log"
checked '"stage":"serving"' "ready is serving" "$work/blastit.log"
echo "--- blastit.log"
cat "$work/blastit.log"

# §7.2: an interpreter a view uid cannot reach is refused at boot, by name.
#
# A view process runs as a uid of its own, so only the "other" permission bits
# reach it: a Python under a private home (`uv python install` puts one in
# `~/.local/share/uv/python`, and `~` is often `0750`) used to lose its whole
# standard library *inside* a view, one `ModuleNotFoundError` at a time
# (`stringprep`, `encodings.idna`), while everything looked fine from the
# outside. The check is a boot refusal now, and this is the shape it is for: a
# standard library the view uids cannot read. `0750` is what a private home is,
# applied to the directory that holds the modules.
#
# Root can still read it, so the boot gets all the way to stage 1 — which is
# where the check is, and what makes the failure a refusal with a message rather
# than a `ModuleNotFoundError` in one view on one path.
stdlib=$(
    "$work/.venv/bin/python" -c 'import sysconfig; print(sysconfig.get_paths()["stdlib"])'
)
sudo chmod 0750 "$stdlib"
if sudo env \
    PORT=8081 \
    BLASTIT_APP=wsgi:application \
    BLASTIT_MANIFEST="$work/blastit.toml" \
    BLASTIT_LOG_FORMAT=json \
    DATABASE_URL="postgresql://demo:not-a-real-password@db.invalid:5432/demo?sslmode=disable" \
    "$work/.venv/bin/blastit" >"$work/private.log" 2>&1; then
    echo "  FAIL a Python the view uids cannot read booted"
    status=1
else
    echo "  ok   a Python the view uids cannot read is refused"
fi
sudo chmod 0755 "$stdlib"
checked 'mode 0750) is not readable' \
    "the message says what is wrong with it" "$work/private.log"
checked "$stdlib" "and names the directory that blocks" "$work/private.log"
checked 'UV_PYTHON_INSTALL_DIR' \
    "and the fix that moves the interpreter" "$work/private.log"
echo "--- private.log"
cat "$work/private.log"

# E2b: the same check one stage earlier, for the file the credentials-holding
# child `execve`s. dbproxy runs as uid B+3, so the binary has to be executable by
# others; `0700` is the mode that used to fail the boot with
# `exec dbproxy: Permission denied (os error 13)` and say nothing else.
chmod 0700 "$dbproxy"
if sudo env \
    PORT=8081 \
    BLASTIT_APP=wsgi:application \
    BLASTIT_MANIFEST="$work/blastit.toml" \
    BLASTIT_LOG_FORMAT=json \
    DATABASE_URL="postgresql://demo:not-a-real-password@db.invalid:5432/demo?sslmode=disable" \
    "$work/.venv/bin/blastit" >"$work/dbproxy-mode.log" 2>&1; then
    echo "  FAIL a dbproxy binary the child cannot execute booted"
    status=1
else
    echo "  ok   a dbproxy binary the child cannot execute is refused"
fi
chmod 0755 "$dbproxy"
checked 'is not executable' \
    "the message says what is wrong with it" "$work/dbproxy-mode.log"
checked "$dbproxy" "and names the file that blocks" "$work/dbproxy-mode.log"
checked 'chmod o+rx' "and the fix that opens it" "$work/dbproxy-mode.log"
echo "--- dbproxy-mode.log"
cat "$work/dbproxy-mode.log"

# A dbproxy that dies before it reports ready has to fail the boot *and* say why
# (§8.1): the wheel ships that binary, so this is the packaging failure the test
# can produce. `cp` then `rm` rather than an in-place edit: the file is a
# hardlink to uv's cache.
sudo kill "$server" 2>/dev/null || true
wait "$server" 2>/dev/null || true

cp "$dbproxy" "$work/blastit-dbproxy.real"
rm -f "$dbproxy"
printf '#!/bin/sh\necho "stub dbproxy says no"\nexit 3\n' >"$dbproxy"
chmod 0755 "$dbproxy"
if run_server >"$work/died.log" 2>&1; then
    echo "  FAIL a dbproxy that dies before ready failed the boot"
    status=1
else
    echo "  ok   a dbproxy that dies before ready fails the boot"
fi
checked 'stub dbproxy says no' "its own words reach the event stream" "$work/died.log"
checked 'dbproxy exited before it was ready' "the boot says what happened" "$work/died.log"
echo "--- died.log"
cat "$work/died.log"

# And the case that used to hang: a dbproxy that *never* reports ready, and
# holds the write end of its own pipes open while it waits. A drain that read to
# EOF there would block init forever, so the boot has to give up on the deadline
# (§4) — and it must still log what the child managed to print.
rm -f "$dbproxy"
printf '#!/bin/sh\necho "stub dbproxy hangs"\nexec /bin/sleep 60\n' >"$dbproxy"
chmod 0755 "$dbproxy"
started=$(date +%s)
if run_server >"$work/hung.log" 2>&1; then
    echo "  FAIL a dbproxy that never reports ready failed the boot"
    status=1
else
    echo "  ok   a dbproxy that never reports ready fails the boot"
fi
elapsed=$(( $(date +%s) - started ))
if [ "$elapsed" -lt 60 ]; then
    echo "  ok   it gave up after ${elapsed}s instead of waiting for the pipe"
else
    echo "  FAIL the boot hung (${elapsed}s)"
    status=1
fi
checked 'stub dbproxy hangs' "what it printed before hanging reaches the event stream" "$work/hung.log"
checked 'did not report ready within' "the boot says it timed out" "$work/hung.log"
echo "--- hung.log"
cat "$work/hung.log"

mv "$work/blastit-dbproxy.real" "$dbproxy"
exit "$status"
