#!/usr/bin/env bash
# Packages a staged Blastit bundle as seven RPMs (`universal-distribution.md`
# §6.1, `MB.65`).
#
#   make install PREFIX=/tmp/opencode/bundle
#   scripts/build-rpm.sh --version 0.2.1 --release 1 --bundle /tmp/opencode/bundle
#
# The layout is `scripts/build-deb.sh`'s, restated for RPM, and the split is
# named once in `packaging/package-set.sh` (this script *reads* it):
#
#   blastit               /usr/bin/{blastit,blastit-lsp}, LICENSE, python/blastit/*.py
#   blastit-python-3.13   /usr/share/blastit/lib/libblastit_py313-<arch>-gnu.so
#   blastit-python-3.14   /usr/share/blastit/lib/libblastit_py314-<arch>-gnu.so
#   blastit-php-8.3 …8.6  /usr/share/blastit/lib/libblastit_php8{3,4,5,6}-<arch>-gnu.so
#
# `scripts/rpm/blastit.spec` is checked in and only moves the staged tree into the
# buildroot. `rpmbuild` runs on the host when it exists; otherwise the same spec is
# built inside a `rockylinux:9` container (the builder distribution, not the
# supported floor). The RPMs' payload is never installed on the host — the proof
# is `e2e/run_packaging.py --leg rpm` inside a container.
#
# The staged tree is asserted against §6.1 before anything is written
# (`scripts/check-bundle-layout.sh`: 2 Python shims, 4 PHP shims, the 7 modules, no
# strays), every shim must be owned by exactly one package, and
# `BUNDLE_SKIP_VERSION=1` lets a cross-arch `--arch arm64` set be built on an
# x86_64 host.
#
# # The dependency floor is measured per package, never as a bundle maximum
#
# `scripts/check-portable-elf.sh --max-glibc` is run for `blastit`+`blastit-lsp`
# (the core) and then for each shim on its own, exactly as
# `scripts/build-deb.sh` does: the core gets `glibc >= <core floor>` (2.34 on
# today's artifacts) and `blastit-php-8.4` gets `glibc >= <its own floor>` (2.17),
# so no package demands a newer glibc than its own bytes need. Each measurement
# also says whether that package's `DT_NEEDED` set carries `libgcc_s.so.1`; only
# those packages declare `libgcc`. The measurement is handed to the spec as
# `blastit_glibc_<tag>`/`blastit_libgcc_<tag>` defines.
# `--max-glibc` enforces a ceiling (the declared `shim_floor`), so a bundle built
# for another distribution fails instead of shipping a floor nobody checked.
# `WEAK` version needs (glibc's own advisory references) do not raise the floor,
# because the loader does not require them.
#
# The interpreter relationship is `Recommends: python3.13` / `Suggests:
# php-embedded` rather than a hard `Requires:`, because Rocky 9 has no
# `python3.13` and its `php-embedded` is 8.0: a shim package that cannot be
# installed on a supported distribution is worse than a visible advisory (see the
# spec's comment).
set -euo pipefail

# One timestamp for the payload, so two hosts produce the same bytes; a release
# caller passes the commit time.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch_base="${TMPDIR:-/tmp}/opencode"
spec="$root/scripts/rpm/blastit.spec"
# shellcheck source=../packaging/package-set.sh
source "$root/packaging/package-set.sh"

version=""
release="1"
arch=""
bundle=""
out="$scratch_base/blastit-packages"
max_glibc=""
builder_image="${BLASTIT_RPM_BUILDER_IMAGE:-rockylinux:9}"

usage_text() {
    cat <<'USAGE'
usage: scripts/build-rpm.sh --version <ver> --bundle <dir>
                            [--release <rel>] [--arch amd64|arm64]
                            [--max-glibc <major>_<minor>] [--out <dir>]

  --bundle     a `make install PREFIX=<dir>` tree (bin/ + share/blastit/)
  --release    RPM release, default 1 (nightly: 0.nightly.<date>.r<n>.g<sha>)
  --arch       defaults to the machine type of the bundle's `blastit`
  --max-glibc  refuse a set whose measured floor is above this ceiling
  --out        where the seven RPMs are written:
                 blastit-<ver>-<rel>.<arch>.rpm
                 blastit-python-3.1{3,4}-<ver>-<rel>.<arch>.rpm
                 blastit-php-8.{3,4,5,6}-<ver>-<rel>.<arch>.rpm

Each package's `Requires: glibc >= …` is measured from its own ELFs, so the
core and a PHP shim do not have to claim the same floor.

Set BLASTIT_RPM_BUILDER_IMAGE to use another builder (default rockylinux:9) when
`rpmbuild` is not installed on the host.
USAGE
}

# An explicit `--help` is success; a bad argument is a usage error (`2`).
usage() {
    if [ "${1:-2}" = 0 ]; then
        usage_text
    else
        usage_text >&2
    fi
    exit "${1:-2}"
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
        -h | --help) usage 0 ;;
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

# --- the split is only honest if every shim has exactly one owner ------------
package_names=()
while IFS= read -r name; do package_names+=("$name"); done < <(blastit_package_names)
[ "${#package_names[@]}" -eq 7 ] ||
    die "packaging/package-set.sh names ${#package_names[@]} packages, this build writes 7"

while IFS=$'\t' read -r owner glob; do
    matches=()
    for candidate in "$lib"/$glob; do
        [ -e "$candidate" ] || continue
        matches+=("$candidate")
    done
    [ "${#matches[@]}" -eq 1 ] ||
        die "$lib/$glob matches ${#matches[@]} file(s); $owner ships exactly one shim, so build the leg or remove the stray"
done < <(blastit_package_shims)

for shim in "$lib"/libblastit_*.so; do
    [ -e "$shim" ] || continue
    basename=$(basename "$shim")
    owners=$(blastit_package_owners "$basename" || true)
    if [ -z "$owners" ]; then
        die "$lib/$basename is owned by no package: add it to packaging/package-set.sh or remove it from the bundle"
    fi
    count=$(printf '%s\n' "$owners" | wc -l)
    [ "$count" -eq 1 ] ||
        die "$lib/$basename is claimed by $count packages ($(printf '%s' "$owners" | tr '\n' ' ')): one shim belongs to one package"
done

mkdir -p "$scratch_base" "$out" || die "cannot create $scratch_base or $out"
# `docker -v` reads a relative source as a volume name: resolve `--out` once so a
# relative directory behaves exactly like an absolute one.
out=$(cd "$out" && pwd -P) || die "cannot resolve $out to an absolute path"
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

# --- one measured floor per package -----------------------------------------
ceiling=auto
[ -z "$max_glibc" ] || ceiling=$max_glibc
measured_floor=""
measured_needed=""

floor_of() {
    # Measures `$@` and leaves `measured_floor`/`measured_needed` set.
    local report
    report=$("$root/scripts/check-portable-elf.sh" --max-glibc "$ceiling" "$@")
    measured_floor=$(printf '%s\n' "$report" |
        sed -n 's/^max-glibc: \([0-9][0-9]*\(\.[0-9][0-9]*\)\+\) (.*/\1/p')
    [ -n "$measured_floor" ] ||
        die "no shipped ELF names a GLIBC_ symbol, so the package floor is unknown: refusing to write an unmeasured Requires"
    measured_needed=$(printf '%s\n' "$report" | sed -n 's/^needed: //p')
    case " $measured_needed " in
        *" libc.so.6 "*) ;;
        *) die "no shipped ELF needs libc.so.6 (needed: ${measured_needed:-none}): this is not a glibc bundle" ;;
    esac
}

#: `1` when the last `floor_of` call's `DT_NEEDED` set carries libgcc.
libgcc_of() {
    case " $measured_needed " in
        *" libgcc_s.so.1 "*) echo 1 ;;
        *) echo 0 ;;
    esac
}

defines=()
floor_of "$stage/usr/share/blastit/lib/blastit" "$stage/usr/share/blastit/lib/blastit-lsp"
echo "build-rpm: blastit: measured glibc floor $measured_floor, libgcc $(libgcc_of), needed: $measured_needed"
defines+=("blastit_glibc_core $measured_floor" "blastit_libgcc_core $(libgcc_of)")
core_floor=$measured_floor

while IFS=$'\t' read -r owner glob; do
    shim=""
    for candidate in "$stage/usr/share/blastit/lib/"$glob; do
        [ -e "$candidate" ] || continue
        shim=$candidate
    done
    [ -n "$shim" ] || die "$owner owns no shim in the staged tree ($glob)"
    floor_of "$shim"
    case "$owner" in
        blastit-python-*)
            tag="py${owner##blastit-python-}"
            tag=${tag//./}
            ;;
        blastit-php-*)
            tag="php${owner##blastit-php-}"
            tag=${tag//./}
            ;;
        *) die "$owner is a shim package whose floor define this builder does not know" ;;
    esac
    echo "build-rpm: $owner: measured glibc floor $measured_floor, libgcc $(libgcc_of), needed: $measured_needed"
    defines+=("blastit_glibc_$tag $measured_floor" "blastit_libgcc_$tag $(libgcc_of)")
done < <(blastit_package_shims)

define_args=()
for define in "${defines[@]}"; do define_args+=(--define "$define"); done
echo "build-rpm: core Requires: glibc >= $core_floor (every shim declares its own measured floor)"

# A same-version rerun must not be able to satisfy the completeness loop with a
# previous run's file: remove every name this run promises before building.
for name in "${package_names[@]}"; do
    rm -f "$out/$name-$version-$release.$rpm_arch.rpm"
done

if command -v rpmbuild >/dev/null 2>&1; then
    echo "build-rpm: rpmbuild on this host, building $spec"
    mkdir -p "$topdir/BUILD" "$topdir/BUILDROOT" "$topdir/RPMS" "$topdir/SOURCES" \
        "$topdir/SPECS" "$topdir/SRPMS"
    rpmbuild -bb "$spec" \
        --define "_topdir $topdir" \
        --define "blastit_version $version" \
        --define "blastit_release $release" \
        --define "blastit_bundle $stage" \
        "${define_args[@]}" \
        --target "$rpm_arch"
    # Every subpackage, not just the first find(1) prints: the set is the point.
    while IFS= read -r rpm; do
        [ -n "$rpm" ] || continue
        mv -f "$rpm" "$out/"
    done < <(find "$topdir/RPMS" -name '*.rpm' -type f)
else
    command -v docker >/dev/null 2>&1 ||
        die "rpmbuild is not installed and docker is not available: install rpm-build (dnf install rpm-build) or docker"
    echo "build-rpm: no rpmbuild on this host, building $spec inside $builder_image"
    # The container gets the staged tree and the spec read-only and writes only the
    # finished RPMs into `--out`: nothing is installed on the host, and the build
    # distribution never becomes the package's floor.
    defines_text=$(printf '%s\n' "${defines[@]}")
    docker run --rm \
        --env "SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH" \
        --env "HOST_UID=$(id -u)" \
        --env "HOST_GID=$(id -g)" \
        --env "BLASTIT_RPM_DEFINES=$defines_text" \
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
            args=()
            while IFS= read -r line; do
                [ -n "$line" ] || continue
                args+=(--define "$line")
            done <<< "$BLASTIT_RPM_DEFINES"
            rpmbuild -bb /spec/blastit.spec \
                --define "_topdir $top" \
                --define "blastit_version $1" \
                --define "blastit_release $2" \
                --define "blastit_bundle /bundle" \
                "${args[@]}" \
                --target "$3"
            copied=()
            for pkg in "$top"/RPMS/*/*.rpm; do
                [ -e "$pkg" ] || continue
                cp "$pkg" /out/
                copied+=("/out/$(basename "$pkg")")
                rpm -qp --requires "$pkg" | sed "s/^/container-requires: /"
            done
            # The build ran as root; only the files this run copied belong to the
            # caller who owns --out (a stale RPM in that directory is not ours).
            [ "${#copied[@]}" -eq 0 ] ||
                chown "$HOST_UID:$HOST_GID" "${copied[@]}"
        ' _ "$version" "$release" "$rpm_arch"
fi

# --- the whole set, by name ---------------------------------------------------
missing=0
for name in "${package_names[@]}"; do
    rpm="$out/$name-$version-$release.$rpm_arch.rpm"
    if [ ! -f "$rpm" ]; then
        echo "error: rpmbuild produced no $name-$version-$release.$rpm_arch.rpm" >&2
        missing=$((missing + 1))
        continue
    fi
    echo "build-rpm: $rpm ($(wc -c <"$rpm") bytes)"
done
[ "$missing" -eq 0 ] || die "$missing of ${#package_names[@]} package(s) are missing from $out"
[ "$shims" -eq 6 ] ||
    die "$shims shim(s) in the bundle; the release ships 6 (2 CPython + 4 PHP)"
printf 'build-rpm: %s package(s) in %s (%s shim(s), %s python module(s))\n' \
    "${#package_names[@]}" "$out" "$shims" "$modules"
if command -v rpm >/dev/null 2>&1; then
    for name in "${package_names[@]}"; do
        rpm -qp --requires "$out/$name-$version-$release.$rpm_arch.rpm" |
            sed "s/^/build-rpm: requires ($name): /"
    done
fi
