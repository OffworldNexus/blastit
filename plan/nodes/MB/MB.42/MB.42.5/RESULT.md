# MB.42.5 — PHP 8.6 end-to-end (PHP 8.3, 8.4, 8.5 and 8.6)

`0.2.0` now ships four PHP minors end-to-end: the per-minor C SAPI shim, the
stage-0 discovery gate, a **16-leg** shim matrix (4 minors x 4 (arch, libc)),
the packaging globs, and the real-load/real-pair proofs on the 8.6 GNU and musl
legs.

## What changed

1. **`crates/blastit-php-sapi`**
   - `Minor::Php86` — `(8, 6) => Some(Self::Php86)`, `code() == 806`,
     `digits() == "86"`, `dotted() == "8.6"`,
     `config_names() == ["php-config8.6", "php-config"]`.
   - `SUPPORTED_MINORS: [Minor; 4] = [Php86, Php85, Php84, Php83]` (newest
     first); `CONFIG_NAMES: [&str; 5]` gains `php-config8.6`.
   - `shim_file()` -> `libblastit_php86-<arch>-<libc>.so`; the built artifact's
     `DT_SONAME` is verified below.
   - `build.rs`: `CANDIDATES: [&str; 5]`, `SUPPORTED: [&str; 4]`, warning names
     the 8.6 packages.
   - `loader.rs` `MinorUnsupported` and `probe/main.rs` say
     `8.3, 8.4, 8.5 and 8.6`.
   - `c/blastit_php_sapi.c` compiled **unmodified** under
     `-Wall -Wextra -Werror` against the real PHP 8.6.0 headers (the GNU leg
     build below is the proof); no flag was weakened and no `#pragma … ignored`
     was added.
2. **`crates/blastit-view-php/src/discovery.rs`**
   - `LIBPHP_CANDIDATES: [&str; 13]` prepends `libphp8.6.so`, `libphp-8.6.so`,
     `libphp86.so`.
   - `LIBPHP_PROBES: [LibphpProbe; 17]` prepends those three plus
     `libphp.so` in `per_minor_dir: Some("php86")`.
   - `library_dirs()` gains `/usr/lib/php86` and `/usr/local/lib/php86` before
     the `php85` pair.
   - `SUPPORTED_PHP = "PHP 8.3, 8.4, 8.5 and 8.6"`; `supported()` accepts
     `8.3..=8.6`.
   - The package hint is now minor-aware
     (`php_packages_hint(Option<PhpVersion>)`, defaulting to 8.6):
     `apt install libphp8.6-embed (stock Debian: apt install libphp-embed)` /
     `apk add php86-embed` / `dnf install php-embedded`.
3. **`scripts/platform_matrix.py`** — `PHP_MINORS` gains `"8.6": "86"`;
   `PHP_BUILDS` gains `("8.6", "gnu")` (`debian:bookworm-slim` +
   `extra_repo: "sury"`, `libphp8.6-embed` -> `/usr/lib/libphp8.6.so`) and
   `("8.6", "musl")` (`alpine:edge` + `extra_repo: "alpine-edge-testing"`,
   `php86-embed` -> `/usr/lib/php86/libphp.so`); a declarative `extra_repo`
   field (validated against the `EXTRA_REPOS` vocabulary) is surfaced in
   `shim_legs()`; `SHIM_REACHES` records both 8.6 x86_64 legs as `served` with
   the measurement, and both `PROVEN` entries name `MB.42.5`.
4. **`e2e/php-sapi/Dockerfile`** — `ARG EXTRA_REPO`; the package layer performs
   the named repository setup before installing anything (Surý keyring +
   `sources.list.d` on apt hosts; `edge/testing` appended on apk hosts) and
   refuses an unknown name. The Alpine branch now resolves
   `clang<ver>-libclang` from the image instead of hard-coding `clang19-libclang`
   (which no longer exists on `alpine:edge`), and the suites layer derives
   `LIBCLANG_PATH` from the installed `libclang.so`.
5. **`e2e/php_sapi_probe.py`** — passes `--build-arg EXTRA_REPO=…` from the
   matrix and accepts `806` in the probe report's minor set.
6. **`.github/workflows/php-sapi.yml` / `publish-pypi.yml`** — no per-leg edit:
   the 8.6 legs arrive from `platform_matrix.py --matrix shims` and the repo
   setup travels as a build arg. Only the prose that enumerated three minors
   was corrected.
7. **Packaging** — `make install`, `check-bundle-layout.sh`,
   `build-standalone.sh`, `collect-shim.sh` and `build-deb/rpm.sh` already glob
   `libblastit_php*-*.so`, so the fifth shim is picked up with no count to
   bump; the bundle proof below ships `libblastit_php84-…` and
   `libblastit_php86-…`. `scripts/test_packaging_gates.py`'s pinned
   `--count shims` values moved 12 -> 16 (and 3 -> 4 for gnu/x86_64).
8. **Tests** — `tests/common/libphp_probes.rs` (17-entry oracle, entries 3 and
   16 mutation checks) and the probe tables the three `critical_php_*` suites
   carry as **copies of the boot's `LIBPHP_PROBES`** were extended in lockstep
   with the drift oracle they exist to enforce: the 8.6 block was prepended and
   the pinned count raised 13 -> 17, with no assertion removed or weakened. The
   support-sentence pins moved to `8.3, 8.4, 8.5 and 8.6`, and
   `critical_shim_contract`'s unsupported-minor fixture moved from `8.6.0` to
   `8.7.0` (8.6 is now supported, so `8.6.0` no longer proves the refusal).
   `crates/**` is in this node's `paths`, which is what authorizes those edits.

## Evidence

### 1. `uv run --no-project scripts/platform_matrix.py --check` (16 shims)

```
platform-matrix: ok — 8 wheels, 4 archives, 16 shims, 5 lsp assets declared;
built and exercised: 4/8 wheels, 2/4 archives, 8/16 shims, 1/5 lsp assets;
reaches: served 7 of 16 shims, 1 probed, 0 refused, 8 unbuilt; 14 served once
the aarch64 legs run; 0 leg(s) dropped
```
(`--count shims` -> `16`; `--count shims --libc gnu --arch x86_64` -> `4`.)

### 2. `make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu` (debian:bookworm-slim + Surý)

```
php-sapi-e2e: docker build --build-arg BASE=debian:bookworm-slim --build-arg PHP_MINOR=8.6 -t blastit-php-sapi-e2e:mb425-86-gnu
php-sapi-e2e: readelf -d libblastit_php86-x86_64-gnu.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.6 abi 1: status 200, cpu_ns 13511545 <= wall_ns 14767627, exit()+fatal contained, next request served
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.6 abi 1: status 200, cpu_ns 9223729 <= wall_ns 11908570, exit()+fatal contained, next request served
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.6 abi 1: status 200, cpu_ns 9717380 <= wall_ns 11818043, exit()+fatal contained, next request served
exit=0
```
(Run twice: once before and once after the musl-branch `libclang` fix in the
same Dockerfile, so this log is the final file's gnu branch.)
The image's `make php-sapi PHP_MINORS=8.6` is where the C shim is compiled
against `php8.6-dev`'s headers with `-Wall -Wextra -Werror`.

### 3. The artifact, its `DT_SONAME`, and stage-0 discovery
```
$ scripts/collect-shim.sh blastit-php-sapi-e2e:mb425-86-gnu target/proven-shims --minor 8.6 --libc gnu --arch x86_64
ok   target/proven-shims/libblastit_php86-x86_64-gnu.so: NEEDED libc.so.6; no RPATH/RUNPATH
collect-shim: target/proven-shims/libblastit_php86-x86_64-gnu.so (21696 bytes)

$ readelf -d target/php-sapi/libblastit_php86-x86_64-gnu.so
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x000000000000000e (SONAME)             Library soname: [libblastit_php86-x86_64-gnu.so]
```

### 4. `make rs-test-php-image` on the 8.6 GNU pair

```
symlinked /tmp/blastit-php-backend-7-symlink/libphp8.so -> /usr/lib/libphp8.6.so  served: HTTP/1.1 200 OK
served: HTTP/1.1 200 OK  cpu_ns=19192166  wall_ns=38891244  (libphp /usr/lib/libphp8.6.so)
test result: ok. 9 passed; 0 failed; ...
fatal: status 500  outcome=bailed  ... retired pid 176  respawned pid 177 (ppid 154) uid 0  live=[177]
test result: ok. 29 passed; 0 failed; ...
test result: ok. 8 passed; 0 failed; ...
test result: ok. 1 passed; 0 failed; ...
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
exit=0
```
### 4b. `make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=musl` (alpine:edge + edge/testing)

```
php-sapi-e2e: docker build --build-arg BASE=alpine:edge --build-arg PHP_MINOR=8.6 -t blastit-php-sapi-e2e:mb425-86-musl
php-sapi-e2e: readelf -d libblastit_php86-x86_64-musl.so
  ok  DT_NEEDED: libc.musl-x86_64.so.1 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.6 abi 1: status 200, cpu_ns 10114653 <= wall_ns 12942754, exit()+fatal contained, next request served
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.6 abi 1: status 200, cpu_ns 9162911 <= wall_ns 13128390, ...
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.6 abi 1: status 200, cpu_ns 11708939 <= wall_ns 13047703, ...
exit=0
```
This leg is `reaches: probed`, not `served`: its `suites` is `False` because
`cargo test -p blastit` cannot compile on musl at all (see the first follow-up).

### 5. `make install` ships the fifth shim

```
py-sapi: 2 shim(s) for x86_64-gnu ...
php-sapi: cc -> target/php-sapi/libblastit_php84-x86_64-gnu.so ...
php-sapi: 1 shim(s) for x86_64-gnu ...; skipped: 8.6 8.5 8.3
ok   /tmp/opencode/mb425-prefix/share/blastit: bin/{blastit,blastit-lsp} -> lib/, 2 Python shims, 2 PHP shims, python/blastit/*.py
install: capabilities: static always; python 2 shim(s); php 2 shim(s)
-rwxr-xr-x libblastit_php84-x86_64-gnu.so
-rwxr-xr-x libblastit_php86-x86_64-gnu.so
```
(`BUNDLE_REQUIRE_PHP_SHIMS=2 make install PREFIX=/tmp/opencode/mb425-prefix`,
exit 0. The 8.6 shim is the container-built one staged in `target/php-sapi`;
`make php-sapi` on this host skips 8.6 because the host has no PHP — the
package layer of every format uses the same `libblastit_php*-*.so` glob.)

### 6. `cargo test -p blastit-php-sapi -p blastit-view-php -p blastit`

```
test result: ok. 282 passed; 0 failed; ...   (blastit unit tests)
test result: ok. 9 passed; 0 failed; ...     (critical_php_backend)
test result: ok. 29 passed; 0 failed; ...    (critical_php_embed)
test result: ok. 15 passed; 0 failed; ...    (critical_parallel_views)
... all suites ok
```

### 7. `make fast-check`

`make fast-check` (guard, critical-guard, rs-fmt, rs-lint, rs-doc, rs-test,
py-lint, plan-validate, universal-smoke) exits **0** on the final tree; the
exact tail is pasted in this node's report. Its `rs-test` step is the whole
workspace, `--features python`, including `critical_php_backend` (9),
`critical_php_embed` (29), `critical_parallel_views` (15),
`critical_dev_and_fail_closed`, `critical_handoff` and the 282 `blastit` unit
tests.

## Follow-ups

- **BLOCKER for every musl leg (pre-existing, not PHP-8.6-specific):**
  `crates/blastit/src/cli/handoff.rs:378` calls `libc::execveat`, which
  `libc 0.2.191` does not declare under `target_env = "musl"`:
  `error[E0425]: cannot find function execveat in crate libc`. A
  `cargo test -p blastit` therefore cannot compile on **any** musl image, so
  the 8.3/8.4/8.5 musl legs' `critical_php_*` suites cannot run either. The
  8.6 musl leg is
  declared `suites: False` with that reason (`suites_because`) and proved at
  shim+probe level instead. The fix is a musl `extern "C" { fn execveat(...) }`
  declaration (or a `cfg`-gated fallback) in `crates/blastit/src/cli/handoff.rs`;
  it belongs to the node that owns the handoff, not to the PHP 8.6 slice.
- `docs/**` is out of this node's edit scope (the dispatch forbids it), so these
  pages still say three minors and need `MB.42.4`'s pass:
  `docs/getting-started.md:6`, `docs/deploy/index.md:21`, `docs/index.md:29`,
  `docs/frameworks/index.md:28`, `docs/frameworks/php/index.md:23`,
  `docs/troubleshooting.md:14`, `README.md:17`, `CHANGELOG.md:12,37`,
  `AGENTS.md:20`, `docs/design/frameworks-restructure.md:151,161`,
  `docs/design/parallel-views.review-dx.md:17,21`,
  `docs/design/frameworks-restructure.review-dx.md:9`,
  `docs/design/backend-runtimes.md` (support sentence),
  `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md:31,49`
  (`8.3/8.4/8.5`, `up to 12`) — counts stay inside the matrix, so
  `platform_matrix.py --check` is green regardless.
- `docs/design/universal-distribution.md` §5.1 names `build.rs`'s spelling set
  as `php-config8.6`, `php-config86`; the shipped `CANDIDATES` is the five
  spellings `php-config` + one `php-config8.m` per minor (the `php-config86`
  spelling lives in `make php-sapi`'s per-leg probe, which tries
  `php-config<m.m>` then `php-config<mm>`). Align the sentence when the design
  doc is next touched.
- `alpine:edge` no longer ships `clang19-libclang`; the Dockerfile now resolves
  `clang<ver>-libclang`. The 8.4/8.5 musl legs are still pinned to
  `alpine:3.21`/`3.23`, so the change is version-independent there too, but
  only the 8.6 musl leg was re-run here.
