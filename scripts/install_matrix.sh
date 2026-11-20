#!/usr/bin/env bash
# Installs one *built* package in one distribution from the matrix (`MB.42.3.4`).
#
#   scripts/install_matrix.sh --deb dist/blastit_0.2.0_amd64.deb --leg debian-13-x86_64
#   scripts/install_matrix.sh --matrix --dir dist        # the whole survey, in order
#
# The leg list is `scripts/platform_matrix.py --matrix install`: the six
# distributions the signed repositories advertise, expanded over the two
# architectures the release ships. This script never chooses an image — it reads
# `image`, `format`, `modes` and `package_arch` from the table and hands them to
# `scripts/test_packages.sh`, which installs the **file** in a clean container and
# drives `blastit --version`, `blastit check` and `blastit dev` there.
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
usage: scripts/install_matrix.sh --leg <name> [--deb PATH] [--rpm PATH] [--dir DIR]
       scripts/install_matrix.sh --matrix     [--deb PATH] [--rpm PATH] [--dir DIR]

  --leg     one leg from `scripts/platform_matrix.py --matrix install`
            (e.g. `debian-13-x86_64`, `rocky-9-aarch64`)
  --matrix  every leg, in table order
  --dir     where the built packages are, when --deb/--rpm is not given (dist)

The image, the package format and the phases each leg runs come from the matrix,
never from this script.
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

newest() { # newest <dir> <glob>
    find "$1" -maxdepth 1 -name "$2" -type f 2>/dev/null | sort | tail -n 1 || true
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
            file=$deb
            [ -n "$file" ] || file=$(newest "$dir" "*_${package_arch}.deb")
            [ -n "$file" ] ||
                die "no $package_arch .deb under $dir: pass --deb, or build it with \`make packages\`"
            [ -f "$file" ] || die "$file does not exist"
            BLASTIT_PACKAGE_MODES="$modes" BLASTIT_DEB="$file" BLASTIT_DEB_IMAGE="$image" \
                "$root/scripts/test_packages.sh"
            ;;
        rpm)
            file=$rpm
            [ -n "$file" ] || file=$(newest "$dir" "*.${package_arch}.rpm")
            [ -n "$file" ] ||
                die "no $package_arch .rpm under $dir: pass --rpm, or build it with \`make packages\`"
            [ -f "$file" ] || die "$file does not exist"
            BLASTIT_PACKAGE_MODES="$modes" BLASTIT_RPM="$file" BLASTIT_RPM_IMAGE="$image" \
                "$root/scripts/test_packages.sh"
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
