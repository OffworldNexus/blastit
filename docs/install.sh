#!/bin/sh
# Blastit installer — the one command the README's first line advertises:
#
#   curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh
#
# One machine install, one obvious path:
#
#   * **Default: the GPG-signed package repository.** apt or dnf verifies every
#     package, and `apt-get upgrade` / `dnf upgrade` keep Blastit current. The
#     `main` component carries releases; `--nightly` switches to `nightly`.
#   * **Fallback: the release `.tar.gz`.** `--user`, `--prefix <dir>`,
#     `--develop [ref]`, a musl host, a distro with neither apt-get nor dnf, and
#     an unprivileged shell with no usable sudo take this path. The archive is
#     checked against a SHA-256 digest before a single byte is unpacked, then
#     staged under a prefix (`~/.local` by default) with `bin/` symlinks. Archive
#     installs do not upgrade themselves.
#
# **The trust anchor is the package signature, not the setup script.** The
# default path downloads Cloudsmith's `bash.deb.sh` / `bash.rpm.sh` over TLS from
# `dl.cloudsmith.io` and runs it as root. That script is *not* signature
# verified (pin it with `BLASTIT_SETUP_SCRIPT_SHA256` if you want that): a
# compromise of that host, or of the repository account, is root here. What it
# enables — the APT/RPM repository — *is* verified: apt and dnf check every
# package and every metadata file against the repository's GPG key, and this
# installer never disables that.
#
# **`stdin` is never consulted.** `curl … | sh` carries this script on fd 0, so
# interactivity is decided by `/dev/tty` alone: questions (and sudo's password)
# are read from `/dev/tty`, every non-interactive child gets `/dev/null` for stdin
# so nothing can swallow the rest of the piped script, and every note goes to
# stderr.
#
# Contract: `docs/design/universal-distribution.md` §6.2 — POSIX `sh`, `set -eu`,
# `NO_COLOR`/`TERM=dumb` safe, usage errors exit 2, apt/dnf signature
# verification is never switched off, and an overridden release origin always
# needs the digest you checked yourself (`BLASTIT_SHA256` or
# `BLASTIT_SHA256SUMS_FILE`).
set -eu

# The version this copy of the installer was published for. The Docs job
# (`.github/workflows/docs.yml`) refuses to publish a copy whose value differs
# from `Cargo.toml`'s `workspace.package.version`.
BLASTIT_DEFAULT_VERSION="0.2.0"
GITHUB_DOWNLOAD="https://github.com/OffworldNexus/blastit/releases/download"
CLOUDSMITH_PUBLIC="https://dl.cloudsmith.io/public/offworld-nexus"
FORGEJO_DEFAULT="https://forge.offworld.nexus"
FORGEJO_OWNER="OffworldNexus"
PKG=blastit
# Every "do it again" hint names *this* command: the user arrived through a pipe,
# so no `install.sh` exists in their working directory to re-run.
SELF="curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh -s --"

if [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != dumb ] && [ -t 2 ]; then
    B=$(printf '\033[1m'); D=$(printf '\033[2m'); R=$(printf '\033[31m')
    G=$(printf '\033[32m'); Y=$(printf '\033[33m'); N=$(printf '\033[0m')
else
    B=; D=; R=; G=; Y=; N=
fi

note() { printf '%sblastit:%s %s\n' "$D" "$N" "$*" >&2; }
step() { printf '%sblastit:%s %s\n' "$B" "$N" "$*" >&2; }
ok() { printf '%sblastit:%s %s\n' "$G" "$N" "$*" >&2; }
warn() { printf '%sblastit: warning:%s %s\n' "$Y" "$N" "$*" >&2; }
die() { code=$1; shift; printf '%sblastit: error:%s %s\n' "$R" "$N" "$*" >&2; exit "$code"; }

usage() {
    cat <<USAGE
usage: install.sh [options]

Install Blastit from its GPG-signed package repository (the default), or place
the release archive under a prefix of your choice.

options:
  --version <ver>   install exactly this version       (default: $BLASTIT_DEFAULT_VERSION)
  --nightly         install from the nightly repository component; re-run
                    without it to return to releases
  --mirror <name>   repository to use: cloudsmith (default) or forgejo
  --user            no root: install the archive under ~/.local
  --prefix <dir>    no root: install the archive under <dir> (absolute path)
  --develop [ref]   install the archive CI built for <ref> (default: develop);
                    that origin is unsigned, so a digest is required
  -y, --yes         do not ask for confirmation
  -h, --help        show this help and exit

environment:
  BLASTIT_RELEASE_BASE_URL  serve release assets from here instead of GitHub;
                            with it (or --develop) a digest is mandatory
  BLASTIT_SHA256            expected sha256 of the archive, hex
  BLASTIT_SHA256SUMS_FILE   local SHA256SUMS file naming the archive
  BLASTIT_DEB, BLASTIT_RPM  install that local .deb / .rpm with dpkg / rpm
  BLASTIT_SETUP_SCRIPT_SHA256  pin the Cloudsmith setup script's sha256

The default needs root/sudo and uses apt-get or dnf. Without one of those, on a
musl host, or with no usable sudo, it installs the release archive under ~/.local
instead and prints the PATH line. Archive installs check the sha256 first and
refuse to unpack a mismatch, and they do not upgrade themselves. Questions and
sudo read /dev/tty, never stdin, so "curl … | sh" is a normal interactive
install.

examples:
  curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh
  curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh -s -- --user
  curl -fsSL https://offworldnexus.github.io/blastit/install.sh | BLASTIT_SHA256=<hex> sh -s -- --develop my-branch
USAGE
}

usage_error() {
    printf '%sblastit: error:%s %s\n\n' "$R" "$N" "$*" >&2
    usage >&2
    exit 2
}

# --- arguments ---------------------------------------------------------------

version=$BLASTIT_DEFAULT_VERSION; version_set=0; pin=
nightly=0; mirror=cloudsmith; user_install=0; prefix=
develop_ref=; assume_yes=0

# A version, ref or tag reaches a URL and a package spec, so it may only contain
# characters those accept, and never a path segment (`..`, `/`) or whitespace.
token_ok() { case "$1" in ''|*[!0-9A-Za-z._-]*|*..*) return 1 ;; *) return 0 ;; esac; }

parse_args() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version) [ "$#" -ge 2 ] || usage_error "--version needs a value, e.g. --version 0.2.0"
                version=$2; version_set=1; shift 2 ;;
            --version=*) version=${1#*=}; version_set=1; shift ;;
            --nightly) nightly=1; shift ;;
            --mirror) [ "$#" -ge 2 ] || usage_error "--mirror needs a value: cloudsmith or forgejo"
                mirror=$2; shift 2 ;;
            --mirror=*) mirror=${1#*=}; shift ;;
            --user) user_install=1; shift ;;
            --prefix) [ "$#" -ge 2 ] || usage_error "--prefix needs a directory, e.g. --prefix \"\$HOME/.local\""
                prefix=$2; shift 2 ;;
            --prefix=*) prefix=${1#*=}; shift ;;
            --develop) develop_ref=develop
                if [ "$#" -ge 2 ]; then case "$2" in -*) ;; *) develop_ref=$2; shift ;; esac; fi
                shift ;;
            --develop=*) develop_ref=${1#*=}; shift ;;
            -y|--yes) assume_yes=1; shift ;;
            -h|--help) usage; exit 0 ;;
            --) shift; break ;;
            -*) usage_error "unknown option: $1" ;;
            *) usage_error "unexpected argument: $1" ;;
        esac
    done
}

writable_dir() {
    probe=$(mktemp "$1/.blastit-install-probe.XXXXXX" 2>/dev/null) || return 1
    rm -f "$probe" 2>/dev/null || true
    return 0
}

validate_args() {
    case "$mirror" in cloudsmith|forgejo) ;; *) usage_error "--mirror must be cloudsmith or forgejo (got '$mirror')" ;; esac
    if [ "$nightly" -eq 1 ] && [ "$version_set" -eq 1 ]; then
        usage_error "--nightly and --version are exclusive: the nightly component has no fixed version"
    fi
    if [ "$nightly" -eq 1 ] && { [ "$user_install" -eq 1 ] || [ -n "$prefix" ] || [ -n "$develop_ref" ]; }; then
        usage_error "--nightly installs from the signed repository; it does not combine with --user, --prefix or --develop"
    fi
    token_ok "$version" || usage_error "--version must be a version such as 0.2.0 (letters, digits, '.', '_', '-' only; got '$version')"
    if [ -n "$develop_ref" ]; then
        token_ok "$develop_ref" ||
            usage_error "--develop must be a branch or tag name (letters, digits, '.', '_', '-' only; got '$develop_ref')"
    fi
    [ "$version_set" -eq 0 ] || pin=$version
    if [ -n "$prefix" ]; then
        case "$prefix" in /*) ;; *) usage_error "--prefix must be an absolute path (got '$prefix')" ;; esac
        mkdir -p "$prefix" 2>/dev/null ||
            usage_error "--prefix $prefix cannot be created: choose a directory you own, e.g. --prefix \"\$HOME/.local\""
        [ -d "$prefix" ] || usage_error "--prefix $prefix is not a directory: choose a directory you own"
        writable_dir "$prefix" ||
            usage_error "--prefix $prefix is not writable by $(id -un 2>/dev/null || printf 'this user'): choose a directory you own"
    fi
}

# --- terminal & privileges ---------------------------------------------------

tty_available() { [ -r /dev/tty ] && [ -w /dev/tty ] && ( : </dev/tty ) 2>/dev/null; }

confirm() {
    tty_available || return 0
    [ "$assume_yes" -eq 1 ] && return 0
    printf '%s [Y/n] ' "$1" >&2
    reply=
    IFS= read -r reply </dev/tty || reply=
    case "$reply" in ''|[Yy]|[Yy][Ee][Ss]) return 0 ;; *) return 1 ;; esac
}

privilege=none

detect_privilege() {
    [ "$(id -u)" -eq 0 ] && { privilege=root; return 0; }
    if command -v sudo >/dev/null 2>&1; then
        if sudo -n true 2>/dev/null </dev/null; then privilege=sudo; return 0; fi
        if tty_available; then privilege=sudo-tty; return 0; fi
    fi
    privilege=none
}

# `</dev/null` on the non-interactive branches is load-bearing: this script
# arrived on fd 0 via `curl … | sh`, and bash reads a piped script lazily, so a
# child that reads stdin (a setup script, apt-get, dpkg, dnf) would otherwise eat
# the rest of the installer and the run would exit 0 having installed nothing.
as_root() {
    case "$privilege" in
        root) "$@" </dev/null ;;
        sudo) sudo -n "$@" </dev/null ;;
        sudo-tty) sudo "$@" </dev/tty ;;
        *) die 1 "this step needs root: re-run with sudo, or install into your home: $SELF --user" ;;
    esac
}

# --- downloads & digests -----------------------------------------------------

fetch() { # <url> <out>
    if command -v curl >/dev/null 2>&1; then
        case "$1" in
            https://*) curl -fsSL --proto '=https' --proto-redir '=https' -o "$2" "$1" </dev/null ;;
            *) curl -fsSL -o "$2" "$1" </dev/null ;;
        esac
    elif command -v wget >/dev/null 2>&1; then
        case "$1" in
            https://*) wget -q --https-only -O "$2" "$1" </dev/null ;;
            *) wget -q -O "$2" "$1" </dev/null ;;
        esac
    else
        die 1 "curl or wget is required to download $1: install one, or use a release asset you already have"
    fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{ print $1 }'
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{ print $1 }'
    elif command -v openssl >/dev/null 2>&1; then openssl dgst -sha256 "$1" | sed 's/^.*= //'
    else return 1
    fi
}

# Digests of the release archives this installer was published with, keyed by the
# version this copy was published for (the Docs job refuses a table keyed to any
# other version, and refuses a `v*` tag whose rows are still `-`). `-` means the
# release published no pinned digest for that target, so the archive is checked
# against the canonical origin's own `SHA256SUMS`: that is the *same* trust root
# as the download, so it proves the bytes arrived intact and proves nothing about
# who published them. An overridden origin never reaches this table at all.
embedded_sha256() { # <version> <target>
    case "$1:$2" in
        "$BLASTIT_DEFAULT_VERSION":x86_64-unknown-linux-gnu) printf '%s\n' - ;;
        "$BLASTIT_DEFAULT_VERSION":aarch64-unknown-linux-gnu) printf '%s\n' - ;;
        "$BLASTIT_DEFAULT_VERSION":x86_64-unknown-linux-musl) printf '%s\n' - ;;
        "$BLASTIT_DEFAULT_VERSION":aarch64-unknown-linux-musl) printf '%s\n' - ;;
    esac
}

sums_digest() { awk -v n="$2" '{ f = $2; sub(/^\*/, "", f) } f == n { print $1; exit }' "$1"; }

digest=

# Resolves the digest to demand *before* a byte is downloaded, so an origin that
# cannot be pinned costs one error line and no traffic.
resolve_digest() { # <asset>
    name=$1; expected=
    if [ -n "${BLASTIT_SHA256:-}" ]; then
        expected=$BLASTIT_SHA256
    elif [ -n "${BLASTIT_SHA256SUMS_FILE:-}" ]; then
        [ -f "$BLASTIT_SHA256SUMS_FILE" ] ||
            die 1 "BLASTIT_SHA256SUMS_FILE=$BLASTIT_SHA256SUMS_FILE does not exist: point it at a local SHA256SUMS file"
        expected=$(sums_digest "$BLASTIT_SHA256SUMS_FILE" "$name")
        [ -n "$expected" ] || die 1 "$name has no entry in $BLASTIT_SHA256SUMS_FILE: set BLASTIT_SHA256=<hex> instead"
    elif [ -n "${BLASTIT_RELEASE_BASE_URL:-}" ] || [ -n "$develop_ref" ]; then
        die 1 "an overridden release origin is unsigned, so this installer will not guess: set BLASTIT_SHA256=<hex> (or BLASTIT_SHA256SUMS_FILE=<local file>) to the digest you checked"
    else
        expected=$(embedded_sha256 "$version" "$target")
        if [ -z "$expected" ] || [ "$expected" = - ]; then
            note "checking $name against the release's SHA256SUMS"
            fetch "$(asset_url SHA256SUMS)" "$work/SHA256SUMS" ||
                die 1 "cannot download the release's SHA256SUMS for v$version: set BLASTIT_SHA256=<hex> to install the archive you verified"
            expected=$(sums_digest "$work/SHA256SUMS" "$name")
            [ -n "$expected" ] || die 1 "$name has no entry in the release's SHA256SUMS: set BLASTIT_SHA256=<hex>, or install from the repository: $SELF"
        fi
    fi
    expected=$(printf '%s' "$expected" | tr 'A-F' 'a-f')
    case "$expected" in
        ''|-) die 1 "no digest to verify $name against: set BLASTIT_SHA256=<hex>, or install from the signed repository: $SELF" ;;
        *[!0-9a-f]*) die 1 "the expected digest for $name is not hexadecimal: set BLASTIT_SHA256=<the 64 hex digits of the file you checked>" ;;
    esac
    [ "${#expected}" -eq 64 ] ||
        die 1 "the expected digest for $name is ${#expected} characters, not 64: copy the full sha256 of $name"
    digest=$expected
}

verify_archive() { # <file> <name> <expected-hex>
    actual=$(sha256_of "$1") || actual=
    case "$actual" in
        ''|*[!0-9a-f]*) die 1 "no usable sha256 digest for $2: install coreutils (sha256sum), shasum or openssl, then re-run" ;;
    esac
    [ "${#actual}" -eq 64 ] ||
        die 1 "the local sha256 tool returned ${#actual} characters for $2, not 64: re-run, or install coreutils/openssl"
    [ "$actual" = "$3" ] ||
        die 1 "sha256 mismatch for $2: expected $3, got $actual — refusing to install it; download it again from the official origin"
    note "verified $2 sha256 $(printf '%.12s' "$actual")…"
}

# --- platform ----------------------------------------------------------------

arch=; libc=; target=; pkgmgr=none; rpm_tool=

detect_platform() {
    case "$(uname -s)" in Linux) ;; *) die 1 "Blastit installs on Linux here (uname -s reports '$(uname -s)'): see https://offworldnexus.github.io/blastit/" ;; esac
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) die 1 "unsupported CPU architecture '$(uname -m)': Blastit ships x86_64 and aarch64 Linux archives; open an issue with this line" ;;
    esac
    if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then libc=musl; else libc=gnu; fi
    target="$arch-unknown-linux-$libc"
    if command -v dnf >/dev/null 2>&1; then pkgmgr=rpm; rpm_tool=dnf
    elif command -v yum >/dev/null 2>&1; then pkgmgr=rpm; rpm_tool=yum
    elif command -v apt-get >/dev/null 2>&1 && [ -f /etc/debian_version ]; then pkgmgr=deb
    fi
}

pick() { if [ "$nightly" -eq 1 ]; then printf '%s\n' "$2"; else printf '%s\n' "$1"; fi; }

# A musl `blastit` is a dynamic binary: it needs `libgcc_s.so.1`, and a bare
# Alpine image carries none (`docs/install.md` names the package). `apk` is how
# those hosts are told apart.
libgcc_present() { [ -e /usr/lib/libgcc_s.so.1 ] || [ -e /lib/libgcc_s.so.1 ]; }

# The root path's one prerequisite, installed before anything is placed: a
# missing `libgcc_s.so.1` otherwise shows up as "blastit did not run" after an
# install that did everything else right.
ensure_libgcc() {
    command -v apk >/dev/null 2>&1 || return 0
    libgcc_present && return 0
    step "installing libgcc with apk add --no-cache libgcc"
    as_root apk add --no-cache libgcc ||
        die 1 "blastit needs libgcc_s.so.1 here and 'apk add --no-cache libgcc' failed: check the network or the apk repository, then re-run this installer"
}

# --- repository installs -----------------------------------------------------

# Switches the component of the source file this run created, never of one that
# was already there: a `--nightly` install may edit its own line and nothing else.
select_apt_component() { # <component> <before-list>
    found=0
    for file in /etc/apt/sources.list.d/*.list; do
        [ -f "$file" ] || continue
        grep -qxF "${file##*/}" "$2" 2>/dev/null && continue
        grep -q 'offworld-nexus' "$file" 2>/dev/null || continue
        found=1
        grep -Eq '^deb .*[[:space:]][A-Za-z0-9._-]+$' "$file" ||
            die 1 "cannot identify the component of $file: run 'grep offworld-nexus $file' and set it to $1 by hand"
        as_root sed -i "/^deb /s/[[:space:]][A-Za-z0-9._-]*\$/ $1/" "$file"
        grep -Eq "^deb .* $1\$" "$file" ||
            die 1 "the repository setup left no '$1' component in $file: run 'grep offworld-nexus $file' and fix the source line, or install the release: $SELF"
    done
    [ "$found" -eq 1 ] ||
        die 1 "the repository setup wrote no new offworld-nexus APT source: run 'grep -r offworld-nexus /etc/apt/sources.list.d' and fix it by hand, or install the release: $SELF"
    note "APT component set to $1"
}

# Snapshots the source directory, then runs the vendor's setup script, so the
# component switch above can tell this run's file from a pre-existing one.
apt_sources_before() {
    ls /etc/apt/sources.list.d 2>/dev/null >"$work/sources.before" || : >"$work/sources.before"
}

setup_cloudsmith() { # <deb|rpm>
    if [ "$1" = deb ]; then
        repo=${CLOUDSMITH_APT_REPO:-apt}; script=bash.deb.sh
        step "adding the signed Blastit APT repository (hosted by Cloudsmith)"
    else
        # RPM repos carry no components: the nightly channel is its own repo, and
        # the release job must publish to the name this reads.
        repo=$(pick "${CLOUDSMITH_RPM_REPO:-rpm}" "${CLOUDSMITH_RPM_NIGHTLY_REPO:-rpm-nightly}")
        script=bash.rpm.sh
        step "adding the signed Blastit RPM repository (hosted by Cloudsmith)"
    fi
    fetch "$CLOUDSMITH_PUBLIC/$repo/$script" "$work/$script" ||
        die 1 "cannot download $CLOUDSMITH_PUBLIC/$repo/$script: check your network, or install the archive: $SELF --user"
    if [ -n "${BLASTIT_SETUP_SCRIPT_SHA256:-}" ]; then
        verify_archive "$work/$script" "$script" "$(printf '%s' "$BLASTIT_SETUP_SCRIPT_SHA256" | tr 'A-F' 'a-f')"
    fi
    note "running Cloudsmith's repository setup script (HTTPS, not signature-verified; the packages it enables are GPG-verified)"
    [ "$1" != deb ] || apt_sources_before
    as_root sh "$work/$script" ||
        die 1 "the Cloudsmith repository setup failed: re-run this installer with sudo, or install the archive: $SELF --user"
    if [ "$1" = deb ] && [ "$nightly" -eq 1 ]; then select_apt_component nightly "$work/sources.before"; fi
}

setup_forgejo() { # <deb|rpm>
    base="${BLASTIT_FORGEJO_URL:-$FORGEJO_DEFAULT}/api/packages/$FORGEJO_OWNER/$1"
    if [ "$1" = deb ]; then step "adding the signed Blastit APT mirror (hosted by Forgejo)"
    else step "adding the signed Blastit RPM mirror (hosted by Forgejo)"
    fi
    fetch "$base/public.key" "$work/forgejo-$1.key" ||
        die 1 "no public key at $base/public.key: nothing to trust, so nothing is installed — re-run without --mirror to use the default signed repository: $SELF"
    if [ "$1" = deb ]; then
        command -v gpg >/dev/null 2>&1 || die 1 "gpg is needed to trust the Forgejo key: install gnupg, then re-run"
        as_root install -d -m 0755 /usr/share/keyrings
        as_root sh -c 'gpg --batch --yes --dearmor --output "$1" "$2"' _ \
            /usr/share/keyrings/blastit-archive-keyring.gpg "$work/forgejo-deb.key" ||
            die 1 "cannot write the Forgejo key into /usr/share/keyrings: re-run with sudo"
        printf 'deb [signed-by=/usr/share/keyrings/blastit-archive-keyring.gpg] %s any-distro %s\n' \
            "$base" "$(pick main nightly)" >"$work/blastit.list"
        as_root install -m 0644 "$work/blastit.list" /etc/apt/sources.list.d/blastit.list
    else
        as_root rpm --import "$work/forgejo-rpm.key" || die 1 "cannot import the Forgejo RPM key: re-run with sudo"
        printf '[blastit]\nname=Blastit (Forgejo mirror)\nbaseurl=%s\nenabled=1\ngpgcheck=1\n' \
            "$base" >"$work/blastit.repo"
        as_root install -m 0644 "$work/blastit.repo" /etc/yum.repos.d/blastit.repo
    fi
}

install_repo_deb() {
    confirm "Install Blastit $(pick "$version" nightly) system-wide from the signed Blastit APT repository? (needs sudo; later updates are plain apt-get upgrade)" ||
        die 1 "cancelled: nothing was installed; re-run with: $SELF -y"
    if [ "$mirror" = forgejo ]; then setup_forgejo deb; else setup_cloudsmith deb; fi
    step "updating the package list, then installing blastit"
    as_root apt-get update || die 1 "apt-get update failed: run 'sudo apt-get update' to see why, then re-run this installer"
    spec=$PKG${pin:+=$pin}
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$spec" ||
        die 1 "apt-get could not install $spec: check 'apt-cache policy $PKG' (components: main, nightly), or install without --version"
    report_installed
}

install_repo_rpm() {
    confirm "Install Blastit $(pick "$version" nightly) system-wide from the signed Blastit RPM repository? (needs sudo; later updates are plain $rpm_tool upgrade)" ||
        die 1 "cancelled: nothing was installed; re-run with: $SELF -y"
    if [ "$mirror" = forgejo ]; then setup_forgejo rpm; else setup_cloudsmith rpm; fi
    spec=$PKG${pin:+-$pin}
    step "installing blastit with $rpm_tool"
    as_root "$rpm_tool" install -y "$spec" ||
        die 1 "$rpm_tool could not install $spec: check '$rpm_tool --showduplicates list $PKG', or install without --version"
    report_installed
}

report_installed() {
    if command -v blastit >/dev/null 2>&1; then
        installed=$(blastit --version 2>/dev/null </dev/null || true)
        ok "installed: ${installed:-blastit}"
    else
        warn "blastit is installed but not on PATH in this shell: open a new shell, or check 'command -v blastit'"
    fi
    note "next: blastit dev — https://offworldnexus.github.io/blastit/getting-started/"
    case "$pkgmgr" in
        deb) note "upgrade later with: sudo apt-get update && sudo apt-get install --only-upgrade $PKG" ;;
        rpm) note "upgrade later with: sudo $rpm_tool upgrade $PKG" ;;
        *) : ;;
    esac
    if [ "$nightly" -eq 1 ]; then note "nightly builds are replaced continuously; return to a release with: $SELF"; fi
}

# --- archive installs --------------------------------------------------------

asset_url() { # <asset>
    if [ -n "${BLASTIT_RELEASE_BASE_URL:-}" ]; then printf '%s/%s\n' "${BLASTIT_RELEASE_BASE_URL%/}" "$1"
    elif [ -n "$develop_ref" ]; then printf '%s/%s/%s\n' "$GITHUB_DOWNLOAD" "$develop_ref" "$1"
    else printf '%s/v%s/%s\n' "$GITHUB_DOWNLOAD" "$version" "$1"
    fi
}

resolve_prefix() {
    if [ -z "$prefix" ]; then
        if [ "$privilege" = root ] && [ "$user_install" -eq 0 ]; then
            prefix=/usr/local
        else
            [ -n "${HOME:-}" ] || die 1 "HOME is unset: pass --prefix <dir>"
            prefix=$HOME/.local
        fi
    fi
    mkdir -p "$prefix" 2>/dev/null ||
        die 1 "cannot create $prefix: pass --prefix <a directory you can write>, e.g. --prefix \"\$HOME/.local\""
    writable_dir "$prefix" ||
        die 1 "$prefix is not writable by $(id -un 2>/dev/null || printf 'this user'): pass --prefix <a directory you can write>"
    note "installing into $prefix"
}

install_tree() { # <tree>
    tree=$1; libdir=$prefix/share/blastit/lib
    if [ -f "$tree/share/blastit/lib/blastit" ]; then
        install -d -m 0755 "$prefix/bin"
        cp -RP "$tree/share" "$prefix/"
    else
        install -d -m 0755 "$prefix/bin" "$prefix/share/blastit" "$libdir"
        for entry in "$tree"/blastit "$tree"/blastit-lsp "$tree"/libblastit_*.so; do
            [ -f "$entry" ] || continue
            [ -h "$entry" ] && continue
            case "$entry" in *.so) install -m 0644 "$entry" "$libdir/" ;; *) install -m 0755 "$entry" "$libdir/" ;; esac
        done
        for entry in "$tree"/LICENSE "$tree"/README.md; do
            [ -f "$entry" ] || continue
            [ -h "$entry" ] && continue
            install -m 0644 "$entry" "$prefix/share/blastit/"
        done
    fi
    for bin in blastit blastit-lsp; do
        [ -f "$libdir/$bin" ] || continue
        ln -sfn "../share/blastit/lib/$bin" "$prefix/bin/$bin"
    done
}

unpack_archive() { # <file>
    listing=$work/archive.list
    tar -tzf "$1" >"$listing" 2>/dev/null || die 1 "$1 is not a readable .tar.gz: delete it and re-run this installer"
    # Names *and* link targets: a member that is a symlink to /etc/hostname would
    # otherwise be recreated here and copied into the prefix as an executable.
    links=$(tar -tzvf "$1" 2>/dev/null | awk '$1 ~ /^[lh]/ { print $NF }') || links=
    [ -z "$links" ] || die 1 "$1 contains a link member ($links): refusing to unpack it"
    if grep -Eq '(^/|^\.\./|/\.\./|/\.\.$)' "$listing"; then
        die 1 "$1 contains an absolute or parent-relative path: refusing to unpack it"
    fi
    toplevel=$(sed -n '1p' "$listing" | cut -d/ -f1)
    case "$toplevel" in ''|.|..) die 1 "$1 does not hold a single top-level directory: refusing to unpack it" ;; esac
    mkdir -p "$work/unpack"
    tar -xzf "$1" -C "$work/unpack" || die 1 "cannot unpack $1"
    [ -f "$work/unpack/$toplevel/blastit" ] || [ -f "$work/unpack/$toplevel/share/blastit/lib/blastit" ] ||
        die 1 "$1 holds neither blastit nor share/blastit/lib/blastit at its top level: refusing to install a tree this installer does not recognise"
    install_tree "$work/unpack/$toplevel"
}

install_archive() {
    resolve_prefix
    asset="blastit-$target.tar.gz"
    resolve_digest "$asset"
    url=$(asset_url "$asset")
    step "downloading $url"
    fetch "$url" "$work/$asset" || die 1 "cannot download $url: check the URL, or install from the repository: $SELF"
    verify_archive "$work/$asset" "$asset" "$digest"
    unpack_archive "$work/$asset"
    installed=$("$prefix/bin/blastit" --version 2>/dev/null </dev/null || true)
    if [ -z "$installed" ]; then
        # `apk` decides whether `apk add` is advice this host can follow: an
        # apk-less musl host (Void, Chimera, busybox) has nothing to install the
        # package with, so it keeps the generic sentence below.
        if command -v apk >/dev/null 2>&1 && [ "$libc" = musl ] && ! libgcc_present; then
            die 1 "$prefix/bin/blastit did not run: this musl host has no libgcc_s.so.1 — run: apk add libgcc, then re-run this installer"
        fi
        die 1 "$prefix/bin/blastit did not run: remove it and re-run this installer, or report the failure at https://github.com/OffworldNexus/blastit/issues"
    fi
    ok "installed: $installed in $prefix"
    note "next: blastit dev — https://offworldnexus.github.io/blastit/getting-started/"
    note "archive installs do not upgrade themselves: re-run this installer to update"
    case ":${PATH:-}:" in
        *":$prefix/bin:"*) note "blastit is already on your PATH: run 'blastit --version'" ;;
        *) note "add it to your PATH: export PATH=\"$prefix/bin:\$PATH\" (put that line in your shell's startup file to keep it)" ;;
    esac
}

# --- local package files -----------------------------------------------------

install_local_file() { # <deb|rpm> <path> <hint>
    [ -f "$2" ] || die 1 "$2 does not exist$3"
    if [ "$1" = deb ]; then
        command -v dpkg >/dev/null 2>&1 ||
            die 1 "dpkg is not installed: a .deb needs a Debian-based host (or install the archive: $SELF --user)"
        pkgmgr=deb
        [ "$privilege" != none ] || die 1 "installing a package needs root. Run: sudo dpkg -i \"$2\""
        step "installing $2 with dpkg -i"
        as_root dpkg -i "$2" || die 1 "dpkg refused $2. Run: sudo dpkg -i \"$2\" to see the full output"
    else
        command -v rpm >/dev/null 2>&1 ||
            die 1 "rpm is not installed: a .rpm needs a Fedora/RHEL-based host (or install the archive: $SELF --user)"
        pkgmgr=rpm
        [ "$privilege" != none ] || die 1 "installing a package needs root. Run: sudo rpm -Uvh --replacepkgs \"$2\""
        step "installing $2 with rpm -Uvh --replacepkgs"
        as_root rpm -Uvh --replacepkgs "$2" ||
            die 1 "rpm refused $2. Run: sudo rpm -Uvh --replacepkgs \"$2\" to see the full output"
    fi
    report_installed
}

# --- main --------------------------------------------------------------------

archive_fallback() {
    if [ "$nightly" -eq 1 ]; then
        die 1 "nightly builds ship through the signed repository, which this host cannot use: re-run without --nightly to install the release archive: $SELF --user"
    fi
    install_archive
}

main() {
    parse_args "$@"
    validate_args
    work=$(mktemp -d "${TMPDIR:-/tmp}/blastit-install.XXXXXX") ||
        die 1 "cannot create a temporary directory under ${TMPDIR:-/tmp}: set TMPDIR to a writable path"
    trap 'rm -rf "$work"' EXIT HUP INT TERM

    if [ -n "${BLASTIT_DEB:-}" ] && [ -n "${BLASTIT_RPM:-}" ]; then
        usage_error "BLASTIT_DEB and BLASTIT_RPM are exclusive: install one file at a time"
    fi
    detect_platform
    detect_privilege

    # A root install on a musl host owes the one library the binary needs. The
    # `--user` path cannot install it — that is the hint in `install_archive`.
    if [ "$user_install" -eq 0 ] && [ -z "$prefix" ] && [ "$privilege" = root ]; then
        ensure_libgcc
    fi

    if [ -n "${BLASTIT_DEB:-}" ]; then
        install_local_file deb "$BLASTIT_DEB" ": build one with 'make packages', or point BLASTIT_DEB at the file"
        return
    fi
    if [ -n "${BLASTIT_RPM:-}" ]; then
        install_local_file rpm "$BLASTIT_RPM" ": build one with 'make packages', or point BLASTIT_RPM at the file"
        return
    fi
    if [ -n "$develop_ref" ] || [ "$user_install" -eq 1 ] || [ -n "$prefix" ]; then
        install_archive
        return
    fi
    if [ "$libc" = musl ]; then
        warn "musl host: the signed .deb/.rpm repositories are glibc-only, so this installs the musl archive"
        archive_fallback
        return
    fi
    if [ "$pkgmgr" = none ]; then
        warn "no apt-get or dnf found: this distro has no signed repository here, so this installs the release archive"
        archive_fallback
        return
    fi
    if [ "$privilege" = none ]; then
        warn "no root or usable sudo here: installing the release archive under \$HOME/.local instead (run with sudo for the signed repository)"
        archive_fallback
        return
    fi
    case "$pkgmgr" in
        deb) install_repo_deb ;;
        rpm) install_repo_rpm ;;
    esac
}

main "$@"
