---
id: MB.1.11
title: Bound mutants memory and keep build scratch off tmpfs
kind: task
status: active
owner: implementer
priority: 1
depends_on: []
paths: [Makefile, "scripts/**", docs/knowledge/LESSONS.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T18:38"
updated: "2026-11-13T18:39"
---

# Goal

Stop `cargo-mutants` from filling `/tmp` (which is **tmpfs** on this host, so its scratch dirs consume RAM) and from running without a memory bound: move the mutants scratch onto disk, run the campaign inside a cgroup memory cap, prune stale scratch automatically, refuse to start when the scratch filesystem is nearly full, and document all of it.

# Context

- `Makefile:27` sets `MUTANTS_TMP ?= /tmp/opencode/blastit-mutants` and `Makefile:137-140` runs `TMPDIR="$(MUTANTS_TMP)" cargo mutants …`. `/tmp` is tmpfs here: 32 G total, ~22 G was held by build/worktree scratch at the time of writing, i.e. the mutant temp dirs were consuming **RAM** and the box swapped/slowed instead of failing loudly.
- `systemd-run --user --scope -p MemoryMax=… true` works on this host (cgroup v2), so a hard memory cap is enforceable; a container/CI without a usable user manager must still work.
- Four campaigns on `MB.1.3`/`MB.1.4`/`MB.1.5` were lost to restarts and to two concurrent drivers sharing scratch; a `systemd-run --user` unit survived where `nohup`/`setsid` did not (see `MB.1.4`/`MB.1.5` RESULT.md).
- Repo `target/` (48 G) and `~/.cargo` (507 M) are on disk and fine; only the default scratch location is wrong.

# Acceptance

```console
# 1. The default is disk, not tmpfs, and the wrapper is complete.
$ grep -n 'MUTANTS_TMP ?=' Makefile          # -> /var/tmp/... or $(XDG_CACHE_HOME:-$HOME/.cache)/...
$ make -n mutants-rust PKG=blastit-env | tail -5   # shows mkdir/prune/preflight/scope/TMPDIR

# 2. End-to-end proof on the smallest crate: the log names the systemd scope and a
#    scratch path that is NOT under /tmp, and the run finishes.
$ df -h "$(make -s print-mutants-tmp)" ; make mutants-rust PKG=blastit-env ; df -h "$(make -s print-mutants-tmp)"
#   paste the tail; describe how the free space moved (or did not).

# 3. The memory cap is real, not just a flag: inside the scope, /proc/self/cgroup names
#    the unit and memory.max equals MUTANTS_MEMORY_MAX.
$ make mutants-scope-probe            # or the documented one-liner; paste /proc/self/cgroup + memory.max

# 4. The low-space preflight fires with a clear message (raise the threshold in a scratch copy).
$ MUTANTS_MIN_FREE_GB=99999 make mutants-rust PKG=blastit-env   # exits non-zero, explains itself, runs no campaign

# 5. Cleanup is one documented command, and gates are green.
$ make mutants-clean            # prunes mutants.out + old cargo-mutants-*.tmp, reports freed bytes
$ make check                    # exit 0
$ uv run --no-project tools/plan.py validate && uv run --no-project tools/plan.py guard 2>/dev/null || uv run --no-project tools/guard.py
$ make critical-guard
```

# Notes

- `MUTANTS_MEMORY_MAX ?=` (default a defensible cap for this 48 G host, justified in a comment), `MUTANTS_MIN_FREE_GB ?=`, `MUTANTS_KEEP_SCRATCH ?=` must all be overridable from the command line.
- Fallback: when `systemd-run` is missing, not usable, or `BLASTIT_MUTANTS_NO_SCOPE=1`, run the plain command and say so in the log — never fail the target in a container.
- Do not touch protected `critical_*` tests or product Rust; scope is `Makefile`, the scripts you own, and `docs/knowledge/LESSONS.md`.
- Record the measured facts (tmpfs size, what one campaign holds, the chosen cap and why) in `LESSONS.md`, next to the existing `cargo-mutants` bullets.

# Worktrees (same hazard class, owner directive)

All git worktrees live in `.worktrees/` **inside the repo** — never under `/tmp` (tmpfs = RAM here; a release worktree alone held 12 G of it) and never in `$HOME`. Convention to document next to the tmpfs note in `docs/knowledge/LESSONS.md`:

```console
$ git worktree add .worktrees/<slug> <branch>
$ git worktree remove .worktrees/<slug>
```

- `.gitignore` gains `.worktrees/` (keep ignoring `mutants.out` and the mutants scratch).
- The gates must stay green with an in-repo worktree present: `uv run --no-project tools/guard.py` and `make check` both exit 0 while `.worktrees/hygiene-check` exists. `guard.py` refuses >5 MiB files and stray binaries — verify it skips the ignored path instead of scanning a whole second checkout; **if it does scan it, that is a defect to fix in this node (respect `.gitignore`)**.
- Combine the two notes: mutants scratch and worktrees are the same class of defect — large build artifacts defaulting onto tmpfs.
