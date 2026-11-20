#!/usr/bin/env bash
# Proves a *locally built* Blastit package by installing it in a throwaway
# container and running what an operator runs (`MB.42.3.1`).
#
#   BLASTIT_DEB=/tmp/opencode/blastit-packages/blastit_0.2.0_amd64.deb \
#   BLASTIT_RPM=/tmp/opencode/blastit-packages/blastit-0.2.0-1.x86_64.rpm \
#       scripts/test_packages.sh
#
# Each leg installs the **package file** (`dpkg -i` / `rpm -Uvh --replacepkgs`) in a
# fresh container and then:
#
#   1. enumerates the installed payload against §6.1 — `dpkg -L` or `rpm -ql` must
#      show the 2 symlinks, `LICENSE`, 8 libraries (2 binaries, 4 PHP shims, 2
#      Python shims) and 7 modules, and nothing else under `/usr/share/blastit`.
#      An archive with zero shims used to pass a static-only smoke; this is the
#      assertion that makes "the package" mean §6.1's package.
#   2. asserts ownership: every `dpkg-deb -c` entry `root/root`, every RPM
#      `%{FILEUSERNAME}:%{FILEGROUPNAME}` `root:root`.
#   3. runs `blastit --version`.
#   4. runs `blastit check` and `blastit dev` (HTTP 200 + the body) in one of two
#      modes:
#
#        static — `debian:trixie` (2.41), `debian:bookworm-slim` (2.36, the
#                 oldest Debian the release claims) **and** `rockylinux:9` (2.34).
#                 Proves the binary, the archive layout and static serving. It
#                 loads no shim and the gate short-circuits (`R14`), so the line
#                 says exactly that.
#        wsgi   — `debian:trixie` only. `entrypoint python "hello:application"`:
#                 `blastit check` imports the app through the package's own
#                 `python/blastit` (no `PYTHONPATH`, `env -i`), and `blastit dev`
#                 serves it through the shipped `libblastit_py313` shim — which
#                 is what the measured glibc floor and the whole
#                 `python/blastit` tree are about.
#
# The three static images are the ones the declared floor is a promise to: a
# package whose measured need is above 2.34 has no business passing the RPM leg,
# and one above 2.36 cannot be called Debian-12-supported. `debian:trixie` is the
# non-`-slim` image because the WSGI leg installs `python3-pytest` +
# `libpython3.13` from Debian (the only network use in this script — never a
# publish); the bookworm and RPM legs run with `--network none`.
#
# Rocky 9 is static-only on purpose: it ships CPython 3.9, and no shim supports a
# minor below 3.13, so a WSGI leg there would be testing an interpreter the
# product never claims. The `debian:trixie` image is the non-`-slim` one because
# the WSGI leg installs `python3-pytest` + `libpython3.13` from Debian (the only
# network use in this script — never a publish), while the RPM leg runs with
# `--network none`.#
# The package is never installed on the host; `make check` does not run this
# (`packages-test` needs Docker and a package build).
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch_base="${TMPDIR:-/tmp}/opencode"

deb="${BLASTIT_DEB:-}"
rpm="${BLASTIT_RPM:-}"
deb_image="${BLASTIT_DEB_IMAGE:-debian:trixie}"
rpm_image="${BLASTIT_RPM_IMAGE:-rockylinux:9}"
static_fixture="${BLASTIT_PACKAGE_FIXTURE:-$root/e2e/packages/fixture}"
wsgi_fixture="${BLASTIT_PACKAGE_WSGI_FIXTURE:-$root/e2e/packages/fixture-wsgi}"
port="${BLASTIT_PACKAGE_TEST_PORT:-8099}"

# Which phases run in each image (`MB.42.3.4`). `scripts/install_matrix.sh` passes
# the leg's own `modes` from `scripts/platform_matrix.py --leg install`, so the
# matrix — not this script — decides whether an image is asked to import a WSGI
# app; the defaults are the `make packages-test` survey, where the trixie leg is
# the only image that ships the CPython the packaged shim binds to.
deb_modes="${BLASTIT_PACKAGE_MODES:-install static wsgi}"
rpm_modes="${BLASTIT_PACKAGE_MODES:-install static}"

usage() {
    cat >&2 <<'USAGE'
usage: scripts/test_packages.sh [--deb PATH] [--rpm PATH]

Install a locally built package in a clean container, enumerate its payload
against §6.1, and run `blastit --version`, `blastit check` and `blastit dev`.

  BLASTIT_DEB   path to blastit_<ver>_<arch>.deb (debian:trixie; static + WSGI)
  BLASTIT_RPM   path to blastit-<ver>-<rel>.<arch>.rpm (rockylinux:9; static)
  BLASTIT_DEB_IMAGE / BLASTIT_RPM_IMAGE   override the container images
  BLASTIT_PACKAGE_MODES                    phases to run there (default:
                                           `install static wsgi` for a .deb,
                                           `install static` for a .rpm; the
                                           install matrix passes the leg's own)
  BLASTIT_PACKAGE_TEST_PORT                loopback port the dev leg binds (8099)

At least one of --deb/--rpm (or BLASTIT_DEB/BLASTIT_RPM) is required.
USAGE
    exit 2
}

die() {
    echo "error: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --deb) [ "$#" -ge 2 ] || usage; deb=$2; shift 2 ;;
        --rpm) [ "$#" -ge 2 ] || usage; rpm=$2; shift 2 ;;
        -h | --help) usage ;;
        *) usage ;;
    esac
done

[ -n "$deb" ] || [ -n "$rpm" ] || usage
[ -z "$deb" ] || [ -f "$deb" ] || die "$deb does not exist: build it with scripts/build-deb.sh"
[ -z "$rpm" ] || [ -f "$rpm" ] || die "$rpm does not exist: build it with scripts/build-rpm.sh"
command -v docker >/dev/null 2>&1 || die "docker is required: the package is installed in a container, never on the host"
[ -d "$static_fixture" ] || die "$static_fixture is missing"
[ -d "$wsgi_fixture" ] || die "$wsgi_fixture is missing (the WSGI leg's app)"

mkdir -p "$scratch_base"
scriptdir=$(mktemp -d "$scratch_base/blastit-package-probe.XXXXXX")
trap 'rm -rf "$scriptdir"' EXIT

# The probe runs *inside* the container. `bash` is what both images ship, and
# `/dev/tcp` is bash's own HTTP client — the point is to prove the installed
# package, not to prove a distro's curl is present.
cat >"$scriptdir/probe.sh" <<'PROBE'
set -eu
kind=$1
pkg=$2
port=$3
mode=$4

fail() {
    echo "package-probe: $kind FAIL $*" >&2
    exit 1
}

installed_files() {
    if [ "$kind" = deb ]; then
        dpkg -L blastit
    else
        rpm -ql blastit
    fi | while IFS= read -r path; do
        [ -f "$path" ] || [ -L "$path" ] || continue
        printf '%s\n' "$path"
    done
}

assert_payload() {
    files=$(installed_files)
    require() {
        printf '%s\n' "$files" | grep -qx "$1" ||
            fail "missing $1 (§6.1's installed tree)"
    }
    for path in \
        /usr/bin/blastit \
        /usr/bin/blastit-lsp \
        /usr/share/blastit/LICENSE \
        /usr/share/blastit/lib/blastit \
        /usr/share/blastit/lib/blastit-lsp \
        /usr/share/blastit/python/blastit/__init__.py \
        /usr/share/blastit/python/blastit/__main__.py \
        /usr/share/blastit/python/blastit/_blastit.py \
        /usr/share/blastit/python/blastit/_check_pytest.py; do
        require "$path"
    done
    # The 8 libraries, by the names `ADR-0010` §4.1 fixes: 4 PHP minors and the 2
    # CPython shims the WSGI capability ships.
    for minor in 83 84 85 86; do
        count=$(printf '%s\n' "$files" |
            grep -cE "^/usr/share/blastit/lib/libblastit_php${minor}-[a-z0-9_]+\-(gnu|musl)\.so$" || true)
        [ "$count" -eq 1 ] || fail "$count PHP $minor shim(s) installed, §6.1 ships exactly 1"
    done
    for minor in 313 314; do
        count=$(printf '%s\n' "$files" |
            grep -cE "^/usr/share/blastit/lib/libblastit_py${minor}-[a-z0-9_]+\-(gnu|musl)\.so$" || true)
        [ "$count" -eq 1 ] || fail "$count Python $minor shim(s) installed, §6.1 ships exactly 1"
    done
    total=$(printf '%s\n' "$files" | grep -c '^/usr/share/blastit/' || true)
    [ "$total" -eq 13 ] ||
        fail "$total paths under /usr/share/blastit, §6.1 ships 13 (LICENSE, 8 libs, 4 modules)"
    [ "$(readlink /usr/bin/blastit)" = ../share/blastit/lib/blastit ] ||
        fail "/usr/bin/blastit does not point at ../share/blastit/lib/blastit"
    [ "$(readlink /usr/bin/blastit-lsp)" = ../share/blastit/lib/blastit-lsp ] ||
        fail "/usr/bin/blastit-lsp does not point at ../share/blastit/lib/blastit-lsp"
    echo "package-probe: $kind payload: 2 entry points, $total paths under share/blastit (§6.1)"
}

assert_ownership() {
    if [ "$kind" = deb ]; then
        bad=$(dpkg-deb -c "$pkg" | awk '{print $2}' | grep -vx 'root/root' || true)
    else
        bad=$(rpm -qp --qf '[%{FILEUSERNAME}:%{FILEGROUPNAME} %{FILENAMES}\n]' "$pkg" |
            awk '$1 != "root:root" {print}' || true)
    fi
    [ -z "$bad" ] || fail "payload entries not owned root: $bad"
    echo "package-probe: $kind ownership: every payload entry root:root"
}

serve_and_get() {
    directory=$1
    expected=$2
    cd "$directory"
    blastit dev --port "$port" --no-reload >/work/dev.log 2>&1 &
    dev=$!
    ready=0
    i=0
    while [ "$i" -lt 100 ]; do
        if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
            ready=1
            break
        fi
        i=$((i + 1))
        sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
        cat /work/dev.log >&2
        fail "blastit dev never bound 127.0.0.1:$port"
    fi
    exec 3<>"/dev/tcp/127.0.0.1/$port"
    printf 'GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n' >&3
    # One reader for the whole response: `head -n 1` would drain the body into its
    # own buffer. The request asked for `Connection: close`, so `cat` ends at EOF.
    response=$(timeout 15 cat <&3 || true)
    exec 3<&-
    status=$(printf '%s\n' "$response" | sed -n '1s/\r$//p')
    kill "$dev" 2>/dev/null || true
    wait "$dev" 2>/dev/null || true
    case "$status" in
        *" 200 "*) ;;
        *)
            echo "GET / returned '$status'" >&2
            cat /work/dev.log >&2
            fail "dev did not answer 200"
            ;;
    esac
    case "$response" in
        *"$expected"*) ;;
        *)
            echo "GET / did not serve the fixture: $(printf '%s' "$response" | head -c 400)" >&2
            fail "dev served another body"
            ;;
    esac
    echo "package-probe: $kind dev: $status, '$expected'"
}

mkdir -p /work

case "$mode" in
    install)
        if [ "$kind" = deb ]; then
            dpkg -i "$pkg"
        else
            rpm -Uvh --replacepkgs "$pkg"
        fi
        command -v blastit >/dev/null 2>&1 || fail "blastit is not on PATH after installing $pkg"
        version=$(blastit --version)
        case "$version" in
            blastit\ *) ;;
            *) fail "blastit --version printed '$version'" ;;
        esac
        echo "package-probe: $kind installed: $version"
        assert_payload
        assert_ownership
        ;;
    static)
        mkdir -p /work/static
        cp -a /fixture/. /work/static/
        cd /work/static
        blastit check
        echo "package-probe: $kind check: all-static (no gate, no shim)"
        serve_and_get /work/static "blastit package fixture"
        ;;
    wsgi)
        mkdir -p /work/wsgi
        cp -a /fixture-wsgi/. /work/wsgi/
        cd /work/wsgi
        # No `PYTHONPATH`: the gate must import the app through the *package's*
        # `python/blastit` (`<exe_dir>/../python`), which is `MB.42.11`'s fix.
        env -i PATH=/usr/local/bin:/usr/bin:/bin HOME=/root TMPDIR=/tmp blastit check
        echo "package-probe: $kind check: WSGI gate clean (bundle python, no PYTHONPATH)"
        serve_and_get /work/wsgi "hello from the packaged shim"
        ;;
    *)
        fail "unknown mode '$mode' (install|static|wsgi)"
        ;;
esac
PROBE

# The two legs differ in what else they need: the WSGI leg installs Debian's
# `python3-pytest` and `libpython3.13` (the interpreter and its shared library the
# packaged shim `dlopen`s), so it has network; the RPM leg is static-only and runs
# with `--network none`.
if [ -n "$deb" ]; then
    echo "package-test: deb — installing $(basename "$deb") in $deb_image ($deb_modes)"
    docker run --rm --network bridge \
        --env DEBIAN_FRONTEND=noninteractive \
        --volume "$(dirname "$deb"):/pkg:ro" \
        --volume "$static_fixture:/fixture:ro" \
        --volume "$wsgi_fixture:/fixture-wsgi:ro" \
        --volume "$scriptdir:/probe:ro" \
        --tmpfs /work:exec \
        "$deb_image" \
        bash -euc '
            # Only a leg whose matrix row carries `wsgi` installs the interpreter
            # and its shared library; the other five distributions ship no CPython
            # 3.13/3.14 and are not asked to import anything through a shim.
            case " $3 " in
                *" wsgi "*)
                    apt-get update -qq >/dev/null
                    apt-get install -y -qq --no-install-recommends python3-pytest libpython3.13 >/dev/null
                    ;;
            esac
            for mode in $3; do
                bash /probe/probe.sh deb "/pkg/$1" "$2" "$mode"
            done
        ' _ "$(basename "$deb")" "$port" "$deb_modes"
fi

# The default survey installs the same `.deb` in Debian 12 as well, because 2.36
# is the oldest glibc the release claims. An explicit `BLASTIT_DEB_IMAGE` means
# exactly that image (an install-matrix leg), so the second leg only runs when the
# caller did not name one.
if [ -n "$deb" ] && [ -z "${BLASTIT_DEB_IMAGE:-}" ]; then
    bookworm_image="${BLASTIT_DEB_BOOKWORM_IMAGE:-debian:bookworm-slim}"
    echo "package-test: deb — installing $(basename "$deb") in $bookworm_image (static, network none: Debian 12 is the oldest supported glibc)"
    docker run --rm --network none \
        --volume "$(dirname "$deb"):/pkg:ro" \
        --volume "$static_fixture:/fixture:ro" \
        --volume "$wsgi_fixture:/fixture-wsgi:ro" \
        --volume "$scriptdir:/probe:ro" \
        --tmpfs /work:exec \
        "$bookworm_image" \
        bash -euc '
            bash /probe/probe.sh deb "/pkg/$1" "$2" install
            bash /probe/probe.sh deb "/pkg/$1" "$2" static
        ' _ "$(basename "$deb")" "$port"
fi

if [ -n "$rpm" ]; then
    echo "package-test: rpm — installing $(basename "$rpm") in $rpm_image ($rpm_modes, network none)"
    docker run --rm --network none \
        --volume "$(dirname "$rpm"):/pkg:ro" \
        --volume "$static_fixture:/fixture:ro" \
        --volume "$wsgi_fixture:/fixture-wsgi:ro" \
        --volume "$scriptdir:/probe:ro" \
        --tmpfs /work:exec \
        "$rpm_image" \
        bash -euc '
            for mode in $3; do
                bash /probe/probe.sh rpm "/pkg/$1" "$2" "$mode"
            done
        ' _ "$(basename "$rpm")" "$port" "$rpm_modes"
fi

echo "package-test: ok"
