#!/usr/bin/env bash
# Installs one built wheel into a bare CPython image and proves it is the
# artifact the release claims (node MB.3.18, acceptance 1).
#
#   docker run --rm -v "$PWD/dist:/dist:ro" -v "$PWD:/src:ro" \
#       python:3.13-slim bash /src/scripts/wheel-smoke.sh /dist/blastit-*.whl
#
# Nothing else is installed: no Rust, no PHP, no uvicorn. What is proved:
#
#   1. `import blastit` and the extension load (`blastit --help`, which prints the
#      usage from the extension: `blastit` has no `--version` by design, see
#      `docs/reference/cli.md`), i.e. the
#      wheel matches this interpreter's ABI — the one thing a `cp3NN` tag
#      promises and a wrongly-built wheel breaks;
#   1b. the extension carries no test-only boot seam (`MB.3.67`): the wheel is the
#      release artifact a `pip install blastit` gets, so the installed
#      `_blastit*.so` is measured for `BLASTIT_TEST_SWAP_POINT` exactly as
#      `scripts/build-standalone.sh` measures the archive's binary. This is the
#      leg the published wheels go through (maturin-action builds them, not
#      `scripts/build-wheel.sh`), so without it the shipped wheel is the one
#      artifact nothing reads;
#   2. `blastit-dbproxy` is present and executable, because stage 0 `execve`s it
#      and a wheel without it refuses to boot (§2.2);
#   3. one static route is served (`200` with `Cache-Control: no-cache`) and an
#      unmatched path is Blastit's own `404` — with no WSGI route and no
#      interpreter in the manifest, so nothing here depends on Python *inside*
#      the app, only on the interpreter that loaded the extension.
#
# Usage: scripts/wheel-smoke.sh [--python PATH] WHEEL [WHEEL...]
#
# `--python` names the interpreter to install into, for a leg whose CPython is not
# the image's own: the musllinux legs build against a `uv python install` musl
# CPython inside an Alpine container, where there is no image to `docker run`.
set -euo pipefail

python=""
if [ "${1:-}" = "--python" ]; then
    python=$2
    shift 2
fi
if [ "$#" -eq 0 ]; then
    echo "usage: $0 [--python PATH] WHEEL [WHEEL...]" >&2
    exit 2
fi
if [ -z "$python" ]; then
    python=$(command -v python3 || command -v python)
fi
version=$("$python" -c 'import sys; print("%d.%d" % sys.version_info[:2])')
echo "wheel-smoke: CPython $version"

for wheel in "$@"; do
    case "$wheel" in
        *-cp"${version//./}"-*) ;;
        *) echo "error: $wheel is not a cp${version//./} wheel for CPython $version" >&2; exit 1 ;;
    esac
done

# `uv` when it is there (an interpreter `uv python install` fetched has no pip of
# its own), pip otherwise. Either way the wheel comes from `dist/`, never a
# cache: a rebuilt wheel at the same version must be the one that runs.
if command -v uv >/dev/null 2>&1; then
    # `--break-system-packages`: the interpreter on a bare leg *is* the target
    # (`uv python install` marks its own builds externally managed, and the
    # image's system CPython is the same idea), and this is a throwaway leg.
    uv pip install --no-cache --quiet --break-system-packages --python "$python" "$@" >/dev/null
else
    "$python" -m pip install --no-cache-dir --disable-pip-version-check "$@" >/dev/null
fi
echo "wheel-smoke: installed $*"

# 1. The extension loads.
"$python" -c 'import blastit; print("wheel-smoke: import blastit ok")'
# The console script lands beside the interpreter that installed it: a uv-managed
# CPython (the musl legs) does not put its `bin/` on `PATH`, and an image's own
# CPython does. Ask both.
blastit_bin="$(dirname "$python")/blastit"
if [ ! -x "$blastit_bin" ]; then
    blastit_bin=$(command -v blastit)
fi
# `blastit` has no `--version` (`docs/reference/cli.md`): `--help` exits 0 and
# the words it prints come from the installed extension, so this is the same
# proof — a wheel whose `_blastit` did not load cannot answer.
usage=$("$blastit_bin" --help)
printf '%s\n' "$usage" | grep -q "stage 0" || {
    echo "error: blastit --help did not print the usage an installed wheel answers with" >&2
    printf '%s\n' "$usage" >&2
    exit 1
}
echo "wheel-smoke: blastit --help ok"

# 1b. The installed extension carries no test-only boot seam (`MB.3.67`).
# `_blastit*.so` is a `--release` build, so `BLASTIT_TEST_SWAP_POINT` (compiled
# under `debug_assertions` alone) must not be in it: a shipped binary that can be
# paused by an environment variable before it binds a listener is a
# denial-of-service surface. Measured on the file that was installed, not on the
# flags the build was asked to use — `scripts/check-no-test-seam.sh` is the
# two-direction reader the archive's binary goes through as well.
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
site_dir=$("$python" - <<'PY'
import pathlib
import blastit

print(pathlib.Path(blastit.__file__).parent)
PY
)
extensions=0
for extension in "$site_dir"/_blastit*.so; do
    [ -e "$extension" ] || continue
    bash "$script_dir/check-no-test-seam.sh" "$extension"
    extensions=$((extensions + 1))
done
if [ "$extensions" -eq 0 ]; then
    echo "error: no _blastit*.so in $site_dir: the wheel's extension is not where" >&2
    echo "  this smoke test looks, so the boot seam went unmeasured" >&2
    exit 1
fi

# 2. dbproxy travels in the wheel and is executable.
dbproxy=$("$python" - <<'PY'
import pathlib
import blastit

print(pathlib.Path(blastit.__file__).parent / "_bin" / "blastit-dbproxy")
PY
)
if [ ! -x "$dbproxy" ]; then
    echo "error: $dbproxy is missing or not executable: the wheel refuses to boot without it" >&2
    exit 1
fi
echo "wheel-smoke: dbproxy executable at $dbproxy"

# 3. One static route, and a 404 that is Blastit's own.
work=$(mktemp -d "${TMPDIR:-/tmp}/blastit-wheel-smoke.XXXXXX")
pid=""
cleanup() {
    local rc=$?
    set +e
    if [ -n "$pid" ]; then kill -TERM "$pid" 2>/dev/null; wait "$pid" 2>/dev/null; fi
    rm -rf "$work"
    exit "$rc"
}
trap cleanup EXIT

mkdir -p "$work/site"
printf 'hello from the %s wheel\n' "$version" > "$work/site/index.html"
cat > "$work/Blastfile" <<'BLASTFILE'
static "/" {
    root "site"
}
BLASTFILE

BLASTIT_MANIFEST="$work/Blastfile" "$blastit_bin" dev --port 18700 \
    > "$work/dev.log" 2>&1 &
pid=$!
for _ in $(seq 1 100); do
    if "$python" -c '
import socket, sys
s = socket.socket()
s.settimeout(0.25)
try:
    s.connect(("127.0.0.1", 18700))
except OSError:
    sys.exit(1)
finally:
    s.close()
'; then
        break
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "error: blastit dev exited before it answered on 18700" >&2
        cat "$work/dev.log" >&2
        exit 1
    fi
    sleep 0.2
done

"$python" - "$work" <<'PY'
import sys
import urllib.error
import urllib.request

base = "http://127.0.0.1:18700"
problems = []

with urllib.request.urlopen(base + "/", timeout=5) as response:
    body = response.read().decode()
    cache = response.headers.get("Cache-Control", "")
    if response.status != 200:
        problems.append(f"/ wanted 200, got {response.status}")
    if "no-cache" not in cache:
        problems.append(f"/ wanted Cache-Control: no-cache, got {cache!r}")
    if "hello from the" not in body:
        problems.append(f"/ body is {body!r}")

# Blastit's own 404 (no route matches), not the server's.
try:
    urllib.request.urlopen(base + "/no-such-path", timeout=5)
    problems.append("/no-such-path answered 200")
except urllib.error.HTTPError as error:
    if error.code != 404:
        problems.append(f"/no-such-path wanted 404, got {error.code}")

if problems:
    for problem in problems:
        print(f"FAIL {problem}", file=sys.stderr)
    sys.exit(1)
print("wheel-smoke: 200 /, 404 /no-such-path")
PY

echo "wheel-smoke: $version wheel ok"
