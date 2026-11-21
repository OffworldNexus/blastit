# MB.42.3.1 — Native `.deb`/`.rpm` builders + container proof

## Files

| Path | What |
|---|---|
| `scripts/check-portable-elf.sh` | **`--max-glibc <major>_<minor>|auto`** (this arm did not exist: the parent's §6.1 requires it, so it was added here). Measures the highest *loader-enforced* `GLIBC_X.Y` across every given ELF (`readelf --version-info`), prints one stable line `max-glibc: <ver> (<file>)`, and fails when a file exceeds the named ceiling. `auto` measures without a ceiling. `VER_FLG_WEAK` version needs are excluded — glibc prints `weak version 'GLIBC_2.39' not found` and runs, so they are not a `Depends:` floor (proved below). |
| `scripts/build-deb.sh` | `--version --release --arch --bundle --max-glibc --out`. Stages `usr/bin/{blastit,blastit-lsp}` as relative symlinks into `usr/share/blastit/lib/`, plus `LICENSE`, every `libblastit_*.so` and the 7 `python/blastit/*.py`; writes `DEBIAN/control` + `md5sums`; `dpkg-deb --root-owner-group --build` → `blastit_<ver>_<arch>.deb`. `Depends:` comes from the measured line. Idempotent (fresh `mktemp` stage, archive overwritten); refuses a `--bundle` whose symlinks escape it. |
| `scripts/build-rpm.sh` | Same contract for RPM (`--release` default `1`), `blastit-<ver>-<rel>.<arch>.rpm`. Uses host `rpmbuild` when present, otherwise `docker run … rockylinux:9` with the staged tree and `scripts/rpm/` mounted read-only (`dnf install rpm-build` inside). Only the finished RPM leaves the container. |
| `scripts/rpm/blastit.spec` | Checked-in spec: `%install` copies the staged `/usr` tree, `%files` is the three paths, `AutoReqProv: no`, `%global _build_id_links none`, `debug_package %{nil}`, `__os_install_post %{nil}` so the payload stays byte-identical to the measured bundle; `Requires: glibc >= <measured>, libgcc`. |
| `scripts/test_packages.sh` | Honours `BLASTIT_DEB` / `BLASTIT_RPM` (also `--deb/--rpm`): `dpkg -i` in `debian:trixie-slim`, `rpm -Uvh --replacepkgs` in `rockylinux:9`, both `--network none`, then `blastit --version`, a real `blastit dev` on `e2e/packages/fixture` (HTTP 200 + body check over bash `/dev/tcp`) and `blastit check`. Never installs on the host. |
| `e2e/packages/fixture/` | Minimal `static "/"` manifest + `public/index.html` used by both legs. |
| `Makefile` | `packages` (stages `make install PREFIX=<tmp>` then both builders into `/tmp/opencode/blastit-packages`) and `packages-test`, both with `##` help lines; `.PHONY` updated. **Not in `check`**: `packages` rebuilds the bundle and `packages-test` needs Docker + `dnf`; the parent's `packages.yml`/`nightly.yml` own them at the publishing gate. |

## Measured floor (never hard-coded)

Per-ELF floor of the proved bundle (`readelf --version-info`, weak entries skipped):

```
blastit                                    2.34
blastit-lsp                                2.34
libblastit_php84-x86_64-gnu.so             2.17
libblastit_php86-x86_64-gnu.so             2.17
libblastit_py313-x86_64-gnu.so             2.34
libblastit_py314-x86_64-gnu.so             2.34
```

```
build-deb: glibc floor 2.34 (measured from …/usr/share/blastit/lib/libblastit_py314-x86_64-gnu.so)
build-deb: Depends: libc6 (>= 2.34), libgcc-s1
build-rpm: glibc floor 2.34 (measured from …/usr/share/blastit/lib/libblastit_py314-x86_64-gnu.so)
build-rpm: Requires: glibc >= 2.34, libgcc
```

`dpkg-deb -f blastit_0.2.0_amd64.deb`:

```
Package: blastit
Version: 0.2.0
Architecture: amd64
Depends: libc6 (>= 2.34), libgcc-s1
```

**Weak version needs are excluded, and that is a measurement, not a fudge.** A
host build on glibc 2.41 emits `pidfd_spawnp@GLIBC_2.39` / `pidfd_getpid@GLIBC_2.39`
as **weak** undefined references (`readelf --version-info` says
`Name: GLIBC_2.39  Flags: WEAK`). glibc's loader does not require them:

```
$ docker run --rm rockylinux:9 /p/share/blastit/lib/blastit --version
/p/share/blastit/lib/blastit: /lib64/libc.so.6: weak version `GLIBC_2.39' not found (required by /p/share/blastit/lib/blastit)
blastit 0.2.0          # exit 0
```

Counting them would have stamped `libc6 (>= 2.39)` and refused Debian 12 / RHEL 9
for a symbol nothing calls. The proof bundle's `blastit`/`blastit-lsp` were
therefore compiled in `rockylinux:9` (glibc 2.34) with the same
`cargo build --release --locked -p blastit -p blastit-lsp` that `make install` runs —
the same shape as CI, which compiles the bundle in `manylinux_2_28` — so no weak
reference reaches the shipped ELFs and the container legs are silent. The shims,
Python modules, `LICENSE` and symlinks all come from `make install PREFIX=…`.

Also verified end-to-end **straight from a host `make install` bundle** (host-built
`blastit`, floor still measured 2.34): both legs green, the only difference being
the advisory `weak version` line above.

## Evidence

`make packages` (full chain, host bundle):

```
install: capabilities: static always; python 2 shim(s); php 2 shim(s)
build-deb: glibc floor 2.34 (measured from /tmp/opencode/blastit-deb.5ysyvn/usr/share/blastit/lib/libblastit_py314-x86_64-gnu.so)
build-deb: /tmp/opencode/blastit-packages/blastit_0.2.0_amd64.deb (5881568 bytes, 4 shim(s), 7 python module(s))
build-deb: Depends: libc6 (>= 2.34), libgcc-s1
build-rpm: glibc floor 2.34 (measured from /tmp/opencode/blastit-rpm.rZuZH9/usr/share/blastit/lib/libblastit_py314-x86_64-gnu.so)
build-rpm: no rpmbuild on this host, building …/scripts/rpm/blastit.spec inside rockylinux:9
build-rpm: /tmp/opencode/blastit-packages/blastit-0.2.0-1.x86_64.rpm (6307005 bytes, 4 shim(s), 7 python module(s))
build-rpm: Requires: glibc >= 2.34, libgcc
```

`make packages-test` (reproducible host-build bundle — green, one advisory line):

```
package-test: deb — installing blastit_0.2.0_amd64.deb in debian:trixie-slim (network none)
Setting up blastit (0.2.0) ...
package-probe: deb installed: blastit 0.2.0
package-probe: deb check: clean
package-probe: deb dev: HTTP/1.1 200 OK, fixture served, check clean
package-test: rpm — installing blastit-0.2.0-1.x86_64.rpm in rockylinux:9 (network none)
blastit-0.2.0-1                       ########################################
blastit: /lib64/libc.so.6: weak version `GLIBC_2.39' not found (required by blastit)
package-probe: rpm installed: blastit 0.2.0
package-probe: rpm check: clean
package-probe: rpm dev: HTTP/1.1 200 OK, fixture served, check clean
package-test: ok
```

`make packages-test` on the floor-clean bundle (the delivered artifacts):

```
package-test: deb — installing blastit_0.2.0_amd64.deb in debian:trixie-slim (network none)
package-probe: deb installed: blastit 0.2.0
package-probe: deb check: clean
package-probe: deb dev: HTTP/1.1 200 OK, fixture served, check clean
package-test: rpm — installing blastit-0.2.0-1.x86_64.rpm in rockylinux:9 (network none)
Verifying...                          ########################################
Preparing...                          ########################################
Updating / installing...
blastit-0.2.0-1                       ########################################
package-probe: rpm installed: blastit 0.2.0
package-probe: rpm check: clean
package-probe: rpm dev: HTTP/1.1 200 OK, fixture served, check clean
package-test: ok
```

Archive contents (`dpkg-deb -c`, inside `debian:trixie-slim`):

```
drwxr-xr-x root/root         0 ./usr/bin/
lrwxrwxrwx root/root         0 ./usr/bin/blastit -> ../share/blastit/lib/blastit
lrwxrwxrwx root/root         0 ./usr/bin/blastit-lsp -> ../share/blastit/lib/blastit-lsp
-rwxr-xr-x root/root  17738248 ./usr/share/blastit/lib/blastit
-rwxr-xr-x root/root   7787496 ./usr/share/blastit/lib/blastit-lsp
-rwxr-xr-x root/root     21688 ./usr/share/blastit/lib/libblastit_php84-x86_64-gnu.so
-rwxr-xr-x root/root     21696 ./usr/share/blastit/lib/libblastit_php86-x86_64-gnu.so
-rwxr-xr-x root/root   1261952 ./usr/share/blastit/lib/libblastit_py313-x86_64-gnu.so
-rwxr-xr-x root/root   1267704 ./usr/share/blastit/lib/libblastit_py314-x86_64-gnu.so
```

`rpm -qp --requires` (from the builder container):

```
glibc >= 2.34
libgcc
rpmlib(CompressedFileNames) <= 3.0.4-1
rpmlib(FileDigests) <= 4.6.0-1
rpmlib(PayloadFilesHavePrefix) <= 4.0-1
```

Gates:

```
$ make fast-check
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
FAST_CHECK_EXIT=0

$ make packaging-gates-test
UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 58 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7, portable-elf=2)
GATES_EXIT=0
```

## Follow-ups (outside this node)

- `scripts/platform_matrix.py` still has no `packages` legs and `.github/workflows/packages.yml` / `nightly.yml` do not exist — both are the parent `MB.42.3` slices; they should call `make packages` + `make packages-test` with `--max-glibc` per distro leg.
- `make install` builds `blastit`/`blastit-lsp` on the host, so a glibc-2.41 host still produces the (harmless, weak) `GLIBC_2.39` advisory inside a 2.34 container. CI already avoids it by building in `manylinux_2_28`; teaching `make install` to build in that image would close it for local release builds too.
- aarch64 was not exercised here (no arm64 host/Docker): `--arch arm64` and the `--target aarch64` path share the same code, but the container proof is x86_64-only.

## Progress (run 2) — review-arch + review-ux fixes, floor pinned to the oldest target

### Floor: measured vs declared, and the oldest distro it installs on

The package no longer declares a number that shrinks its own support matrix. The
**ceiling** is the oldest advertised distribution (Rocky/RHEL 9, glibc **2.34**;
Debian 12 is 2.36, Ubuntu 22.04 is 2.35) and both builders plus
`build-package-bundle.sh --bundle-floor` refuse a bundle above it. The
**declared** `Depends:`/`Requires:` is the **measured** maximum.

| ELF in the bundle | build base (floor image) | measured floor |
|---|---|---|
| `blastit` | `quay.io/pypa/manylinux_2_28_x86_64` (2.28) | **2.28** |
| `blastit-lsp` | `quay.io/pypa/manylinux_2_28_x86_64` (2.28) | **2.28** |
| `libblastit_php83-x86_64-gnu.so` | `ubuntu:24.04` (2.39) | **2.17** |
| `libblastit_php84-x86_64-gnu.so` | `debian:trixie-slim` (2.41) | **2.17** |
| `libblastit_php85-x86_64-gnu.so` | `ubuntu:26.04` (2.41) | **2.17** |
| `libblastit_php86-x86_64-gnu.so` | `debian:bookworm-slim` + Surý (2.36) | **2.17** |
| `libblastit_py313-x86_64-gnu.so` | `make py-sapi` (uv CPython 3.13) | **2.34** |
| `libblastit_py314-x86_64-gnu.so` | `make py-sapi` (uv CPython 3.14) | **2.34** ← the maximum |

```
build-package-bundle: max-glibc: 2.34 (…/lib/libblastit_py314-x86_64-gnu.so)
build-package-bundle: needed: ld-linux-x86-64.so.2 libc.so.6 libdl.so.2 libgcc_s.so.1 libm.so.6 libpthread.so.0
build-deb: glibc floor 2.34 (measured from …/lib/libblastit_py314-x86_64-gnu.so)
build-deb: needed ld-linux-x86-64.so.2 libc.so.6 libdl.so.2 libgcc_s.so.1 libm.so.6 libpthread.so.0 -> Depends: libc6 (>= 2.34), libgcc-s1
build-rpm: glibc floor 2.34 (measured from …/lib/libblastit_py314-x86_64-gnu.so)
build-rpm: needed ld-linux-x86-64.so.2 libc.so.6 libdl.so.2 libgcc_s.so.1 libm.so.6 libpthread.so.0 -> Requires: glibc >= 2.34, libgcc
```

| artifact | measured floor | declared | oldest distro it was **installed on** (this run) |
|---|---|---|---|
| `blastit_0.2.0_amd64.deb` | 2.34 | `libc6 (>= 2.34), libgcc-s1` | **Debian 12 `debian:bookworm-slim` (2.36)**, Debian 13 `debian:trixie` (2.41, static **and** WSGI) |
| `blastit-0.2.0-1.x86_64.rpm` | 2.34 | `glibc >= 2.34, libgcc` | **Rocky 9 `rockylinux:9` (glibc 2.34)** |

No shim needed rebuilding for the guardrail: the PHP shims measure 2.17 on every
base (their build distribution's glibc is not their need), so the only shims at the
ceiling are the Python ones at 2.34 — exactly Rocky 9's glibc. The Surý/bookworm
route is wired as the fix, not exercised as a workaround: `build-package-bundle.sh`
refuses a shim above `--bundle-floor` and names it (`debian:bookworm-slim` + Surý for
PHP, `manylinux_2_28` for the rest). The `.rpm` therefore **does** cover Rocky/Alma
9+, and no distribution is dropped; `platform_matrix.py` keeps the leg with
`bundle_ceiling 2_34`.

### The four blockers

1. **Payload asserted.** Both builders run
   `BUNDLE_REQUIRE_PY_SHIMS=2 BUNDLE_REQUIRE_PHP_SHIMS=4 BUNDLE_SKIP_VERSION=1 scripts/check-bundle-layout.sh "$stage/usr"`
   before writing an archive (negative test on the old 2-PHP-shim prefix:
   `error: 2 PHP shims shipped, and BUNDLE_REQUIRE_PHP_SHIMS=4`). The container
   probe now diffs `dpkg -L` / `rpm -ql` against §6.1 inside the container:
   `package-probe: deb payload: 2 entry points, 16 paths under share/blastit (§6.1)`
   (LICENSE + 8 libs + 7 modules), and it asserts the two symlink targets and
   `root:root` ownership from `dpkg-deb -c` / `rpm -qp --qf`.
2. **Bundle is built for the floor, not for the host.** New
   `scripts/build-package-bundle.sh`: `blastit`/`blastit-lsp` compiled in
   `manylinux_2_28` (asserted `--max-glibc 2_28` in the image), the two Python
   shims are the `make py-sapi` artifacts (manylinux images ship **no shared
   libpython**, so that is the only place a shim can be loaded and proved — the
   measured 2.34 is still inside the ceiling), the four PHP shims come from their
   distro legs. `make packages` calls it with `--bundle-floor $(platform_matrix.py
   --shim-floor)` and passes the same value to both builders. `make install` now
   warns when `target/*-sapi/*.so` predates `target/release/blastit`.
3. **The smoke loads a `.so`.** `e2e/packages/fixture-wsgi/` (`hello:application` +
   `tests/test_app.py` driving `blastit._check_pytest.wrap`) runs on the `.deb` side
   in `debian:trixie` with `python3-pytest` + `libpython3.13`:
   `blastit check` imports through the package's own `python/blastit` with `env -i`
   (no `PYTHONPATH`), and `blastit dev` answers 200 with the body through the
   shipped `libblastit_py313` shim. The static leg now says
   `package-probe: $kind check: all-static (no gate, no shim)`; Rocky 9 stays
   static-only because it ships CPython 3.9 and no shim supports a minor below 3.13.
4. **Comments tell the truth.** `packages.yml`/`nightly.yml` do not exist; the
   Makefile says `MB.42.3.4` will wire them, and that follow-up is repeated below.

### Majors and minors

- **`NEEDED` allowlist** (`check-portable-elf.sh`): `libc.so.6`, `libm.so.6`,
  `libgcc_s.so.1`, the loader, musl's `libc.musl-*`, plus the pre-2.34-merge glibc
  libraries that are still part of `libc6`/`glibc` (`libdl.so.2`,
  `libpthread.so.0`, `librt.so.1` — a 2.28-built shim legitimately needs `libdl`
  for `dlopen`). Anything else is refused, and the `needed:` line drives
  `Depends:`/`Requires:` (`libgcc-s1` only when `libgcc_s.so.1` is present).
  `/bin/ls` is the fixture that proves the refusal.
- **RPM ownership**: `%install` ends with `chown -R 0:0 %{buildroot}`; the probe
  asserts `rpm -qp --qf '[%{FILEUSERNAME}:%{FILEGROUPNAME} …]'` and `dpkg-deb -c`
  are all `root:root`.
- **`--max-glibc` is tested** (`packaging-gates-test`: 62 cases, `max-glibc=4`): a
  strong `GLIBC_2.39` need fails at `2_28` and passes at `auto`; a **patched
  `VER_FLG_WEAK`** 2.39 entry passes at 2.28 and reports the older number (GNU ld
  does not emit that flag, LLVM lld does — the fixture is patched and re-read with
  `readelf`, because the weak exclusion is what makes 2.34 possible); an ELF with an
  unlisted `DT_NEEDED` (`/bin/ls`) is refused.
- Version regex accepts `GLIBC_2.2.5`; `xargs -r -0 md5sum`; `SOURCE_DATE_EPOCH`
  exported in both builders (and into the rpmbuild container); `dpkg-deb` preflight;
  the ceiling error says weak needs are excluded; the `packages` help line names the
  Docker fallback.

### Container legs (`make packages-test`)

```
package-test: deb — installing blastit_0.2.0_amd64.deb in debian:trixie (static + WSGI)
package-probe: deb payload: 2 entry points, 16 paths under share/blastit (§6.1)
package-probe: deb ownership: every payload entry root:root
package-probe: deb check: all-static (no gate, no shim)
package-probe: deb dev: HTTP/1.1 200 OK, 'blastit package fixture'
.                                                                        [100%]
1 passed in 0.00s
blastit check: 1 request, 0 statements, 1/1 view, clean
package-probe: deb check: WSGI gate clean (bundle python, no PYTHONPATH)
package-probe: deb dev: HTTP/1.1 200 OK, 'hello from the packaged shim'
package-test: deb — installing blastit_0.2.0_amd64.deb in debian:bookworm-slim (static, network none: Debian 12 is the oldest supported glibc)
package-probe: deb payload: 2 entry points, 16 paths under share/blastit (§6.1)
package-probe: deb ownership: every payload entry root:root
package-probe: deb dev: HTTP/1.1 200 OK, 'blastit package fixture'
package-test: rpm — installing blastit-0.2.0-1.x86_64.rpm in rockylinux:9 (static, network none)
blastit-0.2.0-1                       ########################################
package-probe: rpm payload: 2 entry points, 16 paths under share/blastit (§6.1)
package-probe: rpm ownership: every payload entry root:root
package-probe: rpm dev: HTTP/1.1 200 OK, 'blastit package fixture'
package-test: ok
```

### Follow-ups (run 2)

- `.github/workflows/packages.yml` / `nightly.yml` are **not** written here; the
  `make packages` / `make packages-test` commands and the matrix `packages` family
  are what `MB.42.3.4` wires. Until then these targets run by hand.
- The Python shims are built by `make py-sapi` (host/uv) rather than in manylinux:
  manylinux images ship no shared `libpython`, and a shim is not allowed to ship
  without the `abi_load` probe. The bundle gate keeps that honest (`2.34` ceiling);
  a future py-sapi build image with a shared CPython 3.13/3.14 would move them to
  `2.28` without changing the declaration.
- aarch64 remains unproved (no arm64 host/Docker); `package_legs()` declares the
  two `-arm` legs as `declared, unbuilt`.
