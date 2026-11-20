#!/usr/bin/env bash
# Proves `packages/composer-blastit` in a `composer:2` container (node MB.42.3.3).
#
#   scripts/composer_smoke.sh
#   BLASTIT_COMPOSER_IMAGE=composer:2 scripts/composer_smoke.sh
#
# Four proofs, because "the script runs" and "Composer puts it on `vendor/bin`"
# and "the table a real `composer require` uses is the one that verifies" are
# different claims:
#
#   1. **Installed**: a throwaway project declares the package as a `path`
#      repository and `composer install`s it offline, then runs
#      `vendor/bin/blastit --version`. This is the per-project pin design §6.1
#      describes, and it exercises the `bin` wiring in `composer.json`.
#   2. **The default origin verifies** (`review` of MB.42.3.3: every previous
#      proof overrode `BLASTIT_RELEASE_BASE_URL`, so the only path a real
#      `composer require` takes — the embedded table — was never run). The
#      package copy is stamped with `scripts/composer_sha256.py` from the fixture
#      archives, and the wrapper runs with **no** environment override except
#      `BLASTIT_ARCHIVE` (a pre-seeded tarball, so no network).
#   3. **A tampered archive is refused** by the wrapper's own digest, before it is
#      extracted (a mirror that can substitute the tarball is the threat).
#   4. **An unstamped (`pending`) table refuses**, even with a perfectly good
#      archive on disk: the digest check comes before the cache, so no fast path
#      can turn "unknown digest" into "run it".
#
# Nothing is installed on this host: the container is the environment, and the
# fixture archives hold a shell stand-in for `blastit` (the real archives are Rust
# binaries, proved by `scripts/check-portable-elf.sh` in their own build).
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
image="${BLASTIT_COMPOSER_IMAGE:-composer:2}"
work=$(mktemp -d "${TMPDIR:-/var/tmp}/blastit-composer-smoke.XXXXXX")
cleanup() { rm -rf "$work" 2>/dev/null || true; }
trap cleanup EXIT
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) echo "composer-smoke: unsupported host architecture $(uname -m)" >&2; exit 1 ;;
esac

# The fixture release: the archive layout `scripts/build-standalone.sh` writes
# (`<target>/blastit` at the top, `SHA256SUMS` beside the tarball). Both libcs are
# built, because the *container's* libc decides which one the wrapper asks for
# (`composer:2` is Alpine, so that is the musl row) and the host's does not.
mkdir -p "$work/fixture" "$work/stage"
for libc in gnu musl; do
    target="blastit-${arch}-unknown-linux-${libc}"
    mkdir -p "$work/stage/$target"
    cat > "$work/stage/$target/blastit" <<'SHIM'
#!/bin/sh
printf 'blastit 0.2.0 (composer wrapper fixture)\n'
SHIM
    chmod 0755 "$work/stage/$target/blastit"
    tar -C "$work/stage" -czf "$work/fixture/$target.tar.gz" "$target"
done
( cd "$work/fixture" && for tarball in ./*.tar.gz; do sha256sum "${tarball#./}"; done > SHA256SUMS )

# The package under test, copied so it can be **stamped**: proof 2 is about the
# table the published package carries, and `/src` is mounted read-only.
cp -a "$root/packages/composer-blastit" "$work/pkg"
chmod -R u+rwX "$work/pkg"
echo "composer-smoke: stamping the digest table from the fixture archives"
uv run --no-project "$root/scripts/composer_sha256.py" \
    --version 0.2.0 --wrapper "$work/pkg/bin/blastit" \
    --archives "$work"/fixture/*.tar.gz >/dev/null

mkdir -p "$work/project"
cat > "$work/project/composer.json" <<JSON
{
    "repositories": [
        {
            "type": "path",
            "url": "/work/pkg",
            "options": { "symlink": false, "versions": { "offworldnexus/blastit": "0.2.0" } }
        },
        { "packagist.org": false }
    ],
    "require": { "offworldnexus/blastit": "0.2.0" },
    "config": { "allow-plugins": false }
}
JSON

echo "composer-smoke: composer install in $image"
docker run --rm --user "$(id -u):$(id -g)" \
    -v "$work:/work" \
    -e COMPOSER_HOME=/tmp/composer-home \
    -e COMPOSER_ALLOW_SUPERUSER=1 \
    "$image" sh -euc '
        cd /work/project
        composer install --no-interaction --no-progress --quiet
        test -x vendor/bin/blastit || { echo "composer install produced no vendor/bin/blastit" >&2; exit 1; }
        # Proof 2: the **default origin** (the embedded table) — no
        # BLASTIT_RELEASE_BASE_URL. The archive is pre-seeded, so no network.
        if [ -e /lib/ld-musl-*.so.1 ] 2>/dev/null; then libc=musl; else libc=gnu; fi
        installed=$(BLASTIT_ARCHIVE="/work/fixture/blastit-$(uname -m | sed "s/amd64/x86_64/;s/arm64/aarch64/")-unknown-linux-${libc}.tar.gz" \
            BLASTIT_COMPOSER_CACHE=/work/cache-default \
            vendor/bin/blastit --version)
        printf "composer-smoke: vendor/bin/blastit -> %s\n" "$installed"
        printf "%s" "$installed" | grep -q "blastit 0.2.0" || {
            echo "composer-smoke: vendor/bin/blastit did not answer with the fixture version" >&2
            exit 1
        }
        # The cache holds the verified tarball, and a warm run is silent.
        warm=$(BLASTIT_ARCHIVE="/work/fixture/blastit-$(uname -m | sed "s/amd64/x86_64/;s/arm64/aarch64/")-unknown-linux-${libc}.tar.gz" \
            BLASTIT_COMPOSER_CACHE=/work/cache-default \
            vendor/bin/blastit --version 2>&1)
        printf "composer-smoke: warm run -> %s\n" "$warm"
        printf "%s" "$warm" | grep -q "fetching" && {
            echo "composer-smoke: a warm run must not print the cold-path fetch line" >&2
            exit 1
        }
        exit 0
    '

# Proof 3: a mirror that substitutes the tarball is refused.
echo "composer-smoke: a tampered archive is refused"
cp -a "$work/fixture" "$work/tampered"
for tarball in "$work"/tampered/*.tar.gz; do
    printf 'appended by the test\n' >> "$tarball"
done
if docker run --rm --user "$(id -u):$(id -g)" \
    -v "$work:/work" "$image" sh -euc '
        BLASTIT_RELEASE_BASE_URL=file:///work/tampered BLASTIT_COMPOSER_CACHE=/work/cache-tampered \
            sh /work/pkg/bin/blastit --version
    ' > "$work/tampered.log" 2>&1; then
    echo "composer-smoke: FAIL the tampered archive was executed" >&2
    tail -n 20 "$work/tampered.log" >&2
    exit 1
fi
grep -q 'does not match its digest' "$work/tampered.log" || {
    echo "composer-smoke: FAIL the refusal did not name the digest" >&2
    tail -n 20 "$work/tampered.log" >&2
    exit 1
}
echo "composer-smoke: $(grep -m1 'does not match its digest' "$work/tampered.log")"

# And a mirror with no digest at all is refused too (fail closed, not "trust me").
if docker run --rm --user "$(id -u):$(id -g)" -v "$work:/work" "$image" sh -euc '
    mkdir -p /work/nosums && cp /work/fixture/*.tar.gz /work/nosums/
    BLASTIT_RELEASE_BASE_URL=file:///work/nosums BLASTIT_COMPOSER_CACHE=/work/cache-nosums \
        sh /work/pkg/bin/blastit --version
' > "$work/nosums.log" 2>&1; then
    echo "composer-smoke: FAIL an overridden origin with no digest was accepted" >&2
    tail -n 20 "$work/nosums.log" >&2
    exit 1
fi
grep -q 'a digest is required' "$work/nosums.log" || {
    echo "composer-smoke: FAIL the failure did not say a digest is required" >&2
    tail -n 20 "$work/nosums.log" >&2
    exit 1
}

# Proof 4: the **shipped** table is all `pending`, and a `pending` row refuses
# before the cache is consulted — a good archive on disk cannot rescue it.
echo "composer-smoke: an unstamped (pending) table refuses"
if docker run --rm --user "$(id -u):$(id -g)" -v "$work:/work" -v "$root:/src:ro" "$image" sh -euc '
    if [ -e /lib/ld-musl-*.so.1 ] 2>/dev/null; then libc=musl; else libc=gnu; fi
    BLASTIT_ARCHIVE="/work/fixture/blastit-$(uname -m | sed "s/amd64/x86_64/;s/arm64/aarch64/")-unknown-linux-${libc}.tar.gz" \
        BLASTIT_COMPOSER_CACHE=/work/cache-pending \
        sh /src/packages/composer-blastit/bin/blastit --version
' > "$work/pending.log" 2>&1; then
    echo "composer-smoke: FAIL a pending digest was treated as a runnable artifact" >&2
    tail -n 20 "$work/pending.log" >&2
    exit 1
fi
grep -q 'publishes no .*archive yet' "$work/pending.log" || {
    echo "composer-smoke: FAIL the refusal did not name the pending row" >&2
    tail -n 20 "$work/pending.log" >&2
    exit 1
}
echo "composer-smoke: $(grep -m1 'publishes no ' "$work/pending.log")"

echo "composer-smoke: ok — installed, default-origin run, warm run silent, tampered archive refused, origin without a digest refused, pending row refused"
