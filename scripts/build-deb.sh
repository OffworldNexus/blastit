#!/usr/bin/env bash
# Packages a staged Blastit bundle as a Debian archive (`universal-distribution.md`
# §6.1).
#
#   make install PREFIX=/tmp/opencode/bundle
#   scripts/build-deb.sh --version 0.2.0 --bundle /tmp/opencode/bundle
#
# The input is a *bundle*: exactly the §6.1 tree `make install PREFIX=<dir>`
# stages, or the deterministic one `scripts/build-package-bundle.sh` builds from
# the floor images. Inside the archive §6.1's prefix is `/usr`:
#
#   /usr/bin/{blastit,blastit-lsp}            -> ../share/blastit/lib/...
#   /usr/share/blastit/LICENSE
#   /usr/share/blastit/lib/{blastit,blastit-lsp,libblastit_php*.so,libblastit_py*.so}
#   /usr/share/blastit/python/blastit/*.py
#
# The staged tree is asserted against §6.1 before anything is written:
# `scripts/check-bundle-layout.sh` requires 2 Python shims, 4 PHP shims (8.3–8.6),
# the 7 modules and no strays, so a bundle that lost a shim cannot become an
# archive whose smoke "passes" on a static route. `BUNDLE_SKIP_VERSION=1` because
# a `--arch arm64` archive is built on an x86_64 host; the container smoke runs
# the binary on the arch it installs.
#
# # The dependency floor is measured, and so are the libraries
#
# `scripts/check-portable-elf.sh --max-glibc` measures the maximum
# `GLIBC_X.Y` across **every** ELF the archive ships — not a constant someone
# typed — and prints the `DT_NEEDED` allowlist that became the dependency names.
# `Depends:` is built from those two facts: `libc6 (>= <measured>)`, plus
# `libgcc-s1` only when a shipped ELF actually needs `libgcc_s.so.1`. A host build
# that outgrew the ceiling fails instead of producing a higher floor, and pass
# `--max-glibc <shim_floor>` (from `scripts/platform_matrix.py`) to enforce it.
# Version needs the loader marks `WEAK` are advisory (glibc prints a note and
# runs), so they do not raise the floor.
#
# There is no PHP or Python dependency: libphp and libpython are `dlopen`ed when a
# view declares them, and §6.1's supported floor for those is a documentation
# statement, not a package relationship.
#
# Everything is staged under `$TMPDIR/opencode` (or `--out`), the archive is
# written with `dpkg-deb --root-owner-group`, and nothing is ever installed on the
# host: the proof is `e2e/run_packaging.py --leg deb` inside `debian:trixie`.
set -euo pipefail

# One timestamp for the ar members and the clamp, so two hosts produce the same
# bytes; a release caller passes the commit time.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch_base="${TMPDIR:-/tmp}/opencode"

version=""
release=""
arch=""
bundle=""
out="$scratch_base/blastit-packages"
max_glibc=""

usage() {
    cat >&2 <<'USAGE'
usage: scripts/build-deb.sh --version <ver> --bundle <dir>
                            [--release <rel>] [--arch amd64|arm64]
                            [--max-glibc <major>_<minor>] [--out <dir>]

  --bundle     a `make install PREFIX=<dir>` tree (bin/ + share/blastit/)
  --release    appended as `-<rel>` to the Debian version (optional)
  --arch       defaults to the machine type of the bundle's `blastit`
  --max-glibc  refuse a bundle whose measured floor is above this ceiling
  --out        where blastit_<ver>_<arch>.deb is written
USAGE
    exit 2
}

die() {
    echo "error: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || usage
            version=$2
            shift 2
            ;;
        --release)
            [ "$#" -ge 2 ] || usage
            release=$2
            shift 2
            ;;
        --arch)
            [ "$#" -ge 2 ] || usage
            arch=$2
            shift 2
            ;;
        --bundle)
            [ "$#" -ge 2 ] || usage
            bundle=$2
            shift 2
            ;;
        --out)
            [ "$#" -ge 2 ] || usage
            out=$2
            shift 2
            ;;
        --max-glibc)
            [ "$#" -ge 2 ] || usage
            max_glibc=$2
            shift 2
            ;;
        -h | --help) usage ;;
        *) usage ;;
    esac
done

[ -n "$version" ] || usage
[ -n "$bundle" ] || usage

command -v dpkg-deb >/dev/null 2>&1 ||
    die "dpkg-deb is required: apt-get install dpkg, or build inside a Debian container"
command -v readelf >/dev/null 2>&1 ||
    die "readelf is required to measure the floor: apt-get install binutils"

# A Debian version starts with a digit and uses `[A-Za-z0-9.+~]`; `--release` is
# the Debian revision. `+` and `~` are what nightly versions are made of
# (`0.2.0~nightly20261120.r12+gabc123`), and both sort below a release.
case "$version" in
    [0-9]*) ;;
    *) die "--version '$version' must start with a digit" ;;
esac
printf '%s' "$version" | grep -Eq '^[0-9][0-9A-Za-z.+~]*$' ||
    die "--version '$version' is not a Debian version (letters, digits, '.', '+', '~')"
if [ -n "$release" ]; then
    printf '%s' "$release" | grep -Eq '^[0-9][0-9A-Za-z.+~]*$' ||
        die "--release '$release' is not a Debian revision (letters, digits, '.', '+', '~')"
fi

lib="$bundle/share/blastit/lib"
share="$bundle/share/blastit"
[ -d "$share" ] || die "$share is not a staged bundle: run \`make install PREFIX=$bundle\` first"
for name in blastit blastit-lsp; do
    [ -f "$lib/$name" ] || die "$lib/$name is missing: run \`make install PREFIX=$bundle\` first"
    [ -x "$lib/$name" ] || die "$lib/$name is not executable"
    [ -L "$bundle/bin/$name" ] || die "$bundle/bin/$name is not a symlink (a bundle has two symlinks into share/blastit/lib)"
    # A bundle is self-contained: a link that resolves outside it would package a
    # path the archive does not contain.
    resolved=$(readlink -f "$bundle/bin/$name")
    case "$resolved" in
        "$(readlink -f "$lib")"/*) ;;
        *) die "$bundle/bin/$name -> $resolved, which is outside $lib" ;;
    esac
done
[ -f "$share/LICENSE" ] || die "$share/LICENSE is missing: run \`make install PREFIX=$bundle\` first"
[ -f "$share/python/blastit/__init__.py" ] ||
    die "$share/python/blastit/__init__.py is missing: run \`make install PREFIX=$bundle\` first"

# The architecture is read off the artifact, not off the host: packaging an
# aarch64 bundle from an x86_64 laptop must not produce an `amd64` archive.
detect_arch() {
    local machine
    machine=$(readelf -h "$1" 2>/dev/null | sed -n 's/^ *Machine: *//p' | head -n 1)
    case "$machine" in
        "Advanced Micro Devices X86-64") echo amd64 ;;
        "AArch64") echo arm64 ;;
        *) echo "" ;;
    esac
}

detected=$(detect_arch "$lib/blastit")
case "$arch" in
    "") arch=$detected ;;
    amd64 | x86_64 | x86-64)
        arch=amd64
        [ -z "$detected" ] || [ "$detected" = amd64 ] ||
            die "--arch amd64 given for a bundle whose blastit is $detected"
        ;;
    arm64 | aarch64)
        arch=arm64
        [ -z "$detected" ] || [ "$detected" = arm64 ] ||
            die "--arch arm64 given for a bundle whose blastit is $detected"
        ;;
    *) die "--arch '$arch' is not amd64 or arm64" ;;
esac
[ -n "$arch" ] || die "could not read the machine type of $lib/blastit: pass --arch amd64|arm64"

mkdir -p "$scratch_base" "$out" || die "cannot create $scratch_base or $out"
stage=$(mktemp -d "$scratch_base/blastit-deb.XXXXXX")
trap 'rm -rf "$stage"' EXIT

# --- §6.1's tree under /usr ---------------------------------------------------
install -d "$stage/usr/bin" "$stage/usr/share/blastit/lib" \
    "$stage/usr/share/blastit/python/blastit" "$stage/DEBIAN"
install -m 0755 "$lib/blastit" "$lib/blastit-lsp" "$stage/usr/share/blastit/lib/"
shims=0
for shim in "$lib"/libblastit_*.so; do
    [ -e "$shim" ] || continue
    install -m 0755 "$shim" "$stage/usr/share/blastit/lib/"
    shims=$((shims + 1))
done
install -m 0644 "$share/LICENSE" "$stage/usr/share/blastit/LICENSE"
modules=0
for module in "$share"/python/blastit/*.py; do
    [ -e "$module" ] || continue
    install -m 0644 "$module" "$stage/usr/share/blastit/python/blastit/"
    modules=$((modules + 1))
done
ln -s ../share/blastit/lib/blastit "$stage/usr/bin/blastit"
ln -s ../share/blastit/lib/blastit-lsp "$stage/usr/bin/blastit-lsp"
# `mktemp -d` creates 0700; the archive's root entry is the package root, not a
# directory the operator should inherit.
chmod 0755 "$stage"

# --- the payload is §6.1's, or this is not a Blastit package ------------------
# Counted here, not by the caller: a bundle with zero shims used to become an
# archive that installed, served a static file and printed "check clean".
layout=$(BUNDLE_REQUIRE_PY_SHIMS=2 BUNDLE_REQUIRE_PHP_SHIMS=4 BUNDLE_SKIP_VERSION=1 \
    "$root/scripts/check-bundle-layout.sh" "$stage/usr")
printf '%s\n' "$layout" | sed 's/^/build-deb: layout: /'

# --- the measured C-library floor --------------------------------------------
artifacts=("$stage/usr/share/blastit/lib/blastit" "$stage/usr/share/blastit/lib/blastit-lsp")
for shim in "$stage/usr/share/blastit/lib"/libblastit_*.so; do
    [ -e "$shim" ] || continue
    artifacts+=("$shim")
done
ceiling=auto
[ -z "$max_glibc" ] || ceiling=$max_glibc
report=$("$root/scripts/check-portable-elf.sh" --max-glibc "$ceiling" "${artifacts[@]}")
floor=$(printf '%s\n' "$report" |
    sed -n 's/^max-glibc: \([0-9][0-9]*\(\.[0-9][0-9]*\)\+\) (.*/\1/p')
[ -n "$floor" ] ||
    die "no shipped ELF names a GLIBC_ symbol, so the package floor is unknown: refusing to write an unmeasured Depends"
floor_from=$(printf '%s\n' "$report" |
    sed -n 's/^max-glibc: .* (\(.*\))$/\1/p')
# The dependency names come from the measured `DT_NEEDED` set, not from a list
# someone typed: `libc6` because a shipped ELF needs `libc.so.6`, `libgcc-s1`
# only when one needs `libgcc_s.so.1` (`libm` and the loader are in libc6).
needed=$(printf '%s\n' "$report" | sed -n 's/^needed: //p')
case " $needed " in
    *" libc.so.6 "*) ;;
    *)
        die "no shipped ELF needs libc.so.6 (needed: ${needed:-none}): this is not a glibc bundle"
        ;;
esac
depends="libc6 (>= $floor)"
case " $needed " in
    *" libgcc_s.so.1 "*) depends="$depends, libgcc-s1" ;;
esac
echo "build-deb: glibc floor $floor (measured from $floor_from)"
echo "build-deb: needed $needed -> Depends: $depends"

# --- control metadata ---------------------------------------------------------
deb_version="$version"
[ -z "$release" ] || deb_version="$version-$release"
installed_kb=$(du -sk "$stage/usr" | cut -f1)

cat >"$stage/DEBIAN/control" <<CONTROL
Package: blastit
Version: $deb_version
Architecture: $arch
Maintainer: Offworld Nexus <maintainers@offworld.nexus>
Installed-Size: $installed_kb
Depends: $depends
Section: admin
Priority: optional
Homepage: https://github.com/offworld-nexus/blastit
Description: zero-trust application harness and sandbox
 Blastit runs 12-factor web applications as isolated per-route sandboxes,
 validates every inbound byte, and keeps credentials behind protocol-aware
 capability proxies. This archive ships the interpreter-free server, the
 language shims it loads at run time, and the pure-Python helper package.
CONTROL

# `dpkg-deb --build` only fills `md5sums` in when it is absent; write it ourselves
# from the staged files so the archive's own integrity list is deterministic.
(cd "$stage" && find usr -type f -print0 | sort -z | xargs -r -0 md5sum >DEBIAN/md5sums)

deb="$out/blastit_${deb_version}_${arch}.deb"
rm -f "$deb"
dpkg-deb --root-owner-group --build "$stage" "$deb" >/dev/null

depends=$(dpkg-deb --field "$deb" Depends)
size=$(wc -c <"$deb")
echo "build-deb: $deb ($size bytes, $shims shim(s), $modules python module(s))"
echo "build-deb: Depends: $depends"
