#!/usr/bin/env bash
# The one place that names the binary packages Blastit ships (`MB.65`).
#
#   source packaging/package-set.sh          # then use the functions
#   packaging/package-set.sh --names         # one package name per line
#   packaging/package-set.sh --shims         # <package> <TAB> <shim glob>
#   packaging/package-set.sh --rows          # the table, verbatim
#   packaging/package-set.sh --check         # the table itself (see the note below)
#
# The language versions are not named here: `packaging/languages.tsv` is the one
# declaration (`MB.67`), `packaging/languages.sh` derives the rows from it, and
# this file adds only the row-less core package `blastit`. Adding a version is a
# one-line edit to the TSV.
#
# The builders and the set check *read* their names here:
#
#   scripts/build-deb.sh             the packages, their shim globs and
#   scripts/build-rpm.sh             their interpreter runtimes
#   scripts/check-bundle-layout.sh   the names a package set must contain
#
# One file cannot run a shell script, so `scripts/rpm/blastit.spec` is
# **generated** from the TSV by `scripts/rpm/gen_spec.py --write`, which derives
# its tag list, `%package -n`/`%files` stanzas and `Recommends:` values. The
# committed spec's freshness gate is `scripts/rpm/gen_spec.py --check`, which
# `scripts/platform_matrix.py --check` runs (and the package workflows run that),
# so `--check` here validates this table only:
#
#   scripts/rpm/gen_spec.py --check  the generated spec reproduces the committed file
#   scripts/platform_matrix.py       `scripts/platform_matrix.py --check` diffs the
#     (`PACKAGE_SET`)               matrix's names against `--names`, its shim names
#                                    against `--shims`, and runs the spec's `--check`
#
# `e2e/run_packaging.py` keeps no copy at all: it runs `--shims` at test time, so
# there is nothing there to drift.
#
# Nothing generates a matrix row from this file: the check names the package that
# drifted, which is what keeps the copies honest.
#
# A row is five tab-separated fields:
#
#   name <TAB> kind <TAB> shim-glob <TAB> deb-runtime <TAB> rpm-runtime
#
# `kind` is `core` (the interpreter-free server, the LSP binary, `LICENSE` and
# the pure-Python package) or `shim` (exactly one `libblastit_*.so`). `(none)`
# is the literal spelling for "no shim", "no runtime package" and "no runtime
# recommendation" — an empty field is invisible in a shell table and reads as a
# mistake. Both runtime columns are the `Recommends:` value, never a `Depends:`
# or `Requires:`.
#
# The split is per **interpreter minor**, because that is the dependency a host
# actually has: a machine installs `blastit` plus `blastit-php-8.4`, and that shim
# *recommends* `libphp8.4-embed` (the interpreter it `dlopen`s) instead of
# depending on it, so a pyenv or source-built interpreter, or a distro without
# that minor, is not refused — the shim is simply inert until one is present.
# `blastit-python-3.13` works the same way.
set -euo pipefail

BLASTIT_PACKAGE_SET_SOURCED=1
BLASTIT_PACKAGE_SET_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# The version rows are derived from the canonical TSV (`MB.67`); this file only
# adds the row-less core package `blastit` on top of them.
# shellcheck source=./languages.sh
source "$BLASTIT_PACKAGE_SET_ROOT/packaging/languages.sh"

# --- readers ----------------------------------------------------------------

# All five fields, in declaration order (core first).
blastit_package_rows() {
    blastit_language_rows
}

# Field `$2` of every row (`$1` is ignored so a call reads like
# `blastit_package_field x 1`).
blastit_package_field() {
    local index=$2 row
    while IFS= read -r row; do
        printf '%s\n' "$row" | cut -f"$index"
    done < <(blastit_package_rows)
}

# The package names, in declaration order (core first).
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

# The deb runtime package a shim package recommends (`(none)` for core).
blastit_package_deb_runtime() {
    blastit_language_field "$1" 4
}

# The RPM runtime recommendation (`Recommends:` for CPython and PHP alike);
# `(none)` for core.
blastit_package_rpm_weak() {
    blastit_language_field "$1" 5
}

blastit_package_kind() {
    blastit_language_field "$1" 2
}

# --- the table is only useful if it is consistent ---------------------------

validate_table() {
    local problems=0 rows=0 core=0 language_rows seen_names=() seen_globs=() row name kind glob
    blastit_language_check || problems=$((problems + 1))
    language_rows=$(blastit_language_rows | tail -n +2 | grep -c '' || true)
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
    [ "$language_rows" -gt 0 ] ||
        { echo "package-set: packaging/languages.tsv declares no language rows" >&2; problems=$((problems + 1)); }
    [ "$rows" -eq "$((1 + language_rows))" ] ||
        { echo "package-set: $rows packages, but packaging/languages.tsv declares $language_rows language rows plus the one core package" >&2; problems=$((problems + 1)); }
    [ "$problems" -eq 0 ]
}

# The RPM spec is generated from this table (`scripts/rpm/gen_spec.py --write`),
# so there is no copy to diff here: its freshness gate is the generator's own
# `--check`, run by `scripts/platform_matrix.py --check`.

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
            echo "package-set: ok ($(blastit_package_rows | grep -c '') packages; packaging/languages.tsv is consistent)"
            ;;
        -h | --help)
            cat >&2 <<'USAGE'
usage: packaging/package-set.sh --names|--rows|--shims|--check

  --names   the binary package names, one per line (core first; language
            versions come from packaging/languages.tsv)
  --rows    name<TAB>kind<TAB>shim-glob<TAB>deb-runtime<TAB>rpm-runtime
  --shims   <package><TAB><glob> for every package that ships a shim
  --check   validate this table and packaging/languages.tsv; the generated
            scripts/rpm/blastit.spec has its own gate, `scripts/rpm/gen_spec.py
            --check`, run by `scripts/platform_matrix.py --check`

The file is also sourceable: `source packaging/package-set.sh` then
`blastit_package_names`, `blastit_package_shims`, `blastit_package_owners FILE`,
`blastit_package_deb_runtime NAME`, `blastit_package_rpm_weak NAME`.
USAGE
            ;;
        *) echo "usage: packaging/package-set.sh --names|--rows|--shims|--check" >&2; exit 2 ;;
    esac
fi
