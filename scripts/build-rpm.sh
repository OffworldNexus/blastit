#!/usr/bin/env bash
# Packages a staged Blastit bundle as an RPM (`universal-distribution.md` §6.1).
#
#   make install PREFIX=/tmp/opencode/bundle
#   scripts/build-rpm.sh --version 0.2.0 --release 1 --bundle /tmp/opencode/bundle
#
# The layout is `scripts/build-deb.sh`'s, restated for RPM:
#
#   /usr/bin/{blastit,blastit-lsp}            -> ../share/blastit/lib/...
#   /usr/share/blastit/{LICENSE,lib/...,python/blastit/*.py}
#
# `scripts/rpm/blastit.spec` is checked in and only moves the staged tree into the
# buildroot. `rpmbuild` runs on the host when it exists; otherwise the same spec is
# built inside a `rockylinux:9` container (the builder distribution, not the
# supported floor). The RPM's payload is never installed on the host — the proof is
# `scripts/test_packages.sh` inside a container.
#
# The staged tree is asserted against §6.1 before anything is written
# (`scripts/check-bundle-layout.sh`: 2 Python shims, 4 PHP shims, the 7 modules, no
# strays), and `BUNDLE_SKIP_VERSION=1` lets a cross-arch `--arch arm64` archive be
# built on an x86_64 host.
#
# # The dependency floor is measured, and so are the libraries
#
# `Requires: glibc >= <n>` comes from the maximum `GLIBC_X.Y` across every ELF in
# `share/blastit/lib/`, measured by `scripts/check-portable-elf.sh --max-glibc`,
# which also prints the `DT_NEEDED` allowlist the requirement names come from:
# `glibc`, plus `libgcc` only when a shipped ELF needs `libgcc_s.so.1`. `--max-glibc`
# enforces a ceiling (the declared `shim_floor`), so a bundle built for another
# distribution fails instead of shipping a floor nobody checked. `WEAK` version
# needs (glibc's own advisory references) do not raise the floor, because the
# loader does not require them.
#
# No PHP or Python requirement is declared: `libphp` and `libpython` are `dlopen`ed
# by a view that asks for them, so `glibc` and `libgcc` are the whole runtime
# contract.
set -euo pipefail

# One timestamp for the payload, so two hosts produce the same bytes; a release
# caller passes the commit time.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch_base="${TMPDIR:-/tmp}/opencode"
spec="$root/scripts/rpm/blastit.spec"

version=""
release="1"
arch=""
bundle=""
out="$scratch_base/blastit-packages"
max_glibc=""
builder_image="${BLASTIT_RPM_BUILDER_IMAGE:-rockylinux:9}"

usage() {
    cat >&2 <<'USAGE'
usage: scripts/build-rpm.sh --version <ver> --bundle <dir>
                            [--release <rel>] [--arch amd64|arm64]
                            [--max-glibc <major>_<minor>] [--out <dir>]

  --bundle     a `make install PREFIX=<dir>` tree (bin/ + share/blastit/)
  --release    RPM release, default 1 (nightly: 0.nightly.<date>.r<n>.g<sha>)
  --arch       defaults to the machine type of the bundle's `blastit`
  --max-glibc  refuse a bundle whose measured floor is above this ceiling
  --out        where blastit-<ver>-<rel>.<arch>.rpm is written

Set BLASTIT_RPM_BUILDER_IMAGE to use another builder (default rockylinux:9) when
`rpmbuild` is not installed on the host.
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

command -v readelf >/dev/null 2>&1 ||
    die "readelf is required to measure the floor: dnf install binutils, or build inside the container"

# An RPM `Version` cannot contain `-` (`Release` owns that separator), so the
# nightly stamp lives in `--release` as `0.nightly.<date>.r<n>.g<sha>`.
printf '%s' "$version" | grep -Eq '^[0-9][0-9A-Za-z.+~]*$' ||
    die "--version '$version' is not an RPM version (letters, digits, '.', '+', '~')"
[ -n "$release" ] || die "--release must not be empty"
printf '%s' "$release" | grep -Eq '^[0-9][0-9A-Za-z.+~_]*$' ||
    die "--release '$release' is not an RPM release (letters, digits, '.', '+', '~', '_')"

share="$bundle/share/blastit"
lib="$share/lib"
[ -d "$share" ] || die "$share is not a staged bundle: run \`make install PREFIX=$bundle\` first"
for name in blastit blastit-lsp; do
    [ -f "$lib/$name" ] || die "$lib/$name is missing: run \`make install PREFIX=$bundle\` first"
    [ -x "$lib/$name" ] || die "$lib/$name is not executable"
    [ -L "$bundle/bin/$name" ] || die "$bundle/bin/$name is not a symlink (a bundle has two symlinks into share/blastit/lib)"
    resolved=$(readlink -f "$bundle/bin/$name")
    case "$resolved" in
        "$(readlink -f "$lib")"/*) ;;
        *) die "$bundle/bin/$name -> $resolved, which is outside $lib" ;;
    esac
done
[ -f "$share/LICENSE" ] || die "$share/LICENSE is missing: run \`make install PREFIX=$bundle\` first"
[ -f "$share/python/blastit/__init__.py" ] ||
    die "$share/python/blastit/__init__.py is missing: run \`make install PREFIX=$bundle\` first"

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
case "$arch" in
    amd64) rpm_arch=x86_64 ;;
    arm64) rpm_arch=aarch64 ;;
esac

mkdir -p "$scratch_base" "$out" || die "cannot create $scratch_base or $out"
stage=$(mktemp -d "$scratch_base/blastit-rpm.XXXXXX")
topdir=$(mktemp -d "$scratch_base/blastit-rpmbuild.XXXXXX")
trap 'rm -rf "$stage" "$topdir"' EXIT

# --- §6.1's tree, as the spec's `/usr` payload --------------------------------
install -d "$stage/usr/bin" "$stage/usr/share/blastit/lib" \
    "$stage/usr/share/blastit/python/blastit"
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

# --- the payload is §6.1's, or this is not a Blastit package ------------------
layout=$(BUNDLE_REQUIRE_PY_SHIMS=2 BUNDLE_REQUIRE_PHP_SHIMS=4 BUNDLE_SKIP_VERSION=1 \
    "$root/scripts/check-bundle-layout.sh" "$stage/usr")
printf '%s\n' "$layout" | sed 's/^/build-rpm: layout: /'

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
    die "no shipped ELF names a GLIBC_ symbol, so the package floor is unknown: refusing to write an unmeasured Requires"
floor_from=$(printf '%s\n' "$report" |
    sed -n 's/^max-glibc: .* (\(.*\))$/\1/p')
# The requirement names come from the measured `DT_NEEDED` set: `glibc` because a
# shipped ELF needs `libc.so.6`, `libgcc` only when one needs `libgcc_s.so.1`.
needed=$(printf '%s\n' "$report" | sed -n 's/^needed: //p')
case " $needed " in
    *" libc.so.6 "*) ;;
    *) die "no shipped ELF needs libc.so.6 (needed: ${needed:-none}): this is not a glibc bundle" ;;
esac
libgcc=""
case " $needed " in
    *" libgcc_s.so.1 "*) libgcc="1" ;;
esac
echo "build-rpm: glibc floor $floor (measured from $floor_from)"
echo "build-rpm: needed $needed -> Requires: glibc >= $floor${libgcc:+, libgcc}"

build() {
    # Built in a scratch topdir, so a rerun cannot reuse an earlier buildroot.
    mkdir -p "$1/BUILD" "$1/BUILDROOT" "$1/RPMS" "$1/SOURCES" "$1/SPECS" "$1/SRPMS"
    shift
    rpmbuild -bb "$spec" "$@"
}

built=""
if command -v rpmbuild >/dev/null 2>&1; then
    echo "build-rpm: rpmbuild on this host, building $spec"
    build "$topdir" \
        --define "_topdir $topdir" \
        --define "blastit_version $version" \
        --define "blastit_release $release" \
        --define "blastit_glibc $floor" \
        --define "blastit_libgcc $libgcc" \
        --define "blastit_bundle $stage" \
        --target "$rpm_arch"
    built=$(find "$topdir/RPMS" -name '*.rpm' -type f | head -n 1)
else
    command -v docker >/dev/null 2>&1 ||
        die "rpmbuild is not installed and docker is not available: install rpm-build (dnf install rpm-build) or docker"
    echo "build-rpm: no rpmbuild on this host, building $spec inside $builder_image"
    # The container gets the staged tree and the spec read-only and writes only the
    # finished RPM into `--out`: nothing is installed on the host, and the build
    # distribution never becomes the package's floor.
    docker run --rm \
        --env "SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH" \
        --volume "$stage:/bundle:ro" \
        --volume "$root/scripts/rpm:/spec:ro" \
        --volume "$out:/out" \
        "$builder_image" \
        bash -euc '
            if ! command -v rpmbuild >/dev/null 2>&1; then
                dnf -y -q install rpm-build >/dev/null
            fi
            top=/work/rpmbuild
            mkdir -p "$top"/BUILD "$top"/BUILDROOT "$top"/RPMS "$top"/SOURCES "$top"/SPECS "$top"/SRPMS
            rpmbuild -bb /spec/blastit.spec \
                --define "_topdir $top" \
                --define "blastit_version $1" \
                --define "blastit_release $2" \
                --define "blastit_glibc $3" \
                --define "blastit_libgcc $4" \
                --define "blastit_bundle /bundle" \
                --target "$5"
            pkg=$(find "$top/RPMS" -name "*.rpm" -type f | head -n 1)
            cp "$pkg" /out/
            rpm -qp --requires "$pkg" | sed "s/^/container-requires: /"
        ' _ "$version" "$release" "$floor" "$libgcc" "$rpm_arch"
    built=$(find "$out" -maxdepth 1 -name "blastit-$version-$release.$rpm_arch.rpm" -type f | head -n 1)
fi

[ -n "$built" ] && [ -f "$built" ] ||
    die "rpmbuild produced no blastit-$version-$release.$rpm_arch.rpm"

rpm="$out/blastit-$version-$release.$rpm_arch.rpm"
if [ "$(readlink -f "$built")" != "$(readlink -f "$rpm")" ]; then
    mv -f "$built" "$rpm"
fi
size=$(wc -c <"$rpm")
echo "build-rpm: $rpm ($size bytes, $shims shim(s), $modules python module(s))"
echo "build-rpm: Requires: glibc >= $floor${libgcc:+, libgcc}"
if command -v rpm >/dev/null 2>&1; then
    rpm -qp --requires "$rpm" | sed 's/^/build-rpm: requires: /'
fi
