#!/usr/bin/env bash
# Installs one built wheel into a bare CPython image and proves it is the
# artifact the release claims (nodes MB.3.18, MB.42.3.3).
#
#   docker run --rm -v "$PWD/dist:/dist:ro" -v "$PWD:/src:ro" \
#       python:3.13-slim bash /src/scripts/wheel-smoke.sh /dist/blastit-*.whl
#
# Nothing else is installed: no Rust, no PHP, no uvicorn. What is proved:
#
#   1. the wheel installs into *this* interpreter at all — `py3-none-*` is the
#      promise that one artifact serves CPython 3.12, 3.13 and 3.14, so the same
#      file is installed in each of those images and answers from each;
#   1a. `blastit --version` answers, and `--help` prints the usage the binary
#      ships (`blastit` became a `py3-none` wheel in `MB.42.3.3`, so the version
#      line is what proves the console script reaches the repacked binary);
#   1b. the two console scripts design §6.1 names exist and are executable:
#      `blastit` and `blastit-lsp` (which had no entry point at all before the
#      repack) — a `bindings = "bin"` wheel shipped none;
#   1c. the embedded `blastit/_bin/blastit` carries no test-only boot seam
#      (`MB.3.67`): the wheel is the release artifact a `pip install blastit`
#      gets, so the file `pip` installed is measured for
#      `BLASTIT_TEST_SWAP_POINT` exactly as `scripts/build-standalone.sh`
#      measures the archive's binary;
#   2. `blastit/_bin/` holds exactly one program: `_bin/blastit` is it, it is
#      executable, and no second one travels beside it — the credentials-holding
#      role is that same file as `--role=dbproxy`, so a wheel that also ships a
#      second binary is an image nobody builds (§2.2, §4);
#   3. one static route is served (`200` with `Cache-Control: no-cache`) and an
#      unmatched path is Blastit's own `404` — with no WSGI route and no
#      interpreter in the manifest, so nothing here depends on Python *inside*
#      the app, only on the interpreter that loaded the package.
#
# `scripts/wheel-drive.sh` is the companion that drives `blastit check` and
# `blastit-lsp`; this script stops at the serving proof so the CI legs that only
# need "the wheel works here" stay short.
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

# A `py3-none` wheel installs anywhere CPython 3 does; a `cp3NN` wheel (a
# developer build straight out of `maturin`, before the repack) is only valid for
# its own minor. Both are accepted, and nothing else is: `pip` would refuse the
# install, and this script must refuse it first, with a sentence.
for wheel in "$@"; do
    case "$wheel" in
        *-py3-none-*) ;;
        *-cp"${version//./}"-*) ;;
        *)
            echo "error: $wheel is neither a py3-none wheel nor a cp${version//./} wheel for CPython $version" >&2
            exit 1
            ;;
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

# 1. The package imports and its binary answers.
"$python" -c 'import blastit; print("wheel-smoke: import blastit ok")'
# The console scripts land beside the interpreter that installed them: a uv-managed
# CPython (the musl legs) does not put its `bin/` on `PATH`, and an image's own
# CPython does. Ask both.
bindir="$(dirname "$python")"
for name in blastit blastit-lsp; do
    if [ ! -x "$bindir/$name" ] && ! command -v "$name" >/dev/null 2>&1; then
        echo "error: the wheel installed no \`$name\` console script beside $python" >&2
        echo "  (design §6.1: blastit, blastit-lsp)" >&2
        exit 1
    fi
done
blastit_bin="$bindir/blastit"
if [ ! -x "$blastit_bin" ]; then
    blastit_bin=$(command -v blastit)
fi
echo "wheel-smoke: console scripts on PATH: blastit, blastit-lsp"

# 1a. `--version` and `--help` come from the binary the repack put in
# `blastit/_bin/`; a wheel whose console script cannot find it dies here.
version_line=$("$blastit_bin" --version)
printf '%s\n' "$version_line" | grep -q '^blastit ' || {
    echo "error: blastit --version printed no version line" >&2
    printf '%s\n' "$version_line" >&2
    exit 1
}
echo "wheel-smoke: $version_line"
usage=$("$blastit_bin" --help)
printf '%s\n' "$usage" | grep -q "stage 0" || {
    echo "error: blastit --help did not print the usage an installed wheel answers with" >&2
    printf '%s\n' "$usage" >&2
    exit 1
}
echo "wheel-smoke: blastit --help ok"

# 1b/1c. The installed binary carries no test-only boot seam (`MB.3.67`) and is
# executable — `pip` drops the wheel's modes, so the repack's 0755 is what makes
# this true, and a `_bin/blastit` installed 0644 would be a wheel that cannot run.
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
binary=$("$python" - <<'PY'
import pathlib
import blastit

print(pathlib.Path(blastit.__file__).parent / "_bin" / "blastit")
PY
)
if [ ! -x "$binary" ]; then
    echo "error: $binary is missing or not executable: the wheel's binary did not" >&2
    echo "  travel at blastit/_bin/blastit with mode 0755" >&2
    exit 1
fi
bash "$script_dir/check-no-test-seam.sh" "$binary"
echo "wheel-smoke: $binary carries no test seam"

# 2. `_bin/` holds exactly the wheel's payload: the one program `blastit` (every
# role, the credentials-holding one as `--role=dbproxy`), `blastit-lsp` and the
# shims. An exact allow-list, not "no `blastit-dbproxy`": a third program a
# staged `_bin/` carried would otherwise install unnoticed.
bin_dir=$("$python" - <<'PY'
import pathlib
import blastit

print(pathlib.Path(blastit.__file__).parent / "_bin")
PY
)
strays=$("$python" - "$bin_dir" <<'PY'
import pathlib
import sys

allowed = {"blastit", "blastit-lsp"}
strays = [
    path.name
    for path in sorted(pathlib.Path(sys.argv[1]).iterdir())
    if path.name not in allowed
    and not (
        path.name.startswith(("libblastit_py", "libblastit_php"))
        and path.name.endswith(".so")
    )
]
print(", ".join(strays))
PY
)
if [ -n "$strays" ]; then
    echo "error: $bin_dir holds $strays: one ELF runs every role, so \`_bin/\` is" >&2
    echo "  \`blastit\` (--role=dbproxy included), \`blastit-lsp\` and the shims" >&2
    echo "  (\`libblastit_py*.so\`, \`libblastit_php*.so\`) and nothing else" >&2
    exit 1
fi
echo "wheel-smoke: $binary is the wheel's only program"

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
