#!/bin/sh
# The two-package witness (`MB.3.61`; caption corrected by `MB.3.62`, packages
# updated by `MB.3.65`): `php83-embed` **and**
# `php84-embed` installed on one Alpine host, so `/usr/lib/libphp83.so` (the
# packed name) sits beside `/usr/lib/php84/libphp.so` (the per-minor directory).
# A name-major discovery walk reaches the 8.3 object first and serves the older
# minor; the version-major walk (`MB.3.61`) must reach the 8.4 library.
#
# Runs inside `e2e/php-sapi/Dockerfile.two-package`, where both packages and the
# PHP legs this image compiled (`e2e_dev --no-default-features --features
# php,static`) are present. Exit 0 only when the legs took their **served** branch
# and the library they resolved is the 8.4 one: a leg that silently skipped would
# otherwise look green (a host with no pair prints `SKIP …` instead).
#
# What this image proves, precisely: with `BLASTIT_LIBPHP` unset, the suite's own
# probe walks this host's real library directories and resolves
# `/usr/lib/php84/libphp.so`, and a boot **handed that path** — which is what the
# PHP legs do — serves the route. So it proves the probe order on a real Alpine
# layout, and that the boot agrees with the probe.
#
# What it does **not** prove: stage 0's own *unset*-override walk. No `blastit
# dev` here runs without `BLASTIT_LIBPHP`, so the walk over
# `PhpSearch::lib_dirs` is pinned only by `stage0.rs`'s unit test
# `two_embed_packages_serve_from_the_newer_library` (injected directories). The
# suite's copy of that order is tied to `stage0.rs`'s `LIBPHP_PROBES` by
# `crates/blastit/tests/common/libphp_probes.rs`, so the order itself is checked
# (`MB.3.62`) — but by `cargo test`, not by this image.
#
#   docker run --rm blastit-php-two:local
set -eu

# The premise of the whole image, asserted rather than assumed: with
# `BLASTIT_LIBPHP` set the suites' probe would simply believe it, and this
# helper would say nothing about the host's layout.
if [ -n "${BLASTIT_LIBPHP:-}" ]; then
    echo "two-package: FAIL — BLASTIT_LIBPHP is set (${BLASTIT_LIBPHP}), so the suites'" >&2
    echo "  probe would not walk the host's directories at all" >&2
    exit 1
fi

old=/usr/lib/libphp83.so
new=/usr/lib/php84/libphp.so

echo "two-package: the host's own layout"
ls -l "$old" "$new"

# Both objects are usable, so only the *order* decides which one serves: that is
# what makes this image a witness for the version-major walk rather than for a
# symbol one of them lacks.
for lib in "$old" "$new"; do
    readelf --dyn-syms "$lib" | grep -qw php_version || {
        echo "two-package: FAIL — $lib exports no php_version, so this image cannot" >&2
        echo "  show which library the order prefers (both must be usable)" >&2
        exit 1
    }
    echo "two-package: $lib exports php_version"
done

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

# What the suite's output proves: its `served:` line names the library its own
# probe resolved on this host, and the suite itself asserts that the boot's log
# names the same pair (a non-zero suite exit fails this helper). So `$new` in the
# log is the probe's answer on this layout — not a sweep of stage 0's
# directories, which no boot here performs.
if skipped=$(grep '^SKIP ' "$log"); then
    echo "two-package: FAIL — a PHP leg skipped on a host whose 8.4 library serves," >&2
    echo "  so nothing proved the discovery reached it (a skipped leg is not a witness):" >&2
    printf '%s\n' "$skipped" >&2
    exit 1
fi
grep -q "served: HTTP/1.1 200" "$log" || {
    echo "two-package: FAIL — no PHP leg served a [route.php] route, so nothing proved the" >&2
    echo "  discovery reached the 8.4 library (a skipped leg is not a witness)" >&2
    exit 1
}
grep -q "$new" "$log" || {
    echo "two-package: FAIL — the probe resolved no $new on this host, so the version-" >&2
    echo "  major order did not reach the 8.4 library (a skipped leg is not a witness)" >&2
    exit 1
}
# The 8.3 library was found first by name: a boot that picked it would have no
# shim (this image builds only the 8.4 one) and say so by name.
if grep -q "libblastit_php83-" "$log"; then
    echo "two-package: FAIL — the boot resolved the 8.3 library on a host where the 8.4" >&2
    echo "  library serves: that is the MB.3.61 defect" >&2
    exit 1
fi

echo "two-package: ok — with php83-embed and php84-embed installed and BLASTIT_LIBPHP unset,"
echo "  each suite's probe resolved $new on this host and a boot handed that path served"
echo "  the route: probe order on a real Alpine layout, and boot agreement with the probe."
echo "  (Stage 0's own unset-override walk is pinned by stage0.rs's unit test, not here.)"
