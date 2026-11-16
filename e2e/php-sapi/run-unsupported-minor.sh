#!/bin/sh
# The 8.2-only witness (`MB.3.65`): what a user on an unsupported minor reads.
#
# Runs inside `e2e/php-sapi/Dockerfile.unsupported-minor` (Debian bookworm, whose
# distro PHP is 8.2). Exit 0 only when the boot refused with the support
# sentence and wrote no serving surface: a reworded, re-hedged or `.dynsym`-
# flavoured refusal is a failure here.
#
#   docker run --rm blastit-php-82:local
set -eu

libphp=/usr/lib/libphp8.2.so
[ -f "$libphp" ] || {
    echo "unsupported-minor: FAIL — $libphp is missing (is libphp8.2-embed installed?)" >&2
    exit 1
}
# The premise: 8.2 is the only PHP this host has, and it is a real NTS libphp.
for other in /usr/lib/libphp8.3.so /usr/lib/libphp8.4.so /usr/lib/libphp8.5.so \
             /usr/lib/libphp83.so /usr/lib/libphp84.so /usr/lib/libphp85.so; do
    if [ -e "$other" ]; then
        echo "unsupported-minor: FAIL — $other is installed, so this is not an 8.2-only host" >&2
        exit 1
    fi
done
readelf --dyn-syms "$libphp" | grep -qw executor_globals || {
    echo "unsupported-minor: FAIL — $libphp is not an NTS libphp" >&2
    exit 1
}
echo "unsupported-minor: the host's own PHP"
readelf --dyn-syms "$libphp" | grep -w php_version \
    && echo "unsupported-minor: $libphp exports php_version (so the refusal is about the minor)" \
    || echo "unsupported-minor: $libphp exports NO php_version"

app=/tmp/unsupported-minor
rm -rf "$app"
mkdir -p "$app/public"
cat >"$app/Blastfile" <<'BLASTFILE'
entrypoint php "public/index.php"

view app regex '.*' {
}
BLASTFILE
echo '<?php echo "hello from php";' >"$app/public/index.php"

log=/tmp/unsupported-minor.log
# `BLASTIT_LIBPHP` and `BLASTIT_PHP_SHIM_DIR` unset in effect: the walk over the
# host's own directories is what must reach the refusal.
if ( cd "$app" && BLASTIT_LIBPHP= BLASTIT_PHP_SHIM_DIR= /src/target/debug/blastit dev ) \
        >"$log" 2>&1; then
    echo "unsupported-minor: FAIL — the boot served on a host whose PHP 8.2 is not supported" >&2
    cat "$log" >&2
    exit 1
fi

sentence="PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4 and 8.5"
grep -qF "$sentence" "$log" || {
    echo "unsupported-minor: FAIL — the refusal does not carry the sentence:" >&2
    cat "$log" >&2
    exit 1
}
if grep -q 'exports no `php_version`' "$log"; then
    echo "unsupported-minor: FAIL — the 8.2 refusal still explains the symbol walk:" >&2
    cat "$log" >&2
    exit 1
fi
if grep -qE 'listening|ready' "$log"; then
    echo "unsupported-minor: FAIL — the refusal still bound a listener:" >&2
    cat "$log" >&2
    exit 1
fi

echo "unsupported-minor: ok — $sentence"
echo "  (refused before the listener; the object is a real NTS libphp8.2 and the"
echo "   only thing wrong with it is that Blastit does not support its minor)"
sed -n '1,3p' "$log"
