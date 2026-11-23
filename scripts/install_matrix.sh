#!/usr/bin/env bash
# Installs one *built* package set in one distribution from the matrix (`MB.42.3.4`).
#
#   scripts/install_matrix.sh --deb dist/blastit_<version>_amd64.deb --leg debian-13-x86_64
#   scripts/install_matrix.sh --deb dist        --leg debian-13-x86_64   # the directory
#   scripts/install_matrix.sh --matrix --dir dist        # the whole survey, in order
#
# The leg list is `scripts/platform_matrix.py --matrix install`: the six
# distributions the signed repositories advertise, expanded over the two
# architectures the release ships. This script never chooses an image — it reads
# `image`, `format`, `modes` and `package_arch` from the table and hands them to
# `e2e/run_packaging.py --leg deb|rpm`, which installs the **core package** in a
# clean container and drives `blastit --version`, `blastit fmt`, `blastit check`,
# `blastit-lsp --version` and a served page there.
#
# `--deb`/`--rpm` take a file **or** a directory. A file is the core package and
# `docs/install.sh` installs the sibling shims beside it for the runtimes the
# image has; a directory is resolved to its core package explicitly (never an
# arbitrary `find | tail` match), and the E2E mounts that directory at `/pkg`.
#
# CI runs one leg per job (`--leg <name>`) so the six distributions are six logs
# and one red leg names itself; `--matrix` is the same survey for a developer.
# A leg the matrix does not declare is refused by name, not silently skipped: a
# package that no leg installs must not reach a repository.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

leg=""
dir="dist"
deb=""
rpm=""
all=0

usage() {
    cat >&2 <<'USAGE'
usage: scripts/install_matrix.sh --leg <name> [--deb PATH|DIR] [--rpm PATH|DIR] [--dir DIR]
       scripts/install_matrix.sh --matrix     [--deb PATH|DIR] [--rpm PATH|DIR] [--dir DIR]

  --leg     one leg from `scripts/platform_matrix.py --matrix install`
            (e.g. `debian-13-x86_64`, `rocky-9-aarch64`)
  --matrix  every leg, in table order
  --deb     the built core .deb, or the directory holding the set
  --rpm     the built core .rpm, or the directory holding the set
  --dir     where the built packages are, when --deb/--rpm is not given (dist)

The image, the package format and the phases each leg runs come from the matrix,
never from this script. A directory is resolved to its one core package
(`blastit_<ver>_<arch>.deb` / `blastit-<ver>-<rel>.<arch>.rpm`); the sibling
`blastit-python-*`/`blastit-php-*` packages beside it are installed with it.
USAGE
    exit 2
}

die() {
    echo "error: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --leg)
            [ "$#" -ge 2 ] || usage
            leg=$2
            shift 2
            ;;
        --matrix)
            all=1
            shift
            ;;
        --deb)
            [ "$#" -ge 2 ] || usage
            deb=$2
            shift 2
            ;;
        --rpm)
            [ "$#" -ge 2 ] || usage
            rpm=$2
            shift 2
            ;;
        --dir)
            [ "$#" -ge 2 ] || usage
            dir=$2
            shift 2
            ;;
        -h | --help) usage ;;
        *) usage ;;
    esac
done

[ -n "$leg" ] || [ "$all" -eq 1 ] || usage
command -v docker >/dev/null 2>&1 ||
    die "docker is required: the package is installed in a container, never on the host"

field() { # field <row> <key>
    printf '%s\n' "$1" | sed -n "s/^$2=//p"
}

# The one core package in `$1` for format `$2` and architecture `$3`. The core is
# named explicitly (`blastit_<ver>_<arch>.deb`, `blastit-<ver>-<rel>.<arch>.rpm`):
# a shim package starts with `blastit-`, so this cannot pick one, and two matching
# cores is a directory this script refuses to guess about.
core_package() { # <dir> <deb|rpm> <package_arch>
    local matches=() candidate
    case "$2" in
        deb)
            for candidate in "$1"/blastit_[0-9]*_"$3".deb; do
                [ -f "$candidate" ] && matches+=("$candidate")
            done
            ;;
        rpm)
            for candidate in "$1"/blastit-[0-9]*."$3".rpm; do
                [ -f "$candidate" ] && matches+=("$candidate")
            done
            ;;
        *) return 1 ;;
    esac
    case "${#matches[@]}" in
        0) return 1 ;;
        1) printf '%s\n' "${matches[0]}" ;;
        *)
            printf 'install-matrix: %s holds %d core %s packages (%s): pass --%s with the one to install\n' \
                "$1" "${#matches[@]}" "$2" "$(printf '%s ' "${matches[@]}")" "$2" >&2
            return 2
            ;;
    esac
}

resolve() { # <format> <explicit file|dir> <fallback dir> <package_arch>
    local chosen=$2
    if [ -n "$chosen" ] && [ -d "$chosen" ]; then
        core_package "$chosen" "$1" "$4" ||
            die "no single $4 core .$1 in $chosen: pass --$1 with the core package"
    elif [ -n "$chosen" ]; then
        printf '%s\n' "$chosen"
    else
        core_package "$3" "$1" "$4" ||
            die "no single $4 core .$1 under $3: pass --$1, or build it with \`make packages\`"
    fi
}

run_leg() {
    name=$1
    row=$(uv run --no-project "$root/scripts/platform_matrix.py" --leg install --name "$name") ||
        die "'$name' is not an install leg: see \`scripts/platform_matrix.py --matrix install\`"
    [ -n "$row" ] ||
        die "'$name' is not an install leg: see \`scripts/platform_matrix.py --matrix install\`"
    image=$(field "$row" image)
    format=$(field "$row" format)
    modes=$(field "$row" modes)
    package_arch=$(field "$row" package_arch)
    echo "install-matrix: $name: $format $package_arch in $image ($modes)"
    case "$format" in
        deb)
            file=$(resolve deb "$deb" "$dir" "$package_arch")
            [ -f "$file" ] || die "$file does not exist"
            uv run --no-project "$root/e2e/run_packaging.py" \
                --leg deb --deb "$file" --image "$image" --modes "$modes"
            ;;
        rpm)
            file=$(resolve rpm "$rpm" "$dir" "$package_arch")
            [ -f "$file" ] || die "$file does not exist"
            uv run --no-project "$root/e2e/run_packaging.py" \
                --leg rpm --rpm "$file" --image "$image" --modes "$modes"
            ;;
        *)
            die "$name: the matrix names package format '$format', which is neither deb nor rpm"
            ;;
    esac
}

if [ "$all" -eq 1 ]; then
    # The order is the table's, so the survey is the same one CI runs.
    legs=$(uv run --no-project "$root/scripts/platform_matrix.py" --matrix install |
        uv run --no-project python -c 'import json, sys; [print(leg["name"]) for leg in json.load(sys.stdin)["include"]]')
    [ -n "$legs" ] || die "the install matrix is empty: scripts/platform_matrix.py declares no leg"
    for name in $legs; do
        run_leg "$name"
    done
    echo "install-matrix: ok — every declared leg installed and served"
else
    run_leg "$leg"
    echo "install-matrix: ok — one leg installed and served"
fi
