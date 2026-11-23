#!/usr/bin/env bash
# The one place that names the binary packages Blastit ships (`MB.65`).
#
#   source packaging/package-set.sh          # then use the functions
#   packaging/package-set.sh --names         # one package name per line
#   packaging/package-set.sh --shims         # <package> <TAB> <shim glob>
#   packaging/package-set.sh --rows          # the table, verbatim
#   packaging/package-set.sh --version       # the workspace version (`Cargo.toml`)
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
#
# Channels (`MB.75`): the set is derived **per channel**, `stable` by default, so
# a stable release is core + the stable rows and a nightly is that plus the
# nightly-only ones (`blastit-python-3.16`). `--check` validates both channels and
# the count a stable release promises.
set -euo pipefail

BLASTIT_PACKAGE_SET_SOURCED=1
BLASTIT_PACKAGE_SET_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# What a `stable` release promises: the core package, CPython 3.13/3.14/3.15 and
# the four PHP shims (`MB.75`). A stable set that is not this count means a row
# changed channel without this file's knowledge.
BLASTIT_PACKAGE_STABLE_COUNT=8

# The version rows are derived from the canonical TSV (`MB.67`); this file only
# adds the row-less core package `blastit` on top of them.
# shellcheck source=./languages.sh
source "$BLASTIT_PACKAGE_SET_ROOT/packaging/languages.sh"

# The workspace version comes from `Cargo.toml` through its one loader, exactly
# as the language rows come from the TSV through theirs.
# shellcheck source=../scripts/version.sh
source "$BLASTIT_PACKAGE_SET_ROOT/scripts/version.sh"

# --- readers ----------------------------------------------------------------

# The release channel this set is derived for (`stable` unless `--channel`, or
# `$BLASTIT_LANGUAGES_CHANNEL`, says otherwise).
blastit_package_channel() {
    printf '%s\n' "$BLASTIT_LANGUAGES_CHANNEL"
}

# Run a function with `$BLASTIT_LANGUAGES_CHANNEL` temporarily set to `$1`, then
# restore it: the two-channel check below derives each set through the one loader.
with_channel() {
    local channel=$1
    shift
    local saved=$BLASTIT_LANGUAGES_CHANNEL status=0
    BLASTIT_LANGUAGES_CHANNEL=$channel
    "$@" || status=$?
    BLASTIT_LANGUAGES_CHANNEL=$saved
    return $status
}

# The version a package set is built at: `Cargo.toml`'s
# `[workspace.package] version`, never a copy.
blastit_package_version() {
    blastit_workspace_version
}

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

# Both channels, on top of `validate_table`'s one-channel pass: each channel's
# derived set must be internally consistent, the two together must name every
# declared row exactly once, and `stable` must be the count a release promises.
validate_channels() {
    local problems=0 channel stable_names nightly_names distinct total
    for channel in $BLASTIT_LANGUAGES_CHANNELS; do
        with_channel "$channel" validate_table || problems=$((problems + 1))
    done
    stable_names=$(with_channel stable blastit_package_names)
    nightly_names=$(with_channel nightly blastit_package_names)
    distinct=$(printf '%s\n%s\n' "$stable_names" "$nightly_names" | sort -u | grep -c '')
    total=$(blastit_language_all_rows | cut -f1 | sort -u | grep -c '')
    [ "$distinct" -eq "$total" ] ||
        { echo "package-set: the two channels name $distinct packages, but packaging/languages.tsv declares $total (core included)" >&2; problems=$((problems + 1)); }
    [ "$(printf '%s\n' "$stable_names" | grep -c '')" -eq "$BLASTIT_PACKAGE_STABLE_COUNT" ] ||
        { echo "package-set: stable names $(printf '%s\n' "$stable_names" | grep -c '') packages, but a release promises $BLASTIT_PACKAGE_STABLE_COUNT (check the channels column in packaging/languages.tsv)" >&2; problems=$((problems + 1)); }
    [ "$problems" -eq 0 ]
}

# --- CLI (only when executed, never when sourced) ---------------------------

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    _usage() {
        cat >&2 <<'USAGE'
usage: packaging/package-set.sh [--channel stable|nightly] --names|--rows|--shims|--version|--check

  --channel CH the release channel the set is derived for: stable (the default)
             or nightly. `--check` validates both channels whatever this is.
  --names   the binary package names, one per line (core first; language
            versions come from packaging/languages.tsv)
  --rows    name<TAB>kind<TAB>shim-glob<TAB>deb-runtime<TAB>rpm-runtime
  --shims   <package><TAB><glob> for every package that ships a shim
  --version the workspace version a set is built at: Cargo.toml's
            `[workspace.package] version`, through scripts/version.sh
  --check   validate both channels of this table and packaging/languages.tsv,
            and that `stable` is the promised 8 packages; the generated
            scripts/rpm/blastit.spec has its own gate, `scripts/rpm/gen_spec.py
            --check`, run by `scripts/platform_matrix.py --check`

The file is also sourceable: `source packaging/package-set.sh` then
`blastit_package_names`, `blastit_package_shims`, `blastit_package_owners FILE`,
`blastit_package_deb_runtime NAME`, `blastit_package_rpm_weak NAME`,
`blastit_package_version`, `blastit_package_channel` — with
`$BLASTIT_LANGUAGES_CHANNEL` (default `stable`) selecting the channel.
USAGE
    }
    command=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --channel)
                if [ "$#" -lt 2 ]; then
                    echo "package-set: --channel needs stable|nightly" >&2
                    _usage
                    exit 2
                fi
                BLASTIT_LANGUAGES_CHANNEL=$2
                shift 2
                ;;
            --channel=*)
                BLASTIT_LANGUAGES_CHANNEL=${1#--channel=}
                shift
                ;;
            -h | --help)
                _usage
                exit 0
                ;;
            --names | --rows | --shims | --version | --check)
                command=$1
                shift
                ;;
            *)
                echo "package-set: unknown argument '$1'" >&2
                _usage
                exit 2
                ;;
        esac
    done
    case " $BLASTIT_LANGUAGES_CHANNELS " in
        *" $BLASTIT_LANGUAGES_CHANNEL "*) ;;
        *)
            echo "package-set: unknown channel '$BLASTIT_LANGUAGES_CHANNEL'; expected stable or nightly" >&2
            _usage
            exit 2
            ;;
    esac
    [ -n "$command" ] || {
        _usage
        exit 2
    }
    case "$command" in
        --names)
            validate_table || exit 1
            blastit_package_names
            ;;
        --version)
            blastit_package_version
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
            validate_channels || exit 1
            echo "package-set: ok ($(blastit_package_rows | grep -c '') $(blastit_package_channel) packages, $(with_channel nightly blastit_package_names | grep -c '') nightly; packaging/languages.tsv is consistent)"
            ;;
    esac
fi
