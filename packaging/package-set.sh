#!/usr/bin/env bash
# The one place that names the binary packages Blastit ships (`MB.65`).
#
#   source packaging/package-set.sh          # then use the functions
#   packaging/package-set.sh --names         # one package name per line
#   packaging/package-set.sh --shims         # <package> <TAB> <shim glob>
#   packaging/package-set.sh --rows          # the table, verbatim
#   packaging/package-set.sh --check         # the table, and the spec that copies it
#
# The builders and the set check *read* their names here:
#
#   scripts/build-deb.sh             the seven packages, their shim globs and
#   scripts/build-rpm.sh             their interpreter runtimes
#   scripts/check-bundle-layout.sh   the names a package set must contain
#
# Two files cannot run a shell script, so they keep their own copy and the gate
# *asserts* it against this table instead of generating it:
#
#   scripts/rpm/blastit.spec         `packaging/package-set.sh --check` diffs its
#                                    `%package -n` stanzas and `%files` globs
#   scripts/platform_matrix.py       `scripts/platform_matrix.py --check` diffs
#     (`PACKAGE_FORMATS[].set`)      its `set` against `--names` and its shim
#                                    names against `--shims`
#
# `e2e/run_packaging.py` keeps no copy at all: it runs `--shims` at test time, so
# there is nothing there to drift.
#
# Nothing generates a stanza or a matrix row from this file: the check names the
# package that drifted, which is what keeps the copies honest.
#
# A row is five tab-separated fields:
#
#   name <TAB> kind <TAB> shim-glob <TAB> deb-runtime <TAB> rpm-weak-dep
#
# `kind` is `core` (the interpreter-free server, the LSP binary, `LICENSE` and
# the pure-Python package) or `shim` (exactly one `libblastit_*.so`). `(none)`
# is the literal spelling for "no shim", "no runtime package" and "no weak
# dependency" — an empty field is invisible in a shell table and reads as a
# mistake.
#
# The split is per **interpreter minor**, because that is the dependency a host
# actually has: a machine installs `blastit` plus `blastit-php-8.4` (whose
# `Depends: libphp8.4-embed` names the interpreter) or `blastit-python-3.13`,
# and a Debian
# package that names a missing interpreter package is a package `apt` refuses
# rather than one that half-works.
set -euo pipefail

# shellcheck disable=SC2034 # read by the functions below and by sourced callers
BLASTIT_PACKAGE_ROWS=(
    $'blastit\tcore\t(none)\t(none)\t(none)'
    $'blastit-python-3.13\tshim\tlibblastit_py313-*-gnu.so\tpython3.13\tpython3.13'
    $'blastit-python-3.14\tshim\tlibblastit_py314-*-gnu.so\tpython3.14\tpython3.14'
    $'blastit-php-8.3\tshim\tlibblastit_php83-*-gnu.so\tlibphp8.3-embed\tphp-embedded'
    $'blastit-php-8.4\tshim\tlibblastit_php84-*-gnu.so\tlibphp8.4-embed\tphp-embedded'
    $'blastit-php-8.5\tshim\tlibblastit_php85-*-gnu.so\tlibphp8.5-embed\tphp-embedded'
    $'blastit-php-8.6\tshim\tlibblastit_php86-*-gnu.so\tlibphp8.6-embed\tphp-embedded'
)
BLASTIT_PACKAGE_SET_SOURCED=1
BLASTIT_PACKAGE_SET_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# --- readers ----------------------------------------------------------------

# All five fields, in declaration order.
blastit_package_rows() {
    printf '%s\n' "${BLASTIT_PACKAGE_ROWS[@]}"
}

# Field `$2` of every row (`$1` is ignored so a call reads like
# `blastit_package_field x 1`).
blastit_package_field() {
    local index=$2
    printf '%s\n' "${BLASTIT_PACKAGE_ROWS[@]}" | cut -f"$index"
}

# The seven package names, in declaration order (core first).
blastit_package_names() {
    blastit_package_field x 1
}

# `<name> <TAB> <glob>` for every package that owns a shim: the mapping a
# builder uses to refuse an unowned or doubly-owned `.so`.
blastit_package_shims() {
    local row name kind glob
    while IFS= read -r row; do
        IFS=$'\t' read -r name kind glob _ _ <<<"$row"
        [ "$kind" = shim ] || continue
        printf '%s\t%s\n' "$name" "$glob"
    done < <(blastit_package_rows)
}

# Every package whose shim glob matches the file name `$1`, one per line. Zero
# lines is an unowned shim, two lines is a shim two packages would ship.
blastit_package_owners() {
    local basename=$1 name glob match=0
    while IFS=$'\t' read -r name glob; do
        # shellcheck disable=SC2053 # the glob is the pattern, deliberately
        if [[ $basename == $glob ]]; then
            printf '%s\n' "$name"
            match=$((match + 1))
        fi
    done < <(blastit_package_shims)
    [ "$match" -gt 0 ]
}

# The deb runtime package a shim package installs beside (`(none)` for core).
blastit_package_deb_runtime() {
    printf '%s\n' "${BLASTIT_PACKAGE_ROWS[@]}" | awk -F'\t' -v n="$1" '$1 == n { print $4 }'
}

# The RPM weak dependency (`Recommends:` for CPython, `Suggests:` for PHP);
# `(none)` for core.
blastit_package_rpm_weak() {
    printf '%s\n' "${BLASTIT_PACKAGE_ROWS[@]}" | awk -F'\t' -v n="$1" '$1 == n { print $5 }'
}

blastit_package_kind() {
    printf '%s\n' "${BLASTIT_PACKAGE_ROWS[@]}" | awk -F'\t' -v n="$1" '$1 == n { print $2 }'
}

# --- the table is only useful if it is consistent ---------------------------

validate_table() {
    local problems=0 rows=0 core=0 seen_names=() seen_globs=() row name kind glob
    while IFS= read -r row; do
        rows=$((rows + 1))
        IFS=$'\t' read -r name kind glob _ _ <<<"$row"
        case "$kind" in
            core)
                core=$((core + 1))
                [ "$glob" = "(none)" ] ||
                    { echo "package-set: $name is core and owns a shim glob ($glob)" >&2; problems=$((problems + 1)); }
                ;;
            shim)
                [ "$glob" = "(none)" ] &&
                    { echo "package-set: $name is a shim and owns no glob" >&2; problems=$((problems + 1)); }
                ;;
            *)
                echo "package-set: $name has kind '$kind', not core|shim" >&2
                problems=$((problems + 1))
                ;;
        esac
        case " ${seen_names[*]-} " in
            *" $name "*) echo "package-set: $name is declared twice" >&2; problems=$((problems + 1)) ;;
        esac
        seen_names+=("$name")
        if [ "$glob" != "(none)" ]; then
            case " ${seen_globs[*]-} " in
                *" $glob "*) echo "package-set: $glob is claimed by two packages" >&2; problems=$((problems + 1)) ;;
            esac
            seen_globs+=("$glob")
        fi
    done < <(blastit_package_rows)
    [ "$core" -eq 1 ] ||
        { echo "package-set: $core core packages, one blastit ships the binary" >&2; problems=$((problems + 1)); }
    [ "$rows" -eq 7 ] ||
        { echo "package-set: $rows packages, the release ships 7 (core + 2 CPython + 4 PHP)" >&2; problems=$((problems + 1)); }
    [ "$problems" -eq 0 ]
}

# The RPM spec keeps its own `%package -n` stanzas and `%files` globs (rpmbuild
# cannot read this table), so `--check` diffs them and names what drifted.
check_spec() {
    local spec="$BLASTIT_PACKAGE_SET_ROOT/scripts/rpm/blastit.spec"
    local problems=0 name glob declared kind
    [ -f "$spec" ] || {
        echo "package-set: $spec is missing: the RPM payload lists cannot be checked" >&2
        return 1
    }
    declared=$(sed -n 's/^%package -n \(.*\)$/\1/p' "$spec" | sort)
    while IFS= read -r name; do
        printf '%s\n' "$declared" | grep -qx "$name" || {
            echo "package-set: $spec has no \`%package -n $name\` stanza" >&2
            problems=$((problems + 1))
        }
    done < <(blastit_package_shims | cut -f1)
    while IFS= read -r name; do
        [ -n "$name" ] || continue
        kind=$(blastit_package_kind "$name")
        [ -n "$kind" ] || {
            echo "package-set: $spec declares \`$name\`, which packaging/package-set.sh does not name" >&2
            problems=$((problems + 1))
        }
    done <<<"$declared"
    while IFS=$'\t' read -r name glob; do
        # The table names the file as the bundle holds it
        # (`libblastit_py313-*-gnu.so`); the spec lists it under `%{_datadir}` with
        # the arch/libc wildcarded, so compare the name stem before the first `*`
        # and let both wildcards be wildcards.
        stem=${glob%%\**}
        if ! awk -v want="$name" '
            $0 == "%files -n " want { inside = 1; next }
            inside && /^%files( |$)/ { exit }
            inside { print }
        ' "$spec" | grep -qF "$stem"; then
            echo "package-set: $spec \`%files -n $name\` lists no $stem… file: that package would ship no shim" >&2
            problems=$((problems + 1))
        fi
    done < <(blastit_package_shims)
    [ "$problems" -eq 0 ]
}

# --- CLI (only when executed, never when sourced) ---------------------------

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    case "${1:-}" in
        --names)
            validate_table || exit 1
            blastit_package_names
            ;;
        --rows)
            validate_table || exit 1
            blastit_package_rows
            ;;
        --shims)
            validate_table || exit 1
            blastit_package_shims
            ;;
        --check)
            validate_table || exit 1
            check_spec || exit 1
            echo "package-set: ok (7 packages; scripts/rpm/blastit.spec's %package stanzas and %files globs match)"
            ;;
        -h | --help)
            cat >&2 <<'USAGE'
usage: packaging/package-set.sh --names|--rows|--shims|--check

  --names   the seven binary package names, one per line
  --rows    name<TAB>kind<TAB>shim-glob<TAB>deb-runtime<TAB>rpm-weak-dep
  --shims   <package><TAB><glob> for every package that ships a shim
  --check   validate the table, and assert scripts/rpm/blastit.spec's
            `%package -n` stanzas and `%files` globs against it

The file is also sourceable: `source packaging/package-set.sh` then
`blastit_package_names`, `blastit_package_shims`, `blastit_package_owners FILE`,
`blastit_package_deb_runtime NAME`, `blastit_package_rpm_weak NAME`.
`scripts/platform_matrix.py --check` diffs the matrix's own copy of the names
against `--names`/`--shims`.
USAGE
            ;;
        *) echo "usage: packaging/package-set.sh --names|--rows|--shims|--check" >&2; exit 2 ;;
    esac
fi
