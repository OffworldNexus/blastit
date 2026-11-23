#!/usr/bin/env bash
# Runs a built `blastit dev` against a static fixture on a host with **no
# CPython**, and proves what the universal artifact promises there (`MB.17`,
# `ADR-0017` decision 1): `200` with `Cache-Control: no-cache`, `404` for a path
# no route matches, and — because no interpreter exists on this host — a WSGI
# manifest refused at boot, exit 1, by stage-0 discovery rather than by a build
# that lacks the capability.
#
# `readelf` (scripts/check-portable-elf.sh) proves the artifact does not *link*
# an interpreter. This script is the execution proof: CI runs it inside
# `debian:trixie-slim`, which ships no Python at all, so a build that served only
# because a `libpython` happened to be present fails here instead of on a user's
# server.
#
# It uses bash's `/dev/tcp` and no `curl`: the image is deliberately bare.
#
# Usage: scripts/standalone-runtime-check.sh [--require-no-python] [BINARY]
#   BINARY defaults to $BLASTIT_BIN, then target/debug/blastit.
set -euo pipefail

require_no_python=0
if [ "${1:-}" = "--require-no-python" ]; then
    require_no_python=1
    shift
fi
binary="${1:-${BLASTIT_BIN:-target/debug/blastit}}"

if [ ! -x "$binary" ]; then
    echo "error: $binary is not an executable: build it first (cargo build -p blastit --locked)" >&2
    exit 1
fi
if [ "$require_no_python" -eq 1 ]; then
    # The guard exists so the closing "3/3 passed on a host with no CPython" is
    # not a lie about the host: with a CPython present the WSGI half below would
    # refuse with `NoShim` (no CPython shim beside this binary), which is a
    # different sentence and would read as a mystery failure. Everything `dev`
    # discovery reads is refused here, not only `PATH`
    # (`crates/blastit/src/stage0/project_python.rs`).
    # The bare names plus any versioned interpreter, found by globbing each PATH
    # entry for a `python[0-9]*` executable: the check spells no minor, so adding
    # or removing one in `packaging/languages.tsv` cannot make it pass by accident.
    # `*-config` are the build helpers, never an interpreter.
    path_dirs=()
    IFS=':' read -r -a path_dirs <<<"${PATH:-}"
    candidates=(python python3)
    for dir in "${path_dirs[@]}"; do
        for found in "$dir"/python[0-9]*; do
            [ -f "$found" ] && [ -x "$found" ] || continue
            case "${found##*/}" in
                *-config) continue ;;
            esac
            candidates+=("$found")
        done
    done
    for candidate in "${candidates[@]}"; do
        if command -v "$candidate" >/dev/null 2>&1; then
            echo "error: $candidate is on PATH ($(command -v "$candidate")), so this run would not" >&2
            echo "  prove the standalone artifact works without an interpreter" >&2
            exit 1
        fi
    done
    for variable in BLASTIT_PYTHON VIRTUAL_ENV BLASTIT_WHEEL_PYTHON BLASTIT_PY_SHIM_DIR; do
        if [ -n "${!variable:-}" ]; then
            echo "error: $variable is set (${!variable}), so interpreter discovery would" >&2
            echo "  read it; unset it, or run this where no CPython is reachable at all" >&2
            exit 1
        fi
    done
    echo "host: no CPython on PATH, no BLASTIT_PYTHON/VIRTUAL_ENV/BLASTIT_WHEEL_PYTHON, no BLASTIT_PY_SHIM_DIR"
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/blastit-standalone.XXXXXX")"
log="$work/dev.log"
pid=""
port=""

cleanup() {
    local rc=$?
    set +e
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null
        for _ in $(seq 1 50); do
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.1
        done
        kill -KILL "$pid" 2>/dev/null
        wait "$pid" 2>/dev/null
    fi
    rm -rf "$work"
    exit "$rc"
}
trap cleanup EXIT

mkdir -p "$work/site"
printf '<!doctype html><title>standalone</title>hello from the standalone binary\n' > "$work/site/index.html"
cat > "$work/Blastfile" <<'BLASTFILE'
static "/" {
    root "site"
}
BLASTFILE

# Never assume a port is free: probe upwards from a high base with bash's own
# TCP connect, the same way `make docs-serve-check` does.
for candidate in $(seq 18400 18450); do
    if ! (exec 3<>"/dev/tcp/127.0.0.1/$candidate") 2>/dev/null; then
        port="$candidate"
        break
    fi
done
if [ -z "$port" ]; then
    echo "error: no free port in 18400..18450" >&2
    exit 1
fi

echo "standalone-runtime-check: $binary dev --port $port"
# The check must drive the binary it was handed, and nothing else.
#
# `env -u BLASTIT_BIN` removes the one variable that would point elsewhere. The
# *auto-discovered* candidates (`.venv/bin/blastit`, `vendor/bin/blastit`,
# `node_modules/.bin/blastit`) are still live, though, and this script never
# `cd`s: a run from a checkout would hand `dev` off to `.venv/bin/blastit` and
# the checks would be about a binary that was never started.
# `BLASTIT_NO_LOCAL_HANDOFF=1` is the documented switch that turns the whole
# handoff off, so what answers here is the artifact CI built (`MB.49`).
env -u BLASTIT_BIN BLASTIT_NO_LOCAL_HANDOFF=1 BLASTIT_MANIFEST="$work/Blastfile" "$binary" dev --port "$port" > "$log" 2>&1 &
pid=$!

waited=0
while ! (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; do
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "error: blastit dev exited before 127.0.0.1:$port answered" >&2
        cat "$log" >&2
        exit 1
    fi
    waited=$((waited + 1))
    if [ "$waited" -ge 80 ]; then
        echo "error: 127.0.0.1:$port did not answer within 20 s" >&2
        cat "$log" >&2
        exit 1
    fi
    sleep 0.25
done

request() {
    local path="$1"
    exec 3<>"/dev/tcp/127.0.0.1/$port"
    printf 'GET %s HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n' "$path" >&3
    cat <&3
    exec 3<&-
    exec 3>&-
}

status_of() {
    printf '%s\n' "$1" | tr -d '\r' | awk 'NR == 1 { print $2 }'
}

header_of() {
    printf '%s\n' "$2" | tr -d '\r' | tr 'A-Z' 'a-z' \
        | awk -v name="$1" 'index($0, name ":") == 1 { sub(/^[^:]*:[[:space:]]*/, ""); print; exit }'
}

fail=0

body="$(request /)"
status="$(status_of "$body")"
cache="$(header_of cache-control "$body")"
if [ "$status" = "200" ] && [ "$cache" = "no-cache" ]; then
    echo "ok   200  /                      Cache-Control: $cache"
else
    echo "FAIL / wanted 200 + Cache-Control: no-cache, got ${status:-<none>} + ${cache:-<none>}" >&2
    printf '%s\n' "$body" >&2
    fail=1
fi

body="$(request /mb17-does-not-exist)"
status="$(status_of "$body")"
if [ "$status" = "404" ]; then
    echo "ok   404  /mb17-does-not-exist"
else
    echo "FAIL /mb17-does-not-exist wanted 404, got ${status:-<none>}" >&2
    printf '%s\n' "$body" >&2
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo "--- $log (tail) ---" >&2
    tail -n 30 "$log" >&2
    exit 1
fi
echo "ok   2/2 static responses without touching an interpreter"

# The second half of the promise (`ADR-0017` decision 1): the artifact carries
# `python`, so the refusal is no longer a build gate — it is stage-0 interpreter
# discovery, and it must still fail closed. Exit 1, before a listener binds, with
# the one sentence that says how to get an interpreter
# (`blastit-py-sapi`'s `DiscoveryError::NoInterpreter`).
#
# `BLASTIT_APP` is exported so the boot gets past `app_rule` ("the WSGI callable
# is required") and actually reaches discovery: without an app the refusal would
# be about the manifest and prove nothing about the interpreter.
mkdir -p "$work/wsgi"
cat > "$work/wsgi/Blastfile" <<'BLASTFILE'
view web regex '.*' {
}
BLASTFILE

wsgi_port=""
for candidate in $(seq 18460 18500); do
    if ! (exec 3<>"/dev/tcp/127.0.0.1/$candidate") 2>/dev/null; then
        wsgi_port="$candidate"
        break
    fi
done
if [ -z "$wsgi_port" ]; then
    echo "error: no free port in 18460..18500" >&2
    exit 1
fi

if [ "$require_no_python" -eq 1 ]; then
    echo "standalone-runtime-check: $binary dev --port $wsgi_port (no CPython on this host)"
else
    echo "standalone-runtime-check: $binary dev --port $wsgi_port"
fi
# In the background, not in the foreground: a build that *did* serve a WSGI route
# on a host with no interpreter is the leak this proves absent, and a blocked
# foreground call would look like a hung check instead of a failure.
# `BLASTIT_NO_LOCAL_HANDOFF=1` for the same reason as the first launch: the
# artifact this run was handed is the only program allowed to answer.
env -u BLASTIT_BIN BLASTIT_NO_LOCAL_HANDOFF=1 BLASTIT_MANIFEST="$work/wsgi/Blastfile" BLASTIT_APP=app:application \
    "$binary" dev --port "$wsgi_port" > "$work/wsgi.out" 2> "$work/wsgi.err" &
wsgi_pid=$!

set +e
waited=0
while kill -0 "$wsgi_pid" 2>/dev/null; do
    if [ "$waited" -ge 80 ]; then
        echo "FAIL a WSGI manifest stayed up for 20 s on a host with no CPython" >&2
        kill -KILL "$wsgi_pid" 2>/dev/null
        wait "$wsgi_pid" 2>/dev/null
        tail -n 20 "$work/wsgi.out" "$work/wsgi.err" >&2
        exit 1
    fi
    sleep 0.25
    waited=$((waited + 1))
done
wait "$wsgi_pid"
wsgi_rc=$?
set -e

if [ "$wsgi_rc" -ne 1 ]; then
    echo "FAIL a WSGI manifest wanted exit 1 from interpreter discovery, got $wsgi_rc" >&2
    tail -n 20 "$work/wsgi.out" "$work/wsgi.err" >&2
    exit 1
fi
if ! grep -Eq 'no CPython .* interpreter was found' "$work/wsgi.err" "$work/wsgi.out"; then
    echo "FAIL the WSGI refusal does not name the missing interpreter:" >&2
    echo "  (run this where no CPython is reachable — CI uses debian:trixie-slim)" >&2
    tail -n 20 "$work/wsgi.out" "$work/wsgi.err" >&2
    exit 1
fi
if ! grep -Eq 'run uv python install [0-9]+\.[0-9]+' "$work/wsgi.err" "$work/wsgi.out"; then
    echo "FAIL the WSGI refusal does not say how to get an interpreter:" >&2
    tail -n 20 "$work/wsgi.out" "$work/wsgi.err" >&2
    exit 1
fi
echo "ok   exit 1 before binding: no CPython (discovery names the 'uv python install <minor>' step)"
if [ "$require_no_python" -eq 1 ]; then
    echo "standalone-runtime-check: 3/3 passed on a host with no CPython"
else
    echo "standalone-runtime-check: 3/3 passed"
fi
