# MB.42 Gate — Universal distribution, PHP 8.3–8.6 & zero-decision install

**Result: PASS.**

## What shipped
One universal `blastit` binary (`DT_NEEDED`: `libc`/`libm`/`libgcc_s`/ld only — zero `libpython`, zero `libphp`, zero `RUNPATH`) that serves Python WSGI (3.13, 3.14), embedded PHP (8.3, 8.4, 8.5, 8.6), static mounts, `dbproxy` and `lsp`; delivered as signed APT/RPM repositories (Cloudsmith primary, Forgejo mirror, `main`/`nightly`), a one-command `install.sh`, four universal `py3-none` wheels, a Composer wrapper, safe local CLI handoff, and a 3-line hero.

## Gate evidence (this host, committed tree)
- `make check` → **exit 0**, 169 `test result: ok` suites, 0 failures. Includes `e2e-universal` (6/6 legs), `zed-check` (59 properties), `plan`.
- `make rs-test-php-image` → pass, incl. the `PR_SET_DUMPABLE(0)` witness (2 roles denied `EACCES` to uid 1000).
- `e2e/run.py` (`Dockerfile.prebuilt`) → **exit 0, 46/46 `ok`** with `E2E_PORT`/`E2E_SQLALCHEMY_PORT`/`E2E_STATIC_PORT` moved to free ports (21080/21081/21095).
- `visual-checker` → **PASS 7/7** on the new hero, Getting Started and Install pages (light/dark/mobile 390×844, 298 links, 0 broken).

## One leg that cannot run here (environment, not code)
`e2e/rootless/run.py --local` needs `CAP_SETFCAP`; this sandbox denies it:
```
$ setcap cap_setuid+ep /tmp/opencode/captest
unable to set CAP_SETFCAP effective capability: Operation not permitted
```
Also, `make e2e` with **default** ports fails on this host because a *foreign* container (`n2f-vuln`) holds `0.0.0.0:18081` and `18443`; the harness's own ports were moved rather than stopping someone else's container. Both facts are environmental and are recorded, not papered over.

## Acceptance from the epic
- `make check` ✅ (above)
- Container smoke proofs: `.deb` in `debian:trixie` + `debian:bookworm-slim`, `.rpm` in `rockylinux:9`, `install.sh` (`--user` + `BLASTIT_DEB`/`BLASTIT_RPM`), universal wheel on CPython 3.12/3.13/3.14 (slim **and** alpine), Composer wrapper in `composer:2` ✅
- Cloudsmith/Forgejo publish steps validated in **dry-run** only (no real publish, no token printed) ✅

## Two honest scope notes for the release record
1. **Packages require glibc 2.34+** — Debian 12+, Ubuntu 22.04+, RHEL/Rocky/Alma 9+. Measured (`libblastit_py314-x86_64-gnu.so`), not assumed; all four PHP shims measure 2.17, so no distro was dropped and no shim was rebuilt.
2. **A musllinux wheel needs `apk add libgcc`** on the host (`_Unwind_*`); documented as a one-line warning.

## Nodes
`MB.42.1` ADR-0017 + design · `MB.42.2.0` §1.2.1 fork/zygote invariants · `.2.1.1`–`.2.1.4` Python SAPI/shims/stage-0/stage-1 · `.2.2.1` `blastit-audit` · `.2.2.2` `--internal-json` + `_blastit.py`, `blastit-py` retired · `.2.3` safe handoff · `.2.4` bundle + 4-runtime E2E · `MB.42.5` PHP 8.6 · `MB.42.6` check fix-text · `MB.42.7` fork-safe tests · `MB.42.9` ambient-venv skip · `MB.42.10` musl `execveat` · `MB.42.11` packaged `check` · `MB.42.12` `blastit-lsp` argv · `MB.42.3.1`–`.3.4` packaging · `MB.42.4` hero + docs · `MB.42.13`/`.14`/`.15` gate fixes.
