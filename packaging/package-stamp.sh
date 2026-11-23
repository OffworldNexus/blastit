#!/usr/bin/env bash
# The version/release a package carries on each channel (`MB.65`, the Offworld
# Nexus packaging skill's §3 stamp grammar).
#
#   packaging/package-stamp.sh --base 0.2.1 --channel stable --format deb
#   # deb_version=0.2.1
#   # deb_release=
#
#   packaging/package-stamp.sh --base 0.2.1 --channel nightly \
#       --run 42 --sha 90cb2db1234 --format rpm
#   # rpm_version=0.2.1
#   # rpm_release=0.nightly.20261122.r42.g90cb2db
#
# The key names are the ones the workflows write straight into `$GITHUB_OUTPUT`
# (`deb_version`, `deb_release`, `rpm_version`, `rpm_release`), so a caller never
# renames a stamp on the way through. `--format both` prints all four lines, for
# a job that builds the two formats from one stamp.
#
# A nightly stamp must sort **strictly below** the release it leads up to:
#
#   deb  0.2.0~nightly20261122.r42+g90cb2db   `~` sorts below the empty string
#   rpm  Version 0.2.0 / Release 0.nightly…    `-0.` sorts below `-1`
#
# so `apt-get install blastit` on the stable channel never picks a nightly, and
# an RPM upgrade from nightly to stable is an upgrade in both directions.
#
# `--run`/`--sha` are required only for the nightly channel: a stable stamp is
# the workspace version the tag carries, and the release workflow calls this
# with `--channel stable --base "$version"` and nothing else.
#
# The date is UTC. `SOURCE_DATE_EPOCH` (nonzero) or `--date` pin it, because a
# reproducible build may not read the wall clock twice.
set -euo pipefail

base=""
channel=""
run=""
sha=""
format=""
date_override=""

usage() {
    cat >&2 <<'USAGE'
usage: packaging/package-stamp.sh --base <version> --channel stable|nightly
                                  --format deb|rpm|both [--date YYYYMMDD]

Nightly builds also need --run <build number> --sha <git sha>; a stable stamp
is the workspace version, so it needs neither.

  --format deb    deb_version=<v>  and  deb_release=<r>   (release may be empty)
  --format rpm    rpm_version=<v>  and  rpm_release=<r>
  --format both   all four lines

  stable   deb: version=<base>, release=            (empty)
           rpm: version=<base>, release=1
  nightly  deb: version=<base>~nightly<YYYYMMDD>.r<run>+g<sha7>
           rpm: version=<base>, release=0.nightly.<YYYYMMDD>.r<run>.g<sha7>

The names are chosen so a workflow can append the two lines straight into
$GITHUB_OUTPUT.
USAGE
    exit 2
}

die() {
    echo "error: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --base) [ "$#" -ge 2 ] || usage; base=$2; shift 2 ;;
        --channel) [ "$#" -ge 2 ] || usage; channel=$2; shift 2 ;;
        --run) [ "$#" -ge 2 ] || usage; run=$2; shift 2 ;;
        --sha) [ "$#" -ge 2 ] || usage; sha=$2; shift 2 ;;
        --format) [ "$#" -ge 2 ] || usage; format=$2; shift 2 ;;
        --date) [ "$#" -ge 2 ] || usage; date_override=$2; shift 2 ;;
        -h | --help) usage ;;
        *) usage ;;
    esac
done

[ -n "$base" ] || usage
[ -n "$channel" ] || usage
[ -n "$format" ] || usage

printf '%s' "$base" | grep -Eq '^[0-9][0-9A-Za-z.+~]*$' ||
    die "--base '$base' is not a package version (letters, digits, '.', '+', '~')"
case "$channel" in
    stable | nightly) ;;
    *) die "--channel '$channel' is not stable|nightly" ;;
esac
case "$format" in
    deb | rpm | both) ;;
    *) die "--format '$format' is not deb|rpm|both" ;;
esac

deb_release=""
rpm_release=""
if [ "$channel" = stable ]; then
    deb_version=$base
    rpm_version=$base
    rpm_release=1
else
    [ -n "$run" ] ||
        die "--run is required on the nightly channel: it orders two builds of the same day"
    printf '%s' "$run" | grep -Eq '^[0-9]+$' || die "--run '$run' is not a build number"
    [ -n "$sha" ] ||
        die "--sha is required on the nightly channel: it names the commit the stamp belongs to"
    printf '%s' "$sha" | grep -Eiq '^[0-9a-f]{7,40}$' ||
        die "--sha '$sha' is not a git sha (7 to 40 hex digits)"
    if [ -n "$date_override" ]; then
        printf '%s' "$date_override" | grep -Eq '^[0-9]{8}$' ||
            die "--date '$date_override' is not YYYYMMDD"
        stamp_date=$date_override
    elif [ -n "${SOURCE_DATE_EPOCH:-}" ] && [ "$SOURCE_DATE_EPOCH" != 0 ]; then
        stamp_date=$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y%m%d)
    else
        stamp_date=$(date -u +%Y%m%d)
    fi
    short_sha=$(printf '%s' "$sha" | cut -c1-7 | tr 'A-F' 'a-f')
    deb_version="${base}~nightly${stamp_date}.r${run}+g${short_sha}"
    rpm_version=$base
    rpm_release="0.nightly.${stamp_date}.r${run}.g${short_sha}"
fi

case "$format" in
    deb)
        printf 'deb_version=%s\n' "$deb_version"
        printf 'deb_release=%s\n' "$deb_release"
        ;;
    rpm)
        printf 'rpm_version=%s\n' "$rpm_version"
        printf 'rpm_release=%s\n' "$rpm_release"
        ;;
    both)
        printf 'deb_version=%s\n' "$deb_version"
        printf 'deb_release=%s\n' "$deb_release"
        printf 'rpm_version=%s\n' "$rpm_version"
        printf 'rpm_release=%s\n' "$rpm_release"
        ;;
esac
