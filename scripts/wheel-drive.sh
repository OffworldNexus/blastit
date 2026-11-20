#!/usr/bin/env bash
# Drives the commands an installed wheel must answer, beyond the serving proof
# `scripts/wheel-smoke.sh` makes (node MB.42.3.3, acceptance 1).
#
#   docker run --rm -v "$PWD/dist:/dist:ro" -v "$PWD:/src:ro" python:3.13-slim \
#       bash -c 'pip install --no-cache-dir /dist/blastit-*-py3-none-*.whl pytest \
#                && bash /src/scripts/wheel-drive.sh'
#
# What each step proves, and why it is here rather than in `wheel-smoke.sh`:
#
#   1. `blastit --version` — the console script reaches the repacked binary in
#      `blastit/_bin/`; a wheel that lost the repack dies with the "incomplete"
#      sentence instead.
#   2. `blastit-manifest fold` — the third console script design §6.1 names,
#      reading a `sql.denied` event and the `Blastfile` it must patch. This is
#      the only command that exercises `_blastit.py`'s NDJSON bridge to
#      `blastit --internal-json`, so a wheel whose bridge cannot spawn the binary
#      fails here and nowhere else.
#   3. `blastit-lsp` — **a real framed session**, not `--help`: the language
#      server takes no arguments by design (`ADR-0017`, §3.4: it reads frames
#      from stdin/stdout and exits 0 only after `shutdown`), so `--help` would
#      start a session against EOF. `scripts/lsp_smoke.py` drives
#      `initialize → didOpen → shutdown → exit` and requires `0` stderr bytes,
#      which is the proof the console script reaches the binary at all.
#   4. `blastit check` — the audit gate on a tiny store-less, 0-proxy project: a
#      static route and one passing pytest test. It proves the repacked layout's
#      `blastit/_bin` is the root `check` finds `blastit/_check_pytest.py` from
#      (the `bundle_root` fix in `crates/blastit/src/check.rs`), with no proxy to
#      talk to and no grant to match.
#
# Usage: scripts/wheel-drive.sh [--python PATH] [--repo DIR]
set -euo pipefail

python=""
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
while [ "$#" -gt 0 ]; do
    case "$1" in
        --python) python=${2:?--python requires an interpreter}; shift 2 ;;
        --repo) repo=${2:?--repo requires a directory}; shift 2 ;;
        *) echo "usage: $0 [--python PATH] [--repo DIR]" >&2; exit 2 ;;
    esac
done
if [ -z "$python" ]; then
    python=$(command -v python3 || command -v python)
fi
version=$("$python" -c 'import sys; print("%d.%d" % sys.version_info[:2])')
bindir="$(dirname "$python")"
resolve() {
    if [ -x "$bindir/$1" ]; then
        printf '%s' "$bindir/$1"
    else
        command -v "$1"
    fi
}
blastit_bin=$(resolve blastit)
manifest_bin=$(resolve blastit-manifest)
lsp_bin=$(resolve blastit-lsp)
echo "wheel-drive: CPython $version, wheel commands in $(dirname "$blastit_bin")"

work=$(mktemp -d "${TMPDIR:-/tmp}/blastit-wheel-drive.XXXXXX")
trap 'rm -rf "$work"' EXIT

# 1. The version line, from the binary the console script found.
"$blastit_bin" --version | grep -q '^blastit ' || {
    echo "error: blastit --version printed no version line" >&2
    exit 1
}
echo "wheel-drive: $("$blastit_bin" --version)"

# 2. `blastit-manifest fold`: one denial in, the grant it is missing out.
cat > "$work/Blastfile" <<'BLASTFILE'
postgres $DATABASE_URL

view notes "/notes/*" {
    postgres {
        query 'SELECT 1'
    }
}
BLASTFILE
printf '%s\n' \
    '{"event":"sql.denied","route":"notes","proxy":"db","sql":"SELECT id FROM notes WHERE id = $1","reason":"no grant"}' \
    > "$work/app.log"
fold_output=$("$manifest_bin" fold "$work/app.log" --manifest "$work/Blastfile")
printf '%s\n' "$fold_output" | grep -q 'notes' || {
    echo "error: blastit-manifest fold proposed nothing for a denied statement" >&2
    printf '%s\n' "$fold_output" >&2
    exit 1
}
echo "wheel-drive: blastit-manifest fold proposed $(printf '%s\n' "$fold_output" | grep -c 'select \|query ' || true) grant line(s)"

# 3. `blastit-lsp`: a real session, zero stderr bytes.
"$python" "$repo/scripts/lsp_smoke.py" --binary "$lsp_bin" \
    > "$work/lsp.log" 2>&1 || {
    echo "error: the language server console script did not complete a session" >&2
    tail -n 20 "$work/lsp.log" >&2
    exit 1
}
echo "wheel-drive: blastit-lsp session $(tail -n 1 "$work/lsp.log")"

# 4. `blastit check` on a store-less, 0-proxy project.
mkdir -p "$work/app/site" "$work/app/tests"
printf 'hello from the wheel\n' > "$work/app/site/index.html"
cat > "$work/app/Blastfile" <<'BLASTFILE'
static "/" {
    root "site"
}
BLASTFILE
cat > "$work/app/tests/test_smoke.py" <<'PYTEST'
def test_the_project_runs():
    assert True
PYTEST
if ! "$python" -m pytest --version >/dev/null 2>&1; then
    if command -v uv >/dev/null 2>&1; then
        uv pip install --quiet --break-system-packages pytest
    else
        "$python" -m pip install --quiet --disable-pip-version-check pytest
    fi
fi
check_output=$(cd "$work/app" && "$blastit_bin" check 2>&1) || {
    echo "error: blastit check did not accept a store-less 0-proxy project" >&2
    printf '%s\n' "$check_output" >&2
    exit 1
}
echo "wheel-drive: blastit check $(printf '%s\n' "$check_output" | grep -Ei 'request|ok' | tail -n 1)"
echo "wheel-drive: CPython $version wheel ok (version, fold, lsp, check)"
