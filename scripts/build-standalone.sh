#!/usr/bin/env bash
# Builds one standalone archive — the universal artifact a host installs without
# a wheel (`ADR-0010` §4, `ADR-0017` decision 1, node MB.3.18).
#
# The archive is `blastit-<target>.tar.gz` and holds:
#
#   * `blastit` — the universal binary, built from `blastit`'s **default**
#     features (`python`, `php`, `static`). None of the three links an
#     interpreter: on glibc `DT_NEEDED` names libc, libm, libgcc_s and the
#     dynamic loader, on musl `libc.musl-<arch>` and libgcc_s — and that one file
#     serves every backend, because each runtime is `dlopen`ed: embedded PHP from
#     the distro's NTS `libphp` plus the PHP shims below, a WSGI app from the
#     host's CPython 3.13/3.14 plus the CPython shims below, and `static` mounts
#     (`ADR-0017` decision 1, `ADR-0016` decision 6). There is no Python-less
#     variant of this artifact any more.
#   * `libblastit_php<digits>-<arch>-<libc>.so` — one PHP shim per minor this leg
#     has built *and* probed (`scripts/platform_matrix.py` is the list).
#   * `libblastit_py3<digits>-<arch>-<libc>.so` — the `3.13` and `3.14` CPython
#     shims this leg built, so a WSGI view boots out of the box instead of
#     asking the operator to copy one in (`MB.47.2`).
#   * `LICENSE`, and `SHA256SUMS` over everything in the archive.
#
# **Both shim families are staged beside the binary.** The `blastit-py-sapi`
# loader resolves `libblastit_py3<minor>-<arch>-<libc>.so` beside the `blastit`
# executable and nowhere else — no `PATH`, no environment variable
# (`crates/blastit-py-sapi/src/discovery/mod.rs`) — which is exactly where this
# script puts them, so `install.sh --user` and an Alpine `.tar.gz` install both
# serve Python WSGI. `--require-py-shims` (default `2`) and `--require-shims`
# (PHP) fail closed when a family is missing: an archive that cannot serve a
# declared backend is refused loudly, never shipped half-working.
#
# `scripts/check-portable-elf.sh` runs on the binary before packaging: no
# `libpython`/`libphp` `NEEDED` entry, no `RPATH`/`RUNPATH` into a build host.
# That is the difference between an archive that runs on a server and one that
# only runs in the checkout that produced it.
#
# **The C-library floor is measured, not assumed.** `--max-glibc` reads the
# highest `GLIBC_<major>.<minor>` the finished binary requires and refuses the
# build above it, so "portable" is a number this script checked and wrote into
# `README.md` beside the binary — not a word in a release note. A host build
# (no `--max-glibc`) keeps the host's floor, which is why the shipping workflow
# builds inside `scripts/build-standalone-in-image.sh` and names a floor.
#
# Usage: scripts/build-standalone.sh [--out DIR] [--target TRIPLE]
#                                   [--shim-dir DIR] [--require-shims N]
#                                   [--py-shim-dir DIR] [--require-py-shims N]
#                                   [--max-glibc 2_28] [--builder-image IMAGE]
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

out="dist"
target=""
shim_dir="${PHP_SAPI_OUT:-$root/target/php-sapi}"
py_shim_dir="${PY_SAPI_OUT:-$root/target/py-sapi}"
require_shims=0
# A tarball with no CPython shim cannot serve a WSGI view, and that refusal
# belongs to the build rather than to a user's first request (`MB.47.2`).
require_py_shims=2
max_glibc=""
builder_image=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) out=$2; shift 2 ;;
        --target) target=$2; shift 2 ;;
        --shim-dir) shim_dir=$2; shift 2 ;;
        --require-shims) require_shims=$2; shift 2 ;;
        --py-shim-dir) py_shim_dir=$2; shift 2 ;;
        --require-py-shims) require_py_shims=$2; shift 2 ;;
        --max-glibc) max_glibc=$2; shift 2 ;;
        --builder-image) builder_image=$2; shift 2 ;;
        *) echo "usage: $0 [--out DIR] [--target TRIPLE] [--shim-dir DIR] [--require-shims N] [--py-shim-dir DIR] [--require-py-shims N] [--max-glibc 2_28] [--builder-image IMAGE]" >&2; exit 2 ;;
    esac
done

# `<major>_<minor>` (the manylinux spelling, as `--max-glibc` takes it) and
# `<major>.<minor>` (the glibc spelling) are the same number here.
if [ -n "$max_glibc" ]; then
    max_glibc=${max_glibc//_/.}
    if ! printf '%s' "$max_glibc" | grep -Eq '^[0-9]+\.[0-9]+$'; then
        echo "error: --max-glibc '$max_glibc' is not <major>_<minor> (e.g. 2_28)" >&2
        exit 2
    fi
fi

host=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
target=${target:-$host}

# A binary for another triple must not be reported as this host's: the archive
# name and the shim tuple both come from the target, and mixing them would ship
# a musl shim beside a glibc binary under one name.
case "$target" in
    x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
    *) echo "edit: $target is not a Linux target this script packages" >&2; exit 2 ;;
esac
arch=${target%%-*}
libc=${target##*-}

# A musl build defaults to static-pie (`-C target-feature=+crt-static`), and a
# static musl binary cannot `dlopen` anything: musl answers "Dynamic loading not
# supported". The whole point of the `php` capability is `dlopen(libphp)`, so a
# musl artifact asks rustc for a dynamic binary instead — and the assertion below
# proves it, because a static one builds, packages, serves static routes and
# silently loses every `[route.php]`.
musl=""
case "$target" in
    *-musl) musl=1 ;;
esac

# `--target` only when it is not the host: passing it makes cargo compile build
# scripts with the host toolchain and *without* RUSTFLAGS, and on musl that means
# a static build script — which cannot `dlopen` libclang, `pg_query`'s bindgen
# dependency. Same-triple builds therefore stay flag-unqualified, so the flags
# reach every compiler invocation.
host=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
target_args=()
[ "$target" = "$host" ] || target_args=(--target "$target")

echo "build-standalone: cargo build --release --locked -p blastit --bin blastit ${target_args[*]:-}"
if [ -n "$musl" ]; then
    # Dynamic musl, because a static musl binary cannot `dlopen` (`Dynamic loading
    # not supported`), and `dlopen(libphp)` is the whole `[route.php]` story.
    #
    # That has a published cost: a dynamically linked musl Rust binary needs
    # `libgcc_s.so.1`, a separate Alpine package (`libgcc`). `-static-libgcc` was
    # tried and does **not** remove it — rustc adds `-lgcc_s` itself — so the
    # requirement is documented in `ADR-0010` §4 and installed by the `verify_musl`
    # job instead of being papered over. A static archive would drop the
    # dependency and the PHP route with it.
    echo "build-standalone: musl target, forcing a dynamic binary (crt-static off)"
    RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static" \
        cargo build --release --locked -p blastit --bin blastit \
        ${target_args[@]+"${target_args[@]}"}
else
    cargo build --release --locked -p blastit --bin blastit \
        ${target_args[@]+"${target_args[@]}"}
fi

# No host fallback: a `--target aarch64-...` whose artifact is missing must fail,
# never package the host's binary under a foreign name.
#
# `CARGO_TARGET_DIR` is honoured because the container builders
# (`scripts/build-standalone-in-image.sh`) point it at a directory of their own:
# a host `target/` full of glibc-2.39 objects must never be reused by a build
# whose whole purpose is a lower floor.
target_root="${CARGO_TARGET_DIR:-$root/target}"
case "$target_root" in
    /*) ;;
    *) target_root="$root/$target_root" ;;
esac
binary="$target_root/release/blastit"
[ "$target" = "$host" ] || binary="$target_root/$target/release/blastit"
if [ ! -x "$binary" ]; then
    echo "error: $binary was not produced: cargo built nothing for $target" >&2
    echo "  (the host binary at $target_root/release/blastit is not this artifact)" >&2
    exit 1
fi
scripts/check-portable-elf.sh "$binary"
# The test-only boot seams must not be in what this archive ships (`MB.3.64`):
# `BLASTIT_TEST_SWAP_POINT` pauses a boot before the listener binds, which is a
# denial-of-service surface for a production binary. The seams are compiled under
# `debug_assertions` alone; this measures the artifact instead of trusting it,
# and fails if `RUSTFLAGS` turned debug assertions back on for a release build.
# A file this cannot read has no verdict, so the readability is checked first.
if [ ! -r "$binary" ]; then
    echo "error: cannot read $binary, so whether it carries the test seams is unknown" >&2
    exit 2
fi
for seam in BLASTIT_TEST_SWAP_POINT BLASTIT_TEST_RELOAD_FAULT BLASTIT_TEST_RELOAD_PAUSE; do
    if grep -a -q -F -- "$seam" "$binary"; then
        echo "error: $binary contains $seam, so a shipped boot could be paused by an" >&2
        echo "  environment variable before it binds a listener — a denial-of-service" >&2
        echo "  surface that must exist only in a test build." >&2
        echo "  the seam is compiled under \`debug_assertions\` alone: build with \`--release\`" >&2
        echo "  and never with \`-C debug-assertions=yes\` or RUSTFLAGS that turns it back on." >&2
        exit 1
    fi
done
echo "build-standalone: no test-only boot seam in $binary"

# The floor, measured once and used twice: `--max-glibc` refuses a build that
# outgrew the image it is shipped for, and the same number goes into the
# archive's `README.md`, so the claim and the artifact cannot drift.
floor=""
if [ -n "$musl" ]; then
    # musl does not version its symbols, so there is no floor to read — and none
    # to assert. The requirement a musl archive does have is `libgcc_s.so.1`
    # (`apk add libgcc`), which is what the README says.
    if [ -n "$max_glibc" ]; then
        echo "error: --max-glibc does not apply to $target: a musl binary carries no glibc symbols" >&2
        echo "  it needs no glibc at all; the runtime package is libgcc (apk add libgcc)" >&2
        exit 2
    fi
    needed=$(readelf -d "$binary" | sed -n 's/.*(NEEDED) *Shared library: \[\(.*\)\]/\1/p')
    case "$needed" in
        *libgcc_s.so.1*) ;;
        *)
            echo "error: $binary does not link libgcc_s.so.1, which the musl runtime note is about" >&2
            printf '%s\n' "$needed" | sed 's/^/  NEEDED /' >&2
            echo "  if that changed, update the archive README in this script rather than the archive" >&2
            exit 1
            ;;
    esac
    echo "build-standalone: no glibc floor (musl); needs libgcc_s.so.1 (apk add libgcc)"
else
    floor=$(
        readelf --version-info "$binary" \
            | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' \
            | sort -V \
            | tail -n 1
    )
    if [ -z "$floor" ]; then
        echo "error: $binary names no GLIBC_<version> symbol, so its C-library floor is unknown" >&2
        echo "  a $target binary that needs no glibc is not what this script packages" >&2
        exit 1
    fi
    echo "build-standalone: glibc floor $floor (readelf --version-info)"
    if [ -n "$max_glibc" ]; then
        newest=$(printf '%s\n%s\n' "$floor" "$max_glibc" | sort -V | tail -n 1)
        if [ "$newest" != "$max_glibc" ]; then
            echo "error: $binary needs glibc $floor, above the $max_glibc this archive must run on" >&2
            echo "  build it inside a container with glibc <= $max_glibc" >&2
            echo "  (scripts/build-standalone-in-image.sh --image quay.io/pypa/manylinux_2_28_\$(uname -m))" >&2
            echo "  or raise --max-glibc and say so in the archive's docs — never lower it silently" >&2
            exit 1
        fi
        echo "build-standalone: floor $floor <= --max-glibc $max_glibc"
    else
        echo "build-standalone: no --max-glibc given, so this artifact keeps the floor of the host that built it ($host)"
    fi
fi

if [ -n "$musl" ]; then
    # `INTERP` is the dynamic loader; a static-pie musl binary has none, and then
    # `dlopen` — i.e. every `[route.php]` — is refused at run time. Captured into
    # a variable rather than piped into `grep -q`: under `pipefail`, grep's early
    # exit SIGPIPEs readelf and turns a *passing* check into a failure.
    headers=$(readelf -l "$binary")
    dynamic=$(readelf -d "$binary")
    case "$headers" in
        *"ld-musl-${arch}.so.1"*) ;;
        *)
            echo "error: $binary does not carry musl's dynamic interpreter ld-musl-${arch}.so.1: it cannot dlopen libphp" >&2
            echo "  build it with -C target-feature=-crt-static (scripts/build-standalone.sh does)" >&2
            exit 1
            ;;
    esac
    case "$dynamic" in
        *libc.musl*) ;;
        *)
            echo "error: $binary does not link musl's libc dynamically" >&2
            printf '%s\n' "$dynamic" | head -5 >&2
            exit 1
            ;;
    esac
    echo "build-standalone: dynamic musl: $(printf '%s\n' "$headers" | sed -n 's/.*Requesting program interpreter: \(.*\)\]/\1/p')"
fi

# The capability, not the manifest: `cargo tree -e normal` is "what the binary
# links", so a `pyo3` here is a `libpython` the archive does not have — and the
# absence of `blastit-py-sapi` or `blastit-view-static` is a capability compiled
# out, which is a boot refusal after a green build: a WSGI manifest that cannot
# embed CPython (`ADR-0017` decision 1), a `static` mount that refuses with
# `SchedulerError::StaticNotCompiled` (`MB.36.6`). Both directions are asserted:
# the negative (no interpreter linked) and the positive (each capability's crate
# is in the graph), on the artifact's real feature set — the defaults.
tree=$(cargo tree -p blastit -e normal --target "$target" --prefix none)
pyo3=$(printf '%s\n' "$tree" | grep -c pyo3 || true)
if [ "$pyo3" -ne 0 ]; then
    echo "error: the standalone build depends on pyo3 ($pyo3): it would link libpython" >&2
    exit 1
fi
echo "build-standalone: 0 pyo3 dependencies (no libpython linked)"

py=$(printf '%s\n' "$tree" | grep -c '^blastit-py-sapi v' || true)
if [ "$py" -eq 0 ]; then
    echo "error: this archive cannot serve a WSGI app: blastit-py-sapi is not in its dependency graph" >&2
    echo "  fix: the default features must carry \`python\` — a \`dlopen\` loader for the" >&2
    echo "  host's CPython, never a linked interpreter (\`ADR-0017\` decision 1)" >&2
    exit 1
fi
echo "build-standalone: blastit-py-sapi present (the dlopen loader: a WSGI view also needs the host's CPython 3.13/3.14 and that minor's shim beside the binary)"

php=$(printf '%s\n' "$tree" | grep -c '^blastit-view-php v' || true)
if [ "$php" -eq 0 ]; then
    echo "error: this archive cannot serve an \`entrypoint php\` route: blastit-view-php is not in its dependency graph" >&2
    echo "  fix: the default features must carry \`php\` — a \`dlopen\` view of the distro's" >&2
    echo "  NTS \`libphp\`, never a linked one (\`ADR-0010\`, \`ADR-0017\` decision 1)" >&2
    exit 1
fi
echo "build-standalone: blastit-view-php present (an \`entrypoint php\` route is served)"

static=$(printf '%s\n' "$tree" | grep -c '^blastit-view-static v' || true)
if [ "$static" -eq 0 ]; then
    echo "error: this archive cannot serve a \`static\` mount: blastit-view-static is not in its dependency graph" >&2
    echo "  fix: the default features must carry \`static\`, and a dropped one is a boot" >&2
    echo "  refusal (\`SchedulerError::StaticNotCompiled\`) a green build cannot see" >&2
    exit 1
fi
echo "build-standalone: blastit-view-static present (a static mount is served)"

stage=$(mktemp -d "${TMPDIR:-/var/tmp}/blastit-standalone-stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
name="blastit-$target"
mkdir -p "$stage/$name"
install -m 0755 "$binary" "$stage/$name/blastit"
install -m 0644 LICENSE "$stage/$name/LICENSE"

# A staged shim's *file name* is not evidence of what was linked against it:
# `$arch` and `$libc` come from `--target`, so a cross leg or a mis-flagged
# `PY_SAPI_ARCH`/`PY_SAPI_LIBC` stages a file whose name promises a machine or a
# C library it cannot `dlopen` on. `scripts/check-portable-elf.sh` refuses a
# linked interpreter, not a name that disagrees with the ELF, so this reads the
# file itself: an archive may only promise what its shims really are.
assert_shim_target() {
    local shim=$1 base machine wanted needed
    base=$(basename "$shim")
    machine=$(readelf -h "$shim" | sed -n 's/.*Machine:[[:space:]]*//p' | head -n 1)
    case "$arch" in
        x86_64) wanted="Advanced Micro Devices X86-64" ;;
        aarch64) wanted="AArch64" ;;
        *) wanted="" ;;
    esac
    if [ -n "$wanted" ] && [ "$machine" != "$wanted" ]; then
        echo "error: $base is an ELF for '${machine:-an unknown machine}', but this archive claims $arch:" >&2
        echo "  a shim built for another architecture cannot be dlopen'ed here; build it on an" >&2
        echo "  $arch leg (this build already globs $arch-$libc)" >&2
        exit 1
    fi
    needed=$(readelf -d "$shim" | sed -n 's/.*(NEEDED) *Shared library: \[\(.*\)\]/\1/p')
    case "$libc" in
        musl)
            case "$needed" in
                *libc.musl-*) ;;
                *)
                    echo "error: $base is not linked against musl, but this archive claims $arch-$libc:" >&2
                    printf '%s\n' "$needed" | sed 's/^/  NEEDED /' >&2
                    echo "  a glibc shim wearing a '-musl' name cannot be dlopen'ed on Alpine" >&2
                    exit 1
                    ;;
            esac
            ;;
        gnu)
            case "$needed" in
                *libc.so.6*) ;;
                *)
                    echo "error: $base is not linked against glibc, but this archive claims $arch-$libc:" >&2
                    printf '%s\n' "$needed" | sed 's/^/  NEEDED /' >&2
                    echo "  a musl shim wearing a '-gnu' name cannot be dlopen'ed here" >&2
                    exit 1
                    ;;
            esac
            ;;
    esac
}

php_contents=""
php_floor_lines=""
shims=0
for shim in "$shim_dir"/libblastit_php*-"$arch"-"$libc".so; do
    [ -f "$shim" ] || continue
    assert_shim_target "$shim"
    base=$(basename "$shim")
    install -m 0644 "$shim" "$stage/$name/$base"
    shims=$((shims + 1))
    # A shim is `dlopen`ed, so the loader applies *its* version needs too: the
    # binary's floor is not the archive's only number. Each one is measured and
    # listed, because a 2.28 binary beside a 2.41 shim is portable only for the
    # routes that do not use PHP (`MB.3.60`).
    if [ -n "$musl" ]; then
        shim_floor_note="musl (no glibc; needs \`libgcc\`)"
    else
        shim_floor=$(
            readelf --version-info "$shim" \
                | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' \
                | sort -V \
                | tail -n 1
        )
        shim_floor_note="glibc ${shim_floor:-none} or newer"
    fi
    php_contents="$php_contents- \`$base\` — PHP SAPI shim
"
    php_floor_lines="$php_floor_lines- PHP \`$base\` — $shim_floor_note
"
done
if [ "$shims" -eq 0 ]; then
    php_contents="- no PHP shim in this archive
"
    php_floor_lines="No PHP shim in this archive: rebuild it on a leg that builds them
(\`scripts/build-standalone.sh\` with a \`make php-sapi\`-provisioned \`target/php-sapi\`).
"
fi
if [ "$shims" -eq 1 ]; then shim_word="shim"; else shim_word="shims"; fi
echo "build-standalone: $shims $shim_word for $arch-$libc from $shim_dir"
if [ "$shims" -lt "$require_shims" ]; then
    echo "error: $shims $shim_word in $shim_dir, this artifact must carry $require_shims:" >&2
    echo "  build them first (make php-sapi PHP_MINORS=... or the per-leg CI job)" >&2
    exit 1
fi

# The CPython shims travel beside the binary for the same reason the PHP ones do
# (`MB.47.2`): a WSGI view resolves `libblastit_py3<minor>-<arch>-<libc>.so` from
# the executable's own directory, and an archive without one can only refuse at
# boot (`NoShim`). Same shape as the PHP loop above: stage what exists, count it,
# and fail the build when the count is short of what this artifact must carry.
# The supported minors below come from the *staged files*, never from a literal:
# an archive carrying one shim must not tell a host on the other minor that it
# works (`--require-py-shims 1` is a legal build).
py_contents=""
py_floor_lines=""
py_minors=""
py_shims=0
for shim in "$py_shim_dir"/libblastit_py3*-"$arch"-"$libc".so; do
    [ -f "$shim" ] || continue
    assert_shim_target "$shim"
    base=$(basename "$shim")
    install -m 0644 "$shim" "$stage/$name/$base"
    py_shims=$((py_shims + 1))
    minor=$(printf '%s' "$base" | sed -n 's/^libblastit_py\([0-9]\)\([0-9][0-9]\)-.*/\1.\2/p')
    if [ -n "$minor" ]; then py_minors="$py_minors $minor"; fi
    if [ -n "$musl" ]; then
        shim_floor_note="musl (no glibc; needs \`libgcc\`)"
    else
        shim_floor=$(
            readelf --version-info "$shim" \
                | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' \
                | sort -V \
                | tail -n 1
        )
        shim_floor_note="glibc ${shim_floor:-none} or newer"
    fi
    py_contents="$py_contents- \`$base\` — CPython SAPI shim
"
    py_floor_lines="$py_floor_lines- CPython \`$base\` — $shim_floor_note
"
done
if [ "$py_shims" -eq 1 ]; then py_shim_word="shim"; else py_shim_word="shims"; fi
echo "build-standalone: $py_shims CPython $py_shim_word for $arch-$libc from $py_shim_dir"
if [ "$py_shims" -lt "$require_py_shims" ]; then
    echo "error: $py_shims CPython $py_shim_word in $py_shim_dir, this artifact must carry $require_py_shims:" >&2
    echo "  build them first (\`make py-sapi\` locally, or let the builder image compile them:" >&2
    echo "  \`scripts/build-standalone-in-image.sh\` runs \`make py-sapi\` before it stages)" >&2
    exit 1
fi
shim_lines="$php_floor_lines$py_floor_lines"

# The minors this archive can actually serve, from the staged basenames, sorted.
py_minor_list=""
if [ -n "$py_minors" ]; then
    for minor in $(printf '%s\n' $py_minors | sort -V); do
        if [ -z "$py_minor_list" ]; then
            py_minor_list="\`$minor\`"
        else
            py_minor_list="$py_minor_list and \`$minor\`"
        fi
    done
fi

# What a WSGI view really needs, once per archive (both libc branches share it).
# The archive removes the "copy the shim beside the binary" step, not the
# interpreter requirement: stage 0 `dlopen`s the *host's* `libpython3.1x.so` and
# refuses with `UnsupportedMinor` or `NoLibpython` when it is absent, which is
# the truth a reader can act on.
if [ "$py_shims" -gt 0 ]; then
    wsgi_contract="**A WSGI view needs the host's CPython $py_minor_list with its shared
\`libpython3.1x.so\`.** This archive removes the “copy the shim beside the binary”
step — the shim for this platform is already next to \`blastit\` — but the
interpreter is still the host's, and its boot refusal names the reason:
\`UnsupportedMinor\` for another minor (listing the minors it supports) and
\`NoLibpython\` when that CPython was built without a shared \`libpython3.1x.so\`.
The app's own dependencies have to be installed for that interpreter too.

    apt install libpython3.13      # Debian/Ubuntu
    apk add python3                # Alpine
    dnf install python3-libs       # Fedora/RHEL
    uv python install 3.14         # anywhere, via uv"
else
    wsgi_contract="**This archive carries no CPython shim**, so a WSGI view refuses at boot
(\`NoShim\`). Rebuild the archive on a leg that builds one (\`scripts/build-standalone.sh\`
after \`make py-sapi\`)."
fi

# The floor, out loud, in the thing the user unzips. `--max-glibc` above is the
# gate; this is the same number written next to the binary, because "portable"
# on a release page is not something a server can act on and a version is.
if [ -n "$musl" ]; then
    requirements="**No glibc requirement** — this is a musl binary, which does not use
glibc at all. It does need \`libgcc_s.so.1\`, which Alpine ships in the \`libgcc\`
package:

    apk add libgcc

The binary is dynamically linked on purpose: a static musl build cannot \`dlopen\`,
and \`dlopen\` is how \`[route.php]\` reaches the distro's \`libphp\` (with the PHP
shims in this archive) and how a WSGI view reaches the host's \`libpython\`."
else
    requirements="**glibc $floor or newer.** That is the floor measured in this binary:

    readelf --version-info blastit | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -1

Check a host with \`getconf GNU_LIBC_VERSION\` before deploying. Nothing is linked
against an interpreter: no \`libpython\`, no \`libphp\`. \`[route.php]\` works with the
distro's NTS \`libphp\` and the PHP shims in this archive."
fi
if [ -n "$builder_image" ]; then
    built_in="Built inside \`$builder_image\`."
else
    built_in="Built on the release machine."
fi
if [ "$py_shims" -gt 0 ]; then
    wsgi_try="A WSGI view (the host needs CPython $py_minor_list with its shared \`libpython\`
— see “What it needs at run time”):

    cat > app.py <<'PY'
    def application(env, start_response):
        start_response(\"200 OK\", [])
        return [b\"hello\\n\"]
    PY
    printf 'entrypoint python \"app:application\"\\n' > Blastfile
    ./blastit dev --port 8080
"
else
    wsgi_try="A WSGI view is not available from this archive, which carries no CPython
shim — see “What it needs at run time”."
fi
cat > "$stage/$name/README.md" <<README
# blastit — $target

The universal artifact from \`ADR-0017\` decision 1: one \`blastit\` binary serves
Python WSGI apps, embedded PHP routes and \`static\` mounts, and links neither
\`libpython\` nor \`libphp\`. Python arrives at run time through \`dlopen\` of the
host's \`libpython\` plus the per-minor CPython shims below, PHP through \`dlopen\`
of the distro's NTS \`libphp\` plus the PHP shims below. There is no Python-less
build of this binary to install instead.

## Contents

- \`blastit\` — the server (\`blastit [--preset NAME]\` for a production boot, \`blastit dev\`, \`blastit check\`)
$php_contents$py_contents- \`LICENSE\`, \`SHA256SUMS\` (verify with \`sha256sum -c SHA256SUMS\`)

## Try it

A \`static\` mount:

    printf 'hello from blastit\\n' > index.html
    printf 'static "/" { root "." }\\n' > Blastfile
    ./blastit dev --port 8080

Then open http://127.0.0.1:8080.

$wsgi_try
## What it needs at run time

$requirements

$wsgi_contract

Each shim is \`dlopen\`ed, so the loader applies its version needs as well: a
shim's floor is its runtime distribution's (PHP or CPython), not this archive's.

$shim_lines
## How it was built

$built_in A binary packaged from a build host keeps that host's C-library floor,
so the floor above is a measurement of this file, not an estimate of the toolchain.

Full install and runtime reference → https://offworldnexus.github.io/blastit/
README

# The list is built from what is actually staged (`find`), not from a glob: a
# leg with no shim yet still gets a valid archive and a checksum over what it
# really contains.
( cd "$stage/$name" && for file in *; do
    [ -f "$file" ] && [ "$file" != SHA256SUMS ] && printf '%s\n' "$file"
done | sort | xargs sha256sum > SHA256SUMS )

mkdir -p "$out"
tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
( cd "$out" && sha256sum "$name.tar.gz" > SHA256SUMS.tmp \
    && mv SHA256SUMS.tmp SHA256SUMS )

echo "build-standalone: $(ls -l "$out/$name.tar.gz" | awk '{ print $5 }') bytes -> $out/$name.tar.gz"
echo "build-standalone: checksum -> $out/SHA256SUMS"
