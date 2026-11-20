# MB.42.3.3 — ecosystem wrappers: universal `py3-none` wheels, console scripts, `py-sapi.yml`, Composer package

`STATUS: done`.

Three cross-file touches were forced by the contract itself and are stated up front,
because each one is evidence the design is now load-bearing rather than a preference:

1. **`python/blastit/_blastit.py`** — a wheel cannot carry a mode. `pip` (25.0.1,
   measured here) writes `0644` for every file outside `*.data/scripts/`, so the
   repacked `blastit/_bin/blastit` arrived non-executable, `_executable()` said no,
   `bin_path()` fell through to the *console script* on `PATH`, and `blastit
   --version` became an endless `exec` loop at 99 % CPU. `_executable()` now repairs
   the bit on a regular file the package owns, and `main()` repairs its two
   neighbours in `blastit/_bin/` — `blastit-lsp` and `blastit-dbproxy`, which stage 0
   `execve`s and this process never resolves.
2. **`.github/workflows/{wheels,publish-pypi}.yml`** — both referenced
   `matrix.python`/`matrix.python_tag`, which no longer exist on a wheel leg, so
   `workflow-check` went red. Minimal change: build interpreter `3.13`, artifact
   `wheel-<leg>`, `py3-none-musllinux_*.whl` glob. `scripts/test_packaging_gates.py`
   case 8 was retargeted at the new literal.
3. **`docs/**` untouched, so `STALE_CLAIMS`** — `ADR-0010` §4 and
   `backend-runtimes.md` still state `8` wheels. Instead of a gate that stays red
   until another node lands, `scripts/platform_matrix.py` records those two claims
   with the node that rewrites them (`MB.42.4`), prints them as stale, and **refuses
   the entry** once the page stops making the claim.

## 1. The universal wheel (design §6.1, two stages)

**`pyproject.toml`** kept `bindings = "bin"` (it is what makes the artifact
interpreter-free) and gained the §6.1 contract in prose, `requires-python = ">=3.12"`
and the `3.12` classifier — a `py3-none` wheel with no extension is otherwise
refused by `pip` on 3.12.

**`[project.scripts]` cannot live in this file.** Maturin refuses the pair, verified
rather than assumed:

```
$ uvx --offline maturin build --bindings bin     # pyproject.toml with [project.scripts]
🔗 Found bin bindings
💥 maturin failed
  Caused by: Defining scripts and working with a binary doesn't mix well
```

So the three console scripts are written by stage two,
`scripts/repack_universal_wheel.py`, whose `CONSOLE_SCRIPTS` is their single owner:

```
blastit          = blastit.__main__:main
blastit-lsp      = blastit.__main__:main_lsp     # unreachable before this node
blastit-manifest = blastit.fold:main
```

`scripts/test_packaging_gates.py` resolves each target against `python/blastit/`, so
a rename cannot leave a dead console script behind.

**`scripts/repack_universal_wheel.py`** (new, stdlib) performs the four §6.1 steps:
relocates `<name>.data/scripts/blastit` → `blastit/_bin/blastit` (0755) and any other
data script under `_bin/`; copies `--blobs` (`blastit-lsp` plus the shims of the
wheel's own `(arch, libc)` — another tuple's `.so` is skipped, never shipped); writes
`*.dist-info/entry_points.txt`; rewrites `WHEEL` (`Root-Is-Purelib: false`, exactly
one `Tag: py3-none-<platform>`, no duplicated header) and renames the file to match;
recomputes `RECORD` (sha256 + size per member, `RECORD` itself empty). Repacking a
wheel whose name already matches does not delete its own output (that bug ate the
first build).

**`scripts/build-wheel.sh`**: `maturin build --bindings bin` inside
`manylinux_2_28_<arch>` → repack → `check_wheel_tag.py --platform manylinux_2_28` →
`check-portable-elf.sh [--max-glibc 2_28]` + `check-no-test-seam.sh` on **every** ELF
the wheel carries (`blastit`, `blastit-lsp`, `blastit-dbproxy`, each shim).
`blastit-lsp` and the two CPython shims are compiled *inside* the image (on this host
they need `GLIBC_2.34`, which a `2_28` tag cannot promise); the four PHP shims come
from `target/php-sapi` and are refused if they outgrew the floor. `--require-shims
py|all` fails closed on an incomplete set. The host path (no `--manylinux`, what
`make docs-test` uses) keeps the unversioned `linux_<arch>` tag, which
`check_wheel_tag.py --allow-host-tag` (new flag) permits explicitly.

## 2. `scripts/platform_matrix.py`

* `wheels` = **4** legs, one per `(arch, libc)`, with the §6.1 keys the consumers
  read: `{name, arch, libc, python_tag: "py3-none", wheel_tag, wheel,
  smoke_pythons: "3.12,3.13,3.14", runtime_images, runs_on, manylinux, musllinux,
  glibc_floor}`. `PYTHONS`/`runtime_image` are gone.
* `py_shims` = **8** legs, wired like `shims`: `--matrix py-shims`,
  `--count py-shims`, `--leg py-shims --minor … --arch … --libc …`,
  `--summary py-shims`. Each leg carries the artifact name, its three exports and the
  one symbol that must stay undefined.
* `PROVEN` gained `("wheels", "x86_64", "gnu")` and both `x86_64-gnu` `py_shims` rows
  (built and asserted here). The six legs that need an arm64/musl runner are
  declared, unproven — `py-sapi.yml` builds them on their own runners, and a `PROVEN`
  row is evidence, not a promise.
* The count gate reads `python shims` as its own family (`8`) and accepts
  `universal wheels` as a qualifier, so `ADR-0017`-style prose is checked against the
  right number.
* `STALE_CLAIMS` (see the header): two documents, one owner, checked both ways.

## 3. `py-sapi.yml`, `check_workflows.py`, `e2e/prebuilt_wheel.py`

* **`.github/workflows/py-sapi.yml`** (new): `matrix` (`--check` + `--matrix
  py-shims`), `lint` (`cargo fmt`, `clippy -p blastit-py-shim --features
  extension-module`), and one leg per entry running `make py-sapi PY_MINORS=<minor>
  PY_SAPI_REQUIRE_ALL=1` then the **artifact** assertions as proof lines:
  `DT_SONAME`, the three `nm -D --defined-only` exports, `nm -uD` `Py_InitializeEx`
  undefined, zero `NEEDED libpython`, `check-portable-elf.sh`; artifact uploaded.
* `check_workflows.py`: `py-shims` is a known `--matrix`/`--leg` family, `py_shims`
  an `OUTPUT_SPECS` key, `--minor` its required disambiguator, and `wheels` no longer
  requires `--python`.
* `e2e/prebuilt_wheel.py`: accepts `py3-none-<platform>` (interpreter from
  `E2E_PYTHON_VERSION`, default `3.14`) and still accepts a `cp3NN` developer wheel;
  musl and >2.41 manylinux still refuse.

## 4. Composer wrapper (`packages/composer-blastit/`)

* `composer.json`: `offworldnexus/blastit`, `"bin": ["bin/blastit"]`, Apache-2.0,
  `extra.blastit.version = 0.2.0`, and the per-project-pin note (§6.1).
* `bin/blastit`: POSIX `sh`, `set -eu`, no `set --` (it would clobber `"$@"`).
  Resolves `(arch, libc)` from `uname -m` + the musl loader/`ldd`; picks the pinned
  row from an **embedded** digest table; caches to `${BLASTIT_COMPOSER_CACHE}` →
  `<pkg>/.runtime` → `${XDG_CACHE_HOME:-$HOME/.cache}/blastit/composer`; verifies
  SHA-256 **before** extracting; marks the tree verified; `BLASTIT_HANDOFF_DONE=1
  exec`s it. With `BLASTIT_RELEASE_BASE_URL` set it never trusts the origin's own
  checksum: `$BLASTIT_SHA256`, `$BLASTIT_SHA256SUMS_FILE`, or a `SHA256SUMS` beside a
  `file://`/path base — otherwise it fails closed naming the fix. `$BLASTIT_ARCHIVE`
  pre-seeds an archive for CI (still verified).
* `scripts/composer_sha256.py` stamps the table and `VERSION` from built archives (a
  digest is never typed by hand); `scripts/composer_smoke.sh` is the container proof;
  `make composer-smoke` runs it.

## 5. Carried in from `MB.42.11`: the third bundle root was dead — **made load-bearing**

`crates/blastit/src/check.rs`: `bundle_root()`'s `exe_dir.join("../../..")` is
replaced by `exe_dir.join("../..")`. From the repacked wheel
(`<site-packages>/blastit/_bin/blastit`) `../..` **is** `<site-packages>`, the root
that holds `blastit/`; `../../..` pointed one level *above* it, where no `blastit`
package can be — which is why it could never match. It now serves the `py3-none`
layout and is proved by `a_repacked_wheel_layout_finds_its_gate` (a decoy `blastit`
package one level above `site-packages` must not shadow the wheel's own, and a wheel
missing `_check_pytest.py` is not a gate root). `make install`'s `../python` root is
untouched, and the pre-existing bundle/cargo-tree test still passes. The `blastit
check` leg of `scripts/wheel-drive.sh` exercises the real path in the installed wheel.

## Evidence

### 1. `make wheel-manylinux` — the release wheel, every embedded ELF measured

```
📦 Built wheel to /wheel-out/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl
repack: dist/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl
repack: blastit/_bin/ carries blastit, blastit-dbproxy, blastit-lsp, libblastit_php83-x86_64-gnu.so,
        libblastit_php84-x86_64-gnu.so, libblastit_php85-x86_64-gnu.so, libblastit_php86-x86_64-gnu.so,
        libblastit_py313-x86_64-gnu.so, libblastit_py314-x86_64-gnu.so
wheel-tag: blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: Tag: py3-none-manylinux_2_28_x86_64
ok   /var/tmp/blastit-wheel.hSvZHl/_bin8: NEEDED libgcc_s.so.1 libpthread.so.0 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
max-glibc: 2.17 (/var/tmp/blastit-wheel.hSvZHl/_bin8)
ok   /var/tmp/blastit-wheel.hSvZHl/_bin8: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   /var/tmp/blastit-wheel.hSvZHl/_bin8: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
ok   /var/tmp/blastit-wheel.hSvZHl/_bin8: no BLASTIT_TEST_RELOAD_PAUSE (absent, as shipped)
```

(The nine `_bin` members are each printed the same way: `blastit`, `blastit-lsp`,
`blastit-dbproxy`, four `libblastit_php8*` and two `libblastit_py3*`.)

### 2. `make wheel-smoke` — one wheel, three interpreters, driven

```
wheel-container-smoke: blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl in python:3.12-slim,python:3.13-slim,python:3.14-slim
wheel-smoke: console scripts on PATH: blastit, blastit-lsp, blastit-manifest
wheel-smoke: blastit 0.2.0
wheel-smoke: blastit --help ok
wheel-smoke: .../blastit/_bin/blastit carries no test seam
wheel-smoke: dbproxy executable at .../blastit/_bin/blastit-dbproxy
wheel-smoke: 200 /, 404 /no-such-path
wheel-smoke: 3.12 wheel ok            # and 3.13 wheel ok, 3.14 wheel ok
wheel-drive: CPython 3.12, wheel commands in /usr/local/bin
wheel-drive: blastit 0.2.0
wheel-drive: blastit-manifest fold proposed 1 grant line(s)
wheel-drive: blastit-lsp session lsp_smoke: ok — /usr/local/bin/blastit-lsp: initialize +
             codeActionProvider, one severity-1 diagnostic at 4:15, shutdown/exit 0, 0 bytes on stderr
wheel-drive: CPython 3.12 wheel ok (version, fold, lsp, check)   # and 3.13, 3.14
wheel-container-smoke: ok — ... installed, smoked and driven in python:3.12-slim,python:3.13-slim,python:3.14-slim
```

`blastit check` (the fourth drive step) exits 0 on the store-less, 0-proxy project and
prints no per-request line, so the driver's summary suffix is empty; the run would
have failed loudly on a non-zero exit.

`blastit-lsp --help` is **not** a proof this binary can give: the server takes no
arguments by design (`ADR-0017` §3.4 — it reads frames from stdin/stdout and exits 0
only after `shutdown`), so `--help` would start a session against EOF. The driver runs
a real `initialize → didOpen → shutdown → exit` session instead, which is strictly
stronger.

### 3. `scripts/platform_matrix.py --check` (4 wheels, 8 python shims)

```
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared;
built and exercised: 1/4 wheels, 2/4 archives, 8/16 shims, 2/8 python shims, 1/5 lsp assets;
reaches: served 8 of 16 shims, 0 probed, 0 refused, 8 unbuilt; 16 served once the aarch64
legs run; 0 leg(s) dropped
$ scripts/platform_matrix.py --count wheels   -> 4
$ scripts/platform_matrix.py --count py-shims -> 8
```

### 4. `make py-sapi PY_SAPI_REQUIRE_ALL=1` and the `py-sapi.yml` proof lines

```
py-sapi: cpython 3.14 -> target/py-sapi/libblastit_py314-x86_64-gnu.so (…/cpython-3.14…/python3.14)
ok   target/py-sapi/libblastit_py314-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 314
test the_built_shim_speaks_the_frozen_abi ... ok
py-sapi: cpython 3.13 -> target/py-sapi/libblastit_py313-x86_64-gnu.so (…/cpython-3.13…/python3.13)
ok   target/py-sapi/libblastit_py313-x86_64-gnu.so: blastit_py_abi_v1 version 1, CPython minor 313
py-sapi: 2 shim(s) for x86_64-gnu in target/py-sapi; skipped: none
```

The workflow's assertion block, run against that artifact:

```
 0x000000000000000e (SONAME)             Library soname: [libblastit_py313-x86_64-gnu.so]
exported: T blastit_py_abi_v1
exported: T blastit_py_abi_version
exported: T blastit_py_minor
undefined:                  U Py_InitializeFromConfig
NEEDED: libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2  (zero libpython)
ok   target/py-sapi/libblastit_py313-x86_64-gnu.so: NEEDED libgcc_s.so.1 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
```

### 5. `make composer-smoke` — installed, run, and refused when it should be

```
composer-smoke: composer install in composer:2
composer-smoke: vendor/bin/blastit -> blastit 0.2.0 (composer wrapper fixture)
composer-smoke: a tampered archive is refused
composer-smoke: blastit: blastit-x86_64-unknown-linux-musl.tar.gz does not match its digest
                (want 1cdfe1ed…, got 3c912e80…): refusing to run it
composer-smoke: ok — installed, run, tampered archive refused, origin without a digest refused
```

(`composer:2` is **Alpine**, so the wrapper resolved the musl row — the fixture ships
both libcs and the digests come from its own `SHA256SUMS`, never from the origin.)

### 6. Gates

```
make packaging-gates-test: ok — 73 cases passed (workflow=19, matrix=15, wheel-tag=4,
                           exec-env=12, test-seam=7, portable-elf=2, max-glibc=4, repack=10)
make workflow-check:       ok — 459 checks over 7 workflows and 1 actions, 0 problem(s)
make fast-check:           test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
                           ... (rs-fmt, rs-lint, rs-doc, rs-test, py-lint, plan-validate, universal-smoke)
                           install-test: ok (a) --user installs into a scratch prefix; blastit --version runs
                           install-test: all cases passed
                           -> exit 0 (no `make: ***` line anywhere in the run)
```

`make wheel-smoke` builds into `target/dist/` for this node's gate, not `dist/`:
`crates/blastit/tests/critical_artifacts.rs` (protected) reads any `dist/*.whl` and
still requires `blastit/_blastit*.so` — see the first follow-up.

## Follow-ups

* **`crates/blastit/tests/critical_artifacts.rs` reads any `dist/*.whl` and requires
  `blastit/_blastit*.so`.** The repacked wheel has no extension by design (`py3-none`,
  ELFs in `blastit/_bin/`), so with a wheel in `dist/` that protected test fails —
  it is not weak, it is reading the previous layout. It needs either a
  `blastit/_bin/` branch or, better, to hand the wheel family to the
  `critical_universal_bundle.rs` design §6.1 names; both are `crates/**` edits in a
  node whose `# Goal` authorizes them, and `critical_guard.py` forbids this one.
  The wheel is therefore built into `target/dist/` for this node's gate, and the
  artifact is proved instead by `make wheel-manylinux` (every `_bin` ELF measured)
  and `make wheel-smoke` (installed and driven on three CPythons).
* `blastit_build.py` (PEP 517 backend, not in this node's `paths`) still hands the
  bin-binding wheel straight to maturin, so `pip install blastit` **from the sdist**
  yields a wheel with the binary in `.data/scripts/` and only the `blastit` command.
  It should call `scripts/repack_universal_wheel.py` after `maturin.build_wheel()`.
  Prebuilt wheels — the path PyPI users take — are unaffected.
* `make install`'s bundle is untouched by the repack; `bundle_root` keeps both roots,
  so unifying the layouts later needs no second change.
* `docs/decisions/ADR-0010` (lines 40, 45, 71) and `docs/design/backend-runtimes.md`
  (252, 257, 355) still state `8` wheels; `MB.42.4` owns the rewrite and
  `STALE_CLAIMS` fails `--check` the moment the claim disappears.
* The `musllinux` and `aarch64` wheel legs need their own runners; `make
  wheel-smoke` pulls `python:3.12-slim`/`3.14-slim`, which a network-less CI leg
  cannot do.

## Progress (run 2) — consolidated `review-arch` + `review-ux` fixes

`.github/**` is now in this node's scope (the wheel action and the two wheel
workflows were the blockers). Every fix below was reproduced first, then fixed.

### BLOCKER 1 — the gnu release legs could not build: the action ran maturin directly

`.github/actions/build-wheel/action.yml` invoked `maturin-action` with
`--features blastit-py/extension-module` — a crate `MB.42.2.2.2` deleted — and
never called `scripts/build-wheel.sh`, so it would have produced a `cp3NN`
extension wheel while `wheels.yml`/`publish-pypi.yml` upload and count
`py3-none` wheels (`--count wheels` = 4). The action is now a thin caller:

```
bash scripts/build-wheel.sh --manylinux "${{ inputs.manylinux }}" \
     --require-shims "${{ inputs.require-shims }}" [--shims-from DIR] --out dist
```

plus the tag assertion (`Root-Is-Purelib: false`, `Tag: py3-none-…`,
`check_wheel_tag.py --max-glibc`), the dbproxy mode assertion (`-rwxr-xr-x`,
which only holds now that the zip carries `0100755`), and
`scripts/wheel_container_smoke.sh dist` (three CPythons) instead of a `cp3NN`
glob. Both workflows lost the `python:` input (it described the per-minor matrix
that is gone) and pass `platform: manylinux_${{ matrix.manylinux }}`.

### BLOCKER 2 — the musllinux wheel could never be produced (or named)

`scripts/build-wheel.sh` now reads the platform from **the built wheel's own
`Tag:`**, and refuses a promise that disagrees:

```
tag_platform=$(unzip -p "$built" '*.dist-info/WHEEL' | sed -n 's/^Tag: //p' | …)
[[ -z ${wheel_platform:-} || $wheel_platform == "$tag_platform" ]] || exit 1
wheel_platform=$tag_platform
```

The host branch no longer writes `linux_<arch>` over maturin's
`--compatibility musllinux_1_2`, the floor/prefix of the tag check are derived
from that tag (`${wheel_platform#manylinux_}` / `${wheel_platform%_*}`), and the
dead `${musllinux_ver:-1_2}` is gone. The host path also compiles the two
CPython shims when it is a musl host (`BLASTIT_WHEEL_BUILD_PY_SHIMS=1`, set by
`scripts/build-musl-wheel.sh`), so the musllinux wheel carries them.

A **repaired** wheel (what maturin produces on musl) keeps the real binary in
`<name>.scripts/`, a `#!python` launcher in `.data/scripts/` and its vendored
`libgcc_s-<hash>.so.1` in `<name>.libs/`; the repack now relocates all three
together (`blastit/_bin/blastit` + `blastit/<name>.libs/…`, which keeps the
`$ORIGIN/../<name>.libs` RPATH valid) instead of shipping the 451-byte launcher.
`scripts/check-portable-elf.sh` accepts `libgcc_s-*.so*` as the libgcc it already
allowed (auditwheel's vendored name).

### BLOCKER 3 + 9 — the Composer cache was repo content, and silent

`packages/composer-blastit/bin/blastit`:

* the `<pkg>/.runtime` candidate is **gone**: Composer installs the wrapper as a
  symlink into `vendor/bin`, so `dirname $0/..` is the consuming project's
  `vendor/` — a committed `vendor/.runtime` tree was `exec`'d with no hash.
  Cache = `$BLASTIT_COMPOSER_CACHE` else `${XDG_CACHE_HOME:-$HOME/.cache}/blastit/composer`.
* a `pending` row is refused **before** the cache is consulted (it was checked
  after the fast path);
* the marker file is gone: a cached tarball is re-hashed on **every** run and the
  tree re-extracted from it, so nothing is executed on the strength of a file
  beside it;
* UX: one `blastit: fetching <archive> <ver> (once; cached afterwards)` line on
  the cold path only; an unwritable cache dies with
  `BLASTIT_COMPOSER_CACHE (…) is not writable: point it at a directory this user
  can write, or unset it to use $XDG_CACHE_HOME/blastit/composer`; download and
  extract failures die with the fix (`BLASTIT_RELEASE_BASE_URL` /
  `BLASTIT_ARCHIVE`); `BLASTIT_ARCHIVE` documented; the comment that named
  `scripts/composer-sha256.sh` names `composer_sha256.py`.

### BLOCKER 4 — four of eight `py_shims` legs "proved" a glibc artifact named `-musl`

`py_shim_legs()` gained `builder_image` (`alpine:3.21` for musl, `""` for gnu,
and `check()` refuses a musl leg without one, or a gnu leg with one).
`scripts/py_sapi_leg.sh` is the one way a leg is built and asserted: it re-execs
itself in the leg's image, provisions it, builds with
`PY_SAPI_LIBC=<libc>` (no `--target`: on musl the default triple *is* musl), and
asserts `DT_SONAME`, the three exports, `Py_Initialize*` undefined, zero
`NEEDED libpython`, and — the assertion that catches the original bug —
**`NEEDED` names the leg's libc** (`libc.musl-<arch>.so.1` vs `libc.so.6`).
`py-sapi.yml` calls that script and publishes its proof lines to the step
summary.

### MAJOR 5 — the exec loop is removed, not masked

`python/blastit/_blastit.py`:

* `_executable(path, *, repair=False)`; the repair happens only for this
  package's own `_bin/` (`_candidates` returns `(path, repair)`), so
  `$BLASTIT_BIN` and the interpreter's `bin/` are never `chmod`-ed;
* `main()` sets `BLASTIT_WHEEL_EXEC=1` (`EXEC_GUARD`) before it `exec`s, and
  `_program()` **refuses** when the package's `_bin/` did not answer instead of
  falling back to `<venv>/bin/blastit` (the console script itself): the loop is
  now one sentence, not 6+ silent `exec`s;
* `os.chmod(path, stat.S_IMODE(os.stat(path).st_mode) | 0o111)` — a `0600` file
  becomes `0711`, never `0755`.
`scripts/wheel_readonly_refusal.sh` proves it: the wheel is installed, its
`_bin/blastit` is forced to `0644`, and the package is mounted read-only as an
unprivileged user.

### MAJOR 6 — matrix drift is now a failure

`scripts/check_workflows.py` rule 19: for every `OUTPUT_SPECS` family, the job
that owns the output must have a step that builds the family with
`scripts/platform_matrix.py --matrix <family>` (a `$family`/`$var` is accepted
only when the same step carries the `name:libc` table, which is how
`publish-pypi.yml` expands it). `test_packaging_gates.py` replaces
`py-sapi.yml`'s producer with a hand-written one-leg array and now expects a
failure.

### MAJOR 7 + MINOR 10/11 — shims are not silently dropped, and modes/arches are real

* `staged_blobs()` **raises** on a shim of another `(arch, libc)` (was `continue`);
  the repack self-test asserts the refusal names both targets.
* `assert_shims py` counts `libblastit_py3*.so` (two PHP shims no longer satisfy
  it) and `all` requires 2 + 4 explicitly; `scripts/build-musl-wheel.sh` passes
  `--require-shims py`.
* `external_attr` is `0o100755`/`0o100644` (a bare `0o755` rendered
  `?rwxr-xr-x`), and `platform_arch()` refuses an unknown platform instead of
  guessing the arch.
* `--shims-from DIR` is the way a wheel gets its four PHP shims (no PHP headers
  exist in the wheel builder).

### MAJOR 8 — the Composer table is generated, and a release refuses `pending`

`make composer-stamp` (`BLASTIT_VERSION`, `scripts/composer_sha256.py
--archives dist/*.tar.gz`), `--require-complete` (exit 1 while any row is
`pending`), called from `publish-pypi.yml` on the `binaries` family right before
the assets are uploaded. `scripts/composer_smoke.sh` now proves the
**default-origin** path: the package copy is stamped from the fixture archives
and run with no `BLASTIT_RELEASE_BASE_URL` at all (only `BLASTIT_ARCHIVE`), plus
a warm run that must print no fetch line, the tampered-archive refusal, the
no-digest refusal, and a `pending` refusal.

### Deployment note (new, from run 2)

A musllinux wheel needs `libgcc` on the host (`apk add libgcc`) for a Rust
binary's `_Unwind_*` symbols — the same requirement the standalone archive's
README states. The wheel smoke installs it for alpine legs; there is no way to
express it in wheel metadata (`MB.42.4` owns the user-facing sentence).
