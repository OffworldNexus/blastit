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
