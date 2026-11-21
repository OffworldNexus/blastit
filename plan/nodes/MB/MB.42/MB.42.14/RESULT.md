# RESULT — MB.42.14

**STATUS: done**

`rs-no-python` no longer asserts a `manylinux_2_28` C-library floor on a **host**
build. The two claims are split by what can actually be proved where: the host leg
keeps the artifact *shape* (zero `libpython`, zero `libphp`, zero `RUNPATH`) and the
`2_28` floor now lives — as `STANDALONE_MAX_GLIBC ?= 2_28` — in `standalone-glibc`,
which compiles inside `quay.io/pypa/manylinux_2_28_<arch>` and hands the number to
`scripts/build-standalone.sh` / `scripts/build-lsp.sh` there. The floor was
relocated, never deleted.

## Changes

`Makefile`
- `rs-no-python`: the release binaries are checked by
  `scripts/check-portable-elf.sh` **without** `--max-glibc`, preceded by the line
  `host build: no glibc ceiling — that is standalone-glibc's job
  (quay.io/pypa/manylinux_2_28_<arch>)`, so no reader can believe 2.28 was proved
  here. The comment above the target states the distinction and why the old
  assertion was structurally red on every host.
- The removed `--max-glibc` "advertised but never parsed" source grep (a prose guard
  over `scripts/check-portable-elf.sh`) is replaced by a *behavioural* ownership
  check that survives the relocation: the leg dry-runs
  `make -n standalone-glibc` and fails if that target no longer carries
  `--max-glibc $(STANDALONE_MAX_GLIBC)`. Deleting the floor would now name the leg
  that lost it instead of silently dropping a claim.
- `STANDALONE_MAX_GLIBC ?= 2_28` sits next to `STANDALONE_ARCH`/`STANDALONE_BUILDER`
  with a comment naming it the **only** place 2.28 is asserted; `standalone-glibc`
  passes it to `scripts/build-standalone-in-image.sh`.
- `rs-no-default`: audited — it asserts a dependency graph (`--no-default-features`
  pulls no `pyo3`/view/`blastit-py-sapi`) and never touches `check-portable-elf.sh`,
  so it smuggled no ceiling in. The comment now says so, so the next editor does not
  add one.

`scripts/check-portable-elf.sh` is unchanged: `--max-glibc` still measures
`.gnu.version_r`, still excludes `WEAK` needs, and still refuses an ELF above the
ceiling. `scripts/build-standalone.sh:186-195` and `scripts/build-lsp.sh:457-467`
are the appliers the image path drives; both keep their refusals.

## Evidence

### `make rs-no-python` — green, and it says what it did not prove

```
$ make rs-no-python
cargo build -p blastit --locked            # default universal artifact
env -u PYO3_PYTHON -u PYTHONHOME cargo build -p blastit --locked
ok   …/target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
cargo build --release -p blastit --locked        # the binaries make install ships
env -u PYO3_PYTHON -u PYTHONHOME cargo build --release -p blastit --locked
env -u PYO3_PYTHON -u PYTHONHOME cargo build --release -p blastit-lsp --locked
host build: no glibc ceiling — that is standalone-glibc's job (quay.io/pypa/manylinux_2_28_<arch>)
ok   …/target/release/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   …/target/release/blastit-lsp: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
glibc floor: standalone-glibc still builds with --max-glibc 2_28 inside its manylinux image
blastit-view: 0 pyo3 / blastit-php-sapi lines
test result: ok. 284 passed; 0 failed   (cargo test -p blastit --lib)
default build: 0 pyo3 dependencies
default build: 0 blastit-view-python dependencies
default build: blastit-py-sapi present, 0 pyo3, no libpython linked
$ echo $?
0
```

### The negative: the 2.28 ceiling still refuses an ELF above it

The relocated floor is unchanged and still bites. Run the **same** host-built
`blastit-lsp` (measured 2.34) through the assertion the manylinux path applies:

```
$ scripts/check-portable-elf.sh --max-glibc 2_28 target/release/blastit-lsp
ok   target/release/blastit-lsp: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
max-glibc: 2.34 (target/release/blastit-lsp)
needed: ld-linux-x86-64.so.2 libc.so.6 libgcc_s.so.1
error: target/release/blastit-lsp needs glibc 2.34, above the 2.28 this artifact must run on
  (weak version needs are excluded: they do not raise the floor)
  build that shim against an older sysroot, or raise --max-glibc and say so in the package docs
$ echo $?
1
```

That is exactly the message the old gate produced — so the assertion is intact; it
just no longer runs against a binary compiled on this machine.

The container path still carries the number and the appliers still refuse:

```
$ make -n standalone-glibc
scripts/build-standalone-in-image.sh \
	--image "quay.io/pypa/manylinux_2_28_x86_64" \
	--target "x86_64-unknown-linux-gnu" \
	--max-glibc 2_28
$ grep -n 'above the \$max_glibc this archive must' scripts/build-standalone.sh scripts/build-lsp.sh
scripts/build-standalone.sh:189: … above the $max_glibc this archive must run on
scripts/build-lsp.sh:460:            … above the $max_glibc this archive must
```

And the gate that proves the measurement itself (`make packaging-gates-test`, a
`check` leg) is unchanged — including the strong-`GLIBC_2.39`-must-fail-`2_28` case:

```
$ make packaging-gates-test
packaging-gates-test: ok — 117 cases passed (workflow=19, matrix=15, wheel-tag=4, exec-env=12, test-seam=7, portable-elf=2, max-glibc=4, repack=13, matrix-producer=2, package-workflow=11, wheel-floor=9, publish=19)
```

### `make check` — runs to the end

`rs-no-python` is no longer the wall. The full unprivileged gate exits **0**:

```
$ make check > /tmp/opencode/mb4214-check.log 2>&1; echo check-exit=$?
check-exit=0
```

The legs after the one this node fixed, in order and each green:

```
$ grep -nE 'host build: no glibc|glibc floor: standalone-glibc|^ok   .*target/release/blastit' /tmp/opencode/mb4214-check.log
4261:host build: no glibc ceiling — that is standalone-glibc's job (quay.io/pypa/manylinux_2_28_<arch>)
4263:ok   …/target/release/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
4264:ok   …/target/release/blastit-lsp: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
4265:glibc floor: standalone-glibc still builds with --max-glibc 2_28 inside its manylinux image

$ sed -n '4631p;4642p;4653p;4720p' /tmp/opencode/mb4214-check.log   # rs-no-default, seam-check, e2e-universal
env -u PYO3_PYTHON -u PYTHONHOME cargo build -p blastit --locked --no-default-features --all-targets
cargo test -p blastit-scheduler --locked --no-default-features --test static_feature_off
scripts/check-no-test-seam.sh --present-all "…/target/debug/blastit"
uv run --no-project e2e/universal_binary.py \

$ grep -nE '^PASS|zed-check: OK|^plan OK' /tmp/opencode/mb4214-check.log | tail -n 8
4729:PASS  python 3.13    200 from CPython 3.13 (python-3.13/.venv), same artifact (0.1s)
4730:PASS  python 3.14    200 from CPython 3.14 (python-3.14/.venv), same artifact (0.1s)
4731:PASS  php 8.4        200 from PHP 8.4 (libblastit_php84-x86_64-gnu.so) (0.1s)
4732:PASS  static         200 with the file's own bytes (0.0s)
4733:PASS  handoff        `dev` handed off silently; production and `--version` did not (0.0s)
4734:PASS  check 3.12     check --json ok + fold on CPython 3.12, no per-minor .so (0.2s)
4840:zed-check: OK (59 properties checked)
4848:plan OK
```

### `make fast-check`

```
$ cargo fmt --all && cargo fmt --all -- --check && make fast-check
…
workflow-check: ok — 674 checks over 9 workflows and 1 actions, 0 problem(s)
plan OK
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
fast-check exit=0
```

One earlier `fast-check` run died in `rs-test` on
`supervise::tests::a_late_ctl_write_to_a_gone_scheduler_is_best_effort`
(`a closed peer cannot take the message`, `supervise.rs:2829`), a timing-dependent
case unrelated to this node: it passed inside the `make check` run minutes earlier
(284 passed) and on the rerun above, which is the exit-0 run quoted here.

## Follow-ups

- `universal-smoke` and `docs-serve-stage` also call `check-portable-elf.sh` without
  a ceiling, which is right: they inspect a debug/host binary too. If a *host* leg
  ever needs a floor claim, it needs a `manylinux_2_28` sysroot, not a smaller
  number.
- The 2.28 ownership check reads `make -n standalone-glibc`, so it travels with the
  Makefile. If `standalone-glibc` ever moves into a script, move that dry-run
  assertion with it (or the leg will name the missing floor, by design).
- `crates/blastit/src/supervise.rs::a_late_ctl_write_to_a_gone_scheduler_is_best_effort`
  is timing-dependent: it failed once under a parallel `fast-check` and passed on the
  immediate rerun and inside `make check`. Not touched here (outside this node's
  path); it wants the same treatment the scheduler's `critical_rewrite` timing cases
  need.
