#!/usr/bin/env bash
# Run `cargo mutants` for `make mutants-rust`: disk scratch (TMPDIR) + a hard cgroup
# memory cap when this host can enforce one.
#
# Why a cap: cargo-mutants builds every mutant in a copy of the workspace, two jobs at
# a time. An unbounded campaign on this 48 GiB box sits next to a 32 GiB tmpfs /tmp, a
# 48 G `target/` and a concurrent `make check`, so an overrun used to thrash the machine
# instead of failing. `systemd-run --user --scope` (cgroup v2) makes MemoryMax a hard
# limit and `MemorySwapMax=0` forbids swapping the campaign out.
#
# Fallbacks (documented, never fail the target): a container/CI without a usable user
# manager, a host without systemd-run, or BLASTIT_MUTANTS_NO_SCOPE=1 runs the plain
# command and says so in the log.
#
# Usage:  scripts/mutants-run.sh [--probe] [cargo-mutants arguments...]
#   --probe   print the cgroup and memory.max a campaign would run under, then exit
set -euo pipefail

tmp=${TMPDIR:-}
memory_max=${MUTANTS_MEMORY_MAX:-12G}
jobs=${MUTANTS_JOBS:-2}
timeout=${MUTANTS_TIMEOUT:-90}
cargo=${CARGO:-cargo}
probe=0
args=()

while [ $# -gt 0 ]; do
	case "$1" in
	--probe)
		probe=1
		shift
		;;
	--memory-max)
		[ $# -ge 2 ] || {
			printf 'mutants: --memory-max needs a value\n' >&2
			exit 2
		}
		memory_max=$2
		shift 2
		;;
	*)
		args+=("$1")
		shift
		;;
	esac
done

[ -n "$tmp" ] || {
	printf 'mutants: TMPDIR is empty.\n' >&2
	printf 'mutants: fix: run campaigns through `make mutants-rust` (it sets TMPDIR to `make -s print-mutants-tmp`), never a bare `cargo mutants`.\n' >&2
	exit 2
}

# Echo the reason `systemd-run --user --scope` cannot be used, or nothing when it works.
scope_unavailable_reason() {
	if [ "${BLASTIT_MUTANTS_NO_SCOPE:-0}" = 1 ]; then
		printf 'BLASTIT_MUTANTS_NO_SCOPE=1'
		return
	fi
	if ! command -v systemd-run >/dev/null 2>&1; then
		printf 'systemd-run is not installed'
		return
	fi
	if ! systemd-run --user --scope --quiet --collect \
		-p MemoryMax="$memory_max" -p MemorySwapMax=0 -- true >/dev/null 2>&1; then
		printf 'the user manager refused a transient scope (no user systemd or no bus)'
	fi
}

# The probe prints the facts the memory cap is judged by, from *inside* the scope.
probe_scope() {
	systemd-run --user --scope --quiet --collect \
		-p MemoryMax="$memory_max" -p MemorySwapMax=0 -- sh -c '
		path=$(sed -n "s/^0:://p" /proc/self/cgroup)
		printf "cgroup: %s\n" "$(cat /proc/self/cgroup)"
		printf "memory.max: %s\n" "$(cat "/sys/fs/cgroup$path/memory.max")"
		printf "memory.swap.max: %s\n" "$(cat "/sys/fs/cgroup$path/memory.swap.max")"
	'
}

if [ "$probe" = 1 ]; then
	if [ -n "$tmp" ]; then
		printf 'mutants: scratch TMPDIR=%s\n' "$tmp"
	fi
	if reason=$(scope_unavailable_reason); [ -n "$reason" ]; then
		printf 'mutants: scope=disabled reason=%s\n' "$reason"
		printf 'mutants: a campaign here would run with no cgroup cap (MemoryMax=%s unenforced)\n' "$memory_max"
		exit 0
	fi
	printf 'mutants: scope=systemd-run --user --scope -p MemoryMax=%s -p MemorySwapMax=0\n' "$memory_max"
	probe_scope
	exit 0
fi

printf 'mutants: scratch TMPDIR=%s (must be on disk; `df -h "$(make -s print-mutants-tmp)"`)\n' "$tmp"
printf 'mutants: command=%s mutants %s -j %s --timeout %s\n' "$cargo" "${args[*]}" "$jobs" "$timeout"

if reason=$(scope_unavailable_reason); [ -n "$reason" ]; then
	printf 'mutants: memory cap: none (fallback: %s) - running the plain command\n' "$reason"
	set +e
	TMPDIR="$tmp" "$cargo" mutants "${args[@]}" -j "$jobs" --timeout "$timeout"
	status=$?
	set -e
	exit "$status"
fi

printf 'mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=%s, MemorySwapMax=0)\n' "$memory_max"
set +e
TMPDIR="$tmp" systemd-run --user --scope --quiet --collect \
	-p MemoryMax="$memory_max" -p MemorySwapMax=0 \
	--setenv=TMPDIR="$tmp" -- \
	"$cargo" mutants "${args[@]}" -j "$jobs" --timeout "$timeout"
status=$?
set -e

if [ "$status" -eq 137 ] || [ "$status" -eq 124 ]; then
	printf 'mutants: the campaign died inside the cgroup cap (status %s).\n' "$status" >&2
	printf 'mutants: fix: raise MUTANTS_MEMORY_MAX (now %s) or lower MUTANTS_JOBS (now %s); the cap exists so an overrun fails here instead of thrashing the host.\n' \
		"$memory_max" "$jobs" >&2
fi
exit "$status"
