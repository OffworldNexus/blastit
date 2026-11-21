#!/usr/bin/env bash
# Builds one `blastit-lsp` archive — the portable language server an editor
# downloads (`MB.35.5`, `ADR-0010` §4, design §8.3a and §14.4).
#
# The archive is `blastit-lsp-<target>.tar.gz` on Linux and macOS and
# `blastit-lsp-<target>.zip` on Windows, and it holds exactly three entries under
# one top-level `blastit-lsp-<target>/` directory:
#
#   * `blastit-lsp` (`blastit-lsp.exe` on Windows) — the server. No Python in its
#     dependency graph at all, so no `libpython` and no build-host `RUNPATH`.
#   * `LICENSE`.
#   * `SHA256SUMS` over the two files above, written with whichever SHA-256 tool
#     the leg's OS ships (`sha256sum` on Linux/Windows, `shasum -a 256` on macOS —
#     coreutils is not preinstalled there) and verifiable with either, because
#     both print `<hex>  <name>`.
#
# The layout is the contract the Zed extension's extractor reads: the executable
# sits one directory down, never at the archive root, and the directory name is
# the target triple the resolution table names.
#
# **Native, never cross.** Every target is built on its own runner, and a
# `--target` that is not this host's triple is refused by name: an artifact
# nothing ever executed is a claim, not a proof (`MB.35.1`). The four
# non-Linux legs are therefore built on `macos-latest`, `macos-15-intel` and
# `windows-latest`.
#
# **The Linux floor is measured and built, not inherited.** `--image IMAGE` runs
# the build inside `quay.io/pypa/manylinux_2_28_<arch>` — the image whose glibc
# *is* the floor — and `--max-glibc 2_28` reads the highest `GLIBC_<x.y>` the
# finished binary needs, refusing a build that outgrew it (`MB.3.60`'s defect
# class, here with an editor binary instead of a serving one). A host build
# without `--image` keeps the host's floor, which is why the shipping workflow
# always names the image for a Linux leg.
#
# Usage:
#   scripts/build-lsp.sh [--target TRIPLE] [--out DIR] [--max-glibc 2_28]
#                        [--image quay.io/pypa/manylinux_2_28_<arch>]
#                        [--archive NAME] [--dir NAME] [--executable NAME]
#
# `--archive`, `--dir` and `--executable` are the three names
# `scripts/platform_matrix.py --matrix lsp` derives per leg; CI passes them from
# `${{ matrix.* }}` so the asset is the table's, and the defaults (`blastit-lsp-
# <target>.tar.gz|.zip`, `blastit-lsp-<target>/`, `blastit-lsp[.exe]`) are the
# same rule for a developer building by hand. The five target triples live in
# `platform_matrix.py` and are matched here as OS **shapes**, never re-listed.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

usage() {
    echo "usage: $0 [--target TRIPLE] [--out DIR] [--max-glibc 2_28]" >&2
    echo "          [--image quay.io/pypa/manylinux_2_28_<arch>]" >&2
    echo "          [--archive NAME] [--dir NAME] [--executable NAME]" >&2
    echo "       (builds one blastit-lsp archive; --image builds it inside the" >&2
    echo "        manylinux image whose glibc is the --max-glibc floor, and the" >&2
    echo "        three names are the ones scripts/platform_matrix.py derives)" >&2
    exit 2
}

out="dist"
target=""
max_glibc=""
image=""
# The three names `MB.35.5`'s one table derives (`platform_matrix.py --matrix
# lsp`: `archive`, `dir`, `executable`). CI passes them, so the asset name is the
# table's and not a second copy of it; a developer who omits them gets the same
# formula from the OS, which is the only default this script may own.
archive=""
directory=""
executable=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) [ "$#" -ge 2 ] || usage; out=$2; shift 2 ;;
        --target) [ "$#" -ge 2 ] || usage; target=$2; shift 2 ;;
        --max-glibc) [ "$#" -ge 2 ] || usage; max_glibc=$2; shift 2 ;;
        --image) [ "$#" -ge 2 ] || usage; image=$2; shift 2 ;;
        --archive) [ "$#" -ge 2 ] || usage; archive=$2; shift 2 ;;
        --dir) [ "$#" -ge 2 ] || usage; directory=$2; shift 2 ;;
        --executable) [ "$#" -ge 2 ] || usage; executable=$2; shift 2 ;;
        -h|--help)
            sed -n '2,/^set -euo/{ /^set -euo/d; s/^# \{0,1\}//; p }' "${BASH_SOURCE[0]}"
            exit 0
            ;;
        *) echo "$(basename "$0"): unknown argument '$1'" >&2; usage ;;
    esac
done

host=$("${RUSTC:-rustc}" -vV | sed -n 's/^host: //p')
target=${target:-$host}
arch=${target%%-*}

# The OS a triple names, matched as a **shape**, never as one of five literals:
# the five targets live in `scripts/platform_matrix.py` (`LSP_LEGS`) and nowhere
# else, so adding a leg there reaches this script without a second edit, and a
# sixth OS shape is refused by pattern rather than by an edit to a list here.
case "$target" in
    *-unknown-linux-gnu)
        platform=linux
        ext=tar.gz
        default_executable=blastit-lsp
        ;;
    *-apple-darwin)
        platform=macos
        ext=tar.gz
        default_executable=blastit-lsp
        ;;
    *-pc-windows-msvc)
        platform=windows
        ext=zip
        default_executable=blastit-lsp.exe
        ;;
    *)
        echo "$(basename "$0"): $target is not an OS this archive ships a language" >&2
        echo "  server for (the shapes are *-unknown-linux-gnu, *-apple-darwin and" >&2
        echo "  *-pc-windows-msvc; the five targets are the table:" >&2
        echo "  uv run --no-project scripts/platform_matrix.py --matrix lsp)" >&2
        exit 2
        ;;
esac
# An arch prefix this project has never built is a target nobody proved, and the
# matrix names arches in one place too.
case "$arch" in
    x86_64|aarch64) ;;
    *)
        echo "$(basename "$0"): $target starts with $arch, which is not an arch this" >&2
        echo "  project builds (x86_64, aarch64 — scripts/platform_matrix.py)" >&2
        exit 2
        ;;
esac

executable=${executable:-$default_executable}
directory=${directory:-blastit-lsp-$target}
archive=${archive:-$directory.$ext}
# A caller-supplied name is checked against the format rather than trusted: the
# extension's extractor reads the suffix, so a `.tar.gz` on a Windows leg would be
# a file nothing unpacks.
case "$archive" in
    *."$ext") ;;
    *)
        echo "$(basename "$0"): --archive '$archive' does not end in .$ext, the format" >&2
        echo "  $platform packages (its asset is \`blastit-lsp-<target>.$ext\`)" >&2
        exit 2
        ;;
esac

# A foreign triple would need a cross toolchain and would never be executed here.
if [ "$target" != "$host" ]; then
    echo "$(basename "$0"): $target is not this host ($host), so it cannot be smoked here" >&2
    echo "  build it on its native runner (scripts/platform_matrix.py --matrix lsp names" >&2
    echo "  the runner per leg); an artifact that was never executed is a claim" >&2
    exit 2
fi

if [ "$platform" = linux ]; then
    if [ -n "$max_glibc" ]; then
        max_glibc=${max_glibc//_/.}
        if ! printf '%s' "$max_glibc" | grep -Eq '^[0-9]+\.[0-9]+$'; then
            echo "error: --max-glibc '$max_glibc' is not <major>_<minor> (e.g. 2_28)" >&2
            exit 2
        fi
    fi
elif [ -n "$max_glibc" ]; then
    echo "error: --max-glibc does not apply to $target: a $platform binary carries no" >&2
    echo "  glibc symbols, and there is no floor to measure" >&2
    exit 2
fi

# --- inside the image: the same script, one container down -------------------
#
# The image supplies the C library, the C compiler and libclang (bindgen needs the
# clang C API for `pg_query`, which `blastit-manifest` reaches); the Rust
# toolchain is the host's, mounted read-only, so no leg downloads a second one.
# `CARGO_TARGET_DIR` is redirected per image: a host `target/` is full of objects
# compiled against the host's glibc, and reusing them puts the runner's floor back
# into an artifact whose purpose is a lower one.
if [ -n "$image" ]; then
    if [ "$platform" != linux ]; then
        echo "error: --image is for the Linux legs: $target is built by $platform's own" >&2
        echo "  toolchain on its native runner" >&2
        exit 2
    fi
    if [ -z "$max_glibc" ]; then
        echo "error: --image needs --max-glibc (e.g. 2_28): the image is the floor, and a" >&2
        echo "  build inside it that never asserts the floor is a container with extra steps" >&2
        exit 2
    fi
    command -v docker >/dev/null 2>&1 || {
        echo "$(basename "$0"): 'docker' is not on PATH" >&2
        exit 2
    }
    container_arch=$(docker version --format '{{.Server.Arch}}' 2>/dev/null || true)
    case "$container_arch" in
        amd64|"") container_arch=x86_64 ;;
        arm64) container_arch=aarch64 ;;
    esac
    if [ "$container_arch" != "$arch" ]; then
        echo "$(basename "$0"): $target needs a $arch container and this host runs" >&2
        echo "  $container_arch images: build the leg on its native runner (an emulated C" >&2
        echo "  compiler is not evidence)" >&2
        exit 2
    fi

    # Only the repository is mounted (at /src), so only a path inside it can be
    # written by the container — and a path it cannot see silently packages
    # nothing.
    case "$out" in
        /*)
            case "$out" in
                "$root"/*) out_in=/src/${out#"$root"/} ;;
                *)
                    echo "$(basename "$0"): '$out' is outside the repository, so the" >&2
                    echo "  container cannot write it; pass a path inside $root" >&2
                    exit 2
                    ;;
            esac
            ;;
        *) out_in=$out ;;
    esac

    rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
    cargo_home=${CARGO_HOME:-$HOME/.cargo}
    if [ ! -d "$rustup_home/toolchains" ]; then
        echo "$(basename "$0"): no Rust toolchain at $rustup_home/toolchains" >&2
        echo "  install one (rustup, or dtolnay/rust-toolchain in CI); it is mounted" >&2
        echo "  read-only instead of downloaded again per image" >&2
        exit 2
    fi
    target_subdir="lsp-${max_glibc//./_}"

    # The trailing space inside the array is load-bearing: the repository's
    # offline scan requires a strict boolean and takes `\S*` as the value.
    offline_env=()
    if [ "${CARGO_NET_OFFLINE:-}" = "true" ]; then
        offline_env=(-e CARGO_NET_OFFLINE=true )
    fi

    echo "build-lsp: $target inside $image (floor $max_glibc)"
    docker run --rm \
        -e HOST_UID="$(id -u)" \
        -e HOST_GID="$(id -g)" \
        -e TARGET="$target" \
        -e MAX_GLIBC="$max_glibc" \
        -e OUT="$out_in" \
        -e BUILDER_IMAGE="$image" \
        -e RUSTUP_HOME=/host-rustup \
        -e CARGO_HOME=/tmp/build-cargo-home \
        -e CARGO_TARGET_DIR="/src/target/$target_subdir" \
        "${offline_env[@]}" \
        -v "$root:/src" \
        -v "$rustup_home:/host-rustup:ro" \
        -v "$cargo_home:/host-cargo:ro" \
        -w /src \
        "$image" \
        bash -euo pipefail -c '
            # Root does two things: install what the image does not carry
            # (`clang` for bindgen, `libcap` for the `getcap` assertion), then
            # hand the build to HOST_UID so one identity writes the cache, the
            # target directory and the archive (`MB.3.63`).
            if command -v dnf >/dev/null 2>&1; then
                dnf install -y --setopt=install_weak_deps=False clang binutils tar gzip libcap \
                    >/dev/null
            elif command -v apt-get >/dev/null 2>&1; then
                apt-get update >/dev/null
                apt-get install -y --no-install-recommends clang libclang-dev binutils \
                    libcap2-bin tar gzip >/dev/null
            elif command -v apk >/dev/null 2>&1; then
                apk add --no-cache clang llvm-dev binutils libcap tar gzip >/dev/null
                export LIBCLANG_PATH=/usr/lib/llvm19/lib
            else
                echo "error: $BUILDER_IMAGE has no package manager this script knows" >&2
                exit 1
            fi
            command -v setpriv >/dev/null 2>&1 || {
                echo "error: $BUILDER_IMAGE has no setpriv (util-linux), so this build cannot" >&2
                echo "  drop to uid $HOST_UID: one identity must write the cache, the target" >&2
                echo "  directory and the archive (MB.3.63)" >&2
                exit 1
            }

            # No second Rust toolchain: the host one is mounted read-only, and this
            # one PATH entry is what both the container and the dropped process use.
            export PATH="/host-rustup/toolchains/stable-$(uname -m)-unknown-linux-gnu/bin:$PATH"
            if ! command -v cargo >/dev/null 2>&1; then
                toolchain_bin=$(find /host-rustup/toolchains -maxdepth 2 -type d -name bin | head -1)
                export PATH="$toolchain_bin:$PATH"
            fi

            # The body runs as HOST_UID, so it is a heredoc written straight to a
            # file rather than a quoted string with its own copy of the build.
            # It is deliberately NOT wrapped in `$(cat <<...)`: manylinux_2_28 is
            # AlmaLinux 8 and ships Bash 4.4, whose parser fails on a comment
            # carrying a closing parenthesis inside a command-substitution
            # heredoc, with `unexpected EOF while looking for matching )`, before
            # a single byte of the build ran (PR #64). The delimiter is
            # backslash-quoted rather than `<<BUILD_BODY` so the body is taken
            # literally, and NOT apostrophe-quoted because this whole container
            # script is one single-quoted `bash -c` argument: a literal quote
            # would end it early. `CARGO_HOME` is a writable
            # directory **seeded by copying** the read-only host cache: cargo
            # extracts `registry/src/<crate>` there, and a symlink into the mount
            # made the first cold-cache extraction die with
            # `Read-only file system os error 30` (MB.3.63).
            cat <<\BUILD_BODY > /tmp/blastit-inner-build.sh
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
echo "build-lsp: building as uid $(id -u):$(id -g), cache $CARGO_HOME"
echo "build-lsp: glibc $(getconf GNU_LIBC_VERSION 2>/dev/null || ldd --version | head -1)"
bash scripts/build-lsp.sh --target "$TARGET" --out "$OUT" --max-glibc "$MAX_GLIBC"
BUILD_BODY
            chmod 0755 /tmp/blastit-inner-build.sh

            exec setpriv --reuid "$HOST_UID" --regid "$HOST_GID" --clear-groups \
                env HOME=/tmp/build-home \
                bash -euo pipefail /tmp/blastit-inner-build.sh
        '

    echo "build-lsp: $out/blastit-lsp-$target.$ext"
    exit 0
fi

# --- the checksum tool ------------------------------------------------------
#
# Every leg writes the same file with whichever SHA-256 tool its OS actually
# ships, and the written format is identical because both tools print
# `<hex>  <name>` (two spaces):
#
#   sha256sum <files>              Linux, Windows (Git Bash), manylinux images
#   shasum -a 256 <files>          macOS — coreutils is *not* preinstalled there,
#                                  so `sha256sum` does not exist on `macos-*`
#   sha256sum -c SHA256SUMS        verifies on Linux/Windows
#   shasum -a 256 -c SHA256SUMS    verifies on macOS; `-a 256` is required,
#                                  because a bare `shasum -c` means SHA-1
#
# The selector is overridable (`BLASTIT_SHA_TOOL=shasum`) so both branches are
# testable on one host, and a host with neither tool is refused by name instead
# of writing an empty `SHA256SUMS` that would verify nothing.
sha_tool=${BLASTIT_SHA_TOOL:-}
case "$sha_tool" in
    "")
        if command -v sha256sum >/dev/null 2>&1; then
            sha_tool=sha256sum
        elif command -v shasum >/dev/null 2>&1; then
            sha_tool=shasum
        else
            echo "$(basename "$0"): neither 'sha256sum' nor 'shasum' is on PATH, so the" >&2
            echo "  archive's SHA256SUMS cannot be written (an empty one verifies nothing)" >&2
            echo "  fix: install coreutils (sha256sum), or perl's shasum (macOS has it:" >&2
            echo "  /usr/bin/shasum). BLASTIT_SHA_TOOL=sha256sum|shasum forces one" >&2
            exit 1
        fi
        ;;
    sha256sum|shasum)
        command -v "$sha_tool" >/dev/null 2>&1 || {
            echo "$(basename "$0"): BLASTIT_SHA_TOOL=$sha_tool but '$sha_tool' is not on PATH" >&2
            echo "  install it, or unset BLASTIT_SHA_TOOL to use whichever tool this host has" >&2
            exit 1
        }
        ;;
    *)
        echo "$(basename "$0"): BLASTIT_SHA_TOOL='$sha_tool' is not one of sha256sum, shasum" >&2
        exit 2
        ;;
esac

# One file per tool, one format for both: `sha256sum FILE` and `shasum -a 256
# FILE` emit the same line, so a macOS-written `SHA256SUMS` verifies on a Linux
# reader and the other way round (checked here, and in CI per leg).
sha256() {
    if [ "$sha_tool" = shasum ]; then
        shasum -a 256 "$@"
    else
        sha256sum "$@"
    fi
}
sha_verify() {
    if [ "$sha_tool" = shasum ]; then
        shasum -a 256 -c "$1"
    else
        sha256sum -c "$1"
    fi
}

# --- build ------------------------------------------------------------------

echo "build-lsp: cargo build -p blastit-lsp --release --locked --target $target"
cargo build -p blastit-lsp --release --locked --target "$target"

target_root="${CARGO_TARGET_DIR:-$root/target}"
case "$target_root" in
    /*) ;;
    *) target_root="$root/$target_root" ;;
esac
binary="$target_root/$target/release/blastit-lsp"
[ "$platform" = windows ] && binary="$binary.exe"
if [ ! -f "$binary" ]; then
    echo "error: $binary was not produced: cargo built nothing for $target" >&2
    echo "  (a binary at $target_root/release/blastit-lsp is not this artifact)" >&2
    exit 1
fi

# The portable-artifact promise, measured: `pyo3` anywhere in the normal closure
# would be a `libpython` this archive does not ship, and the crate's whole point
# is a dependency graph with no Python in it at all (`R69`, `R73`).
pyo3=$(cargo tree -p blastit-lsp -e normal --target "$target" | grep -c pyo3 || true)
if [ "$pyo3" -ne 0 ]; then
    echo "error: blastit-lsp depends on pyo3 ($pyo3): the archive would need libpython" >&2
    echo "  (it must depend on blastit-manifest and serde_json only — R69)" >&2
    exit 1
fi
echo "build-lsp: 0 pyo3 dependencies (no libpython in the graph)"

if [ "$platform" = linux ]; then
    # No `NEEDED libpython`, no build-host `RPATH`/`RUNPATH`: the same assertion
    # the standalone archive carries, on a binary an editor spawns from an
    # arbitrary working directory.
    scripts/check-portable-elf.sh "$binary"

    # `getcap` empty: the server is supported unprivileged only, so a capability
    # bit on the shipped file would be a promise nobody tested (ADR §1.3, §11).
    command -v getcap >/dev/null 2>&1 || {
        echo "error: 'getcap' is not on PATH, so the capability assertion cannot run" >&2
        echo "  install libcap2-bin (Debian/Ubuntu) or libcap (RHEL/Alpine)" >&2
        exit 1
    }
    caps=$(getcap "$binary")
    if [ -n "$caps" ]; then
        echo "error: $binary carries file capabilities: $caps" >&2
        echo "  the language server is supported unprivileged: clear them (setcap -r) and" >&2
        echo "  never grant them in the release path" >&2
        exit 1
    fi
    echo "build-lsp: getcap empty (supported unprivileged)"

    floor=$(
        readelf --version-info "$binary" \
            | sed -n 's/.*Name: GLIBC_\([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' \
            | sort -V \
            | tail -n 1
    )
    if [ -z "$floor" ]; then
        echo "error: $binary names no GLIBC_<version> symbol, so its floor is unknown" >&2
        exit 1
    fi
    echo "build-lsp: glibc floor $floor (readelf --version-info)"
    if [ -n "$max_glibc" ]; then
        newest=$(printf '%s\n%s\n' "$floor" "$max_glibc" | sort -V | tail -n 1)
        if [ "$newest" != "$max_glibc" ]; then
            echo "error: $binary needs glibc $floor, above the $max_glibc this archive must" >&2
            echo "  run on (Debian 12 carries 2.36)" >&2
            echo "  build it with --image quay.io/pypa/manylinux_2_28_$arch, or raise" >&2
            echo "  --max-glibc and say so in the docs — never lower it silently" >&2
            exit 1
        fi
        echo "build-lsp: floor $floor <= --max-glibc $max_glibc"
    else
        echo "build-lsp: no --max-glibc given, so this artifact keeps the floor of the" >&2
        echo "  host that built it ($host); the release path always names the image" >&2
    fi
fi

# --- stage and archive ------------------------------------------------------

# A staging directory under a temp base that exists **on this host**. Git Bash on
# `windows-latest` has `/tmp` (and `$RUNNER_TEMP`) but no `/var/tmp`, so a
# hardcoded `/var/tmp` makes the Windows leg die at `mktemp` with
# `failed to create directory via template '/var/tmp/blastit-lsp-stage.XXXXXX':
# No such file or directory` (PR #64). `$TMPDIR` wins when it is a directory,
# then `/var/tmp` (the Linux default), then `/tmp`.
tmp_base=${TMPDIR:-}
if [ -z "$tmp_base" ] || [ ! -d "$tmp_base" ]; then
    if [ -d /var/tmp ]; then tmp_base=/var/tmp; else tmp_base=/tmp; fi
fi
stage=$(mktemp -d "$tmp_base/blastit-lsp-stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
name=$directory
mkdir -p "$stage/$name"
cp "$binary" "$stage/$name/$executable"
chmod 0755 "$stage/$name/$executable"
cp LICENSE "$stage/$name/LICENSE"
# The in-archive list is built from what is actually staged, and excludes itself:
# `sha256sum -c SHA256SUMS` (Linux/Windows) or `shasum -a 256 -c SHA256SUMS`
# (macOS) in the unpacked directory must verify the two files a reader can see.
# It authenticates nothing (the trust root is HTTPS to `github.com`, `R75`) — it
# makes a truncated download visible. Written with `$sha_tool` and verified here
# with the same one, so a tool that writes what it cannot read is a failure at
# build time rather than a surprise on a user's machine.
(
    cd "$stage/$name"
    for file in *; do
        if [ -f "$file" ] && [ "$file" != SHA256SUMS ]; then
            sha256 "$file"
        fi
    done | sort -k2 > SHA256SUMS
    sha_verify SHA256SUMS
)

mkdir -p "$out"
archive="$out/$archive"
case "$ext" in
    tar.gz)
        tar -C "$stage" -czf "$archive" "$name"
        ;;
    zip)
        # No new dependency: `python -m zipfile` is stdlib, deterministic (one
        # top-level directory, forward slashes), and present on the Windows
        # runner. `zip` is the fallback on a machine without a Python, because a
        # missing archiver must not be the reason a release stops.
        archive_abs=$(cd "$(dirname "$archive")" && pwd)/$(basename "$archive")
        zipped=0
        for candidate in python3 python py; do
            if command -v "$candidate" >/dev/null 2>&1; then
                if [ "$candidate" = py ]; then
                    py_args=(-3)
                else
                    py_args=()
                fi
                ( cd "$stage" && "$candidate" "${py_args[@]}" -m zipfile -c "$archive_abs" "$name" )
                zipped=1
                break
            fi
        done
        if [ "$zipped" -eq 0 ]; then
            if command -v zip >/dev/null 2>&1; then
                ( cd "$stage" && zip -qr "$archive_abs" "$name" )
            else
                echo "error: no zip writer on PATH: need python3/python (stdlib zipfile) or zip" >&2
                exit 2
            fi
        fi
        ;;
esac

# What a release page and a verifier read: the archive's own digest, beside it.
# The workflow renames this file per target, because the release job downloads
# every family with `merge-multiple`.
( cd "$out" && sha256 "$(basename "$archive")" > SHA256SUMS.tmp \
    && mv SHA256SUMS.tmp SHA256SUMS \
    && sha_verify SHA256SUMS )

echo "build-lsp: $(ls -l "$archive" | awk '{ print $5 }') bytes -> $archive"
echo "build-lsp: entries -> $name/{$executable,LICENSE,SHA256SUMS}"
echo "build-lsp: checksums written and verified with $sha_tool -> $out/SHA256SUMS"
