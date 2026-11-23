#!/bin/sh
# Proves `docs/install.sh` without touching the host: every install lands in a
# scratch prefix, every download comes from a fixture directory served on
# 127.0.0.1, and no package is ever installed (`MB.42.3.2`, MAJOR 6).
#
#   sh scripts/test_install.sh
#
# Cases: (a) `--user` installs and `--version` runs, (b) a wrong digest refuses,
# (c) an overridden origin with no digest refuses *before* any request, (d) an
# archive with a symlink member is refused, (e) `--help` works with `env -i`,
# (f) usage errors exit 2, (g) a piped `curl … | bash` run whose child reads
# stdin still reaches the installer's last line (the BLOCKER 1 regression),
# (h) the apt path writes the canonical `.sources` file and installs the shim of
# the interpreter this host has (and none when it has none), (i) the local-file
# path installs the core package plus only the sibling shims whose runtime is
# present, in one transaction, and its rpm half passes `--nosignature` only when
# `BLASTIT_PKG_ACCEPT_UNSIGNED=1`, and (j) the generated blocks in
# `docs/install.md`/`docs/install.sh` match `packaging/languages.tsv` and the
# workspace version, read from `Cargo.toml` through `scripts/version.sh` — the
# same loader the faked package names below come from, so a language row or a
# version bump needs no edit here. (h) and (i) redirect every `/etc` write into the
# scratch tree with a stub `install`, and stub `apt-get`/`dpkg`/`curl`/`sudo`, so
# no real package tool and no real repository is ever involved. The interpreter
# minors in every case are the table's, read here.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
installer=$root/docs/install.sh
scratch=${BLASTIT_INSTALL_TEST_DIR:-${TMPDIR:-/tmp}/opencode}/install-sh-test
uv=${BLASTIT_TEST_UV:-uv}
asset=blastit-x86_64-unknown-linux-gnu.tar.gz
# The workspace version is `Cargo.toml`'s, through its one loader: the fixture
# binary and every faked package name below carry it, so a release needs no edit
# here.
workspace_version=$(bash "$root/scripts/version.sh") ||
    { printf 'install-test: FAIL scripts/version.sh printed no workspace version\n' >&2; exit 1; }
fixture_version="blastit $workspace_version (fixture)"

# The interpreter versions are `packaging/languages.tsv`'s, read here instead of
# spelled: adding or withdrawing a row changes what (h) and (i) exercise with no
# edit to this file.
languages=$root/packaging/languages.sh
py_minors=$(bash "$languages" --py-minors | sort -t. -k1,1n -k2,2n | tr '\n' ' ')
php_minors=$(bash "$languages" --php-minors | sort -t. -k1,1n -k2,2n | tr '\n' ' ')
py_primary=$(printf '%s\n' $py_minors | sed -n 1p)
py_other=$(printf '%s\n' $py_minors | sed -n 2p)
py_slashed=$(printf '%s\n' $py_minors | paste -sd/ -)
php_primary=$(printf '%s\n' $php_minors | sed -n 1p)

fail() { printf 'install-test: FAIL %s\n' "$*" >&2; [ ! -f "$scratch/last.log" ] || tail -20 "$scratch/last.log" >&2; exit 1; }
pass() { printf 'install-test: ok %s\n' "$*"; }

server=
cleanup() { [ -z "$server" ] || kill "$server" 2>/dev/null || true; }
trap cleanup EXIT HUP INT TERM

command -v curl >/dev/null 2>&1 || fail "curl is required to drive the fixture over HTTP"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required to build the fixture's digests"
[ -f "$installer" ] || fail "$installer is missing"

rm -rf "$scratch"
mkdir -p "$scratch/site" "$scratch/bin" "$scratch/tmp"
TMPDIR=$scratch/tmp; export TMPDIR

# The fixture archive is the layout `scripts/build-standalone.sh` ships — one flat
# `blastit-<target>/` — and its `blastit` answers `--version` like the real one,
# so what is under test is the installer, never a stand-in for it.
tree=$scratch/tree/$asset
tree=${tree%.tar.gz}
mkdir -p "$tree"
printf '#!/bin/sh\nprintf "%%s\\n" "%s"\n' "$fixture_version" >"$tree/blastit"
printf 'fixture license\n' >"$tree/LICENSE"
chmod 0755 "$tree/blastit"
tar -czf "$scratch/site/$asset" -C "$scratch/tree" "${tree##*/}"
(cd "$scratch/site" && sha256sum "$asset" >SHA256SUMS)
sha=$(awk '{print $1}' "$scratch/site/SHA256SUMS")
zeros=$(awk 'BEGIN { while (i++ < 64) printf "0" }')

start_server() {
    for candidate in 8745 8746 8747 8748 8749 8750; do
        "$uv" run --no-project python -m http.server "$candidate" --bind 127.0.0.1 \
            --directory "$scratch/site" >>"$scratch/http.log" 2>&1 &
        server=$!
        tries=0
        while [ "$tries" -lt 60 ]; do
            if curl -fsS -o /dev/null "http://127.0.0.1:$candidate/" 2>/dev/null; then
                port=$candidate
                return 0
            fi
            kill -0 "$server" 2>/dev/null || break
            tries=$((tries + 1)); sleep 0.05
        done
        kill "$server" 2>/dev/null || true
        wait "$server" 2>/dev/null || true
        server=
    done
    return 1
}

start_server || fail "no fixture port (8745-8750) could be bound on 127.0.0.1"
base=http://127.0.0.1:$port
requests() { sleep 0.2; grep -c 'GET ' "$scratch/http.log" 2>/dev/null || true; }

log() { sed "s|$scratch|SCRATCH|g" "$1" >&2; }

# (a) the happy archive path --------------------------------------------------
log_a=$scratch/a.log
if ! BLASTIT_RELEASE_BASE_URL=$base BLASTIT_SHA256=$sha \
    sh "$installer" --user --prefix "$scratch/prefix" -y >"$log_a" 2>&1; then
    log "$log_a"; fail "(a) --user install exited non-zero"
fi
version=$("$scratch/prefix/bin/blastit" --version)
[ "$version" = "$fixture_version" ] || { log "$log_a"; fail "(a) installed blastit answered '$version'"; }
[ "$(readlink "$scratch/prefix/bin/blastit")" = ../share/blastit/lib/blastit ] ||
    fail "(a) bin/blastit is not the §6.1 symlink"
grep -q 'verified .* sha256' "$log_a" || fail "(a) the digest was never reported as verified"
grep -q 'archive installs do not upgrade themselves' "$log_a" || fail "(a) the upgrade caveat is missing"
pass "(a) --user installs into a scratch prefix; blastit --version runs ($version)"

# (b) a wrong digest ----------------------------------------------------------
log_b=$scratch/b.log
code=0
BLASTIT_RELEASE_BASE_URL=$base BLASTIT_SHA256=$zeros \
    sh "$installer" --user --prefix "$scratch/bad" -y >"$log_b" 2>&1 || code=$?
[ "$code" -eq 1 ] || { log "$log_b"; fail "(b) a wrong digest exited $code, not 1"; }
grep -q 'sha256 mismatch' "$log_b" || { log "$log_b"; fail "(b) the refusal does not name the mismatch"; }
[ ! -e "$scratch/bad/bin/blastit" ] || fail "(b) a mismatched archive reached the prefix"
pass "(b) a wrong digest exits 1 and installs nothing"

# (c) an overridden origin with no digest -------------------------------------
before=$(requests)
log_c=$scratch/c.log
code=0
BLASTIT_RELEASE_BASE_URL=$base sh "$installer" --user --prefix "$scratch/nodigest" -y >"$log_c" 2>&1 || code=$?
[ "$code" -eq 1 ] || { log "$log_c"; fail "(c) an unpinned origin exited $code, not 1"; }
grep -q 'BLASTIT_SHA256' "$log_c" || { log "$log_c"; fail "(c) the refusal names no next command"; }
after=$(requests)
[ "$before" = "$after" ] || fail "(c) the fixture served $((after - before)) request(s) before refusing"
pass "(c) an overridden origin without a digest fails closed with zero requests"

# (d) a hostile archive: a symlink member -------------------------------------
mkdir -p "$scratch/evil/tree-1.0"
printf '#!/bin/sh\necho pwn\n' >"$scratch/evil/tree-1.0/blastit"
chmod 0755 "$scratch/evil/tree-1.0/blastit"
ln -s /etc/hostname "$scratch/evil/tree-1.0/blastit-lsp"
tar -czf "$scratch/site/$asset" -C "$scratch/evil" tree-1.0
evil_sha=$(sha256sum "$scratch/site/$asset" | awk '{ print $1 }')
log_d=$scratch/d.log
code=0
BLASTIT_RELEASE_BASE_URL=$base BLASTIT_SHA256=$evil_sha \
    sh "$installer" --user --prefix "$scratch/hostile" -y >"$log_d" 2>&1 || code=$?
[ "$code" -eq 1 ] || { log "$log_d"; fail "(d) an archive with a symlink member exited $code, not 1"; }
grep -q 'link member' "$log_d" || { log "$log_d"; fail "(d) the refusal does not name the link member"; }
[ ! -e "$scratch/hostile/bin/blastit" ] || fail "(d) a hostile archive reached the prefix"
[ ! -e "$scratch/hostile/share/blastit/lib/blastit-lsp" ] || fail "(d) a link member was copied into the prefix"
pass "(d) an archive with a symlink member is refused before unpacking"

# (e) --help with nothing in the environment ----------------------------------
log_e=$scratch/e.log
if ! env -i PATH=/usr/bin:/bin sh "$installer" --help >"$log_e" 2>&1; then
    log "$log_e"; fail "(e) --help exited non-zero under env -i"
fi
grep -q '^usage: install.sh' "$log_e" || fail "(e) --help printed no usage"
grep -q 'curl … | sh' "$log_e" || fail "(e) --help lost the pipe sentence"
pass "(e) --help exits 0 with env -i (no root, no network)"

# (f) usage errors ------------------------------------------------------------
code=0
sh "$installer" --prefix relative/path >/dev/null 2>&1 || code=$?
[ "$code" -eq 2 ] || fail "(f) a relative --prefix exited $code, not 2"
code=0
sh "$installer" --nightly --user >/dev/null 2>&1 || code=$?
[ "$code" -eq 2 ] || fail "(f) --nightly --user exited $code, not 2"
code=0
sh "$installer" --version '../../evil' >/dev/null 2>&1 || code=$?
[ "$code" -eq 2 ] || fail "(f) a path-shaped --version exited $code, not 2"
pass "(f) usage errors exit 2 (relative --prefix, --nightly --user, path-shaped --version)"

# (g) BLOCKER 1: a child must not eat the piped script ------------------------
# The child that reads stdin is a stub `dpkg` (the shape of apt-get, dpkg and
# dnf); a stub `sudo` passes its stdin through, as the real one does. Both sit
# first on PATH, so no real package tool is ever invoked.
cat >"$scratch/bin/dpkg" <<'STUB'
#!/bin/sh
if IFS= read -r line; then
    printf 'install-probe: child read stdin: %s\n' "$line"
    exit 3
fi
printf 'install-probe: child stdin was empty\n'
exit 0
STUB
cat >"$scratch/bin/sudo" <<'STUB'
#!/bin/sh
[ "${1:-}" = "-n" ] && shift
[ "${1:-}" = "true" ] && exit 0
exec "$@"
STUB
chmod 0755 "$scratch/bin/dpkg" "$scratch/bin/sudo"
: >"$scratch/fake.deb"
log_g=$scratch/g.log
code=0
{
    cat "$installer"
    printf '%s\n' 'echo install-probe: reached-the-end-of-the-script'
} | env PATH="$scratch/bin:$PATH" BLASTIT_DEB="$scratch/fake.deb" \
    bash -s -- >"$log_g" 2>&1 || code=$?
[ "$code" -eq 0 ] || { log "$log_g"; fail "(g) the piped run exited $code, not 0"; }
grep -q 'child stdin was empty' "$log_g" || { log "$log_g"; fail "(g) a child inherited the installer's fd 0"; }
grep -q 'reached-the-end-of-the-script' "$log_g" || { log "$log_g"; fail "(g) the piped script was truncated before its last line"; }
grep -q 'child read stdin' "$log_g" && { log "$log_g"; fail "(g) the child read the installer's script text"; }
pass "(g) a piped 'curl | bash' run whose child reads stdin reaches the last line"

# (h) the apt repository path -------------------------------------------------
# Nothing here touches the host: `curl` writes the fixture, `apt-get`/`dpkg`/`rpm`
# only record their argv, `sudo` passes its stdin through, `install` rewrites
# every `/etc` destination under a scratch root, and `dpkg-query`/`rpm -q` answer
# from a `present` list — so which runtime packages "exist" is data, not this
# host's own installation.
# The stubs that answer "is this runtime package installed?" are shared by (h)
# and (i) and rewrite nothing, so they exist whether or not this host has apt.
hbin=$scratch/h/bin
present=$scratch/h/present
mkdir -p "$hbin"
: >"$present"
# What the *package manager* says is installed: the installer offers a shim from
# the distro runtime package it recommends, never from a `python3.13` on PATH.
cat >"$hbin/dpkg-query" <<STUB
#!/bin/sh
pkg=
for arg in "\$@"; do
    case "\$arg" in
        -W|-f=*|--showformat=*) ;;
        *) pkg=\$arg ;;
    esac
done
if grep -qx "\$pkg" "$present"; then
    printf 'install ok installed'
    exit 0
fi
printf 'unknown ok not-installed'
exit 1
STUB
cat >"$hbin/rpm" <<STUB
#!/bin/sh
query=0
pkg=
for arg in "\$@"; do
    case "\$arg" in
        -q|--quiet) query=1 ;;
        -*) ;;
        *) pkg=\$arg ;;
    esac
done
if [ "\$query" -eq 1 ]; then
    grep -qx "\$pkg" "$present" || exit 1
    exit 0
fi
printf 'rpm %s\n' "\$*" >>"$scratch/h/rpm.log"
exit 0
STUB
chmod 0755 "$hbin/dpkg-query" "$hbin/rpm"

if [ ! -f /etc/debian_version ] || ! command -v apt-get >/dev/null 2>&1; then
    pass "(h) skipped: this host has no apt-get and /etc/debian_version"
else
    install_bin=$(command -v install)
    hroot=$scratch/h/root
    mkdir -p "$hroot"
    # The directories a real Debian host already has: the stub `install` rewrites
    # `/etc` into `$hroot`, and `install` never creates a parent directory.
    mkdir -p "$hroot/etc/apt/keyrings" "$hroot/etc/apt/sources.list.d"
    printf '#!/bin/sh\n[ "${1:-}" = "-n" ] && shift\n[ "${1:-}" = "true" ] && exit 0\nexec "$@"\n' \
        >"$hbin/sudo"
    cat >"$hbin/curl" <<'STUB'
#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        -*) shift ;;
        *) shift ;;
    esac
done
[ -n "$out" ] || { echo "curl stub: no -o destination" >&2; exit 1; }
printf 'stub keyring bytes\n' >"$out"
STUB
    cat >"$hbin/apt-get" <<STUB
#!/bin/sh
printf 'apt-get %s\n' "\$*" >>"$scratch/h/apt.log"
if [ -f "$scratch/h/fail-shims" ]; then
    case " \$* " in
        *" blastit-python-"*|*" blastit-php-"*) exit 100 ;;
    esac
fi
exit 0
STUB
    cat >"$hbin/install" <<STUB
#!/bin/sh
args=
for arg in "\$@"; do
    case "\$arg" in
        /etc/*) args="\$args $hroot\$arg" ;;
        *) args="\$args \$arg" ;;
    esac
done
# shellcheck disable=SC2086
exec $install_bin \$args
STUB
    # The `dpkg-query`/`rpm` probes are shared with (i) and created above.
    printf '#!/bin/sh\nprintf "blastit %s (stub)\\n"\n' "$workspace_version" >"$hbin/blastit"
    chmod 0755 "$hbin/sudo" "$hbin/curl" "$hbin/apt-get" "$hbin/install" \
        "$hbin/dpkg-query" "$hbin/rpm" "$hbin/blastit"

    # Run 1: the table's first CPython is installed as a *package*, so its shim is
    # offered — and installed as its own transaction, after the core.
    printf 'python%s\n' "$py_primary" >"$present"
    : >"$scratch/h/apt.log"
    log_h=$scratch/h/h1.log
    if ! env PATH="$hbin:$PATH" sh "$installer" -y >"$log_h" 2>&1; then
        log "$log_h"; fail "(h) the apt-repository install exited non-zero"
    fi
    sources=$hroot/etc/apt/sources.list.d/offworld-nexus.sources
    [ -f "$sources" ] || { log "$log_h"; fail "(h) no .sources file was written"; }
    for line in 'Types: deb' 'URIs: https://apt.offworld-nexus.app' \
        'Suites: any-distro' 'Components: main' \
        'Signed-By: /etc/apt/keyrings/offworld-nexus.gpg'; do
        grep -qxF "$line" "$sources" || { log "$log_h"; fail "(h) the .sources file lacks '$line'"; }
    done
    [ -s "$hroot/etc/apt/keyrings/offworld-nexus.gpg" ] ||
        { log "$log_h"; fail "(h) the keyring was not installed"; }
    core_line=$(grep -n 'apt-get install .*--no-install-recommends blastit$' "$scratch/h/apt.log" | cut -d: -f1)
    shim_line=$(grep -nF "blastit-python-$py_primary" "$scratch/h/apt.log" | cut -d: -f1)
    [ -n "$core_line" ] || { log "$log_h"; fail "(h) apt-get never installed the core on its own"; }
    [ -n "$shim_line" ] || { log "$log_h"; fail "(h) the shim of the installed python$py_primary was not installed"; }
    [ "$core_line" -lt "$shim_line" ] ||
        { log "$log_h"; fail "(h) the shim went in before the core (lines $core_line/$shim_line)"; }
    grep -qF "installing blastit blastit-python-$py_primary with apt-get" "$log_h" ||
        { log "$log_h"; fail "(h) the step line does not name the transaction"; }
    grep -qF "language shims installed: blastit-python-$py_primary" "$log_h" ||
        { log "$log_h"; fail "(h) the installed shim is not reported"; }

    # Run 2: the shim's install fails (its runtime package exists but cannot be
    # fetched). The core must stay installed and the refusal must be a warning.
    : >"$scratch/h/apt.log"
    : >"$scratch/h/fail-shims"
    log_h2=$scratch/h/h2.log
    if ! env PATH="$hbin:$PATH" sh "$installer" -y >"$log_h2" 2>&1; then
        log "$log_h2"; fail "(h) a shim that cannot be installed aborted the whole install"
    fi
    rm -f "$scratch/h/fail-shims"
    grep -qF "blastit-python-$py_primary could not be installed" "$log_h2" ||
        { log "$log_h2"; fail "(h) the shim refusal does not name the shim"; }
    grep -q 'candidate:' "$log_h2" ||
        { log "$log_h2"; fail "(h) the shim refusal names no package-manager candidate"; }
    grep -q 'blastit is installed and static views work' "$log_h2" ||
        { log "$log_h2"; fail "(h) the shim refusal does not say blastit is installed"; }
    grep -q 'installed: blastit' "$log_h2" ||
        { log "$log_h2"; fail "(h) blastit is not reported installed after a shim refusal"; }
    grep -q 'language shims installed' "$log_h2" &&
        { log "$log_h2"; fail "(h) a shim that failed is reported as installed"; }

    # Run 3: no runtime package at all — the core alone, and one skip note.
    : >"$present"
    : >"$scratch/h/apt.log"
    log_h3=$scratch/h/h3.log
    if ! env PATH="$hbin:$PATH" sh "$installer" -y >"$log_h3" 2>&1; then
        log "$log_h3"; fail "(h) the install with no runtime package exited non-zero"
    fi
    if grep -Eq 'blastit-(python-3|php-8)' "$scratch/h/apt.log"; then
        log "$log_h3"; fail "(h) a shim was installed with no runtime package on the host"
    fi
    notes=$(grep -cF "this host has no CPython $py_slashed" "$log_h3" || true)
    [ "$notes" -eq 1 ] ||
        { log "$log_h3"; fail "(h) the skipped CPython shims are $notes note(s), not one"; }
    pass "(h) apt installs the core first, each shim alone, and warns instead of aborting on a shim it cannot install"
fi

# (i) the local-file path -----------------------------------------------------
# The core .deb with two sibling shims: the first CPython (its interpreter is a
# package here) and the oldest PHP (no libphp<minor>.so on this host). One dpkg
# transaction, the core plus only the shim this host can load.
case "$(uname -m)" in
    x86_64|amd64) deb_arch=amd64 ;;
    aarch64|arm64) deb_arch=arm64 ;;
    *) fail "(i) unsupported test architecture $(uname -m)" ;;
esac
setdir=$scratch/h/set
mkdir -p "$setdir"
: >"$setdir/blastit_${workspace_version}_${deb_arch}.deb"
: >"$setdir/blastit-python-${py_primary}_${workspace_version}_${deb_arch}.deb"
: >"$setdir/blastit-php-${php_primary}_${workspace_version}_${deb_arch}.deb"
# Both CPython runtimes are installed as packages here, but only the first one's
# shim package is in the directory: the local-file install must warn about the
# other rather than claim it.
printf 'python%s\npython%s\n' "$py_primary" "$py_other" >"$present"
cat >"$scratch/h/bin/dpkg" <<STUB
#!/bin/sh
printf 'dpkg %s\n' "\$*" >>"$scratch/h/dpkg.log"
exit 0
STUB
chmod 0755 "$scratch/h/bin/dpkg"
: >"$scratch/h/dpkg.log"
log_i=$scratch/h/i.log
if ! env PATH="$scratch/h/bin:$PATH" \
    BLASTIT_DEB="$setdir/blastit_${workspace_version}_${deb_arch}.deb" \
    sh "$installer" -y >"$log_i" 2>&1; then
    log "$log_i"; fail "(i) the local-file install exited non-zero"
fi
runs=$(grep -c '^dpkg ' "$scratch/h/dpkg.log" || true)
[ "$runs" -eq 1 ] || { log "$log_i"; fail "(i) dpkg ran $runs times for one package set, not once"; }
command=$(cat "$scratch/h/dpkg.log")
case "$command" in
    *"$setdir/blastit_${workspace_version}_${deb_arch}.deb"*) ;;
    *) log "$log_i"; fail "(i) the core package is not in the dpkg transaction" ;;
esac
case "$command" in
    *"blastit-python-${py_primary}_${workspace_version}_"*) ;;
    *) log "$log_i"; fail "(i) the shim of the installed python$py_primary was not installed" ;;
esac
case "$command" in
    *"blastit-python-${py_other}"*) log "$log_i"; fail "(i) a shim with no sibling package was installed" ;;
esac
case "$command" in
    *"blastit-php-${php_primary}"*) log "$log_i"; fail "(i) a shim with no libphp${php_primary}-embed was installed" ;;
esac
grep -qF "no sibling package for blastit-python-$py_other" "$log_i" ||
    { log "$log_i"; fail "(i) the absent sibling package is not named"; }
grep -qF "sudo apt-get install blastit-python-$py_other" "$log_i" ||
    { log "$log_i"; fail "(i) the warning names no command that installs it"; }
grep -qF "language shims installed: blastit-python-$py_primary" "$log_i" ||
    { log "$log_i"; fail "(i) the installed shim is not reported, or an absent one is claimed"; }
pass "(i) the local-file path installs the core plus only the shim packages it has, and warns about the rest"

# (i) rpm half: Fedora sets `%_pkgverify_level all` and refuses an unsigned
# locally built package, so the escape is explicit and opt-in — `--nosignature`
# with BLASTIT_PKG_ACCEPT_UNSIGNED=1, and no bypass flag at all without it.
case "$(uname -m)" in
    x86_64|amd64) rpm_arch=x86_64 ;;
    aarch64|arm64) rpm_arch=aarch64 ;;
    *) fail "(i) unsupported test architecture $(uname -m)" ;;
esac
rpmset=$scratch/h/rpmset
mkdir -p "$rpmset"
: >"$rpmset/blastit-${workspace_version}-1.${rpm_arch}.rpm"
: >"$rpmset/blastit-php-${php_primary}-${workspace_version}-1.${rpm_arch}.rpm"

: >"$scratch/h/rpm.log"
log_i2=$scratch/h/i2.log
if ! env PATH="$scratch/h/bin:$PATH" \
    BLASTIT_RPM="$rpmset/blastit-${workspace_version}-1.${rpm_arch}.rpm" \
    BLASTIT_PKG_ACCEPT_UNSIGNED=1 \
    sh "$installer" -y >"$log_i2" 2>&1; then
    log "$log_i2"; fail "(i) the unsigned rpm install exited non-zero"
fi
grep -q -- '--nosignature' "$scratch/h/rpm.log" ||
    { log "$log_i2"; fail "(i) BLASTIT_PKG_ACCEPT_UNSIGNED=1 did not pass --nosignature to rpm"; }
grep -q -- '--replacepkgs' "$scratch/h/rpm.log" ||
    { log "$log_i2"; fail "(i) the rpm transaction lost --replacepkgs"; }
grep -q 'only for a package you built yourself' "$log_i2" ||
    { log "$log_i2"; fail "(i) the unsigned install printed no warning about what it switched off"; }

: >"$scratch/h/rpm.log"
log_i3=$scratch/h/i3.log
if ! env PATH="$scratch/h/bin:$PATH" \
    BLASTIT_RPM="$rpmset/blastit-${workspace_version}-1.${rpm_arch}.rpm" \
    sh "$installer" -y >"$log_i3" 2>&1; then
    log "$log_i3"; fail "(i) the signed-path rpm install exited non-zero"
fi
if grep -q -- '--nosignature' "$scratch/h/rpm.log"; then
    log "$log_i3"; fail "(i) rpm was called with --nosignature without BLASTIT_PKG_ACCEPT_UNSIGNED=1"
fi
grep -q 'only for a package you built yourself' "$log_i3" &&
    { log "$log_i3"; fail "(i) the unsigned warning printed without the opt-in"; }
pass "(i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1"

# (j) the generated docs/installer blocks are the table's and the version's ----
# `make docs-check` runs the same `--check`; proving it here means a stale block
# fails `make fast-check` too (no network, no root, no site build).
log_j=$scratch/j.log
if ! "$uv" run --no-project python "$root/scripts/gen_language_blocks.py" --check \
    >"$log_j" 2>&1; then
    log "$log_j"; fail "(j) docs/install.md or docs/install.sh is stale"
fi
pass "(j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version"

printf 'install-test: all cases passed (fixture on %s, scratch %s)\n' "$base" "$scratch"
