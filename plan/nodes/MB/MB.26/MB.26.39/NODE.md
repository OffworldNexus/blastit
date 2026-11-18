---
id: MB.26.39
title: "Agent scratch on disk: one scratch root, retention cleanup, LESSONS entry"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: [Makefile, "scripts/**", docs/knowledge/LESSONS.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T20:36"
updated: "2026-11-18T20:36"
---

# Goal

Close the last tmpfs leak: agents' own scratch (review copies, per-leaf `target/` dirs, mutation probes) still lands in `/tmp`, which is RAM on this host. Make disk-backed scratch the default for every agent, and give it one cleanup command.

# Context

`MB.1.11` fixed the tools: `MUTANTS_TMP` is `$(XDG_CACHE_HOME)/blastit-mutants`, campaigns run in a 12 G cgroup with `MemorySwapMax=0`, a low-space preflight refuses to start, and `make mutants-clean` frees it. Worktrees moved to `.worktrees/`. `docs/knowledge/LESSONS.md` already records why (tmpfs = RAM; a 12 G worktree; a full `/tmp` showing up as a linker bus error).

What is still true: leaf agents create ad-hoc scratch — `/tmp/opencode/rv-*`, `/tmp/opencode/<node>-probe`, `/tmp/review-mut-target`, `/tmp/probe-*` — when copying a checkout to run a review or a mutation campaign without touching the working tree. Measured after a week of runs: `/tmp` at 8.6 G of a 32 G tmpfs, of which ~6 G was agent scratch; several of those dirs outlived the leaf that made them.

# Acceptance

1. A single documented scratch root for agents, defaulting to disk — e.g. `$(XDG_CACHE_HOME:-$HOME/.cache)/blastit-scratch` — exposed as `make -s print-scratch-dir` alongside the existing `print-mutants-tmp`, and used by any script that copies the tree.
2. `make scratch-clean` (or an extension of `mutants-clean`) prunes that root: removes entries older than a retention window (`SCRATCH_STALE_MIN`, mirroring `MUTANTS_STALE_MIN`), reports bytes freed, refuses to touch anything currently held open (a simple `lsof`/`/proc/*/cwd` check is enough), and never removes the repo's own `target/`.
3. `docs/knowledge/LESSONS.md` gains one short entry: agent scratch goes under that root, `/tmp` is tmpfs here, and a `review-mut-target`-style copy left behind costs RAM until the next cleanup. Keep it to the pattern, not a lecture.
4. Directory-creating helpers in the repo that currently default to `/tmp` are repointed (grep `"/tmp` and `mktemp -d` in `scripts/`, `e2e/`, `tools/`); each remaining `/tmp` use gets either a repoint or a one-line reason it is genuinely ephemeral and small.
5. `make check` exit 0, `plan.py validate` ok, `guard.py` ok.

# Notes

- Do not delete other agents' live scratch: the cleanup is retention-based and reports what it frees, so a running campaign is never broken (the same lesson as `MB.1.11`'s "never clean while a campaign runs").
- CI is unaffected (`/tmp` there is not a RAM-backed tmpfs of this size), so keep the default overridable and do not add host-specific branches beyond the `XDG_CACHE_HOME` default.

# Provenance

Re-filed from `MB.10` at the `v0.2.0` gate: disk-backed agent scratch helper and retention cleanup (`MB.1.11` already moved `MUTANTS_TMP` and `.worktrees/` off `/tmp`).
