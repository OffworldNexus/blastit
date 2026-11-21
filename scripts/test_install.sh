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
# stdin still reaches the installer's last line (the BLOCKER 1 regression).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
installer=$root/docs/install.sh
scratch=${BLASTIT_INSTALL_TEST_DIR:-${TMPDIR:-/tmp}/opencode}/install-sh-test
uv=${BLASTIT_TEST_UV:-uv}
asset=blastit-x86_64-unknown-linux-gnu.tar.gz
fixture_version='blastit 0.2.0 (fixture)'

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
sh "$installer" --mirror bogus >/dev/null 2>&1 || code=$?
[ "$code" -eq 2 ] || fail "(f) --mirror bogus exited $code, not 2"
code=0
sh "$installer" --nightly --user >/dev/null 2>&1 || code=$?
[ "$code" -eq 2 ] || fail "(f) --nightly --user exited $code, not 2"
code=0
sh "$installer" --version '../../evil' >/dev/null 2>&1 || code=$?
[ "$code" -eq 2 ] || fail "(f) a path-shaped --version exited $code, not 2"
pass "(f) usage errors exit 2 (bad mirror, --nightly --user, path-shaped --version)"

# (g) BLOCKER 1: a child must not eat the piped script ------------------------
# The child that reads stdin is a stub `dpkg` (the shape of the Cloudsmith setup
# script, apt-get, dpkg, dnf); a stub `sudo` passes its stdin through, as the real
# one does. Both sit first on PATH, so no real package tool is ever invoked.
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

printf 'install-test: all cases passed (fixture on %s, scratch %s)\n' "$base" "$scratch"
