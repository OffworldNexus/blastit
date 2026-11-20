#!/usr/bin/env bash
# What `e2e/venv-smoke.sh` runs inside the container: the pilot's layout.
#
# Creates a virtualenv with uv, installs the wheel into it, starts the server
# through the venv's console script — `sudo .venv/bin/blastit`, because init has
# to be root to bind the view sockets, chown them and setuid (§4) — and checks
# that a request is served with that venv's dependencies importable. The venv's
# own python is the runner here, which is the whole point of the test.
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
# `e2e/venv-smoke.sh` creates this directory with root's `mktemp -d` (mode 0700)
# and hands it to this uid, so the logs below are this shell's to write. The
# *project* is not this directory: root owns it (see `$app`).
#
# The directory has to stay traversable by the uids blastit drops to: every view
# imports the app and loads the CPython shim as uid B+1000+i, and the role child
# is this image re-executed as `/proc/self/exe --role=dbproxy` (uid B+3) — the
# wheel's `_bin/blastit-dbproxy` copy is on neither path (§4). A 0700 directory
# is the one realistic layout that would hide the interpreter or the wheel from
# the sandbox — and /app, /srv/app or a project checkout are all 0755.
chmod 0755 "$work"

# **The project — and its virtualenv — is root's.** Production `blastit` applies
# D6 to all three claims: the Python installation, the project virtualenv and the
# manifest directory. A tree owned by uid 1000 is refused with `… could be
# replaced by another uid: it is owned by uid 1000, and only root or init's uid
# (0) may own it` — the message `MB.49`'s first fix run hit *after* the venv was
# chowned, because the manifest directory was still this uid's. So the fixture
# and the venv are laid out by this uid and handed to root in one `chown -R`
# below; this shell keeps writing only the logs in `$work` beside them. `$work`
# itself only has to stay *not* group- or other-writable (0755, uid 1000).
#
# The venv lives beside the manifest, which is §2.2's production order
# (`<manifest>/.venv/bin/python`), so the boot finds it instead of falling
# through to whatever interpreter the image happens to have.
app=$work/app
mkdir -p "$app"
cp /fixture/wsgi.py /fixture/Blastfile /fixture/check.py "$app/"

uv venv "$app/.venv" --python "$interpreter"
# The app's dependency, stood in for by a module that is in the venv's
# site-packages and nowhere else. A real project would `uv add` something here.
site_packages=$(ls -d "$app"/.venv/lib/python*/site-packages)
printf 'VALUE = "blastit-venv-smoke"\n' >"$site_packages/venv_marker.py"
dbproxy=$site_packages/blastit/_bin/blastit-dbproxy

uv pip install --python "$app/.venv/bin/python" "$wheel"

# One `chown -R` for the three claims D6 names: a venv installed by this uid
# (1000), every `blastit/_bin/libblastit_py3NN-*.so` in it, and the manifest
# directory. Moving the tree out of `/tmp` only stopped the *third* clause of the
# refusal (a writable ancestor); the owner clause is this one.
#
# Surgical, not blanket: this uid can still *read* and *execute* what root owns
# (0755/0644 survive `chown -R`), and the logs stay in `$work`, which is this
# uid's. The project is the one tree this shell stops writing, so every mutation
# below that lands inside it goes through `sudo` (`sudo tee` for the stub,
# `sudo chmod` for its mode).
sudo chown -R root:root "$app"

# The server's own working directory is the project, whose directory root owns.
cd "$app"

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

# The console script `pip` wrote names the venv's own interpreter, which is what
# makes `sudo .venv/bin/blastit` run *this* environment rather than the image's.
head -1 "$app/.venv/bin/blastit" >"$work/console-script.log"
shebang=$(cat "$work/console-script.log")
checked "#!$app/.venv/bin/python" \
    "the console script runs on the venv's interpreter (shebang: $shebang)" \
    "$work/console-script.log"

# `active_count() == 1`: the console script's interpreter, and nothing blastit
# started behind its back. Stage 0 forks dbproxy from this process (§4), so a
# thread here would be a thread in the child.
"$app/.venv/bin/python" - <<'PY'
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
# No `PYTHONPATH`: the app is copied into the working directory (`$app`) and
# never installed, so `wsgi:application` importing at all is the check that
# blastit makes the directory it was started in importable (§5.3) — the way
# gunicorn, uwsgi and granian do. `sudo` keeps that directory.
run_server() {
    sudo env \
        PORT=8080 \
        BLASTIT_APP=wsgi:application \
        BLASTIT_MANIFEST="$app/Blastfile" \
        BLASTIT_LOG_FORMAT=json \
        DATABASE_URL="postgresql://demo:not-a-real-password@db.invalid:5432/demo?sslmode=disable" \
        ${extra_env[@]+"${extra_env[@]}"} \
        "$app/.venv/bin/blastit" "$@"
}
# Extra variables for the `BLASTIT_EXEC` launches below. Empty otherwise.
extra_env=()

run_server >"$work/blastit.log" 2>&1 &
server=$!
# The boot behind `sudo`, and why *both* go: `$server` is the `sudo` that started
# it, and a SIGTERM there can leave the server (its child) holding port 8080 —
# the next scenario then fails with `binding 0.0.0.0:8080: Address already in
# use` instead of the sentence it is checking. The child is signalled first, then
# the wrapper, and the wait is bounded.
stop_server() {
    local pid=$1 child="" waited=0
    # `|| true` on the probe: `pgrep -P` exits 1 when the wrapper is already
    # gone, and this function also runs from the `EXIT` trap — where a failing
    # pipeline under `set -e`/`pipefail` becomes the *script's* exit status.
    child=$(sudo pgrep -P "$pid" 2>/dev/null | head -1 || true)
    sudo kill -TERM ${child:+"$child"} "$pid" 2>/dev/null || true
    while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 60 ]; do
        waited=$((waited + 1))
        sleep 0.25
    done
    sudo kill -KILL ${child:+"$child"} "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    return 0
}
cleanup() {
    stop_server "$server"
    return 0
}
trap cleanup EXIT

if ! "$app/.venv/bin/python" check.py \
    --base-url http://127.0.0.1:8080 \
    --site-packages "$site_packages" \
    --cwd "$app"; then
    echo "--- blastit.log"
    cat "$work/blastit.log"
    exit 1
fi

checked '"event":"boot"' "the event stream carries boot" "$work/blastit.log"
checked '"event":"ready"' "the event stream carries ready" "$work/blastit.log"
checked '"stage":"stage0"' "boot is stage 0" "$work/blastit.log"
checked '"stage":"serving"' "ready is serving" "$work/blastit.log"
checked "\"environment\":\"$app/.venv\"" \
    "the boot records the venv it serves from" "$work/blastit.log"
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
    "$app/.venv/bin/python" -c 'import sysconfig; print(sysconfig.get_paths()["stdlib"])'
)
sudo chmod 0750 "$stdlib"
if sudo env \
    PORT=8081 \
    BLASTIT_APP=wsgi:application \
    BLASTIT_MANIFEST="$app/Blastfile" \
    BLASTIT_LOG_FORMAT=json \
    DATABASE_URL="postgresql://demo:not-a-real-password@db.invalid:5432/demo?sslmode=disable" \
    "$app/.venv/bin/blastit" >"$work/private.log" 2>&1; then
    echo "  FAIL a Python the view uids cannot read booted"
    status=1
else
    echo "  ok   a Python the view uids cannot read is refused"
fi
sudo chmod 0755 "$stdlib"
checked 'mode 0750) is not readable' \
    "the message says what is wrong with it" "$work/private.log"
# The directory the message names, compared by *real* path: a
# python-build-standalone venv records the unversioned prefix
# (`…/cpython-3.14-linux-x86_64-gnu`, a symlink) in `pyvenv.cfg`, so the boot
# names that one while `sysconfig` — the directory this leg made `0750` — reports
# the versioned `…/cpython-3.14.10-…`. They are the same directory; what matters
# is that the message names the one that was blocked.
named=$(
    sed -n 's/.*"line":"\([^"]*\) (mode 0750).*/\1/p' "$work/private.log" | head -1
)
if [ -n "$named" ] && [ "$(readlink -f "$named")" = "$(readlink -f "$stdlib")" ]; then
    echo "  ok   and names the directory that blocks ($named)"
else
    echo "  FAIL and names the directory that blocks (named ${named:-<none>}, blocked $stdlib)"
    status=1
fi
checked 'UV_PYTHON_INSTALL_DIR' \
    "and the fix that moves the interpreter" "$work/private.log"
echo "--- private.log"
cat "$work/private.log"

# E2b: the credentials-holding role's binary has to be executable by the uids
# blastit drops to. dbproxy runs as uid B+3, so a `_bin/` member `pip` installed
# as `0700` is a wheel that is dead on the platform while every other check
# stays green — the mode is what the wheel records (`0100755`, asserted by
# `scripts/check_wheel_shims.py` and `scripts/test_packaging_gates.py`).
#
# This leg measures the **packaged mode**: `-x` here is this uid's `access(2)` on
# a root-owned file, so it answers the "other" bit for the uid this shell runs as. It cannot
# show a boot refusal by itself — measured under `MB.49` with the
# `_bin/blastit-dbproxy` copy at `0700`: the boot still serves, because the child
# that runs the role is this image (`/proc/self/exe --role=dbproxy`), not that
# copy. The next leg drives the refusal through the seam the boot really reads.
if [ -x "$dbproxy" ]; then
    echo "  ok   the wheel's dbproxy is executable by uid $(id -u) ($(stat -c '%a %U:%G' "$dbproxy"))"
else
    echo "  FAIL the wheel's dbproxy is not executable by uid $(id -u) ($(stat -c '%a %U:%G' "$dbproxy"))"
    status=1
fi

# E2b, the boot half: the program a child `execve`s for the credentials role has
# to be executable by the uid that child runs as. `BLASTIT_EXEC` is that seam (the
# capability-free binary a child runs, D6/B2), so a root-owned `0700` stub named
# there is the shape a wheel under a private tree has — and the boot has to refuse,
# naming the file and the fix. This is the only witness for
# `stage0::fork_exec::fork_dbproxy`'s `unexecutable` call: deleting it leaves every
# other check above green.
stop_server "$server"

unexecutable=$work/dbproxy-unexecutable
printf '#!/bin/sh\nexit 0\n' | sudo tee "$unexecutable" >/dev/null
sudo chmod 0700 "$unexecutable"
extra_env=(BLASTIT_EXEC="$unexecutable")
if run_server >"$work/unexecutable.log" 2>&1; then
    echo "  FAIL a dbproxy another uid cannot execute booted"
    status=1
else
    echo "  ok   a dbproxy another uid cannot execute is refused"
fi
checked 'is not executable' "the message says it is not executable" "$work/unexecutable.log"
checked "$unexecutable" "and names the file that blocks" "$work/unexecutable.log"
checked 'chmod o+rx' "and the fix that opens it" "$work/unexecutable.log"
echo "--- unexecutable.log"
cat "$work/unexecutable.log"

# A dbproxy that dies before it reports ready has to fail the boot *and* say why
# (§8.1). The child that runs the credentials role is this image
# (`/proc/self/exe --role=dbproxy`), so the stub is handed over the supported
# seam — `BLASTIT_EXEC`, the capability-free binary the child `execve`s — rather
# than by replacing `blastit/_bin/blastit-dbproxy`: measured under `MB.49`, that
# file is not on this boot's exec path, so replacing it changed nothing and the
# boot simply served.
stub=$work/dbproxy-stub
printf '#!/bin/sh\necho "stub dbproxy says no"\nexit 3\n' | sudo tee "$stub" >/dev/null
sudo chmod 0755 "$stub"
extra_env=(BLASTIT_EXEC="$stub")
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
printf '#!/bin/sh\necho "stub dbproxy hangs"\nexec /bin/sleep 60\n' | sudo tee "$stub" >/dev/null
sudo chmod 0755 "$stub"
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

exit "$status"
