#!/usr/bin/env bash
# Asserts the installed release tree (`universal-distribution.md` §6.1,
# `MB.65`), from a prefix, from a `.deb` set or from an RPM set.
#
#   scripts/check-bundle-layout.sh PREFIX
#   scripts/check-bundle-layout.sh --deb-set dist [--arch amd64|arm64]
#   scripts/check-bundle-layout.sh --rpm-set  dist [--arch x86_64|aarch64]
#
# `<PREFIX>/bin/{blastit,blastit-lsp}` are symlinks to
# `<PREFIX>/share/blastit/lib/{blastit,blastit-lsp}`; the lib directory holds the
# per-minor shims; `share/blastit/LICENSE` and `share/blastit/python/blastit/*.py`
# are the licence and the pure-Python package. Nothing else may appear under
# `share/blastit` — not a retired wheel layout (a `_bin/`), not a second binary,
# not an eighth module — because a bundle is what the operator runs, and an extra
# path is a claim nobody checked.
#
# A *package set* is checked by extracting every package into one scratch root
# (`dpkg-deb -x`, or `rpm2cpio | cpio -id` — inside a container when the host has
# no RPM tools) and running the **same** tree assertions on the merged `/usr`.
# The set must be exactly the packages `packaging/package-set.sh` names — the
# core plus one per shim row of `packaging/languages.tsv` — because a split
# package set that silently lost a shim is exactly the regression the split could
# introduce; the failure names the missing package, and any unexpected package in
# the directory is refused too.
#
# Every installed ELF is also run through `scripts/check-portable-elf.sh`:
# §6.1 puts `blastit`, `blastit-lsp` and both shim families under the same
# "libpython/libphp are dlopen'ed, no RUNPATH ships" floor, and a `blastit-lsp`
# that only ran on the build host is as unshippable as a `blastit` that did.
#
# The count of shims is a host fact, not a layout fact: a host with one CPython
# builds one Python shim and a host with no `php-config` builds none, so at least
# one Python shim is required (the WSGI capability is the point) and PHP shims
# are reported. `BUNDLE_REQUIRE_PY_SHIMS=N` / `BUNDLE_REQUIRE_PHP_SHIMS=N` demand
# **at least** N of each (`BUNDLE_REQUIRE_PHP_SHIMS=1` is what `make
# e2e-universal` sets: the gate may not claim PHP was proved on a bundle with no
# PHP shim). The set modes demand one shim per row `packaging/languages.tsv`
# declares for each language, because a package set that ships fewer is the
# defect they exist to catch.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=../packaging/package-set.sh
source "$root/packaging/package-set.sh"

# How many shims one language family ships. `packaging/languages.tsv` names every
# shim row `blastit-<language>-<version>`, so the family is the segment after
# `blastit-`: the count is the table's, never a literal here.
shim_count_of() {
    local wanted=$1 name _ rest count=0
    while IFS=$'\t' read -r name _; do
        rest=${name#blastit-}
        [ "${rest%%-*}" = "$wanted" ] || continue
        count=$((count + 1))
    done < <(blastit_package_shims)
    printf '%s\n' "$count"
}

mode=prefix
prefix=""
setdir=""
arch=""

usage_text() {
    cat <<'USAGE'
usage: scripts/check-bundle-layout.sh PREFIX
       scripts/check-bundle-layout.sh --deb-set <dir> [--arch amd64|arm64]
       scripts/check-bundle-layout.sh --rpm-set <dir> [--arch x86_64|aarch64]

  PREFIX       a `make install PREFIX=<dir>` tree (or an unpacked archive's root)
  --deb-set    a directory of the `.deb`s `scripts/build-deb.sh` wrote
  --rpm-set    a directory of the `.rpm`s `scripts/build-rpm.sh` wrote
  --arch       the package architecture of the set (default: this machine)

A set check extracts every package into one scratch root and asserts the same
§6.1 tree as the PREFIX check; a set missing one of the packages
`packaging/package-set.sh` names fails, naming the package.
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
        --deb-set) [ "$#" -ge 2 ] || usage; mode=deb; setdir=$2; shift 2 ;;
        --rpm-set) [ "$#" -ge 2 ] || usage; mode=rpm; setdir=$2; shift 2 ;;
        --arch) [ "$#" -ge 2 ] || usage; arch=$2; shift 2 ;;
        -h | --help) usage 0 ;;
        -*) usage ;;
        *)
            [ "$mode" = prefix ] || usage
            prefix=$1
            shift
            ;;
    esac
done

if [ "$mode" = prefix ]; then
    [ -n "$prefix" ] || usage
else
    [ -n "$setdir" ] || usage
    [ -d "$setdir" ] || die "$setdir is not a directory"
    # `docker -v` refuses a relative source path (it reads as a volume name), and
    # `dpkg-deb`/`cpio` should not depend on the caller's cwd either: resolve the
    # set once, so a relative directory behaves exactly like an absolute one.
    setdir=$(cd "$setdir" && pwd -P) || die "cannot resolve $setdir to an absolute path"
fi

modules="__init__ __main__ _blastit _check_pytest"
problems=0

fail() {
    echo "error: $*" >&2
    problems=$((problems + 1))
}

#: `1 problem` / `2 problems`, `1 Python shim` / `2 Python shims`.
plural() {
    if [ "$1" -eq 1 ]; then printf '%s %s' "$1" "$2"; else printf '%s %ss' "$1" "$2"; fi
}

# --- the §6.1 tree, whichever way it was assembled --------------------------
py_shims=0
php_shims=0
version=""
check_tree() {
    share="$prefix/share/blastit"
    lib="$share/lib"
    package="$share/python/blastit"

    # the two entry points, and the artifacts they name
    for name in blastit blastit-lsp; do
        artifact="$lib/$name"
        link="$prefix/bin/$name"
        if [ ! -f "$artifact" ]; then
            fail "$artifact is missing; re-run make install"
        elif [ ! -x "$artifact" ]; then
            fail "$artifact is not executable; re-run make install"
        fi
        if [ ! -L "$link" ]; then
            fail "$link is not a symlink to $artifact; re-run make install"
            continue
        fi
        resolved="$(readlink -f "$link" 2>/dev/null || true)"
        expected="$(readlink -f "$artifact" 2>/dev/null || true)"
        if [ -z "$resolved" ] || [ "$resolved" != "$expected" ]; then
            fail "$link -> ${resolved:-nowhere}, not $artifact; re-run make install"
        fi
    done

    # the licence and the four modules of the pure-Python package
    [ -f "$share/LICENSE" ] ||
        fail "$share/LICENSE is missing; re-run make install (the bundle ships the licence)"

    if [ -e "$package/_bin" ]; then
        fail "$package/_bin is a retired wheel layout (one universal blastit ships): remove -r $package/_bin, or re-run make install"
    fi

    for module in $modules; do
        [ -f "$package/$module.py" ] || fail "$package/$module.py is missing; re-run make install"
    done

    # nothing else under share/blastit (stray files *and* directories)
    while IFS= read -r -d '' path; do
        relative="${path#"$share"/}"
        if [ -d "$path" ]; then
            case "$relative" in
                lib | python | python/blastit) ;;
                *)
                    fail "$share/$relative is a stray directory: the tree is lib/, python/blastit/ and LICENSE; re-run make install"
                    ;;
            esac
            continue
        fi
        case "$relative" in
            LICENSE) ;;
            lib/blastit | lib/blastit-lsp) ;;
            lib/libblastit_php*-*.so) ;;
            lib/libblastit_py3*-*.so) ;;
            python/blastit/*.py)
                module="$(basename "$relative" .py)"
                case " $modules " in
                    *" $module "*) ;;
                    *)
                        fail "$share/$relative is not one of the four modules §6.1 ships ($modules); re-run make install"
                        ;;
                esac
                ;;
            *)
                fail "$share/$relative is a stray file: §6.1's tree is LICENSE, lib/{blastit,blastit-lsp,libblastit_*}, python/blastit/{$modules}"
                ;;
        esac
    done < <(find "$share" -mindepth 1 -print0)

    # the shims the capabilities load
    py_shims="$(find "$lib" -maxdepth 1 -name 'libblastit_py3*-*.so' | wc -l)"
    php_shims="$(find "$lib" -maxdepth 1 -name 'libblastit_php*-*.so' | wc -l)"

    if [ "$py_shims" -eq 0 ]; then
        fail "no libblastit_py3<mm>-<arch>-<libc>.so: WSGI views cannot boot; run make py-sapi, then re-run make install"
    fi
    if [ -n "${BUNDLE_REQUIRE_PY_SHIMS:-}" ] && [ "$py_shims" -lt "$BUNDLE_REQUIRE_PY_SHIMS" ]; then
        fail "$(plural "$py_shims" 'Python shim') shipped, and BUNDLE_REQUIRE_PY_SHIMS=$BUNDLE_REQUIRE_PY_SHIMS; run make py-sapi"
    fi
    if [ -n "${BUNDLE_REQUIRE_PHP_SHIMS:-}" ] && [ "$php_shims" -lt "$BUNDLE_REQUIRE_PHP_SHIMS" ]; then
        fail "$(plural "$php_shims" 'PHP shim') shipped, and BUNDLE_REQUIRE_PHP_SHIMS=$BUNDLE_REQUIRE_PHP_SHIMS; run make php-sapi on a host with php<m.m>-dev and libphp<m.m>-embed"
    fi

    # every installed ELF meets the §6.1 floor
    if [ "$problems" -eq 0 ]; then
        if ! command -v readelf >/dev/null 2>&1; then
            fail "readelf is required to assert the bundle's ELF floor (install binutils: apt install binutils, apk add binutils, dnf install binutils)"
        else
            artifacts=("$lib/blastit" "$lib/blastit-lsp")
            for shim in "$lib"/libblastit_*.so; do
                [ -e "$shim" ] && artifacts+=("$shim")
            done
            detail="$("$(dirname "$0")/check-portable-elf.sh" "${artifacts[@]}" 2>&1)" || {
                printf '%s\n' "$detail" >&2
                fail "an installed ELF does not meet the §6.1 floor (see above): libpython/libphp must be dlopen'ed and no RUNPATH may ship"
            }
        fi
    fi

    # the entry point really runs, from the prefix it was staged into
    # `BUNDLE_SKIP_VERSION=1` is for the *packaging* builders: they assert the
    # shape of an archive for `--arch arm64` on an x86_64 host, where the staged
    # binary is the right file and the wrong machine. The container smoke runs
    # `--version` there, on the arch it installs.
    if [ "$problems" -eq 0 ] && [ "${BUNDLE_SKIP_VERSION:-0}" = 1 ]; then
        echo "ok   $prefix/bin/blastit --version: skipped (BUNDLE_SKIP_VERSION=1: the package may be another arch)"
    elif [ "$problems" -eq 0 ]; then
        version="$("$prefix/bin/blastit" --version 2>&1)" || {
            fail "$prefix/bin/blastit --version exited $?"
            version=""
        }
        case "$version" in
            blastit\ *) ;;
            "") ;;
            *) fail "$prefix/bin/blastit --version printed ${version}, not a blastit version" ;;
        esac
    fi
}

finish() {
    local where=$1
    if [ "$problems" -ne 0 ]; then
        echo "error: $(plural "$problems" problem) in $where" >&2
        exit 1
    fi
    echo "ok   $share: bin/{blastit,blastit-lsp} -> lib/, $(plural "$py_shims" 'Python shim'), $(plural "$php_shims" 'PHP shim'), python/blastit/*.py"
    [ -z "$version" ] || echo "ok   $prefix/bin/blastit --version: $version"
}

# --- PREFIX mode ------------------------------------------------------------
if [ "$mode" = prefix ]; then
    check_tree
    finish "the bundle under $prefix"
    exit 0
fi

# --- package-set modes ------------------------------------------------------
if [ -z "$arch" ]; then
    case "$(uname -m)" in
        x86_64) arch=amd64 ;;
        aarch64 | arm64) arch=arm64 ;;
        *) die "cannot map $(uname -m) to a package architecture: pass --arch" ;;
    esac
fi
case "$mode:$arch" in
    deb:amd64 | deb:arm64) ;;
    rpm:x86_64 | rpm:aarch64) ;;
    deb:*) die "--deb-set takes --arch amd64|arm64, not $arch" ;;
    rpm:*) die "--rpm-set takes --arch x86_64|aarch64, not $arch" ;;
esac

# One core package plus one package per shim row: both counts are the table's.
shim_total=$(blastit_package_shims | grep -c '' || true)
package_names=()
while IFS= read -r name; do package_names+=("$name"); done < <(blastit_package_names)
[ "${#package_names[@]}" -eq "$((shim_total + 1))" ] ||
    die "packaging/package-set.sh names ${#package_names[@]} packages but packaging/languages.tsv declares $shim_total shims (one package each) plus the core"
# The core is the first row of the table: it is the one package a set cannot be
# missing without being no set at all, so it gets its own sentence below.
core_name=${package_names[0]}

found=()
for name in "${package_names[@]}"; do
    # A version starts with a digit, and the name separator differs per format:
    # `blastit_<ver>_<arch>.deb` is not `blastit-<language>-<version>_<ver>_<arch>.deb`,
    # and `blastit-<ver>-<rel>.<arch>.rpm` is not `blastit-<language>-<version>-<ver>…`.
    case "$mode" in
        deb) pattern="$setdir/${name}_[0-9]*_${arch}.deb" ;;
        rpm) pattern="$setdir/${name}-[0-9]*.${arch}.rpm" ;;
    esac
    matches=()
    for candidate in $pattern; do
        [ -e "$candidate" ] || continue
        matches+=("$candidate")
    done
    if [ "${#matches[@]}" -eq 0 ]; then
        if [ "$name" = "$core_name" ]; then
            fail "the core package is missing from the $mode set ($pattern): run \`make packages\` for $arch, or point --$mode-set at the build directory"
        else
            fail "the $mode set has no $name package ($pattern): the release ships 7 and a set missing a shim would install a tree with no $name shim"
        fi
        continue
    fi
    [ "${#matches[@]}" -eq 1 ] ||
        fail "${#matches[@]} packages match $name in $setdir ($(printf '%s ' "${matches[@]}")): one package per interpreter"
    found+=("${matches[0]}")
done

# Anything else in the directory is a package this check cannot account for.
unexpected=0
case "$mode" in
    deb) glob="$setdir/*_${arch}.deb" ;;
    rpm) glob="$setdir/*.${arch}.rpm" ;;
esac
for candidate in $glob; do
    [ -e "$candidate" ] || continue
    unexpected=$((unexpected + 1))
done
release_total=${#package_names[@]}
if [ "$unexpected" -lt "$release_total" ]; then
    fail "$setdir holds $unexpected $mode package(s) for $arch; the release ships $release_total (the core plus one package per shim in packaging/languages.tsv) and $((release_total - unexpected)) is missing — see the package named above"
elif [ "$unexpected" -gt "$release_total" ]; then
    fail "$setdir holds $unexpected $mode package(s) for $arch; the release ships $release_total (the core plus one package per shim in packaging/languages.tsv), so at least one is a package no name in packaging/package-set.sh accounts for"
fi

[ "$problems" -eq 0 ] || {
    finish "the $mode set in $setdir"
    exit 1
}

# One scratch root, one merged `/usr`: exactly what installing the set does.
mkdir -p "${TMPDIR:-/tmp}/opencode" ||
    die "cannot create ${TMPDIR:-/tmp}/opencode for the set's scratch root"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/opencode/blastit-set.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/root"

if [ "$mode" = deb ]; then
    command -v dpkg-deb >/dev/null 2>&1 ||
        die "dpkg-deb is required to unpack a .deb set: apt-get install dpkg, or run inside a Debian container"
    for pkg in "${found[@]}"; do
        dpkg-deb -x "$pkg" "$scratch/root" ||
            die "cannot unpack $pkg"
    done
elif command -v rpm2cpio >/dev/null 2>&1 && command -v cpio >/dev/null 2>&1; then
    for pkg in "${found[@]}"; do
        (cd "$scratch/root" && rpm2cpio "$pkg" | cpio -idm --quiet) ||
            die "cannot unpack $pkg"
    done
else
    command -v docker >/dev/null 2>&1 ||
        die "no rpm2cpio/cpio and no docker: install rpm2cpio+cpio, or docker, to unpack an RPM set"
    image="${BLASTIT_RPM_BUILDER_IMAGE:-rockylinux:9}"
    echo "check-bundle-layout: unpacking ${#found[@]} RPM(s) with rpm2cpio in $image"
    docker run --rm \
        --env "HOST_UID=$(id -u)" \
        --env "HOST_GID=$(id -g)" \
        --volume "$setdir:/set:ro" \
        --volume "$scratch/root:/root" \
        --volume "$root/scripts/dnf-retry.sh:/usr/local/lib/blastit/dnf-retry.sh:ro" \
        "$image" \
        bash -euc '
            # `scripts/dnf-retry.sh` (`MB.74`): the mirror can serve an index
            # its own metadata does not match while it publishes; the helper
            # is one read-only file, so nothing else gains a mount.
            . /usr/local/lib/blastit/dnf-retry.sh
            if ! command -v rpm2cpio >/dev/null 2>&1 || ! command -v cpio >/dev/null 2>&1; then
                dnf_install_retry rpm cpio >/dev/null
            fi
            cd /root
            for pkg in /set/*.rpm; do
                [ -e "$pkg" ] || continue
                rpm2cpio "$pkg" | cpio -idm --quiet
            done
            chown -R "$HOST_UID:$HOST_GID" /root
        '
fi

prefix="$scratch/root/usr"
# The set is the release's: one shim per row packaging/languages.tsv declares.
export BUNDLE_REQUIRE_PY_SHIMS="$(shim_count_of python)"
export BUNDLE_REQUIRE_PHP_SHIMS="$(shim_count_of php)"
# A set may be unpacked on another architecture; the container smoke runs it.
case "$mode:$arch" in
    deb:amd64 | rpm:x86_64) host_arch=amd64 ;;
    *) host_arch=arm64 ;;
esac
case "$(uname -m)" in
    x86_64) machine_arch=amd64 ;;
    aarch64 | arm64) machine_arch=arm64 ;;
    *) machine_arch="" ;;
esac
if [ "$host_arch" != "$machine_arch" ]; then
    export BUNDLE_SKIP_VERSION=1
fi

check_tree
finish "the $mode set in $setdir"
