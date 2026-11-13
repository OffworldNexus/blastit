#!/usr/bin/env bash
# Keep the `cargo-mutants` scratch directory on disk, bounded, and pruned.
#
# `cargo-mutants` copies the whole workspace into
# "$TMPDIR/cargo-mutants-<crate>-XXXX.tmp" and builds every mutant inside that copy,
# so TMPDIR holds multi-GiB build trees per job — not a handful of temp files. On a
# host whose /tmp is tmpfs (32 GiB of RAM here) the old default filled RAM instead of
# disk, so `Makefile` defaults MUTANTS_TMP under the user cache and this script owns
# every operation that touches it (`docs/knowledge/LESSONS.md`):
#
#   scripts/mutants-scratch.sh prune <dir>      drop a stale mutants.out + old scratch copies
#   scripts/mutants-scratch.sh preflight <dir>  refuse to start on a nearly full filesystem
#   scripts/mutants-scratch.sh clean <dir>      delete every copy and mutants.out, report bytes
#
# Environment (every knob is overridable from the make command line):
#   MUTANTS_MIN_FREE_GB   refuse below this much free space on the scratch fs (default 5)
#   MUTANTS_KEEP_SCRATCH  keep this many newest cargo-mutants-*.tmp copies (default 2)
#   MUTANTS_STALE_MIN     never prune a copy touched this recently, in minutes (default 240)
set -euo pipefail

GIB=1073741824

die() {
	printf 'mutants: %s\n' "$*" >&2
	exit 1
}

human() {
	awk -v bytes="${1:-0}" 'BEGIN {
		split("B KiB MiB GiB TiB", unit, " ");
		value = bytes + 0; idx = 1;
		while (value >= 1024 && idx < 5) { value /= 1024; idx++ }
		if (idx == 1) printf "%d %s", value, unit[idx];
		else printf "%.1f %s", value, unit[idx];
	}'
}

# Echo the bytes `path` held and delete it; prints 0 for a path that does not exist.
# Callers sum the result (`freed=$((freed + $(remove "$path")))`).
remove() {
	local path=$1 bytes=0
	if [ -e "$path" ]; then
		bytes=$(du -sb -- "$path" 2>/dev/null | cut -f1) || bytes=0
		rm -rf -- "$path"
		bytes=${bytes:-0}
		printf 'mutants: removed %s (%s)\n' "$path" "$(human "$bytes")" >&2
	fi
	printf '%s' "${bytes:-0}"
}

free_bytes() {
	df -Pk -- "$1" | awk 'NR==2 {print $4 * 1024}'
}

describe() {
	local dir=$1 fs_type
	fs_type=$(stat -f -c %T -- "$dir")
	printf 'mutants: scratch=%s fs=%s\n' "$dir" "$fs_type"
	df -Ph -- "$dir" | awk 'NR==2 {
		printf "mutants: filesystem=%s size=%s used=%s avail=%s capacity=%s\n", $1, $2, $3, $4, $5
	}'
	case "$fs_type" in
	tmpfs | ramfs)
		printf 'mutants: warning: scratch %s is on %s (RAM, not disk): every scratch copy of the workspace costs memory. Move it with MUTANTS_TMP=/var/tmp/blastit-mutants.\n' \
			"$dir" "$fs_type" >&2
		;;
	esac
}

prune() {
	local dir=$1 keep=${MUTANTS_KEEP_SCRATCH:-2} stale_min=${MUTANTS_STALE_MIN:-240}
	local freed=0 index=0 _mtime path
	case "$keep" in '' | *[!0-9]*) die "MUTANTS_KEEP_SCRATCH must be a whole number, got '$keep'" ;; esac
	case "$stale_min" in '' | *[!0-9]*) die "MUTANTS_STALE_MIN must be a whole number of minutes, got '$stale_min'" ;; esac
	mkdir -p -- "$dir"

	# A previous campaign's report lives in the repository root (cargo-mutants writes
	# mutants.out there) and is regenerated per run, so it is always stale.
	freed=$((freed + $(remove mutants.out)))
	freed=$((freed + $(remove mutants.out.old)))

	# Scratch copies, newest first. `cargo-mutants` itself deletes stale copies under
	# its TMPDIR parent when a run starts, so never prune a copy that is being written:
	# anything touched within MUTANTS_STALE_MIN minutes survives regardless of rank.
	while IFS=$'\t' read -r _mtime path; do
		[ -n "$path" ] || continue
		index=$((index + 1))
		if [ "$index" -le "$keep" ]; then
			continue
		fi
		if [ -n "$(find "$path" -maxdepth 0 -mmin "-$stale_min" -print -quit 2>/dev/null)" ]; then
			printf 'mutants: keeping %s (touched within MUTANTS_STALE_MIN=%s, a campaign may own it)\n' "$path" "$stale_min" >&2
			continue
		fi
		freed=$((freed + $(remove "$path")))
	done < <(find "$dir" -mindepth 1 -maxdepth 1 -type d -name 'cargo-mutants-*.tmp' -printf '%T@\t%p\n' | sort -nr)

	printf 'mutants: prune: freed %s; %s now has %s free\n' \
		"$(human "$freed")" "$dir" "$(human "$(free_bytes "$dir")")"
}

preflight() {
	local dir=$1 min_gb=${MUTANTS_MIN_FREE_GB:-5} avail
	case "$min_gb" in '' | *[!0-9.]* | *.*.*) die "MUTANTS_MIN_FREE_GB must be a number, got '$min_gb'" ;; esac
	mkdir -p -- "$dir"
	describe "$dir"
	avail=$(free_bytes "$dir")
	if awk -v b="$avail" -v min="$min_gb" 'BEGIN {exit !(b < min * 1073741824)}'; then
		{
			printf 'mutants: refusing to start: %s has %s free (%s GiB), below MUTANTS_MIN_FREE_GB=%s.\n' \
				"$dir" "$(human "$avail")" "$(awk -v b="$avail" 'BEGIN {printf "%.1f", b / 1073741824}')" "$min_gb"
			printf 'mutants: cargo-mutants copies the workspace for every job, so an interrupted campaign leaves multi-GiB copies behind.\n'
			printf 'mutants: fix: `make mutants-clean` frees mutants.out and every scratch copy, or point the scratch at a bigger filesystem:\n'
			printf 'mutants:      make mutants-rust PKG=<crate> MUTANTS_TMP=/var/tmp/blastit-mutants\n'
		} >&2
		exit 2
	fi
	printf 'mutants: preflight ok: %s free on %s (MUTANTS_MIN_FREE_GB=%s)\n' \
		"$(human "$avail")" "$dir" "$min_gb"
}

clean() {
	local dir=$1 freed=0 path
	mkdir -p -- "$dir"
	freed=$((freed + $(remove mutants.out)))
	freed=$((freed + $(remove mutants.out.old)))
	while IFS= read -r -d '' path; do
		freed=$((freed + $(remove "$path")))
	done < <(find "$dir" -mindepth 1 -maxdepth 1 -name 'cargo-mutants-*' -print0)
	printf 'mutants: clean: freed %s; %s now has %s free\n' \
		"$(human "$freed")" "$dir" "$(human "$(free_bytes "$dir")")"
}

command=${1:-}
dir=${2:-}
case "$command" in
prune | preflight | clean)
	[ -n "$dir" ] || die "usage: scripts/mutants-scratch.sh {prune|preflight|clean} <dir>"
	"$command" "$dir"
	;;
*)
	die "usage: scripts/mutants-scratch.sh {prune|preflight|clean} <dir>"
	;;
esac
