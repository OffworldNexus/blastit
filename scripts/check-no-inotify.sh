#!/usr/bin/env bash
# Fails when anything in this tree could start a second filesystem watcher
# (`MB.23.3`, `ADR-0014`).
#
# `blastit dev` reloads what changed through exactly one mechanism: the
# unprivileged `fanotify` file-handle group in
# `crates/blastit-sys/src/fanotify.rs`. `inotify` is banned outright — not as a
# fallback, not behind a flag, not in a dependency — and this script is the
# mechanical proof, in three independent checks:
#
#   1. **Symbols.** Every ELF named on the command line is read with
#      `readelf --dyn-syms` (and `nm -D` when it exists) and must carry no
#      dynamic symbol matching `inotify_(init1?|add_watch|rm_watch)`. A binary
#      that links one has a second watcher in it, whatever the source says.
#   2. **Crates.** `cargo tree -e normal --locked -p blastit` must name no
#      `notify`/`inotify` crate: the ban covers a dependency that would bring
#      the calls in without a line of our source.
#   3. **Sources.** No source file — tracked **or** untracked — may contain
#      `inotify_init`, `inotify_init1`, `inotify_add_watch` or
#      `inotify_rm_watch` outside the documented allowlist below;
#      `fanotify_init` may appear only in the one watcher module (plus the
#      protected suite that proves the mask encoder); and `Watcher::new(` may
#      appear only there too, because the watcher's constructor is `pub` and
#      re-exported, so a second group needs no `fanotify_init` of its own.
#      That is what makes "a second watcher cannot be added quietly" true: a
#      new call site is a red gate, not a review note. Untracked files are
#      included on purpose: a call planted in a file that is not in the index
#      yet is exactly the case a guard must catch (the watcher this node added
#      was itself untracked when it was written).
#
# The allowlist is deliberately tiny and literal:
#
#   * `scripts/check-no-inotify.sh` — this file spells the banned names, so it
#     must contain them;
#   * `docs/research/**` and `plan/**` are Markdown and are not source files, so
#     the extension filter below already excludes them (the research note and
#     the plan quote the banned names on purpose);
#   * `fanotify_init` and `Watcher::new(` are allowed in
#     `crates/blastit-sys/src/fanotify.rs` (the watcher),
#     `crates/blastit/src/dev_reload.rs` (the policy that owns it) and
#     `crates/blastit-sys/tests/critical_fanotify.rs` (the protected suite that
#     creates a test group to prove the mask encoder carries a bit the
#     production mask never asks for; test crates are not linked into any
#     shipped artifact, and check 1 covers the artifacts).
#
# `--self-test` proves the guard can fail: it plants a call in a scratch copy of
# a source file, plants the same call in an **untracked** file of a scratch git
# repository, plants a `Watcher::new(`, a symbol in a fake `readelf` dump and a
# crate in a fake `cargo tree` dump, and requires each to be refused while the
# clean cases pass. A guard that cannot fail is not a guard.
#
# Usage:
#   scripts/check-no-inotify.sh BINARY [BINARY...]
#   scripts/check-no-inotify.sh --self-test
#   scripts/check-no-inotify.sh [--source-root DIR] BINARY
#
# Exit status: 0 clean, 1 a banned symbol/crate/source, 2 usage or a guard that
# could not run (a missing binary, an unreadable ELF, no `cargo tree`).
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)

# The source files this guard reads: code, not prose. A page may quote a banned
# name; a program that calls it may not.
source_extensions='\.(rs|py|pyi|sh|bash|zsh|fish|c|h|cc|cpp|cxx|hpp|hh|js|mjs|cjs|jsx|ts|mts|cts|tsx|php|phtml|go|rb|java|kt|kts|scala|clj|cljs|ex|exs|erl|hrl|lua|pl|pm|swift|cs|fs|vb|dart|nim|zig|v|sv|vhd|toml|yaml|yml|json|mk|cmake)$'
source_names='(^|/)(Makefile|GNUmakefile|Dockerfile)$'

# The one watcher module, the policy that owns it, and the protected suite that
# proves the mask encoder (see the header). The constructor is `pub` and
# re-exported, so banning only the syscall would leave a second group one
# `Watcher::new()` away.
fanotify_init_allowed='^(crates/blastit-sys/src/fanotify\.rs|crates/blastit/src/dev_reload\.rs|crates/blastit-sys/tests/critical_fanotify\.rs)$'
watcher_new_allowed=$fanotify_init_allowed

usage() {
    sed -n 's/^# \{0,1\}//p' <<'EOF'
# Usage:
#   scripts/check-no-inotify.sh BINARY [BINARY...]
#   scripts/check-no-inotify.sh --self-test
#   scripts/check-no-inotify.sh [--source-root DIR] BINARY
EOF
}

# The banned dynamic symbols in a symbol dump on stdin.
banned_symbols() {
    grep -E 'inotify_(init1?|add_watch|rm_watch)' || true
}

# The banned crates in a `cargo tree` dump on stdin.
banned_crates() {
    grep -E '(^|[^[:alnum:]_])(notify|inotify)([^[:alnum:]_]|$)' || true
}

# Check 1: one ELF carries no `inotify_*` dynamic symbol.
check_symbols() {
    local artifact=$1 status=0 dump hits
    if [ ! -f "$artifact" ]; then
        echo "error: $artifact does not exist: build it first (cargo build -p blastit --locked)" >&2
        return 2
    fi
    if ! dump=$(readelf --dyn-syms --wide "$artifact" 2>&1); then
        echo "error: $artifact is not an ELF readelf can read: $dump" >&2
        return 2
    fi
    hits=$(printf '%s\n' "$dump" | banned_symbols)
    if [ -n "$hits" ]; then
        echo "error: $artifact links an inotify symbol, so a second watcher is in the binary:" >&2
        printf '  %s\n' "$hits" >&2
        echo "  fix: the only watcher is crates/blastit-sys/src/fanotify.rs (ADR-0014); remove the inotify call." >&2
        status=1
    fi
    if command -v nm >/dev/null 2>&1; then
        if dump=$(nm -D --defined-only "$artifact" 2>/dev/null); then
            hits=$(printf '%s\n' "$dump" | banned_symbols)
            if [ -n "$hits" ]; then
                echo "error: $artifact exports an inotify symbol (nm -D):" >&2
                printf '  %s\n' "$hits" >&2
                echo "  fix: the only watcher is crates/blastit-sys/src/fanotify.rs (ADR-0014)." >&2
                status=1
            fi
        fi
    fi
    return "$status"
}

# Check 2: the default build's dependency graph names no watch crate.
check_crates() {
    local dump hits
    if ! dump=$(cd "$repo_root" && cargo tree -e normal --locked -p blastit 2>&1); then
        echo "error: cargo tree -e normal --locked -p blastit failed, so the crate guard cannot run:" >&2
        printf '%s\n' "$dump" >&2
        return 2
    fi
    hits=$(printf '%s\n' "$dump" | banned_crates)
    if [ -n "$hits" ]; then
        echo "error: the default build depends on a watch crate, so a second watcher is linked in:" >&2
        printf '  %s\n' "$hits" >&2
        echo "  fix: the only watcher is the fanotify group in crates/blastit-sys/src/fanotify.rs (ADR-0014)." >&2
        return 1
    fi
    return 0
}

# The source files under a root, relative to it, NUL-delimited.
#
# Tracked **and** untracked: a call planted in a file that is not in the index
# yet is exactly the case this guard must catch, and listing only `ls-files`
# would leave the allowlist branches unexercised on a tree where the watcher
# itself is still untracked.
list_sources() {
    local root=$1 top
    top=$(git -C "$root" rev-parse --show-toplevel 2>/dev/null || true)
    if [ -n "$top" ] && [ "$top" = "$root" ]; then
        {
            git -C "$root" ls-files -z
            git -C "$root" ls-files -z --others --exclude-standard
        } | grep -zE "$source_extensions|$source_names" || true
    else
        # `-printf '%P\0'` is the NUL-safe `-print` with the `./` prefix
        # already stripped, so the allowlist regexes match the same
        # repo-relative paths in both branches.
        (cd "$root" && find . \
            -path ./.git -prune -o \
            -path ./target -prune -o \
            -path ./node_modules -prune -o \
            -path ./.venv -prune -o \
            -type f -printf '%P\0') | grep -zE "$source_extensions|$source_names" || true
    fi
}

# How many source files a root holds, counted from the NUL-delimited list.
count_sources() {
    list_sources "$1" | tr -cd '\0' | wc -c | tr -d ' '
}

# Check 3: no source file calls a banned function outside the allowlist.
check_sources() {
    local root=$1 status=0 rel path found count
    if [ ! -d "$root" ]; then
        echo "error: the source root $root does not exist" >&2
        return 2
    fi
    count=$(count_sources "$root")
    if [ "$count" -eq 0 ]; then
        echo "error: no source files under $root: the source guard would pass vacuously" >&2
        return 2
    fi
    while IFS= read -r -d '' rel; do
        [ -n "$rel" ] || continue
        case "$rel" in
            scripts/check-no-inotify.sh) continue ;;
        esac
        path="$root/$rel"
        found=$(grep -nE 'inotify_(init1?|add_watch|rm_watch)' "$path" 2>/dev/null || true)
        if [ -n "$found" ]; then
            echo "error: $rel calls a banned inotify function:" >&2
            printf '%s\n' "$found" | sed "s|^|  $rel:|" >&2
            echo "  fix: the only watcher is crates/blastit-sys/src/fanotify.rs (ADR-0014)." >&2
            status=1
        fi
        found=$(grep -nE 'fanotify_init' "$path" 2>/dev/null || true)
        if [ -n "$found" ] && ! printf '%s' "$rel" | grep -qE "$fanotify_init_allowed"; then
            echo "error: $rel creates a fanotify group outside the one watcher:" >&2
            printf '%s\n' "$found" | sed "s|^|  $rel:|" >&2
            echo "  fix: there is exactly one group, in crates/blastit-sys/src/fanotify.rs (ADR-0014)." >&2
            status=1
        fi
        # The constructor is `pub` and re-exported (`lib.rs`), so a second group
        # would not need a `fanotify_init` of its own.
        found=$(grep -nE 'Watcher::new\(' "$path" 2>/dev/null || true)
        if [ -n "$found" ] && ! printf '%s' "$rel" | grep -qE "$watcher_new_allowed"; then
            echo "error: $rel opens a second watcher outside the one policy:" >&2
            printf '%s\n' "$found" | sed "s|^|  $rel:|" >&2
            echo "  fix: one watcher, opened in crates/blastit/src/dev_reload.rs (ADR-0014)." >&2
            status=1
        fi
    done < <(list_sources "$root")
    return "$status"
}

# Run every check against the real tree and the named binaries.
run_checks() {
    local status=0 artifact
    for artifact in "$@"; do
        check_symbols "$artifact" || status=$?
    done
    check_crates || status=$?
    check_sources "$source_root" || status=$?
    if [ "$status" -eq 0 ]; then
        local count
        count=$(count_sources "$source_root")
        echo "ok: no inotify: $# binary/binaries, no notify/inotify crate, $count source files (one fanotify group, one Watcher::new)"
    fi
    return "$status"
}

# A self-test that proves the guard can fail.
run_self_test() {
    local tmp tree status=0 out
    tmp=$(mktemp -d "${TMPDIR:-/tmp}/blastit-no-inotify.XXXXXX")
    # shellcheck disable=SC2064 # expand $tmp now, on purpose
    trap "rm -rf '$tmp'" EXIT
    tree="$tmp/tree"
    mkdir -p "$tree/crates/blastit-sys/src"
    printf 'pub fn nothing() {}\n' >"$tree/crates/blastit-sys/src/lib.rs"
    # A scratch repository, so the self-test drives the same `git ls-files`
    # branch the real run uses — and can prove the untracked case below.
    tree=$(cd "$tree" && pwd -P)
    git -C "$tree" init -q
    git -C "$tree" add crates/blastit-sys/src/lib.rs

    # The clean tree passes.
    if check_sources "$tree" >/dev/null 2>&1; then
        echo "ok: self-test: a clean source tree passes"
    else
        echo "error: self-test: a clean source tree was refused" >&2
        status=1
    fi

    # A call planted in an **untracked** file is caught too: the guard must not
    # depend on someone having staged the file (the watcher this node added was
    # untracked when it was written, and so is every file before its first add).
    printf 'pub fn planted() { let _ = inotify_add_watch(0, "x", 1); }\n' \
        >"$tree/crates/blastit-sys/src/untracked.rs"
    if out=$(check_sources "$tree" 2>&1); then
        echo "error: self-test: a planted call in an untracked file was accepted" >&2
        status=1
    elif printf '%s' "$out" | grep -q 'crates/blastit-sys/src/untracked.rs'; then
        echo "ok: self-test: a planted call in an untracked file is refused and named"
    else
        echo "error: self-test: the refusal did not name the untracked file: $out" >&2
        status=1
    fi
    rm -f "$tree/crates/blastit-sys/src/untracked.rs"

    # A planted inotify call is refused and the file is named.
    printf 'pub fn nothing() { let _ = inotify_add_watch(0, "x", 1); }\n' \
        >"$tree/crates/blastit-sys/src/lib.rs"
    if out=$(check_sources "$tree" 2>&1); then
        echo "error: self-test: a planted inotify_add_watch was accepted" >&2
        status=1
    elif printf '%s' "$out" | grep -q 'crates/blastit-sys/src/lib.rs'; then
        echo "ok: self-test: a planted inotify_add_watch is refused and named"
    else
        echo "error: self-test: the refusal did not name the planted file: $out" >&2
        status=1
    fi
    printf 'pub fn nothing() {}\n' >"$tree/crates/blastit-sys/src/lib.rs"

    # A second fanotify group outside the one watcher is refused.
    printf 'pub fn second() { let _ = fanotify_init(0, 0); }\n' \
        >"$tree/crates/blastit-sys/src/second.rs"
    if out=$(check_sources "$tree" 2>&1); then
        echo "error: self-test: a second fanotify_init was accepted" >&2
        status=1
    elif printf '%s' "$out" | grep -q 'crates/blastit-sys/src/second.rs'; then
        echo "ok: self-test: a second fanotify_init is refused and named"
    else
        echo "error: self-test: the refusal did not name the second group: $out" >&2
        status=1
    fi
    rm -f "$tree/crates/blastit-sys/src/second.rs"

    # ...while the one watcher module may create the group.
    printf 'pub fn one() { let _ = fanotify_init(0, 0); }\n' \
        >"$tree/crates/blastit-sys/src/fanotify.rs"
    if check_sources "$tree" >/dev/null 2>&1; then
        echo "ok: self-test: fanotify_init in the one watcher module is allowed"
    else
        echo "error: self-test: fanotify_init in the watcher module was refused" >&2
        status=1
    fi

    # The constructor is `pub` and re-exported, so a second watcher needs no
    # `fanotify_init` of its own: opening one outside the policy is refused...
    printf 'pub fn second() { let _ = Watcher::new(); }\n' \
        >"$tree/crates/blastit-sys/src/second.rs"
    if out=$(check_sources "$tree" 2>&1); then
        echo "error: self-test: a second Watcher::new was accepted" >&2
        status=1
    elif printf '%s' "$out" | grep -q 'crates/blastit-sys/src/second.rs'; then
        echo "ok: self-test: a second Watcher::new is refused and named"
    else
        echo "error: self-test: the refusal did not name the second watcher: $out" >&2
        status=1
    fi
    rm -f "$tree/crates/blastit-sys/src/second.rs"

    # ...while the policy that owns the watcher may open it.
    mkdir -p "$tree/crates/blastit/src"
    printf 'pub fn one() { let _ = Watcher::new(); }\n' \
        >"$tree/crates/blastit/src/dev_reload.rs"
    if check_sources "$tree" >/dev/null 2>&1; then
        echo "ok: self-test: Watcher::new in the one policy module is allowed"
    else
        echo "error: self-test: Watcher::new in the policy module was refused" >&2
        status=1
    fi

    # The symbol matcher refuses a planted symbol and accepts a clean dump.
    out=$(printf '  12: 0000000000000000 0 FUNC GLOBAL DEFAULT UND inotify_add_watch@GLIBC_2.4\n' \
        | banned_symbols)
    if [ -n "$out" ]; then
        echo "ok: self-test: a planted inotify dynamic symbol is refused"
    else
        echo "error: self-test: the symbol matcher accepted inotify_add_watch" >&2
        status=1
    fi
    out=$(printf '  12: 0000000000000000 0 FUNC GLOBAL DEFAULT UND fanotify_init@GLIBC_2.4\n' \
        | banned_symbols)
    if [ -z "$out" ]; then
        echo "ok: self-test: a fanotify symbol is not an inotify symbol"
    else
        echo "error: self-test: the symbol matcher refused fanotify: $out" >&2
        status=1
    fi

    # The crate matcher refuses a planted crate and accepts a clean tree.
    out=$(printf 'blastit v0.2.0\n├── notify v6.1.1\n└── libc v0.2.191\n' | banned_crates)
    if [ -n "$out" ]; then
        echo "ok: self-test: a planted notify crate is refused"
    else
        echo "error: self-test: the crate matcher accepted notify" >&2
        status=1
    fi
    out=$(printf 'blastit v0.2.0\n├── libc v0.2.191\n└── rustix v1.1.5\n' | banned_crates)
    if [ -z "$out" ]; then
        echo "ok: self-test: a clean dependency tree passes"
    else
        echo "error: self-test: the crate matcher refused a clean tree: $out" >&2
        status=1
    fi

    if [ "$status" -eq 0 ]; then
        echo "ok: self-test: the guard fails on a planted call (tracked or untracked), a second watcher, a planted symbol and a planted crate"
    fi
    return "$status"
}

self_test=0
source_root=""
binaries=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --self-test)
            self_test=1
            shift
            ;;
        --source-root)
            if [ "$#" -lt 2 ]; then
                echo "error: --source-root needs a directory" >&2
                exit 2
            fi
            source_root=$2
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        --)
            shift
            while [ "$#" -gt 0 ]; do
                binaries+=("$1")
                shift
            done
            ;;
        -*)
            echo "error: unknown option $1" >&2
            usage >&2
            exit 2
            ;;
        *)
            binaries+=("$1")
            shift
            ;;
    esac
done

if [ "$self_test" -eq 1 ]; then
    run_self_test
    exit $?
fi

if [ "${#binaries[@]}" -eq 0 ]; then
    echo "error: name at least one binary, or pass --self-test" >&2
    usage >&2
    exit 2
fi

if [ -z "$source_root" ]; then
    source_root=$repo_root
fi

run_checks "${binaries[@]}"
exit $?
