# RESULT — MB.3.58 (attempt 3): every leg builds where it runs, one platform tag, and a gate that tells the truth

**Status: done** — `make check` exit 0, `make docs-check` exit 0, `plan validate` OK. The two
protections review 1 found missing are real (an unprivileged witness of `PR_SET_DUMPABLE(0)`, and an
auditor rule that fails on the exact defect it was written for), and the support statement now lives
where a user reads it: `docs/frameworks/php.md` §1.

The rule, applied to every leg below: **a container that runs our binary must be the one that built
it, or must have a C library at least as new as the builder's.** And the statement that follows from
it: **"built and exercised" is not "works" — this node prints what each leg reached, and the release
must not claim more.**

---

## 0. Review 1 and review 2 → attempt 3

| finding | fix | evidence |
|---|---|---|
| **BLOCKER** (review 1): the root run skips the only assertion of `PR_SET_DUMPABLE(0)` | `rs-test-php-image` keeps the root run for the whole suite **and** adds `docker run --user $(id -u):$(id -g)` filtered to `no_libphp_is_mapped_above_the_view_worker`; the target refuses to pass unless that run reports a `denied (EACCES)` read | §2, `denied` = 2 |
| **MAJOR 1** (review 1): rule 14 missed the defect | rule 14 flags **any** host `cargo build\|test\|…` (or a `target/…` or `/tmp/artifact` path) followed by any `docker run\|exec`, unless the job calls `scripts/check-exec-env.sh`; the restored pre-node step is a self-test | §4, 358 → FAIL |
| **MAJOR 2** (review 1): "6 built & exercised" reads as six working shims | per-leg `SHIM_REACHES` (`built\|probed\|served\|refused`) with the measured reason, a printed served count, and `--check` consistency rules | §5 |
| **owner (review 3)**: the support line can land in this node | `docs/frameworks/php.md` §1 now carries it — per-minor targets, the private directory requirement, the embed package names; `docs/knowledge/LESSONS.md` too | §5, §7 |
| **owner (review 4)**: the excerpt gave one target for two libraries and no ownership requirement | both fixed in `docs/frameworks/php.md`: `libphp8.3.so` from `/usr/lib/libphp83.so`, `libphp8.4.so` from `/usr/lib/php84/libphp.so`, root-owned `0644` in `/usr/local/lib` (never `/tmp`), plus the `MinorMismatch` sentence; the 8.2 sentence names `libphp8.2-embed` / `php82-embed` | §5 |
| **owner (review 3)**: the discovery gap is release-blocking | filed as **MB.3.59** (priority 1, `depends_on: [MB.3.58]`): resolve the minor from the library, not its file name; 8.2's refusal must name the missing symbol | §9 |
| **owner (review 3)**: `--check` and `--summary` must agree word for word | one `reaches_sentence()` feeds both: `served 2 of 12 shims, 2 probed, 2 refused, 6 unbuilt; 4 served once the aarch64 gnu legs run` | §5 |
| MINORs (review 1) | `PHP_SAPI_SUITES` skips the 8.2 compile; `python:*-slim` rows corrected to Debian 13.7 / 2.41; `GLIBC_ABI_DT_RELR` → 2.36 and unknown ABI nodes refused; LESSONS bullet re-stated | §2, §3, §6 |

## 1. Wheels: the tag was supplied twice (accepted in review 1, unchanged)

`.github/actions/build-wheel` passed both `manylinux: 2_28` to `maturin-action` **and**
`--compatibility …` to maturin, which refuses two platform tags after a full compile. `--compatibility`
is gone from the composite action (the image *is* the tag), the action refuses an empty `manylinux:`,
and `scripts/build-wheel.sh` refuses `--compatibility` from its caller when `--manylinux` already chose
the tag. `scripts/check_wheel_tag.py` stays the verification, `--max-glibc 2_28` and `--platform`
unchanged.

## 2. Shim + probe: build where you run — and the two witnesses that were missing

`make php-sapi-e2e` always built the shim **and** the probe inside the leg image; the failing half was
`rs-test-php-image`, which ran `cargo test --no-run` on the runner (glibc 2.39) and executed the
binaries in the leg image (bookworm, 2.36):

```
/lib/x86_64-linux-gnu/libc.so.6: weak version 'GLIBC_2.39' not found (required by target/debug/blastit)
```

`e2e/php-sapi/Dockerfile` now installs `clang`/`libclang-dev` (glibc legs) and compiles both suites in
the image, recording them in `/src/target/php-suite-executables`; `make rs-test-php-image` runs exactly
those, as the image's root, with no host build and no host bind mount. Raising the leg's base was
rejected: a leg on a base ≥ the runner proves nothing about the older distros the shim ships for.

**The root run is not the `PR_SET_DUMPABLE(0)` witness, and this attempt says so in the code.** Under
root the maps suite takes its `euid == 0` branch, which only *content*-checks the roles above the view
(`--cap-add SYS_PTRACE` provides the capability Docker's default root set lacks); the `denied > 0`
assertion lives in the unprivileged branch, and `ci.yml`'s root run never reaches it. So the target now
performs both runs and parses the second one:

```
docker run blastit-php-sapi-e2e:mb358b-php84  # the suites this image built (root)
test result: ok. 5 passed; 0 failed …              (critical_php_backend)
init pid 431: no libphp/shim; above the view 2 map(s) content-checked, 0 denied (EACCES), 0 gone, of 3 children; euid 0
test result: ok. 21 passed; 0 failed …             (critical_php_embed)
docker run --user 1000:1000 blastit-php-sapi-e2e:mb358b-php84  # the PR_SET_DUMPABLE(0) witness
init pid 9: no libphp/shim; above the view 0 map(s) content-checked, 2 denied (EACCES), 0 gone, of 3 children; euid 1000
test result: ok. 1 passed; 0 failed …              (the maps leg alone)
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
```

and it fails closed if the count is zero (`NO role was denied its maps to an unprivileged caller`), so
dropping `set_dumpable(false)` in `php_view.rs` now fails this target.

**`PHP_SAPI_SUITES`** follows the leg's `suites` field: 0 where the image can never pass (8.2, and the
Alpine legs), 1 where it does. The 8.2 build with 0 finished without the ~7-minute `blastit` compile,
recorded an **empty** suite list (`0 /src/target/php-suite-executables`), built the shim and passed all
three probe runs — the publish pipeline's `shims` job pays the same saving.

## 3. The audit: one row per leg (24 rows)

`builds-inside` = the container that executes the artefact compiled it. `base≥builder` = it did not,
but the executing image's C library is at least the builder's, with the reason. `unproven` = neither,
which is a failure, not a note. (Corrected: `python:3.13-slim` and `3.14-slim` are **Debian 13.7 /
glibc 2.41**, not Debian 12 / 2.36 — measured with `getconf GNU_LIBC_VERSION` in each image; the
conclusion is unchanged, and now over a wider margin.)

| # | leg (job / target) | builds in | runs in | verdict |
|---|---|---|---|---|
| 1 | wheel `x86_64-gnu-cp313` | `quay.io/pypa/manylinux_2_28_x86_64` (maturin-action `manylinux:`) | `python:3.13-slim` (Debian 13, 2.41) | builds-inside; smoke image 2.41 ≥ 2.28, and the tag is the proof (`check_wheel_tag --max-glibc 2_28 --platform manylinux_2_28`) |
| 2 | wheel `x86_64-gnu-cp314` | same | `python:3.14-slim` (2.41) | builds-inside; same proof |
| 3 | wheel `aarch64-gnu-cp313` | `quay.io/pypa/manylinux_2_28_aarch64` | `python:3.13-slim` (2.41) | builds-inside (CI-only: no arm runner here) |
| 4 | wheel `aarch64-gnu-cp314` | same | `python:3.14-slim` (2.41) | builds-inside (CI-only) |
| 5 | wheel `x86_64-musl-cp313` | `docker run alpine:3.21` (`scripts/build-musl-wheel.sh`) | the same container, same interpreter | builds-inside |
| 6 | wheel `x86_64-musl-cp314` | same | same | builds-inside |
| 7 | wheel `aarch64-musl-cp313` | same (native arm runner) | same | builds-inside (CI-only) |
| 8 | wheel `aarch64-musl-cp314` | same | same | builds-inside (CI-only) |
| 9 | archive `x86_64-unknown-linux-gnu` (`binaries_gnu`) | runner, `cargo build --release` (2.39) | `debian:trixie-slim` (2.41) | base≥builder, **checked per run** by `check-exec-env.sh` |
| 10 | archive `aarch64-unknown-linux-gnu` | arm runner (2.39) | `debian:trixie-slim` (2.41) | base≥builder, same check (CI-only) |
| 11 | archive `x86_64-unknown-linux-musl` (`binaries_musl`) | `docker run alpine:3.21` | `alpine:3.21` | builds-inside; the check records "no glibc floor" |
| 12 | archive `aarch64-unknown-linux-musl` | same, native | `alpine:3.21` | builds-inside (CI-only) |
| 13 | shim `libblastit_php82-x86_64-gnu.so` | `debian:bookworm` image | the same image (probe) | builds-inside; **`reaches: refused`** (no `php_version` export) |
| 14 | shim `libblastit_php83-x86_64-gnu.so` | `ubuntu:24.04` image | the same image (probe + suites + unprivileged maps leg) | builds-inside; **`served`** |
| 15 | shim `libblastit_php84-x86_64-gnu.so` | `debian:trixie-slim` image | the same image (probe + suites + unprivileged maps leg) | builds-inside; **`served`** |
| 16 | shims 8.2/8.3/8.4 `x86_64-musl` | `alpine:3.20` / `alpine:3.21` images | the same images (probe) | builds-inside; **probed** 8.3/8.4, **refused** 8.2 (§5) |
| 17 | shims 8.2/8.3/8.4 `aarch64-gnu` | the same distro images, native arm runner | the same images | builds-inside (CI-only) |
| 18 | shims 8.2/8.3/8.4 `aarch64-musl` | the same Alpine images, native | the same images | builds-inside (CI-only) |
| 19 | `ci.yml` `standalone` (debug binary, not shipped) | runner (2.39) | `debian:trixie-slim` (2.41) | base≥builder, **checked** by `check-exec-env.sh` in its own step |
| 20 | `e2e` demo image (`Dockerfile` **and** `Dockerfile.prebuilt`) | `e2e/Dockerfile`'s builder stage (in-image); `make e2e` uses the prebuilt variant with the manylinux 2.28 wheel | `python:3.x-slim-trixie` (2.41) | builds-inside / base≥builder (2.41 ≥ 2.28), tag checked by `check_wheel_tag.py`, install floor by `e2e/prebuilt_wheel.py` |
| 21 | `e2e` rootless image (`ci.yml` `rootless`, `live.yml`) | `e2e/rootless/Dockerfile` (in-image); the `.prebuilt` variant installs the runner-built wheel | trixie (2.41), DigitalOcean (Ubuntu) for `live.yml` | builds-inside / base≥builder (2.41 ≥ 2.39), `prebuilt_wheel.py` refuses a tag above 2.41 or a musl wheel |
| 22 | `e2e/php` (`php2shell`) image | the wheel above | `python:3.x-slim-trixie@sha256:…` (2.41) | base≥builder, same `prebuilt_wheel.py` proof |
| 23 | `e2e/php-sapi/seccomp-exec` harness | inside the leg image (Dockerfile) | the same image | builds-inside |
| 24 | `e2e/php-sapi` probe (`php-sapi-probe`) | inside the leg image (Dockerfile) | the same image | builds-inside |

Nothing in `.github/**` or `e2e/**` executes a runner-built binary in a container without either
building it there or checking the C library relation; rows 9, 19 and 21 are the three crossings, and
each is machine-checked (9 and 19 by `check-exec-env.sh` per run, 21 by the wheel's own tag before
Docker installs it).

## 4. The auditor now fails on the defect it was written for

Rule 14 used to look for a literal `target/debug|release` or `/tmp/artifact` beside a `docker run`, so
the pre-node step — `cargo test --no-run`, then `docker run … "$binary"` — sailed through ("358 checks
ok"). It now fires on **any** job that builds Rust on the runner and then runs a container, unless the
job carries the runtime check. Restoring that exact step in a throwaway copy:

```
$ uv run --no-project /tmp/opencode/make_pre_node.py . /tmp/opencode/pre-node
restored the pre-node step in /tmp/opencode/pre-node/.github/workflows/php-sapi.yml
$ UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py --repo /tmp/opencode/pre-node
FAIL php-sapi.yml: job legs builds Rust on the runner (or hands a build artefact to a container) and
then runs a container, without `scripts/check-exec-env.sh`; a container may only run a binary it built
itself, or one whose glibc floor it still meets. Add the check (it names both versions and the fix), or
build inside the image
workflow-check: FAIL — 360 checks over 6 workflows and 1 actions, 1 problem(s)
exit=1
```

That shape is now case 12 of `test_packaging_gates.py` (27 cases), next to the two maturin cases and the
deleted-guard case.

**What the rule still cannot see** (in the module docstring too): a job that invokes a
`scripts/build-*.sh` wrapper instead of naming `cargo`, and a `docker run` issued from Python inside
the e2e drivers. Both are covered at run time — the wrappers exit non-zero on a bad floor, and
`e2e/prebuilt_wheel.py` refuses a wheel whose platform tag is above the image it installs into —
but the static audit is silent about them, and a future matrix leg could use either shape.

## 5. What a user can actually run (`reaches`) — the support statement

`PROVEN` says an artefact was *built and exercised*; that is not the same claim as *works*, and 4 of the
6 x86_64 shims are not usable for `[route.php]` today. `scripts/platform_matrix.py` now records what
each built leg reached, with the measurement, and prints the count:

```
$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and exercised: 4/8 wheels,
2/4 archives, 6/12 shims; reaches: served 2 of 12 shims, 2 probed, 2 refused, 6 unbuilt; 4 served
once the aarch64 gnu legs run; 0 leg(s) dropped
```

`--check` and `--summary` print that sentence **verbatim** (one `reaches_sentence()`, so the gate and
the release body cannot drift apart). Those two commands and this RESULT are the only places it is
printed: `scripts/release-notes.sh` emits `CHANGELOG.md`, which has no `reaches` line, and
`plan/HUMAN.md`'s H-009 note quotes it for the owner.

* **8.2 (gnu and musl) is refused**, measured in both legs' own images: `libphp8.2` exports no
  `php_version`, and `crates/blastit-php-sapi/src/loader.rs` resolves it before `php_module_startup`,
  so the worker's `dlopen` of the shim fails (`Symbol not found: php_version`). Debian bookworm:
  `dlsym(libphp, "php_version")` → `undefined symbol`; `alpine:3.20`:
  `the loaded images have no php_version: Symbol not found`. The shim's real-load probe still serves —
  which is why `PROVEN` records the leg and `reaches` does not.
* **8.3/8.4 on glibc is served**: `critical_php_backend` 5/5 and `critical_php_embed` 21/21 in each
  image, `served: HTTP/1.1 200 OK` through the worker.
* **Alpine 8.3/8.4 is probed, not served as shipped**: the probe serves 200, and `blastit dev` serves
  `GET / → app 200` (13–18 ms) — but only when `BLASTIT_LIBPHP` names a *versioned* file. Alpine's own
  `/usr/lib/libphp83.so` and `/usr/lib/php84/libphp.so` carry no major.minor,
  so the boot refuses them (`no shim can be chosen`). Measured by copying each distro library to
  `/tmp/libphp8.3.so` / `/tmp/libphp8.4.so` in the leg image:

  ```
  curl: HTTP/1.1 200 OK|Content-type: text/plain;charset=UTF-8|…
  16:34:05 INFO  request     GET / → app  200  17.6ms      (musl 8.4)
  16:34:26 INFO  request     GET / → app  200  13.3ms      (musl 8.3)
  ```

That is why the docs say "PHP 8.3/8.4 supported, 8.2 built but not served, Alpine needs a versioned
`BLASTIT_LIBPHP`" — and they now say exactly that, in `docs/frameworks/php.md` §1, which this node's
`paths` gained in review 3:

```
The supported versions are PHP **8.3 and 8.4**; 8.2 is built but not served, because its embed
package (`libphp8.2-embed` on Debian 12, `php82-embed` on Alpine 3.20, which ships `libphp82`)
exports no `php_version` for Blastit to read.

Alpine's packaged libraries state no version in their names (`libphp83.so`,
`/usr/lib/php84/libphp.so`), so the boot refuses them with `no shim can be chosen`. Install each
under a versioned name that matches **its own** minor —
`install -m 0644 /usr/lib/libphp83.so /usr/local/lib/libphp8.3.so` for 8.3,
`install -m 0644 /usr/lib/php84/libphp.so /usr/local/lib/libphp8.4.so` for 8.4 — root-owned,
`0644`, in a directory no other uid can write (`/usr/local/lib`, never `/tmp`), and set
`BLASTIT_LIBPHP` to that path. A copy whose name claims a different minor than the library is
refused: the shim is built for the minor the name states, the library reports its own, and the two
have to agree.
```

Both refusal facts are one defect and are release-blocking, not a footnote: **MB.3.59** (§9) resolves
the minor from the library rather than its file name, serves Alpine 8.4/8.3 through their own package
names, and gives 8.2 a sentence that names the missing `php_version`. When it lands, `reaches` rises
and the paragraph above is re-stated.

For the planner: the 8.4 case is a **directory** gap as well as a naming one — `library_dirs()`
(`crates/blastit/src/stage0.rs:602-617`) lists `/usr/lib/<triplet>`, `/usr/lib64`, `/usr/lib`,
`/usr/local/lib/<triplet>` and `/usr/local/lib`, so with `BLASTIT_LIBPHP` unset a boot cannot reach
`/usr/lib/php84/libphp.so` even when the name is right; the fix has to add the per-minor directory (or
read the version from the library wherever it is found). The path I documented above is
`/usr/local/lib`, which *is* in that list, so the workaround works without the code change.

The two new consistency rules are not vacuous (each was checked by patching the table and running the
gate, then restoring the file byte-identically):

```
$ # 8.2 gnu patched from `refused` to `served` (its suites are false)
platform-matrix: SHIM_REACHES 8.2/gnu/x86_64: claims `served` for a leg whose `suites` is false, so no
CI leg re-proves it; run the suites there or record `probed`                                   exit=1
$ # 8.3 gnu patched from `served` to `probed` (its suites are true)
platform-matrix: x86_64-gnu-php83: `suites` is true, so CI runs the worker-level suites here, but
`SHIM_REACHES` says probed: the two cannot both be right                                     exit=1
```

## 6. `check-exec-env.sh`: the rule as a check, and the floor a DT_RELR binary states

```
$ scripts/check-exec-env.sh --image debian:bookworm --binary target/debug/blastit
check-exec-env: FAIL — blastit needs glibc 2.39, debian:bookworm has 2.36
  running it there fails at execve with:
    weak version 'GLIBC_2.39' not found (required by blastit)
  fix: build the binary inside debian:bookworm (or a builder no older than it), or
       raise the image to glibc >= 2.39 — never the binary's floor
exit=1
$ scripts/check-exec-env.sh --image debian:trixie-slim --binary target/debug/blastit
check-exec-env: ok — blastit needs glibc 2.39, debian:trixie-slim provides 2.41
$ scripts/check-exec-env.sh --image alpine:3.21 --binary target/debug/blastit
check-exec-env: FAIL — blastit needs glibc 2.39, and alpine:3.21 provides musl
$ scripts/check-exec-env.sh --image alpine:3.21 --binary /tmp/opencode/busybox-musl
check-exec-env: ok — busybox-musl requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
```

The `GLIBC_ABI_*` nodes are no longer dropped. A DT_RELR executable
(`gcc -Wl,-z,pack-relative-relocs`) states `GLIBC_ABI_DT_RELR` (glibc 2.36) and may state nothing newer
than `GLIBC_2.34`; reading only `GLIBC_2.x` called it 2.34, and a binary whose *only* need was the ABI
node read as "requires nothing":

```
$ readelf --version-info /tmp/opencode/relr_exe | grep Name:
  0x0010:   Name: GLIBC_ABI_DT_RELR  Flags: none  Version: 4
  0x0020:   Name: GLIBC_2.2.5  Flags: none  Version: 3
  0x0030:   Name: GLIBC_2.34  Flags: none  Version: 2
$ scripts/check-exec-env.sh --image debian:bullseye-slim --binary /tmp/opencode/relr_exe
check-exec-env: FAIL — relr_exe needs glibc 2.36, debian:bullseye-slim has 2.31
  running it there fails at execve with:
    version 'GLIBC_ABI_DT_RELR' not found (required by relr_exe)
exit=1
$ scripts/check-exec-env.sh --image debian:trixie-slim --binary /tmp/opencode/relr_exe
check-exec-env: ok — relr_exe needs glibc 2.36, debian:trixie-slim provides 2.41
```

An `GLIBC_ABI_*` node this check does not know is refused (exit 2, naming the node) rather than
guessed at.

## 7. Acceptance evidence

```
$ make check
…
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims; reaches: served 2 of 12 shims, 2 probed, 2 refused, 6 unbuilt; 4 served once the aarch64 gnu legs run; 0 leg(s) dropped
workflow-check: ok — 358 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 27 cases passed (workflow=12, matrix=11, wheel-tag=4)
plan OK
EXIT=0
```

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.18s
docs-check exit=0
```

```
$ make e2e
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: Tag: cp314-cp314-manylinux_2_28_x86_64
… (demo + rootless scenarios: 411 `ok` lines)
  ok   a restart serves the page as it is now
  ok   and the restarted boot serves it (200)
  ok   still 0 decoy opens (0)
EXIT=0
```

Attempt 3 moved no `Makefile` and no Dockerfile (docs, the matrix's printed sentence and the plan only),
so that green `make e2e` is the run for the shipped pair; `docs/frameworks/php.md` is exercised by
`docs-check` (zensical `--strict`) and by `blastit-manifest`'s `docs_fences` suite, both of which ran
inside `make check` above.

```
$ uv run --no-project scripts/check_wheel_tag.py --max-glibc 2_28 --platform manylinux_2_28 dist/*.whl
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: Tag: cp314-cp314-manylinux_2_28_x86_64
exit=0
$ uv run --no-project scripts/check_wheel_tag.py --max-glibc 2_28 --platform manylinux_2_28 \
      /tmp/opencode/fabricated/blastit-0.2.0-cp313-cp313-manylinux_2_39_x86_64.whl
wheel-tag: FAIL … has platform 'manylinux_2_39_x86_64', expected prefix 'manylinux_2_28_'
wheel-tag: FAIL … requires glibc 2.39 (manylinux_2_39), which is above the manylinux_2_28 floor
exit=1
$ uv run --no-project tools/plan.py validate
plan OK
```

```
$ make rs-test-php-image PHP_SAPI_IMAGE=blastit-php-sapi-e2e:mb358b-php84 PHP_SAPI_IMAGE_MINOR=8.4
docker run blastit-php-sapi-e2e:mb358b-php84  # the suites this image built (root)
test result: ok. 5 passed; 0 failed …                 (critical_php_backend)
init pid 431: … 2 map(s) content-checked, 0 denied (EACCES), … euid 0
test result: ok. 21 passed; 0 failed …                (critical_php_embed)
docker run --user 1000:1000 blastit-php-sapi-e2e:mb358b-php84  # the PR_SET_DUMPABLE(0) witness
init pid 9: … 0 map(s) content-checked, 2 denied (EACCES), … euid 1000
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
EXIT=0
```

```
$ make php-sapi-e2e PHP_MINOR=8.2 PHP_SAPI_KEEP=1 PHP_SAPI_IMAGE=blastit-php-sapi-e2e:mb358b-php82
… (no suite compile: PHP_SAPI_SUITES=0)
  ok  php 8.2 abi 1: status 200 …                    (three probe runs)
EXIT=0
$ docker run --rm blastit-php-sapi-e2e:mb358b-php82 sh -c 'wc -c /src/target/php-suite-executables'
0 /src/target/php-suite-executables
```

YAML: `workflow-check`'s check 1 is "every YAML file parses into a mapping" over all six workflows and
the composite action, and it passed above; a separate `yaml.safe_load` sweep printed `ok` for each of
the seven files. `bash -n` is clean on `scripts/check-exec-env.sh`, `scripts/build-wheel.sh` and
`scripts/collect-shim.sh`.

One wording gate bit this attempt, worth knowing before anyone edits these pages: `docs-unit`'s
`test_the_real_tree_carries_no_retired_php_vocabulary` scans the whole tree — node bodies included —
and refuses the vocabulary of the PHP transport that `MB.3.31`/`MB.3.32` retired, so `MB.3.59`'s body
now says "the honest answer is a refusal" where it said "the honest fallback is a refusal". It failed
`make check` on exactly one word, which is the gate working as designed.

Counts unchanged: `8` wheels / `4` archives / `12` shims declared, `4` / `2` / `6` built and exercised;
the new dimension is `reaches`, which is what the docs may state. Check count 352 → 358 and 22 → 27
cases, all additions (two maturin rules, three covered jobs, one new action step, four self-tests).

## 8. Files changed

- `.github/actions/build-wheel/action.yml` — no `--compatibility`; an empty `manylinux:` is refused.
- `.github/workflows/php-sapi.yml` — `if: matrix.suites == 'true'`; leg image builds the suites; no
  runner Rust toolchain or libclang; 40-minute bound.
- `.github/workflows/ci.yml`, `.github/workflows/publish-pypi.yml` — the builder-vs-image glibc check.
- `e2e/php-sapi/Dockerfile` — libclang on glibc legs; `ARG PHP_SAPI_SUITES`; suites compiled in-image
  and recorded; `BLASTIT_LIBPHP`/`BLASTIT_PHP_SHIM_DIR` exported.
- `e2e/php_sapi_probe.py` — passes `PHP_SAPI_SUITES` from the leg's `suites`.
- `Makefile` — `rs-test-php-image` runs the image's own binaries as root **and** the maps leg as the
  caller's uid, refusing a zero `denied` count.
- `scripts/check-exec-env.sh` — new; `GLIBC_ABI_*` floors; both directions proven.
- `scripts/build-wheel.sh` — refuses `--compatibility` when `--manylinux` chose the tag.
- `scripts/platform_matrix.py` — `suites`/`suites_because`, `SHIM_REACHES` (per-leg `reaches`), the
  reached counts, the support table, `--check` consistency rules, and `reaches_sentence()` — the single
  sentence `--check` and `--summary` print.
- `scripts/check_workflows.py`, `scripts/test_packaging_gates.py` — rules 13/14, the limitation, 27 cases.
- `docs/frameworks/php.md` §1 — the support line: 8.3/8.4 supported; 8.2 built but not served
  (`libphp8.2-embed` / `php82-embed`); Alpine needs each library copied under *its own* versioned
  name, root-owned `0644` in `/usr/local/lib` (never `/tmp`).
- `docs/knowledge/LESSONS.md` — the two glibc-leak shapes, and the fixture bullet re-stated for the
  in-image build, the root run, the unprivileged maps leg and the missing `CAP_SYS_PTRACE`.
- `plan/nodes/MB/MB.3/MB.3.59/` — the discovery gap, filed (priority 1, `depends_on: [MB.3.58]`).

## 9. Follow-ups

**MB.3.59** — `plan/nodes/MB/MB.3/MB.3.59/`, priority 1, `depends_on: [MB.3.58]`, owner implementer,
paths `crates/blastit/src/**, e2e/php-sapi/**, scripts/**`. Title: *"The Alpine libphp cannot be
versioned by name: the boot refuses 'no shim can be chosen' unless BLASTIT_LIBPHP is versioned"*. It
carries the measured evidence (musl 8.4/8.3 serve with a versioned `BLASTIT_LIBPHP`, refuse with
Alpine's names), the two code paths (`stage0.rs:453-457` candidate list, `:821-826`
`PhpRefusal::NoVersion`), the fix (resolve the minor from the library — `php_version`/the probe's
report — with the file name as a fast path, and a *separate* sentence for a library that genuinely
lacks the symbol), and the acceptance the owner stated: Alpine 8.4 serves via its own package name,
8.2's refusal names the missing `php_version`, the filename fast path still works, then re-measure and
raise `reaches` (and re-state `docs/frameworks/php.md` §1).

**H-009** (`plan/HUMAN.md`) is updated with what attempt 3 closed: items **1** (the php.md support
line) and **3** (the LESSONS bullet) are **done here**, since the lead widened `paths` to those two
files. **Still the owner's:** item 2 (the H-008 sentence — the gate's exact wording is now the
`reaches: served 2 of 12 shims, …` line) and item 4 (a two-line comment in
`crates/blastit/tests/critical_php_embed.rs`, which is a protected file outside these paths).

Also unchanged and still true: `publish-pypi.yml`'s `shims` job builds the same e2e image; with
`PHP_SAPI_SUITES` it now pays the suite compile only on the legs that can pass.

## 10. The per-leg reaches table and the served count the docs will print

Printed by `scripts/platform_matrix.py --summary` (one `reaches_sentence()`, the same string `--check`
prints):

| PHP minor | glibc (Debian/Ubuntu) | musl (Alpine) |
|---|---|---|
| 8.2 | **refused** — `libphp8.2` exports no `php_version` (built, probed) | **refused** — same, plus the unversioned Alpine name |
| 8.3 | **served** — suites 5/5 + 21/21, `HTTP/1.1 200 OK` | **probed** — probe 200; `blastit dev` 200 only with a versioned `BLASTIT_LIBPHP` |
| 8.4 | **served** — suites 5/5 + 21/21, `GET / → app 200` | **probed** — as 8.3 |

`reaches: served 2 of 12 shims, 2 probed, 2 refused, 6 unbuilt; 4 served once the aarch64 gnu legs
run.` The Alpine rows stay `probed` until **MB.3.59** lands (it will raise them to `served`); 8.2
stays refused with the honest sentence.

RELEASABLE: yes
