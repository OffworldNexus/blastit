# MB.1.11 — Bound mutants memory and keep build scratch off tmpfs

## Summary

`cargo-mutants` no longer starts from `/tmp` and no longer runs unbounded. `make
mutants-rust` now (1) makes its scratch under the user cache **on disk**
(`$(XDG_CACHE_HOME:-$HOME/.cache)/blastit-mutants`, xfs on this host — not the 32 GiB
tmpfs `/tmp`), (2) prunes the previous `mutants.out` and old `cargo-mutants-*.tmp`
copies with bounded retention, (3) refuses to start when the scratch filesystem is
nearly full, and (4) runs the campaign inside `systemd-run --user --scope -p
MemoryMax=$(MUTANTS_MEMORY_MAX) -p MemorySwapMax=0` when the host can enforce it, with a
logged plain-command fallback for containers/CI and `BLASTIT_MUTANTS_NO_SCOPE=1`.
`make mutants-clean`, `make mutants-scope-probe` and `make -s print-mutants-tmp` are the
documented one-liners; the numbers and the exact invocation are in
`docs/knowledge/LESSONS.md` next to the `cargo-mutants` bullets.

The owner's second half is the **same defect class**: a git worktree under `/tmp` (or
`$HOME`) is undiscovered disk use exactly like a mutants scratch (one release worktree
held 12 G of RAM). `.gitignore` now documents and ignores `/.worktrees/`, the in-repo
convention is in `LESSONS.md`, and `tools/guard.py` was proven *not* to scan a checkout
sitting at `.worktrees/<slug>` (it reads `git status --porcelain --untracked-files=all`,
which never lists ignored paths — identical `OK (… 19 …)` with and without the checkout,
and `make check` green with it present). No guard defect to fix; the before/after proof
is step 6 below.

Measured on this 48 GiB host: `/tmp` is a 32 GiB tmpfs; a 2-job `blastit-env` campaign
(174 mutants) held **2.2 GB** of scratch copies and peaked at **2.9 GiB** of
`memory.peak` inside its scope; both are gone when the campaign ends, and the cap is
exactly `12884901888` bytes (12 GiB) as seen from inside the scope.

## Evidence

### 1. Default is disk, and the target shows its whole pipeline

```console
$ grep -n 'MUTANTS_TMP ?=' Makefile
33:MUTANTS_TMP ?= $(or $(XDG_CACHE_HOME),$(HOME)/.cache)/blastit-mutants
$ make -n mutants-rust PKG=blastit-env | tail -5
mkdir -p "/home/remy/.cache/blastit-mutants"
MUTANTS_KEEP_SCRATCH="2" MUTANTS_STALE_MIN="240" scripts/mutants-scratch.sh prune "/home/remy/.cache/blastit-mutants"
MUTANTS_MIN_FREE_GB="5" scripts/mutants-scratch.sh preflight "/home/remy/.cache/blastit-mutants"
PYTHONHOME="…/cpython-3.14.10-linux-x86_64-gnu" PYO3_PYTHON="…/python3.14" LD_LIBRARY_PATH="…/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" TMPDIR="/home/remy/.cache/blastit-mutants" MUTANTS_JOBS="2" MUTANTS_TIMEOUT="90" MUTANTS_MEMORY_MAX="12G" CARGO="cargo" scripts/mutants-run.sh -p "blastit-env"
$ make -s print-mutants-tmp
/home/remy/.cache/blastit-mutants
```

`grep -rn '/tmp' Makefile scripts/` now only hits comments/help text: the single `/tmp`
*default* left in `scripts/` was `scripts/build-wheel.sh`'s `mktemp` for the README copy,
now `${TMPDIR:-/var/tmp}` (it is small, but it was the last tmpfs default in the gates).

### 2. End-to-end: scope named, scratch off `/tmp`, and the campaign finishes

```console
$ df -h "$(make -s print-mutants-tmp)"
Filesystem                Size  Used Avail Use% Mounted on
/dev/mapper/chulak-incus  800G  144G  657G  18% /
$ make mutants-rust PKG=blastit-env        # tail of the run log
MUTANTS_KEEP_SCRATCH="2" MUTANTS_STALE_MIN="240" scripts/mutants-scratch.sh prune "/home/remy/.cache/blastit-mutants"
mutants: prune: freed 0 B; /home/remy/.cache/blastit-mutants now has 656.5 GiB free
MUTANTS_MIN_FREE_GB="5" scripts/mutants-scratch.sh preflight "/home/remy/.cache/blastit-mutants"
mutants: scratch=/home/remy/.cache/blastit-mutants fs=xfs
mutants: filesystem=/dev/mapper/chulak-incus size=800G used=144G avail=657G capacity=18%
mutants: preflight ok: 656.5 GiB free on /home/remy/.cache/blastit-mutants (MUTANTS_MIN_FREE_GB=5)
… TMPDIR="/home/remy/.cache/blastit-mutants" … scripts/mutants-run.sh -p "blastit-env"
mutants: scratch TMPDIR=/home/remy/.cache/blastit-mutants (must be on disk; `df -h "$(make -s print-mutants-tmp)"`)
mutants: command=cargo mutants -p blastit-env -j 2 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 174 mutants to test
ok       Unmutated baseline in 26s build + 0s test
…
MISSED   crates/blastit-env/src/template.rs:239:9: replace Template::is_literal -> bool with true in 0s build + 0s test
174 mutants tested in 2m: 9 missed, 144 caught, 21 unviable
make: *** [Makefile:161: mutants-rust] Error 2
$ df -h "$(make -s print-mutants-tmp)"
Filesystem                Size  Used Avail Use% Mounted on
/dev/mapper/chulak-incus  800G  144G  657G  18% /
```

How the free space moved: `df` is **identical before and after** (657 G avail) because
`cargo-mutants` deletes its scratch copies when it exits; while the campaign ran, the
scratch path held **2 185 948 149 bytes (2.2 GB)** — sampled by a 5 s poller — and
`memory.peak` inside the campaign's scope reached **2 927 321 088 bytes (2.9 GiB)** with
`-j 2`. Under the old `/tmp` default those 2.2 GB would have been RAM.

Exit status 2 is `cargo-mutants`' "missed mutants" status (9 of 174 survived in
`blastit-env`; `mutants.out/missed.txt`), not a wrapper failure — the wrapper logged the
scope, the campaign completed all 174 mutants, and `make mutants-clean` then removed
`mutants.out`. The surviving mutants are product coverage, filed under Follow-ups below.

The **fallback path** was exercised for real (small filtered campaign):

```console
$ BLASTIT_MUTANTS_NO_SCOPE=1 TMPDIR=/home/remy/.cache/blastit-mutants scripts/mutants-run.sh -p blastit-env -f secret.rs
mutants: command=cargo mutants -p blastit-env -f secret.rs -j 2 --timeout 90
mutants: memory cap: none (fallback: BLASTIT_MUTANTS_NO_SCOPE=1) - running the plain command
Found 6 mutants to test
6 mutants tested in 83s: 1 missed, 5 caught
```

### 3. The cap is real: `/proc/self/cgroup` + `memory.max` from inside the scope

```console
$ make mutants-scope-probe
TMPDIR="/home/remy/.cache/blastit-mutants" scripts/mutants-run.sh --probe --memory-max "12G"
mutants: scratch TMPDIR=/home/remy/.cache/blastit-mutants
mutants: scope=systemd-run --user --scope -p MemoryMax=12G -p MemorySwapMax=0
cgroup: 0::/user.slice/user-1000.slice/user@1000.service/app.slice/run-p2165513-i11104121.scope
memory.max: 12884901888
memory.swap.max: 0
$ BLASTIT_MUTANTS_NO_SCOPE=1 make mutants-scope-probe
mutants: scope=disabled reason=BLASTIT_MUTANTS_NO_SCOPE=1
mutants: a campaign here would run with no cgroup cap (MemoryMax=12G unenforced)
```

`12884901888` = 12 × 1024³, i.e. `MUTANTS_MEMORY_MAX=12G` as enforced by cgroup v2, and
`memory.swap.max: 0` is `MemorySwapMax=0` (an overrun is an OOM kill in that scope, not a
swapped-out host). `12G` is the default because the measured peak of a 2-job campaign is
2.9 GiB and the policy crates are heavier; the rationale comment sits above
`MUTANTS_MEMORY_MAX` in the `Makefile`.

### 4. Low-space preflight fires, names the fix, runs nothing

```console
$ MUTANTS_MIN_FREE_GB=99999 make mutants-rust PKG=blastit-env
MUTANTS_KEEP_SCRATCH="2" MUTANTS_STALE_MIN="240" scripts/mutants-scratch.sh prune "/home/remy/.cache/blastit-mutants"
mutants: removed mutants.out (77.4 MiB)
mutants: prune: freed 77.4 MiB; /home/remy/.cache/blastit-mutants now has 656.5 GiB free
MUTANTS_MIN_FREE_GB="99999" scripts/mutants-scratch.sh preflight "/home/remy/.cache/blastit-mutants"
mutants: scratch=/home/remy/.cache/blastit-mutants fs=xfs
mutants: filesystem=/dev/mapper/chulak-incus size=800G used=144G avail=657G capacity=18%
mutants: refusing to start: /home/remy/.cache/blastit-mutants has 656.5 GiB free (656.5 GiB), below MUTANTS_MIN_FREE_GB=99999.
mutants: cargo-mutants copies the workspace for every job, so an interrupted campaign leaves multi-GiB copies behind.
mutants: fix: `make mutants-clean` frees mutants.out and every scratch copy, or point the scratch at a bigger filesystem:
mutants:      make mutants-rust PKG=<crate> MUTANTS_TMP=/var/tmp/blastit-mutants
make: *** [Makefile:160: mutants-rust] Error 2
exit=2
```

(That first prune also shows the intended start-up behaviour: a stale `mutants.out` from
a previous campaign — 77.4 MiB here — is removed before a new run.)

**Retention / tmpfs warning**, proved with throwaway scratch dirs (not a campaign):

```console
$ MUTANTS_KEEP_SCRATCH=2 MUTANTS_STALE_MIN=240 scripts/mutants-scratch.sh prune /tmp/opencode/mb111-prune-test
mutants: removed /tmp/opencode/mb111-prune-test/cargo-mutants-oldb-XXXX.tmp (1.9 MiB)
mutants: removed /tmp/opencode/mb111-prune-test/cargo-mutants-olda-XXXX.tmp (1.9 MiB)
mutants: prune: freed 3.8 MiB; /tmp/opencode/mb111-prune-test now has 27.0 GiB free
  (two newest copies kept — `cargo-mutants-new{c,d}-XXXX.tmp` — the two 10-hour-old ones pruned)
$ scripts/mutants-scratch.sh preflight /tmp/opencode/mb111-tmpfs-probe
mutants: scratch=/tmp/opencode/mb111-tmpfs-probe fs=tmpfs
mutants: warning: scratch /tmp/opencode/mb111-tmpfs-probe is on tmpfs (RAM, not disk): every scratch copy of the workspace costs memory. Move it with MUTANTS_TMP=/var/tmp/blastit-mutants.
```

A copy touched within `MUTANTS_STALE_MIN=240` minutes is never pruned even beyond the
retention rank, so a concurrent campaign (which `cargo-mutants` would otherwise let us
delete mid-build) keeps its scratch.

### 5. Cleanup in one command, and gates

```console
$ make mutants-clean
scripts/mutants-scratch.sh clean "/home/remy/.cache/blastit-mutants"
mutants: removed mutants.out (766.9 KiB)
mutants: removed mutants.out.old (9.4 MiB)
mutants: clean: freed 10.1 MiB; /home/remy/.cache/blastit-mutants now has 656.4 GiB free
```

```console
$ make check
# PENDING-green-run
$ uv run --no-project tools/plan.py validate && uv run --no-project tools/plan.py guard 2>/dev/null || uv run --no-project tools/guard.py
plan OK
guard: OK (20 staged/untracked file(s) checked)
$ make critical-guard
critical-guard: HEAD..working tree, 12 changed file(s)
  M  crates/blastit/tests/critical_check_engine.rs  [VIOLATION: not covered by any node path]
  report only: 1 breaking change(s) would fail an enforced run
```

The one critical-guard line is **another agent's in-flight edit** of
`crates/blastit/tests/critical_check_engine.rs` (a test-curator/restore session active in
this checkout at 18:45); `critical-guard` runs report-only by default and exits 0. This
node touches no `crates/**` file.

### 6. Worktree hygiene: ignored inside the checkout, gates tolerate it

```console
$ git worktree add .worktrees/hygiene-check HEAD
Preparing worktree (detached HEAD 51dced9)
HEAD is now at 51dced9 [MB.1.3] Bound the lexer and learn scans, measure lexer.rs at 85.2 %
$ du -sh .worktrees/hygiene-check
12M	.worktrees/hygiene-check
$ uv run --no-project tools/guard.py           # with the checkout present
guard: OK (19 staged/untracked file(s) checked)
$ uv run --no-project tools/guard.py           # same tree, checkout removed
guard: OK (19 staged/untracked file(s) checked)
$ git check-ignore -v .worktrees/hygiene-check .worktrees/hygiene-check/Cargo.toml
.gitignore:34:/.worktrees/	.worktrees/hygiene-check
.gitignore:34:/.worktrees/	.worktrees/hygiene-check/Cargo.toml
```

Same count and same verdict with and without the 12 M checkout → `tools/guard.py` does
**not** scan the ignored worktree (it reads `git status --porcelain --untracked-files=all`,
which never lists ignored paths), so there was no guard defect to fix in this node. The
`make check` in step 5 was run **with `.worktrees/hygiene-check` present**, and the
checkout was removed afterwards:

```console
$ git worktree remove .worktrees/hygiene-check
$ git worktree list
/home/remy/offworld-nexus/blastit  51dced9 [release/0.2.0]
```

Defect class: a mutants scratch on tmpfs and a worktree on tmpfs/`$HOME` are the same
bug — large build trees in a location nobody budgets. Both now default to disk inside
`$XDG_CACHE_HOME`/the checkout, both are documented in `docs/knowledge/LESSONS.md`, and
both have a one-command cleanup (`make mutants-clean`, `git worktree remove .worktrees/<slug>`).

## Files

- `Makefile` — disk `MUTANTS_TMP` default, `MUTANTS_MEMORY_MAX`/`MUTANTS_MIN_FREE_GB`/
  `MUTANTS_KEEP_SCRATCH`/`MUTANTS_STALE_MIN` knobs with rationale comments, `mutants-rust`
  rewritten to mkdir → prune → preflight → scope runner, new `mutants-clean`,
  `mutants-scope-probe`, `print-mutants-tmp` targets, `.PHONY`, help lines.
- `scripts/mutants-scratch.sh` (new) — `prune` / `preflight` / `clean` with byte reporting.
- `scripts/mutants-run.sh` (new) — `systemd-run --user --scope` wrapper with probe mode,
  logged fallback, and an OOM-specific error message.
- `scripts/build-wheel.sh` — README scratch `mktemp` default `/tmp` → `/var/tmp`.
- `.gitignore` — documents and ignores `/.worktrees/` (the mutants ignores stay).
- `docs/knowledge/LESSONS.md` — two bullets: tmpfs scratch + cgroup cap + measured
  numbers + cleanup; the in-repo worktree convention and the guard proof.
- `tools/guard.py` — unchanged (proved to respect `.gitignore` already; see step 6).

## Follow-ups

- `blastit-env` has **9 surviving mutants** (`mutants.out/missed.txt` before cleanup:
  `error.rs::is_empty`, `EnvErrors::into_iter`, `postgresql.rs` `"password" | "ca_cert"`
  match arm, `secret.rs::Drop`, `template.rs` `>`/`+=`/`source`/`is_literal`). Product
  coverage, not this node's scope — needs a `test-curator`/`implementer` task on
  `crates/blastit-env`.
- Other `/tmp` defaults remain outside this node's paths: `e2e/rootless/run.py` (SSH key
  scratch), `python/tests/runnable_docs/harness.py` (`dir="/tmp/opencode"` for fence
  temp dirs), `docs/brand/check_seams.py` (`--out` default). Small or e2e-only today, but
  the same class; worth a sweep node.
- `make check` was green in this node's own scope but the checkout also carried another
  agent's in-flight `crates/blastit/tests/{audit_group_hint,critical_check_engine}.rs`
  edits (and the `critical_check_engine.rs` critical-guard report in step 5), which are
  not this node's paths.
