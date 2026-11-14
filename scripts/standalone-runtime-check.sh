#!/usr/bin/env bash
# Runs a built `blastit dev` against a static fixture on a host with **no
# CPython** and proves the three responses the standalone artifact promises
# (`MB.17`, `ADR-0010` §4): `200` with `Cache-Control: no-cache`, `404` for a
# path no route matches, and a boot that never needed an interpreter.
#
# `readelf` (scripts/check-portable-elf.sh) proves the artifact does not *link*
# an interpreter. This script is the execution proof: CI runs it inside
# `debian:trixie-slim`, which ships no Python at all, so a manifest that worked
# only because a `libpython` happened to be present fails here instead of on a
# user's server.
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
    # Without this the script would pass on a host where a CPython is installed,
    # which is exactly the false green `MB.17` exists to remove.
    for candidate in python python3 python3.13 python3.14; do
        if command -v "$candidate" >/dev/null 2>&1; then
            echo "error: $candidate is on PATH ($(command -v "$candidate")), so this run would not" >&2
            echo "  prove the standalone artifact works without an interpreter" >&2
            exit 1
        fi
    done
    echo "host: no CPython on PATH"
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
cat > "$work/blastit.toml" <<'TOML'
version = 1

[[route]]
name = "site"
path = [".*"]

[route.static]
root = "site"
TOML

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
BLASTIT_MANIFEST="$work/blastit.toml" "$binary" dev --port "$port" > "$log" 2>&1 &
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
echo "standalone-runtime-check: 2/2 passed without touching an interpreter"
