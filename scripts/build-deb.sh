#!/usr/bin/env bash
# Packages a staged Blastit bundle as one Debian archive per package
# `packaging/package-set.sh` names (`universal-distribution.md` §6.1, `MB.65`).
#
#   make install PREFIX=/tmp/opencode/bundle
#   scripts/build-deb.sh --version 0.2.1 --bundle /tmp/opencode/bundle
#   scripts/build-deb.sh --print-set        # the rows, no bundle needed
#
# The input is a *bundle*: exactly the §6.1 tree `make install PREFIX=<dir>`
# stages, or the deterministic one `scripts/build-package-bundle.sh` builds from
# the floor images. Inside every archive §6.1's prefix is `/usr`, and the split
# is one package per row of `packaging/languages.tsv`:
#
#   blastit                       /usr/bin/{blastit,blastit-lsp}, LICENSE, python/blastit/*.py
#   blastit-<language>-<version>  /usr/share/blastit/lib/libblastit_<slug>-<arch>-gnu.so
#
# The split is per **interpreter minor**, because that is the dependency a host
# has: each shim package recommends its interpreter's distro runtime package and
# depends on `blastit (= <version>)`. The interpreter is a **`Recommends:`**,
# never a `Depends:`: apt pulls the distro runtime by default, and the shim still
# installs beside a pyenv or source-built interpreter, or on a distro without
# that minor (it is inert until one is present).
#
# The names, globs and runtimes are read from `packaging/package-set.sh`, which
# derives them from `packaging/languages.tsv`; this script refuses to write
# anything if a shim in the bundle is owned by no package or by two. The staged
# tree is also asserted against §6.1 before anything is written:
# `scripts/check-bundle-layout.sh` requires every shim the table declares, the
# four pure-Python modules and no strays, so a bundle that lost a shim cannot
# become a package set whose install "passes" on a static route.
# `BUNDLE_SKIP_VERSION=1` because an `--arch arm64` set is built on an x86_64
# host; the container smoke runs the binary on the arch it installs.
#
# # The dependency floor is measured, and so are the libraries
#
# `scripts/check-portable-elf.sh --max-glibc` measures the maximum
# `GLIBC_X.Y` across **every** ELF a package ships — not a constant someone
# typed — and prints the `DT_NEEDED` allowlist that became the dependency names.
# `blastit` gets `libc6 (>= <its measured floor>)`, plus `libgcc-s1` only when a
# shipped ELF actually needs `libgcc_s.so.1`; each shim package gets its own
# measured `libc6 (>= …)` beside its `blastit` dependency, and names the
# interpreter only in `Recommends:`. A host build that outgrew the ceiling fails
# instead of producing a higher floor, and pass `--max-glibc <shim_floor>` (from
# `scripts/platform_matrix.py`) to enforce it. Version needs the loader marks
# `WEAK` are advisory (glibc prints a note and runs), so they do not raise the
# floor.
#
# Everything is staged under `$TMPDIR/opencode` (or `--out`), the archives are
# written with `dpkg-deb --root-owner-group`, and nothing is ever installed on
# the host: the proof is `e2e/run_packaging.py --leg deb` inside Debian.
set -euo pipefail

# One timestamp for the ar members and the clamp, so two hosts produce the same
# bytes; a release caller passes the commit time.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch_base="${TMPDIR:-/tmp}/opencode"
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

version=""
release=""
arch=""
bundle=""
out="$scratch_base/blastit-packages"
max_glibc=""

usage_text() {
    cat <<'USAGE'
usage: scripts/build-deb.sh --version <ver> --bundle <dir>
                            [--release <rel>] [--arch amd64|arm64]
                            [--max-glibc <major>_<minor>] [--out <dir>]
       scripts/build-deb.sh --print-set

  --bundle     a `make install PREFIX=<dir>` tree (bin/ + share/blastit/)
  --release    appended as `-<rel>` to the Debian version (optional)
  --arch       defaults to the machine type of the bundle's `blastit`
  --max-glibc  refuse an archive whose measured floor is above this ceiling
  --out        where the archives are written; `packaging/package-set.sh` names
               them (the core `blastit` plus one per shim row of
               `packaging/languages.tsv`), as `blastit_<ver>_<arch>.deb` and
               `<package>_<ver>_<arch>.deb`
  --print-set  print the package rows (`packaging/package-set.sh --rows`) and
               exit without needing a bundle or root
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
        --print-set) print_set=1; shift ;;
        -h | --help) usage 0 ;;
        *) usage ;;
    esac
done

# `--print-set` is the table, not a build: it prints the rows an operator (or a
# test) can compare before a bundle or root exists.
if [ -n "${print_set:-}" ]; then
    "$root/packaging/package-set.sh" --rows
    exit 0
fi

[ -n "$version" ] || usage
[ -n "$bundle" ] || usage

command -v dpkg-deb >/dev/null 2>&1 ||
    die "dpkg-deb is required: apt-get install dpkg, or build inside a Debian container"
command -v readelf >/dev/null 2>&1 ||
    die "readelf is required to measure the floor: apt-get install binutils"

# A Debian version starts with a digit and uses `[A-Za-z0-9.+~]`; `--release` is
# the Debian revision. `+` and `~` are what nightly versions are made of
# (`0.2.1~nightly20261122.r12+gabc123`), and both sort below a release.
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

# --- the split is only honest if every shim has exactly one owner ------------
# One core package plus one package per shim row: both counts are the table's.
shim_count=$(blastit_package_shims | grep -c '' || true)
package_names=()
while IFS= read -r name; do package_names+=("$name"); done < <(blastit_package_names)
[ "${#package_names[@]}" -eq "$((shim_count + 1))" ] ||
    die "packaging/package-set.sh names ${#package_names[@]} packages but packaging/languages.tsv declares $shim_count shims (one package each) plus the core"

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
# A relative `--out` would appear verbatim in the package paths this script
# prints; resolve it once so a relative directory reads exactly like an absolute
# one (and so a caller can compare the printed paths with `ls`).
out=$(cd "$out" && pwd -P) || die "cannot resolve $out to an absolute path"
scratch=$(mktemp -d "$scratch_base/blastit-deb.XXXXXX")
trap 'rm -rf "$scratch"' EXIT

# --- the payload is §6.1's, or these are not Blastit packages ----------------
layout=$(BUNDLE_REQUIRE_PY_SHIMS="$(shim_count_of python)" \
    BUNDLE_REQUIRE_PHP_SHIMS="$(shim_count_of php)" BUNDLE_SKIP_VERSION=1 \
    "$root/scripts/check-bundle-layout.sh" "$bundle")
printf '%s\n' "$layout" | sed 's/^/build-deb: layout: /'

# --- the measured C-library floor --------------------------------------------
ceiling=auto
[ -z "$max_glibc" ] || ceiling=$max_glibc
measured_floor=""
measured_needed=""
floor_of() {
    local report
    report=$("$root/scripts/check-portable-elf.sh" --max-glibc "$ceiling" "$@")
    measured_floor=$(printf '%s\n' "$report" |
        sed -n 's/^max-glibc: \([0-9][0-9]*\(\.[0-9][0-9]*\)\+\) (.*/\1/p')
    [ -n "$measured_floor" ] ||
        die "no shipped ELF names a GLIBC_ symbol, so the package floor is unknown: refusing to write an unmeasured Depends"
    measured_needed=$(printf '%s\n' "$report" | sed -n 's/^needed: //p')
    case " $measured_needed " in
        *" libc.so.6 "*) ;;
        *)
            die "no shipped ELF needs libc.so.6 (needed: ${measured_needed:-none}): this is not a glibc bundle"
            ;;
    esac
    echo "build-deb: $(basename "$1"): measured glibc floor $measured_floor, needed: $measured_needed"
}

c_depends() {
    # `libc6` because a shipped ELF needs `libc.so.6`, `libgcc-s1` only when one
    # needs `libgcc_s.so.1` (`libm` and the loader are in libc6).
    local depends="libc6 (>= $measured_floor)"
    case " $measured_needed " in
        *" libgcc_s.so.1 "*) depends="$depends, libgcc-s1" ;;
    esac
    printf '%s' "$depends"
}

write_control() {
    # $1 stage, $2 package, $3 Depends, $4 section, $5 summary, $6 body,
    # $7 Recommends (empty for the core, which recommends no interpreter)
    local stage=$1 package=$2 depends=$3 section=$4 summary=$5 body=$6
    local recommends=${7:-} installed_kb
    installed_kb=$(du -sk "$stage/usr" | cut -f1)
    {
        cat <<CONTROL
Package: $package
Version: $deb_version
Architecture: $arch
Maintainer: Offworld Nexus <maintainers@offworld.nexus>
Installed-Size: $installed_kb
Depends: $depends
CONTROL
        [ -z "$recommends" ] || printf 'Recommends: %s\n' "$recommends"
        cat <<CONTROL
Section: $section
Priority: optional
Homepage: https://github.com/offworld-nexus/blastit
Description: $summary
$(printf '%s\n' "$body" | sed 's/^/ /')
CONTROL
    } >"$stage/DEBIAN/control"
}

finish_package() {
    # $1 stage, $2 package: md5sums, owner normalisation and the ar archive.
    local stage=$1 package=$2 filename recommends
    # `dpkg-deb --build` only fills `md5sums` in when it is absent; write it
    # ourselves from the staged files so the archive's own integrity list is
    # deterministic.
    (cd "$stage" && find usr -type f -print0 | sort -z | xargs -r -0 md5sum >DEBIAN/md5sums)
    # `mktemp -d` creates 0700; the archive's root entry is the package root, not
    # a directory the operator should inherit.
    chmod 0755 "$stage"
    if [ "$package" = blastit ]; then
        filename="$out/blastit_${deb_version}_${arch}.deb"
    else
        filename="$out/${package}_${deb_version}_${arch}.deb"
    fi
    rm -f "$filename"
    dpkg-deb --root-owner-group --build "$stage" "$filename" >/dev/null
    recommends=$(dpkg-deb --field "$filename" Recommends)
    [ -z "$recommends" ] || recommends=", Recommends: $recommends"
    echo "build-deb: $filename ($(wc -c <"$filename") bytes, Depends: $(dpkg-deb --field "$filename" Depends)$recommends)"
}

deb_version="$version"
[ -z "$release" ] || deb_version="$version-$release"

# --- blastit: the interpreter-free server and the pure-Python package --------
core_stage="$scratch/blastit.stage"
install -d "$core_stage/usr/bin" "$core_stage/usr/share/blastit/lib" \
    "$core_stage/usr/share/blastit/python/blastit" "$core_stage/DEBIAN"
install -m 0755 "$lib/blastit" "$lib/blastit-lsp" "$core_stage/usr/share/blastit/lib/"
install -m 0644 "$share/LICENSE" "$core_stage/usr/share/blastit/LICENSE"
modules=0
for module in "$share"/python/blastit/*.py; do
    [ -e "$module" ] || continue
    install -m 0644 "$module" "$core_stage/usr/share/blastit/python/blastit/"
    modules=$((modules + 1))
done
ln -s ../share/blastit/lib/blastit "$core_stage/usr/bin/blastit"
ln -s ../share/blastit/lib/blastit-lsp "$core_stage/usr/bin/blastit-lsp"

floor_of "$lib/blastit" "$lib/blastit-lsp"
core_depends=$(c_depends)
echo "build-deb: blastit -> Depends: $core_depends"
write_control "$core_stage" blastit "$core_depends" admin \
    "zero-trust application harness and sandbox" \
    "Blastit runs 12-factor web applications as isolated per-route sandboxes,
validates every inbound byte, and keeps credentials behind protocol-aware
capability proxies. This package ships the interpreter-free server, the native
language server, and the pure-Python helper package; the language shims are
separate packages so a host installs only the interpreter it runs." ""
finish_package "$core_stage" blastit

# --- one package per interpreter minor --------------------------------------
shims=0
for shim in "$lib"/libblastit_*.so; do
    [ -e "$shim" ] || continue
    basename=$(basename "$shim")
    owner=$(blastit_package_owners "$basename")
    runtime=$(blastit_package_deb_runtime "$owner")
    [ -n "$runtime" ] && [ "$runtime" != "(none)" ] ||
        die "$owner has no deb runtime in packaging/package-set.sh"

    stage="$scratch/$owner.stage"
    install -d "$stage/usr/share/blastit/lib" "$stage/DEBIAN"
    install -m 0755 "$shim" "$stage/usr/share/blastit/lib/"

    floor_of "$shim"
    depends="blastit (= $deb_version), $(c_depends)"
    echo "build-deb: $owner -> Depends: $depends, Recommends: $runtime"
    # The minor is in the package name, so it is read back out of it rather than
    # spelled a second time here.
    case "$owner" in
        blastit-python-* | blastit-php-*) minor=${owner##blastit-*-} ;;
        *) die "$owner is a shim package whose description this builder does not know" ;;
    esac
    digits=${minor//./}
    case "$owner" in
        blastit-python-*)
            write_control "$stage" "$owner" "$depends" python \
                "CPython ${minor} view shim for blastit" \
                "The libblastit_py${digits} shim lets blastit run Python WSGI views on
CPython ${minor}. It is loaded into the host interpreter by the blastit
binary; install it together with blastit, never instead of it." \
                "$runtime"
            ;;
        blastit-php-*)
            write_control "$stage" "$owner" "$depends" php \
                "PHP ${minor} embedded SAPI shim for blastit" \
                "The libblastit_php${digits} shim lets blastit run PHP view scripts
through the host's embedded NTS libphp ${minor}. It is loaded by the blastit
binary; install it together with blastit, never instead of it." \
                "$runtime"
            ;;
        *) die "$owner is a shim package whose description this builder does not know" ;;
    esac
    finish_package "$stage" "$owner"
    shims=$((shims + 1))
done

[ "$shims" -eq "$shim_count" ] ||
    die "wrote $shims shim package(s); packaging/languages.tsv declares $shim_count"
printf 'build-deb: %s package(s) in %s (%s python module(s))\n' "$((shims + 1))" "$out" "$modules"
