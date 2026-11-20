#!/usr/bin/env bash
# Asserts the installed release bundle (`universal-distribution.md` §6.1).
#
#   scripts/check-bundle-layout.sh PREFIX
#
# `<PREFIX>/bin/{blastit,blastit-lsp}` are symlinks to
# `<PREFIX>/share/blastit/lib/{blastit,blastit-lsp}`; the lib directory holds the
# per-minor shims; `share/blastit/LICENSE` and `share/blastit/python/blastit/*.py`
# are the licence and the pure-Python package. Nothing else may appear under
# `share/blastit` — not a retired wheel layout (a `_bin/blastit-dbproxy`), not a
# second binary, not an eighth module — because a bundle is what the operator
# runs, and an extra path is a claim nobody checked.
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
# PHP shim).
set -euo pipefail

if [ "$#" -ne 1 ] || [ -z "${1:-}" ]; then
    echo "usage: $0 PREFIX" >&2
    exit 2
fi

prefix="$1"
share="$prefix/share/blastit"
lib="$share/lib"
package="$share/python/blastit"
modules="__init__ __main__ _blastit _check_pytest fold logs pytest_plugin"
problems=0

fail() {
    echo "error: $*" >&2
    problems=$((problems + 1))
}

#: `1 problem` / `2 problems`, `1 Python shim` / `2 Python shims`.
plural() {
    if [ "$1" -eq 1 ]; then printf '%s %s' "$1" "$2"; else printf '%s %ss' "$1" "$2"; fi
}

# --- the two entry points, and the artifacts they name -----------------------
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

# --- the licence and the seven modules of the pure-Python package ------------
[ -f "$share/LICENSE" ] ||
    fail "$share/LICENSE is missing; re-run make install (the bundle ships the licence)"

if [ -e "$package/_bin" ]; then
    fail "$package/_bin is a retired wheel layout (one universal blastit ships): remove -r $package/_bin, or re-run make install"
fi

for module in $modules; do
    [ -f "$package/$module.py" ] || fail "$package/$module.py is missing; re-run make install"
done

# --- nothing else under share/blastit (stray files *and* directories) -------
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
                    fail "$share/$relative is not one of the seven modules §6.1 ships ($modules); re-run make install"
                    ;;
            esac
            ;;
        *)
            fail "$share/$relative is a stray file: §6.1's tree is LICENSE, lib/{blastit,blastit-lsp,libblastit_*}, python/blastit/{$modules}"
            ;;
    esac
done < <(find "$share" -mindepth 1 -print0)

# --- the shims the capabilities load ---------------------------------------
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

# --- every installed ELF meets the §6.1 floor ------------------------------
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

# --- the entry point really runs, from the prefix it was staged into --------
# `BUNDLE_SKIP_VERSION=1` is for the *packaging* builders: they assert the shape
# of an archive for `--arch arm64` on an x86_64 host, where the staged binary is
# the right file and the wrong machine. The container smoke runs `--version`
# there, on the arch it installs.
version=""
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

if [ "$problems" -ne 0 ]; then
    echo "error: $(plural "$problems" problem) in the bundle under $prefix" >&2
    exit 1
fi

echo "ok   $share: bin/{blastit,blastit-lsp} -> lib/, $(plural "$py_shims" 'Python shim'), $(plural "$php_shims" 'PHP shim'), python/blastit/*.py"
[ -z "$version" ] || echo "ok   $prefix/bin/blastit --version: $version"
