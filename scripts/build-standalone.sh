#!/usr/bin/env bash
# Builds one standalone archive — the artifact a host installs without Python
# (`ADR-0010` §4, node MB.3.18).
#
# The archive is `blastit-<target>.tar.gz` and holds:
#
#   * `blastit` — the interpreter-free binary. It is built with
#     `--no-default-features --features php`, which is what makes "no Python" a
#     property of the *artifact* and not of the manifest it is given: the
#     capability is off, so a WSGI manifest is refused at exit 1 instead of
#     failing later (see `crates/blastit/tests/no_python_build.rs`).
#   * `libblastit_php<digits>-<arch>-<libc>.so` — one shim per PHP minor this
#     leg has built *and* probed (`scripts/platform_matrix.py` is the list).
#   * `LICENSE`, and `SHA256SUMS` over everything in the archive.
#
# `scripts/check-portable-elf.sh` runs on the binary before packaging: no
# `libpython` `NEEDED` entry, no `RPATH`/`RUNPATH` into a build host. That is the
# difference between an archive that runs on a server and one that only runs in
# the checkout that produced it.
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
#                                   [--max-glibc 2_28] [--builder-image IMAGE]
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

out="dist"
target=""
shim_dir="${PHP_SAPI_OUT:-$root/target/php-sapi}"
require_shims=0
max_glibc=""
builder_image=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) out=$2; shift 2 ;;
        --target) target=$2; shift 2 ;;
        --shim-dir) shim_dir=$2; shift 2 ;;
        --require-shims) require_shims=$2; shift 2 ;;
        --max-glibc) max_glibc=$2; shift 2 ;;
        --builder-image) builder_image=$2; shift 2 ;;
        *) echo "usage: $0 [--out DIR] [--target TRIPLE] [--shim-dir DIR] [--require-shims N] [--max-glibc 2_28] [--builder-image IMAGE]" >&2; exit 2 ;;
    esac
done

# `<major>_<minor>` (the manylinux spelling, as `check_wheel_tag.py --max-glibc`
# takes it) and `<major>.<minor>` (the glibc spelling) are the same number here.
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

echo "build-standalone: cargo build --release --no-default-features --features php ${target_args[*]:-}"
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
        --no-default-features --features php ${target_args[@]+"${target_args[@]}"}
else
    cargo build --release --locked -p blastit --bin blastit \
        --no-default-features --features php ${target_args[@]+"${target_args[@]}"}
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
# links", so a `pyo3` here is a `libpython` the archive does not have.
pyo3=$(cargo tree -p blastit -e normal --no-default-features --features php \
    --target "$target" | grep -c pyo3 || true)
if [ "$pyo3" -ne 0 ]; then
    echo "error: the standalone build depends on pyo3 ($pyo3): it would link libpython" >&2
    exit 1
fi
echo "build-standalone: 0 pyo3 dependencies (no libpython linked)"

stage=$(mktemp -d "${TMPDIR:-/var/tmp}/blastit-standalone-stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
name="blastit-$target"
mkdir -p "$stage/$name"
install -m 0755 "$binary" "$stage/$name/blastit"
install -m 0644 LICENSE "$stage/$name/LICENSE"

shims=0
shim_lines=""
for shim in "$shim_dir"/libblastit_php*-"$arch"-"$libc".so; do
    [ -f "$shim" ] || continue
    install -m 0644 "$shim" "$stage/$name/$(basename "$shim")"
    shims=$((shims + 1))
    # A shim is `dlopen`ed, so the loader applies *its* version needs too: the
    # binary's floor is not the archive's only number. Each one is measured and
    # listed, because a 2.28 binary beside a 2.41 shim is portable only for the
    # routes that do not use PHP (`MB.3.60`).
    if [ -n "$musl" ]; then
        shim_lines="$shim_lines- \`$(basename "$shim")\` — musl (no glibc; needs \`libgcc\`)
"
    else
        shim_floor=$(
            readelf --version-info "$shim" \
                | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' \
                | sort -V \
                | tail -n 1
        )
        shim_lines="$shim_lines- \`$(basename "$shim")\` — glibc ${shim_floor:-none} or newer
"
    fi
done
if [ "$shims" -eq 0 ]; then
    shim_lines="No shims in this archive. Build them with \`make php-sapi\` (or use the
wheel, which ships the interpreter and no shims)."
fi
echo "build-standalone: $shims shim(s) for $arch-$libc from $shim_dir"
if [ "$shims" -lt "$require_shims" ]; then
    echo "error: $shims shim(s) in $shim_dir, this artifact must carry $require_shims:" >&2
    echo "  build them first (make php-sapi PHP_MINORS=... or the per-leg CI job)" >&2
    exit 1
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
and \`dlopen(libphp)\` is how \`[route.php]\` works."
else
    requirements="**glibc $floor or newer.** That is the floor measured in this binary:

    readelf --version-info blastit | grep -o 'GLIBC_[0-9.]*' | sort -V | tail -1

Check a host with \`getconf GNU_LIBC_VERSION\` before deploying. There is no other
runtime requirement: no Python, no \`libpython\`, no PHP unless you declare
\`[route.php]\` (then the distro's NTS \`libphp\` for the shim above)."
fi
if [ -n "$builder_image" ]; then
    built_in="Built inside \`$builder_image\`."
else
    built_in="Built on the machine that ran \`scripts/build-standalone.sh\`."
fi
cat > "$stage/$name/README.md" <<README
# blastit — $target

The interpreter-free artifact from \`ADR-0010\` §4: the \`php\` capability, no
\`libpython\`. \`[route.php]\` works; a WSGI manifest is refused at exit 1 with
\`uv add blastit\` as the fix, because this build has no Python by design.

## Contents

- \`blastit\` — the server (\`blastit dev\`, \`blastit check\`)
- \`libblastit_php<minor>-$arch-$libc.so\` — PHP SAPI shim(s), $shims in this archive
- \`LICENSE\`, \`SHA256SUMS\` (verify with \`sha256sum -c SHA256SUMS\`)

## What it needs at run time

$requirements

Each shim is \`dlopen\`ed, so the loader applies its version needs as well: a
shim's floor is the PHP distribution's, not this archive's.

$shim_lines
## How it was built

$built_in A binary packaged from a build host keeps that host's C-library floor,
so the floor above is a measurement of this file, not an estimate of the toolchain.
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
