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
# Usage: scripts/build-standalone.sh [--out DIR] [--target TRIPLE]
#                                   [--shim-dir DIR] [--require-shims N]
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

out="dist"
target=""
shim_dir="${PHP_SAPI_OUT:-$root/target/php-sapi}"
require_shims=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) out=$2; shift 2 ;;
        --target) target=$2; shift 2 ;;
        --shim-dir) shim_dir=$2; shift 2 ;;
        --require-shims) require_shims=$2; shift 2 ;;
        *) echo "usage: $0 [--out DIR] [--target TRIPLE] [--shim-dir DIR] [--require-shims N]" >&2; exit 2 ;;
    esac
done

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
binary="target/$target/release/blastit"
[ "$target" = "$host" ] && binary="target/release/blastit"
if [ ! -x "$binary" ]; then
    echo "error: $binary was not produced: cargo built nothing for $target" >&2
    echo "  (the host binary at target/release/blastit is not this artifact)" >&2
    exit 1
fi
scripts/check-portable-elf.sh "$binary"

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
for shim in "$shim_dir"/libblastit_php*-"$arch"-"$libc".so; do
    [ -f "$shim" ] || continue
    install -m 0644 "$shim" "$stage/$name/$(basename "$shim")"
    shims=$((shims + 1))
done
echo "build-standalone: $shims shim(s) for $arch-$libc from $shim_dir"
if [ "$shims" -lt "$require_shims" ]; then
    echo "error: $shims shim(s) in $shim_dir, this artifact must carry $require_shims:" >&2
    echo "  build them first (make php-sapi PHP_MINORS=... or the per-leg CI job)" >&2
    exit 1
fi

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
