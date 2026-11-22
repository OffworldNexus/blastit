#!/usr/bin/env bash
# Builds the one §6.1 bundle the seven `.deb`s and seven `.rpm`s are packaged
# from (`MB.42.3.1`, `MB.65`, `universal-distribution.md` §6.1).
#
#   scripts/build-package-bundle.sh --out /tmp/opencode/blastit-packages/bundle
#
# `make install` builds on the *host*, so a glibc-2.41 machine stamps 2.39 into
# the artifact and an older runner would stamp something else for the same commit.
# A package is a claim about *other* machines, so the bundle that becomes one is
# built where the floors are declared:
#
#   * `blastit`, `blastit-lsp` are compiled inside
#     `quay.io/pypa/manylinux_2_28_<arch>` (the matrix's `glibc_floor`) and
#     measured with `--max-glibc 2_28`, exactly as `scripts/build-standalone-in-image.sh`
#     does for the archive family;
#   * the two `gnu` Python shims and the four PHP shims are the artifacts `make
#     py-sapi` / the distro `shims` legs already built **and probed** (`--py-shim-dir`,
#     `--shim-dir`). The Python shims cannot be built in manylinux: those images
#     ship no shared `libpython` at all, and a shim is only allowed to ship after
#     `make py-sapi` loaded it against a real interpreter. Their projects are
#     built for glibc 2.28 and measure 2.34, which is inside the package's ceiling
#     (below);
#   * the four PHP shims come from their distro legs, and measure far below the
#     ceiling (2.17 today).
#
# The result is assembled here (bin symlinks, `LICENSE`, the 7 modules) and asserted
# with `scripts/check-bundle-layout.sh` (2 Python + 4 PHP shims, no strays) and
# `check-portable-elf.sh --max-glibc <bundle-floor>` before it is returned: a bundle
# that lost a shim, or whose measured floor exceeds the oldest advertised
# distribution, never reaches a package.
#
# The container never writes into the repository as root: it installs the C
# toolchain as root, then `setpriv`s to the caller's uid, like
# `scripts/build-standalone-in-image.sh` (`MB.3.63`).
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

arch=""
out=""
shim_dir=""
py_shim_dir=""
binary_floor="2_28"
bundle_floor=""
require_php=4
require_py=2

usage() {
    cat >&2 <<'USAGE'
usage: scripts/build-package-bundle.sh --out <dir> [--arch x86_64|aarch64]
                                       [--shim-dir <dir>] [--py-shim-dir <dir>]
                                       [--binary-floor 2_28] [--bundle-floor 2_34]

  --out           where the §6.1 bundle (bin/ + share/blastit/) is assembled
  --arch          default: this machine (only native builds are allowed)
  --shim-dir      the 4 PHP shims `make php-sapi` / the distro legs built
                  (default target/php-sapi)
  --py-shim-dir   the 2 Python shims `make py-sapi` built and probed
                  (default target/py-sapi)
  --binary-floor  ceiling for blastit/blastit-lsp in the manylinux image (2_28)
  --bundle-floor  ceiling for the whole bundle, every shim included (the matrix's
                  `--shim-floor`: the oldest advertised distribution)

Both builders take this bundle next (`build-deb.sh` writes the seven `.deb`s,
`build-rpm.sh` the seven `.rpm`s), and each package's own ceiling is the matrix's
shim floor: `scripts/platform_matrix.py --shim-floor`.
USAGE
    exit 2
}

die() {
    echo "error: $*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) [ "$#" -ge 2 ] || usage; out=$2; shift 2 ;;
        --arch) [ "$#" -ge 2 ] || usage; arch=$2; shift 2 ;;
        --shim-dir) [ "$#" -ge 2 ] || usage; shim_dir=$2; shift 2 ;;
        --py-shim-dir) [ "$#" -ge 2 ] || usage; py_shim_dir=$2; shift 2 ;;
        --binary-floor) [ "$#" -ge 2 ] || usage; binary_floor=$2; shift 2 ;;
        --bundle-floor) [ "$#" -ge 2 ] || usage; bundle_floor=$2; shift 2 ;;
        -h | --help) usage ;;
        *) usage ;;
    esac
done

[ -n "$out" ] || usage
command -v docker >/dev/null 2>&1 ||
    die "docker is required: this bundle is built inside its floor image, never on the host"

host_machine=$(uname -m)
arch=${arch:-$host_machine}
case "$arch" in
    x86_64 | aarch64) ;;
    *) die "--arch '$arch' is not x86_64 or aarch64" ;;
esac
shim_dir=${shim_dir:-$root/target/php-sapi}
py_shim_dir=${py_shim_dir:-$root/target/py-sapi}

container_arch=$(docker version --format '{{.Server.Arch}}' 2>/dev/null || true)
case "$container_arch" in
    amd64 | "") container_arch=x86_64 ;;
    arm64) container_arch=aarch64 ;;
esac
[ "$container_arch" = "$arch" ] ||
    die "$arch needs a $arch container and this host runs $container_arch images: an emulated compiler is not evidence"

image="quay.io/pypa/manylinux_2_28_${arch}"
target_dir="target/packages-bundle-${arch}"
rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
cargo_home=${CARGO_HOME:-$HOME/.cargo}
[ -d "$rustup_home/toolchains" ] ||
    die "no Rust toolchain at $rustup_home/toolchains: install one (rustup)"

for directory in "$out" "$target_dir"; do
    if ! mkdir -p "$directory" 2>/dev/null || [ ! -w "$directory" ]; then
        die "'$directory' cannot be written by $(id -un) ($(id -u)): the container builds as this uid"
    fi
done

# The shims exist before the build, because a bundle missing one is not a package
# this script may hand to a builder. PHP shims come from their distro legs; the
# Python shims are what `make py-sapi` built *and proved* (manylinux has no shared
# libpython, so that is the only place they can be loaded).
php_shims=0
for shim in "$shim_dir"/libblastit_php*-"$arch"-gnu.so; do
    [ -e "$shim" ] || continue
    php_shims=$((php_shims + 1))
done
[ "$php_shims" -ge "$require_php" ] ||
    die "$shim_dir has $php_shims PHP shim(s), a package needs $require_php (8.3-8.6): build the missing legs (\`make php-sapi-e2e PHP_MINOR=8.3\` then \`scripts/collect-shim.sh\`)"
py_shims_present=0
for shim in "$py_shim_dir"/libblastit_py3*-"$arch"-gnu.so; do
    [ -e "$shim" ] || continue
    py_shims_present=$((py_shims_present + 1))
done
[ "$py_shims_present" -ge "$require_py" ] ||
    die "$py_shim_dir has $py_shims_present Python shim(s), a package needs $require_py (3.13, 3.14): run \`make py-sapi\`"

echo "build-package-bundle: $arch in $image (binaries <= $binary_floor; Python shims from $py_shim_dir; PHP shims from $shim_dir)"

docker run --rm \
    -e HOST_UID="$(id -u)" \
    -e HOST_GID="$(id -g)" \
    -e ARCH="$arch" \
    -e CEILING="$binary_floor" \
    -e TARGET_DIR="$target_dir" \
    -e RUSTUP_HOME=/host-rustup \
    -e CARGO_HOME=/tmp/build-cargo-home \
    -e CARGO_TARGET_DIR="/src/$target_dir" \
    -v "$root:/src" \
    -v "$rustup_home:/host-rustup:ro" \
    -v "$cargo_home:/host-cargo:ro" \
    -w /src \
    "$image" \
    bash -euo pipefail -c '
        if command -v dnf >/dev/null 2>&1; then
            dnf install -y --setopt=install_weak_deps=False clang binutils tar gzip git >/dev/null
        else
            echo "error: $0 needs a builder with dnf (manylinux is AlmaLinux)" >&2
            exit 1
        fi
        command -v setpriv >/dev/null 2>&1 || {
            echo "error: no setpriv (util-linux): the build cannot drop to uid $HOST_UID" >&2
            exit 1
        }
        export PATH="/host-rustup/toolchains/stable-$ARCH-unknown-linux-gnu/bin:$PATH"
        cat <<\INNER > /tmp/package-bundle-inner.sh
set -euo pipefail
mkdir -p "$HOME" "$CARGO_HOME/registry" "$CARGO_HOME/git"
if [ -d /host-cargo/registry ]; then
    cp -a /host-cargo/registry/. "$CARGO_HOME/registry/"
fi
if [ -d /host-cargo/git ]; then
    cp -a /host-cargo/git/. "$CARGO_HOME/git/"
fi
if [ -f /host-cargo/config.toml ]; then
    cp /host-cargo/config.toml "$CARGO_HOME/config.toml"
fi
cargo --version
echo "build-package-bundle: glibc $(getconf GNU_LIBC_VERSION 2>/dev/null || ldd --version | head -1)"
cargo build --release --locked -p blastit -p blastit-lsp
for binary in blastit blastit-lsp; do
    test -x "$CARGO_TARGET_DIR/release/$binary" || {
        echo "error: cargo produced no $CARGO_TARGET_DIR/release/$binary (CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-unset})" >&2
        exit 1
    }
done
scripts/check-portable-elf.sh --max-glibc "$CEILING" \
    "$CARGO_TARGET_DIR/release/blastit" \
    "$CARGO_TARGET_DIR/release/blastit-lsp"
INNER
        chmod 0755 /tmp/package-bundle-inner.sh
        exec setpriv --reuid "$HOST_UID" --regid "$HOST_GID" --clear-groups \
            env HOME=/tmp/build-home \
            bash -euo pipefail /tmp/package-bundle-inner.sh
    '

# --- assemble §6.1 -------------------------------------------------------------
rm -rf "$out/bin" "$out/share"
install -d "$out/bin" "$out/share/blastit/lib" "$out/share/blastit/python/blastit"
install -m 0755 "$target_dir/release/blastit" "$target_dir/release/blastit-lsp" \
    "$out/share/blastit/lib/"
py_shims=0
for shim in "$py_shim_dir"/libblastit_py3*-"$arch"-gnu.so; do
    [ -e "$shim" ] || continue
    install -m 0755 "$shim" "$out/share/blastit/lib/"
    py_shims=$((py_shims + 1))
done
[ "$py_shims" -eq "$require_py" ] ||
    die "$py_shim_dir has $py_shims Python shim(s), a package needs $require_py (3.13, 3.14)"
copied_php=0
for shim in "$shim_dir"/libblastit_php*-"$arch"-gnu.so; do
    [ -e "$shim" ] || continue
    install -m 0755 "$shim" "$out/share/blastit/lib/"
    copied_php=$((copied_php + 1))
done
[ "$copied_php" -eq "$require_php" ] ||
    die "$shim_dir now has $copied_php PHP shim(s), expected $require_php"
install -m 0644 LICENSE "$out/share/blastit/LICENSE"
install -m 0644 python/blastit/*.py "$out/share/blastit/python/blastit/"
ln -s ../share/blastit/lib/blastit "$out/bin/blastit"
ln -s ../share/blastit/lib/blastit-lsp "$out/bin/blastit-lsp"

layout=$(BUNDLE_REQUIRE_PY_SHIMS="$require_py" BUNDLE_REQUIRE_PHP_SHIMS="$require_php" \
    scripts/check-bundle-layout.sh "$out")
printf '%s\n' "$layout" | sed 's/^/build-package-bundle: layout: /'

# The bundle is a promise about *other* machines. When the matrix's oldest
# supported distribution gives a ceiling (`--bundle-floor 2_34`), a shim that
# needs more is refused here, by name, instead of silently shipping a package
# that refuses Debian 12 or Rocky 9. The fix is always to rebuild that shim on an
# older base (Surý/bookworm for PHP, `manylinux_2_28` for the rest), never to
# drop the distribution.
ceiling=auto
[ -z "$bundle_floor" ] || ceiling=$bundle_floor
report=$(scripts/check-portable-elf.sh --max-glibc "$ceiling" "$out"/share/blastit/lib/blastit \
    "$out"/share/blastit/lib/blastit-lsp "$out"/share/blastit/lib/libblastit_*.so) || {
    if [ -n "$bundle_floor" ]; then
        echo "build-package-bundle: a shipped shim is above the $bundle_floor this package promises:" >&2
        echo "  rebuild that shim on an older base (PHP: debian:bookworm-slim + Ondřej Surý's repository;" >&2
        echo "  blastit/blastit-lsp/Python shims: manylinux_2_28) and re-run with that --shim-dir" >&2
    fi
    exit 1
}
printf '%s\n' "$report" | sed 's/^/build-package-bundle: /'
echo "build-package-bundle: $out"
