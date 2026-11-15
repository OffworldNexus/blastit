#!/bin/sh
# The two-package witness (`MB.3.61`): with `php82-embed` **and** `php84-embed`
# installed and `BLASTIT_LIBPHP` unset, the boot must serve from
# `/usr/lib/php84/libphp.so` — the library a name-major walk never reached,
# because it found `/usr/lib/libphp82.so` (an object with no `php_version`)
# first and refused.
#
# Runs inside `e2e/php-sapi/Dockerfile.two-package`, where both packages and
# both suites this image compiled are present. Exit 0 only when the suites took
# their **served** branch and the resolved library is the 8.4 one: a suite that
# silently skipped would otherwise look green (its refusal branch is a valid
# outcome on a host with no pair at all).
#
#   docker run --rm --cap-add SYS_PTRACE blastit-php-two:local
#
# The maps leg needs the capability: Docker's default root set cannot read the
# maps of a `PR_SET_DUMPABLE(0)` role, and `critical_php_embed` asserts every
# role above the view is content-checked when it runs as root (the same reason
# `make rs-test-php-image` passes `--cap-add SYS_PTRACE`).
set -eu

old=/usr/lib/libphp82.so
new=/usr/lib/php84/libphp.so

echo "two-package: the host's own layout"
ls -l "$old" "$new"

# The two facts that make the ordering matter, measured rather than assumed: the
# 8.2 object exports no `php_version` (so a boot that picks it must refuse), and
# the 8.4 one does (so it serves).
for lib in "$old" "$new"; do
    if readelf --dyn-syms "$lib" | grep -qw php_version; then
        echo "two-package: $lib exports php_version"
    else
        echo "two-package: $lib exports NO php_version"
    fi
done
if readelf --dyn-syms "$old" | grep -qw php_version; then
    echo "two-package: FAIL — $old exports php_version, so this image cannot show the defect" >&2
    exit 1
fi
readelf --dyn-syms "$new" | grep -qw php_version || {
    echo "two-package: FAIL — $new exports no php_version, so it cannot serve either" >&2
    exit 1
}

log=/tmp/two-package-suites.log
: >"$log"
for binary in $(cat /src/target/php-suite-executables); do
    echo "two-package: $binary --nocapture"
    # The binary's own status is kept: a pipeline into `tee` would report
    # *tee's* status and turn a failing suite into a green image (`MB.3.58`'s
    # lesson, in a second place).
    part="$log.part"
    if "$binary" --nocapture >"$part" 2>&1; then
        cat "$part" | tee -a "$log"
    else
        cat "$part" | tee -a "$log"
        echo "two-package: FAIL — $binary reported a failure" >&2
        exit 1
    fi
done

# The served branch proves the boot resolved a library it could use; the path in
# the boot's own `manifest.warning` proves *which* one, and the refusal marker
# must be absent.
if grep -q "refused (no usable libphp" "$log"; then
    echo "two-package: FAIL — the boot refused on a host whose 8.4 library serves:" >&2
    grep "refused (no usable libphp" "$log" >&2
    exit 1
fi
grep -q "served: HTTP/1.1 200" "$log" || {
    echo "two-package: FAIL — no suite served a [route.php] route, so nothing proved the" >&2
    echo "  discovery reached the 8.4 library (a skipped suite is not a witness)" >&2
    exit 1
}
grep -q "$new" "$log" || {
    echo "two-package: FAIL — the boot never named $new, so it may have served from" >&2
    echo "  another library; the manifest warning names the resolved pair" >&2
    exit 1
}
if grep -q "$old is an NTS libphp" "$log"; then
    echo "two-package: FAIL — the boot refused the 8.2 object on a host where the 8.4" >&2
    echo "  library serves: that is the MB.3.61 defect" >&2
    exit 1
fi

echo "two-package: ok — with php82-embed and php84-embed installed and no BLASTIT_LIBPHP,"
echo "  the boot served from $new (the 8.2 object at $old was not preferred)"
