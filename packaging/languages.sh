#!/usr/bin/env bash
# The one bash loader for `packaging/languages.tsv` (`MB.67`, `MB.75.1`).
#
#   source packaging/languages.sh
#   blastit_language_rows          # package-set rows of the selected channel, core first
#   blastit_language_all_rows      # the same, every row regardless of channel
#   blastit_language_field NAME 4  # field of one package-set row
#   blastit_language_php_minors    # newest first
#   blastit_language_py_minors     # newest first
#   blastit_language_check         # validate the TSV (every row)
#
# `packaging/package-set.sh` sources this file and derives every package it
# names from it; the builders source `package-set.sh`, so a language version is
# added in `languages.tsv` alone. Parsing is `while IFS=$'\t' read` only — bash
# must be able to read the file without a Python interpreter.
#
# Derivation (kept identical to `scripts/languages.py`):
#
#   package  blastit-<language>-<version>
#   shim     libblastit_<slug>-*-gnu.so   (the arch is globbed, as the table does)
#   core     blastit  core  (none) (none) (none)
#   runtime  the `Recommends:` value, never `Depends:`/`Requires:`
#
# Channels (`MB.75`): the TSV's sixth column says which release channel(s) a row
# ships in. **The selected channel filters the listings**, and it defaults to
# `stable`, so a stable build (`package-set.sh`, the builders, the RPM spec, the
# docs blocks) names only stable rows while `--channel nightly` names 3.16 too.
# `blastit_language_check` always validates the whole file, never one channel.
#
# A version is a string compared per component (`8.10` after `8.9`), never a
# float. Order is the file's order.

BLASTIT_LANGUAGES_SH_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
: "${BLASTIT_LANGUAGES_TSV:=$BLASTIT_LANGUAGES_SH_ROOT/languages.tsv}"
# The channel the listings below select. `--channel` (or this variable, which the
# nightly workflow sets) is the only way to change it.
: "${BLASTIT_LANGUAGES_CHANNEL:=stable}"
# The declared channels, in the order a `channels` cell spells them.
BLASTIT_LANGUAGES_CHANNELS="stable nightly"

# Non-comment, non-blank lines after the header, verbatim.
_blastit_language_data_lines() {
    local line language version header=1
    while IFS= read -r line || [ -n "$line" ]; do
        line=${line%$'\r'}
        case "$line" in '' | '#'*) continue ;; esac
        if [ "$header" -eq 1 ]; then
            header=0
            IFS=$'\t' read -r language version _ <<<"$line"
            if [ "$language" = language ] && [ "$version" = version ]; then
                continue
            fi
        fi
        printf '%s\n' "$line"
    done <"$BLASTIT_LANGUAGES_TSV"
}

# Whether the space-separated `$1` contains the word `$2`.
_blastit_language_has_channel() {
    case " $1 " in *" $2 "*) return 0 ;; esac
    return 1
}

# The six-field package-set rows of the selected channel, core first.
blastit_language_rows() {
    local language version slug deb_runtime rpm_runtime channels
    printf 'blastit\tcore\t(none)\t(none)\t(none)\n'
    while IFS= read -r line; do
        IFS=$'\t' read -r language version slug deb_runtime rpm_runtime channels <<<"$line"
        _blastit_language_has_channel "$channels" "$BLASTIT_LANGUAGES_CHANNEL" || continue
        printf 'blastit-%s-%s\tshim\tlibblastit_%s-*-gnu.so\t%s\t%s\n' \
            "$language" "$version" "$slug" "$deb_runtime" "$rpm_runtime"
    done < <(_blastit_language_data_lines)
}

# The six-field package-set rows of **every** row, core first. Only the channel
# validator needs this: every other reader selects one channel.
blastit_language_all_rows() {
    local language version slug deb_runtime rpm_runtime _channels
    printf 'blastit\tcore\t(none)\t(none)\t(none)\n'
    while IFS= read -r line; do
        IFS=$'\t' read -r language version slug deb_runtime rpm_runtime _channels <<<"$line"
        printf 'blastit-%s-%s\tshim\tlibblastit_%s-*-gnu.so\t%s\t%s\n' \
            "$language" "$version" "$slug" "$deb_runtime" "$rpm_runtime"
    done < <(_blastit_language_data_lines)
}

# Field `$2` (1-5) of the package-set row named `$1`; prints nothing when no row
# has that name.
blastit_language_field() {
    local name=$1 index=$2 row first
    while IFS= read -r row; do
        IFS=$'\t' read -r first _ <<<"$row"
        [ "$first" = "$name" ] || continue
        printf '%s\n' "$row" | cut -f"$index"
        return 0
    done < <(blastit_language_rows)
    return 0
}

# Versions of one language in the selected channel, in file order.
_blastit_language_versions_of() {
    local want=$1 line language version _slug _deb _rpm channels
    while IFS= read -r line; do
        IFS=$'\t' read -r language version _slug _deb _rpm channels <<<"$line"
        _blastit_language_has_channel "$channels" "$BLASTIT_LANGUAGES_CHANNEL" || continue
        [ "$language" = "$want" ] || continue
        printf '%s\n' "$version"
    done < <(_blastit_language_data_lines)
}

# PHP minors of the selected channel, newest first (component-wise, so `8.10`
# beats `8.9`).
blastit_language_php_minors() {
    _blastit_language_versions_of php | sort -t. -k1,1nr -k2,2nr
}

# CPython minors of the selected channel, newest first.
blastit_language_py_minors() {
    _blastit_language_versions_of python | sort -t. -k1,1nr -k2,2nr
}

# Validate the TSV: header, field count, duplicate package/slug, `core` used as
# a language, a shim without a runtime, a runtime value that carries `%`/`#`/`{`/
# `}` or surrounding whitespace (it becomes a `Recommends:` value), a version that
# is not `<major>.<minor>` (`8` and `8.1.0` are refused; `8.10` is allowed), and a
# `channels` cell that is not `stable`, `nightly` or `stable nightly`.
blastit_language_check() {
    local tsv=${BLASTIT_LANGUAGES_TSV:-}
    local problems=0 rows=0 headers=0 number=0 line
    local -a fields=()
    local -A names=() slugs=()
    local language version slug deb_runtime rpm_runtime channels name runtime
    if [ ! -f "$tsv" ]; then
        echo "languages: $tsv is missing" >&2
        return 1
    fi
    while IFS= read -r line || [ -n "$line" ]; do
        number=$((number + 1))
        line=${line%$'\r'}
        case "$line" in '' | '#'*) continue ;; esac
        IFS=$'\t' read -r -a fields <<<"$line"
        if [ "$headers" -eq 0 ]; then
            headers=1
            if [ "${#fields[@]}" -ne 6 ] ||
                [ "${fields[0]-}" != language ] || [ "${fields[1]-}" != version ] ||
                [ "${fields[2]-}" != slug ] || [ "${fields[3]-}" != deb-runtime ] ||
                [ "${fields[4]-}" != rpm-runtime ] || [ "${fields[5]-}" != channels ]; then
                echo "languages: $tsv:$number: bad header; expected language<TAB>version<TAB>slug<TAB>deb-runtime<TAB>rpm-runtime<TAB>channels" >&2
                problems=$((problems + 1))
            fi
            continue
        fi
        if [ "${#fields[@]}" -ne 6 ]; then
            echo "languages: $tsv:$number: ${#fields[@]} fields, a row is exactly 6" >&2
            problems=$((problems + 1))
            continue
        fi
        language=${fields[0]}
        version=${fields[1]}
        slug=${fields[2]}
        deb_runtime=${fields[3]}
        rpm_runtime=${fields[4]}
        channels=${fields[5]}
        name="blastit-$language-$version"
        if [[ ! $version =~ ^[0-9]+\.[0-9]+$ ]]; then
            echo "languages: $tsv:$number: $name has version '$version'; expected <major>.<minor> like 8.10 (not 8 or 8.1.0)" >&2
            problems=$((problems + 1))
        fi
        if [[ ! $slug =~ ^[a-z0-9_]+$ ]]; then
            echo "languages: $tsv:$number: $name has slug '$slug'; expected lowercase letters, digits or '_'" >&2
            problems=$((problems + 1))
        fi
        case "$language" in
            core | blastit | language)
                echo "languages: $tsv:$number: '$language' is not a language (core, blastit and language are reserved)" >&2
                problems=$((problems + 1))
                ;;
        esac
        # The channels cell is a selection key, so it is one of exactly three
        # spellings: an empty cell would ship nowhere, a repeat is ambiguous and a
        # reordered one would make the two loaders disagree.
        case "$channels" in
            stable | nightly | 'stable nightly') ;;
            *)
                echo "languages: $tsv:$number: $name has channels '$channels'; expected 'stable', 'nightly' or 'stable nightly' (space-separated, stable first, never empty, never repeated)" >&2
                problems=$((problems + 1))
                ;;
        esac
        if [ "$deb_runtime" = "(none)" ] || [ "$rpm_runtime" = "(none)" ]; then
            echo "languages: $tsv:$number: shim $name needs a deb-runtime and rpm-runtime Recommends value ((none) is only for the core package)" >&2
            problems=$((problems + 1))
        fi
        # A runtime column is interpolated into `Recommends:` in a Debian control
        # file and an RPM spec, so it is a package name: `%`/`#`/`{`/`}` would
        # expand a macro or start a comment in one of the two consumers, and
        # surrounding whitespace would silently change the package asked for.
        for runtime in "$deb_runtime" "$rpm_runtime"; do
            case "$runtime" in
                ' '* | *' ' | $'\t'* | *$'\t')
                    echo "languages: $tsv:$number: $name has runtime '$runtime' with leading or trailing whitespace: it is a Recommends: value, not prose" >&2
                    problems=$((problems + 1))
                    ;;
            esac
            case "$runtime" in
                *%* | *#* | *'{'* | *'}'*)
                    echo "languages: $tsv:$number: $name has runtime '$runtime' containing % # { or }: a runtime column is a package name for Recommends:, never spec/control syntax" >&2
                    problems=$((problems + 1))
                    ;;
            esac
        done
        if [ -n "${names[$name]+set}" ]; then
            echo "languages: $tsv:$number: package $name is declared twice (first on line ${names[$name]})" >&2
            problems=$((problems + 1))
        else
            names[$name]=$number
        fi
        if [ -n "${slugs[$slug]+set}" ]; then
            echo "languages: $tsv:$number: slug $slug is declared twice (first on line ${slugs[$slug]})" >&2
            problems=$((problems + 1))
        else
            slugs[$slug]=$number
        fi
        rows=$((rows + 1))
    done <"$tsv"
    if [ "$headers" -eq 0 ]; then
        echo "languages: $tsv: no header line" >&2
        problems=$((problems + 1))
    fi
    if [ "$headers" -eq 1 ] && [ "$rows" -eq 0 ]; then
        echo "languages: $tsv: declares no language rows" >&2
        problems=$((problems + 1))
    fi
    [ "$problems" -eq 0 ]
}

# --- CLI (only when executed, never when sourced) ---------------------------

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -euo pipefail
    _usage() {
        cat >&2 <<'USAGE'
usage: packaging/languages.sh [--channel stable|nightly] --rows|--names|--shims|--php-minors|--py-minors|--check

  --channel CH  the release channel the listings select (stable, the default,
                or nightly); every row is validated whatever this is
  --rows        name<TAB>kind<TAB>shim-glob<TAB>deb-runtime<TAB>rpm-runtime, core first
  --names       the binary package names, one per line (core first)
  --shims       <package><TAB><glob> for every package that ships a shim
  --php-minors  PHP minors, newest first
  --py-minors   CPython minors, newest first
  --check       validate packaging/languages.tsv (every row, both channels)

The file is sourceable: `source packaging/languages.sh` then
`blastit_language_rows`, `blastit_language_all_rows`,
`blastit_language_field NAME INDEX`, `blastit_language_php_minors`,
`blastit_language_py_minors`. The channel listings select is
`$BLASTIT_LANGUAGES_CHANNEL` (default `stable`).
USAGE
    }
    action=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --channel)
                if [ "$#" -lt 2 ]; then
                    echo "languages: --channel needs stable|nightly" >&2
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
            --rows | --names | --shims | --php-minors | --py-minors | --check)
                action=$1
                shift
                ;;
            *)
                echo "languages: unknown argument '$1'" >&2
                _usage
                exit 2
                ;;
        esac
    done
    case " $BLASTIT_LANGUAGES_CHANNELS " in
        *" $BLASTIT_LANGUAGES_CHANNEL "*) ;;
        *)
            echo "languages: unknown channel '$BLASTIT_LANGUAGES_CHANNEL'; expected stable or nightly" >&2
            _usage
            exit 2
            ;;
    esac
    [ -n "$action" ] || {
        _usage
        exit 2
    }
    case "$action" in
        --rows) blastit_language_rows ;;
        --names) blastit_language_rows | cut -f1 ;;
        --shims) blastit_language_rows | awk -F'\t' '$2 == "shim" { print $1 "\t" $3 }' ;;
        --php-minors) blastit_language_php_minors ;;
        --py-minors) blastit_language_py_minors ;;
        --check)
            blastit_language_check || exit 1
            echo "languages: ok ($(blastit_language_all_rows | tail -n +2 | grep -c '') language rows in packaging/languages.tsv; selected channel: $BLASTIT_LANGUAGES_CHANNEL)"
            ;;
    esac
fi
