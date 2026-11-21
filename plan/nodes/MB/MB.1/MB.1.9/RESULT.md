# RESULT MB.1.9 — Align intra-workspace version requirements and harden `scripts/dev-version.sh`

## Summary

Both halves are done, and the failure from publish run `43426326826` (job `131888508825`) is
reproduced and then repaired on a throwaway copy.

1. **Requirements aligned.** All **28** intra-workspace requirements now name the workspace version
   `0.1.1` (`version = "0.1.1", path = "../…"`). The 27 that said `0.1.0` were rewritten, and
   `crates/blastit-env/Cargo.toml:14`'s `blastit-manifest = { path = … }` — the one requirement with
   *no* version at all — gained `version = "0.1.1"` so every path dependency names the version it
   requires. No external dependency, `[workspace.package]`, or `Cargo.lock` entry changed
   (`cargo metadata --locked --no-deps` on the untouched tree still passes, so the lock already
   agreed).
2. **`scripts/dev-version.sh` hardened** (bash + `sed`/`awk` only, no new deps):
   - the requirement rewrite now matches **any release version** before `, path` —
     `s/version = "[0-9][0-9.]*", path/…/` — so a requirement that lags the workspace version can no
     longer make the rewrite a silent no-op;
   - after rewriting, an `awk` pass over every `path = "../…` line **proves** each one names
     `${dev}`, printing `file:line: <line>` and exiting 1 otherwise; it also fails if it finds *no*
     intra-workspace requirement at all (the same "the scan matched nothing" class of bug);
   - a cheap `grep -qx "version = \"${dev}\"" Cargo.toml` check proves the workspace version was
     actually rewritten;
   - the per-member `Cargo.lock` line rewrite and the final
     `cargo metadata --locked --no-deps --format-version 1` proof are unchanged;
   - the header comment now says the rewrite tolerates a lagging requirement and that the check runs
     afterwards (the version examples read `0.1.1`, and the "never mistaken for the release on PyPI"
     wording no longer hardcodes `0.1.0`).

Why the two halves are separate: the tolerant rewrite alone would have *repaired* a single-line
`0.1.0` drift; the guard is what turns a shape the rewrite structurally cannot repair (a version on
its own line, or no version at all) into a loud failure instead of a wheel that no build can resolve.
That is exactly what acceptance item (c) shows below.

## Evidence

### a. Requirements aligned in the repository

```console
$ grep -rn 'version = "0\.1\.0", path' crates/*/Cargo.toml || echo '(no matches)'
(no matches)

$ grep -rn 'version = "0\.1\.1", path' crates/*/Cargo.toml | wc -l
28

$ cargo metadata --locked --no-deps --format-version 1 >/dev/null && echo 'metadata --locked --no-deps OK'
metadata --locked --no-deps OK

$ git diff --stat -- Cargo.toml Cargo.lock        # nothing: no version/lock churn
$ git diff --stat -- 'crates/*/Cargo.toml'
 crates/blastit-dbproxy/Cargo.toml   |  6 +++---
 crates/blastit-env/Cargo.toml       |  2 +-
 crates/blastit-http/Cargo.toml      |  4 ++--
 crates/blastit-manifest/Cargo.toml  |  4 ++--
 crates/blastit-py/Cargo.toml        |  8 ++++----
 crates/blastit-scheduler/Cargo.toml |  8 ++++----
 crates/blastit-view/Cargo.toml      |  2 +-
 crates/blastit/Cargo.toml           | 22 +++++++++++-----------
 8 files changed, 28 insertions(+), 28 deletions(-)
```

### b. Throwaway copy, `/tmp/opencode/mb1-9/work` (never in the repo)

```console
$ rsync -a --exclude target/ --exclude .git/ --exclude dist/ ./ /tmp/opencode/mb1-9/work/   # 135M
$ cd /tmp/opencode/mb1-9/work && scripts/dev-version.sh 14
0.1.1-dev14
exit=0

$ grep -n '^version' Cargo.toml
6:version = "0.1.1-dev14"
$ grep -rn 'version = "0\.1\.1-dev14", path' crates/*/Cargo.toml | wc -l
28
$ sed -n '221,222p' Cargo.lock
221:name = "blastit-py"
222-version = "0.1.1-dev14"

$ cargo metadata --locked --no-deps --format-version 1 >/dev/null && echo 'metadata --locked OK'
metadata --locked OK

$ PYO3_PYTHON=/home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/bin/python3.14 \
  PYTHONHOME=/home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu \
  LD_LIBRARY_PATH=/home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/lib \
  cargo build --locked -p blastit-py
   Compiling blastit-dbproxy v0.1.1-dev14 (/tmp/opencode/mb1-9/work/crates/blastit-dbproxy)
   Compiling blastit-py v0.1.1-dev14 (/tmp/opencode/mb1-9/work/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.94s
```

**A full `scripts/build-wheel.sh` run *was* attempted and succeeded** in the same copy (maturin
1.16.0 from the offline uv cache, CPython 3.14, `MATURIN="uvx --offline maturin"`):

```console
$ MATURIN="uvx --offline maturin" PYO3_PYTHON=… PYTHONHOME=… LD_LIBRARY_PATH=… scripts/build-wheel.sh
   Compiling blastit-py v0.1.1-dev14 (/tmp/opencode/mb1-9/work/crates/blastit-py)
    Finished `release` profile [optimized] target(s) in 40.61s
📦 Including files matching "python/blastit/_bin/blastit-dbproxy"
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.1.dev14-cp314-cp314-manylinux_2_39_x86_64.whl
exit=0

$ ls -l dist/*.whl
-rw-r--r-- 1 remy remy 8430952 Nov 13 14:48 dist/blastit-0.1.1.dev14-cp314-cp314-manylinux_2_39_x86_64.whl
```

So the artifact name the develop job must attach (`blastit-0.1.1.dev<N>-*.whl`) is produced end to
end, not just resolvable.

**Before/after on that same drift** (copy `c4` reverted to the `cab377e` state — 28 requirements at
`0.1.0` — then built with the *old* script from `HEAD`):

```console
$ scripts/old-dev-version.sh 14; echo "old-script exit=$?"
0.1.1-dev14
old-script exit=0
$ cargo metadata --locked --no-deps --format-version 1 >/dev/null; echo $?
0
$ cargo build --locked -p blastit-py
error: failed to select a version for the requirement `blastit-dbproxy = "^0.1.0"`
candidate versions found which didn't match: 0.1.1-dev14
location searched: /tmp/opencode/mb1-9/c4/crates/blastit-dbproxy
required by package `blastit v0.1.1-dev14 (/tmp/opencode/mb1-9/c4/crates/blastit)`
```

That is the CI failure, byte for byte: exit 0, metadata clean, build dead. The **same drift with the
hardened script** (copy `c5`):

```console
$ scripts/dev-version.sh 14
0.1.1-dev14
new-script exit=0
requirements@0.1.1-dev14 = 28
metadata --locked OK
   Compiling blastit-py v0.1.1-dev14 (/tmp/opencode/mb1-9/c5/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 38.68s
```

### c. Guard firing (loud, `file:line`, non-zero)

A single-line `version = "0.1.0", path` revert is now *repaired* by the tolerant rewrite (that is the
point of half 2) — copy `c1`:

```console
$ grep -n 'blastit-events = ' crates/blastit/Cargo.toml     # after reverting one requirement
20:blastit-events = { version = "0.1.0", path = "../blastit-events" }
$ scripts/dev-version.sh 14; echo "exit=$?"
0.1.1-dev14
exit=0
$ grep -n 'blastit-events = ' crates/blastit/Cargo.toml
20:blastit-events = { version = "0.1.1-dev14", path = "../blastit-events" }
```

The guard fires on the drift the line rewrite structurally cannot repair. Copy `c2` (requirement
written as a table, i.e. version and path on their own lines — a shape a future `cargo add` can
produce):

```console
$ sed -n '20,22p' crates/blastit/Cargo.toml
20:[dependencies.blastit-events]
21-version = "0.1.0"
22-path = "../blastit-events"
$ scripts/dev-version.sh 14; echo "exit=$?"
intra-workspace requirements not rewritten to `0.1.1-dev14`:
crates/blastit/Cargo.toml:22: path = "../blastit-events"
a requirement naming a release version does not accept the pre-release `0.1.1-dev14`, so no build of it can resolve
exit=1
```

Copy `c3` (versionless path dependency — the exact shape `crates/blastit-env` had at `cab377e`, and
the shape that a *release* build accepts happily):

```console
$ sed -n '14p' crates/blastit-env/Cargo.toml
14:blastit-manifest = { path = "../blastit-manifest" }
$ scripts/dev-version.sh 14; echo "exit=$?"
intra-workspace requirements not rewritten to `0.1.1-dev14`:
crates/blastit-env/Cargo.toml:14: blastit-manifest = { path = "../blastit-manifest" }
a requirement naming a release version does not accept the pre-release `0.1.1-dev14`, so no build of it can resolve
exit=1
```

`bash -n scripts/dev-version.sh` passes; `shellcheck` is not installed on this host (nothing else
lints shell in `make check`).

### d. Repository gates

```console
$ make check; echo "make-check-exit=$?"
…
PASS  docs/security.md  fence 1  static        0.0s  statically checked every command, flag and snippet (…)
38 fences: 26 executed, 12 accounted, 0 failed · 25.0s
uv run --no-project tools/plan.py validate
plan OK
make-check-exit=0

$ uv run --no-project tools/plan.py validate
plan OK

$ uv run --no-project tools/critical_guard.py --base cab377e --node MB.1.9; echo "critical-guard-exit=$?"
critical-guard: cab377e..working tree, 31 changed file(s)
node MB.1.9: paths ['scripts/dev-version.sh']
  no change to a protected critical test
critical-guard-exit=0
```

`make check` includes `rs-fmt`, `rs-lint` (`-D warnings`), `rs-doc`, `rs-test` + `rs-test-critical`,
`py-lint`, `docs-unit`, `docs-check`, `docs-test` (which rebuilds the release wheel at `0.1.1` — the
normal, non-dev path still builds), and `plan-validate`.

### e. What the next develop publish/dev-wheel run must show

- `wheel (CPython 3.13)` and `wheel (CPython 3.14)` **green** in run for the merge commit, each
  attaching an artifact named `blastit-0.1.1.dev<N>-<cp3NN>-<cp3NN>-manylinux_*.whl`;
- `sdist` and the `uv virtualenv` job green as well;
- and the invariant this node buys: **no `Cargo.toml` requirement may name a version other than the
  workspace version** — if one ever lags again, `scripts/dev-version.sh` now stops the wheel job at
  the version step with a `file:line`, instead of letting it die later in dependency resolution.

## Files

- `scripts/dev-version.sh` — tolerant requirement rewrite, post-rewrite `file:line` guard (+
  workspace-version and "found nothing" checks), truthful header, unchanged `Cargo.lock` line
  rewrite and `cargo metadata --locked` proof.
- `crates/blastit/Cargo.toml`, `crates/blastit-dbproxy/Cargo.toml`, `crates/blastit-env/Cargo.toml`,
  `crates/blastit-http/Cargo.toml`, `crates/blastit-manifest/Cargo.toml`, `crates/blastit-py/Cargo.toml`,
  `crates/blastit-scheduler/Cargo.toml`, `crates/blastit-view/Cargo.toml` — requirements to `0.1.1`
  (and `blastit-env`'s versionless dependency given one).
- `Cargo.lock` — **unchanged** (it already recorded `0.1.1`; `--locked` proves agreement).
- This `RESULT.md`.

No protected test, `plan/` frontmatter, `CHANGELOG.md`, workspace version, or external dependency was
touched; `scripts/dev-version.sh` was never run in the repository (all five runs were in
`/tmp/opencode/mb1-9/{work,c1,c2,c3,c4,c5}`).

## Follow-ups

- **Node `paths` is narrower than its own Goal/Acceptance.** `NODE.md` declares
  `paths: ["scripts/dev-version.sh"]`, but the task cannot be done without
  `Cargo.toml`/`crates/*/Cargo.toml` (the dispatch brief listed them explicitly, and
  `critical_guard` shows the mismatch: `node MB.1.9: paths ['scripts/dev-version.sh']`). `paths` is
  managed frontmatter, so `lead`/`planner` should widen it to
  `["scripts/dev-version.sh", "Cargo.lock", "crates/*/Cargo.toml"]` for the record.
- **The rewrite is still line-oriented.** A `[dependencies.x]` table with `version`/`path` on
  separate lines is caught (loudly) but not repaired. If `cargo add`/`taplo` ever reformats the
  manifests that way, the fix is a whole-manifest (TOML-aware or `sed -n` multi-line) rewrite; the
  guard makes that day visible instead of silent.
- **`scripts/build-wheel.sh` and `dev-version.sh` are separate steps** and nothing in-repo runs the
  former after the latter, so a local "dev wheel" is only reproducible by calling both in order. The
  CI action does that (`.github/actions/build-wheel`); worth a one-line note if a contributor asks.
- **Disk, as requested:** `/tmp` went from 31 % to 47 % (32 G tmpfs, 17 G still free) while the
  five throwaway copies plus their `target/` dirs existed; the repository's `target/` is 40 G and
  unchanged by me. The `/tmp/opencode/mb1-9` copies are deleted after this report (5.1 G reclaimed).
