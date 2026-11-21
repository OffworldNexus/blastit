---
id: MB.42.1
title: "Write ADR-0017 & technical design for universal libpython-free binary, local CLI handoff, .deb/.rpm + install.sh, py3-none wheels & Composer wrapper"
kind: task
status: done
owner: architect
priority: 1
depends_on: []
paths: ["docs/decisions/**", "docs/design/**", ARCHITECTURE.md]
attempts: 1
max_attempts: 3
escalation: architect
created: "2026-11-19T14:30"
updated: "2026-11-19T17:17"
---

# Goal
Author `docs/decisions/ADR-0017-universal-distribution.md` (status: `Accepted`) and a concise `<250`-line technical blueprint `docs/design/universal-distribution.md` specifying the exact interfaces for `MB.42.2`–`MB.42.4`.

# Exact Scope to Specify
1. **Universal `blastit` Binary (`DT_NEEDED`: `libc`/`libm`/`libgcc_s` only — zero `libpython`, zero `libphp`):**
   - Mirror `ADR-0010`'s PHP SAPI shim pattern for Python: compile per-minor `libblastit_py3{13,14}-<arch>-<libc>.so` `cdylib` shims (wrapping `blastit-view-python` + `PyOS_BeforeFork`/`AfterFork` + `immortal` + `PyCode::compile` behind a small C-ABI vtable, linked with undefined dynamic `Py*` symbols resolved when stage 1 `dlopen(..., RTLD_NOW | RTLD_GLOBAL)`s the pinned `/proc/self/fd/<libpython_fd>` followed by `dlopen("/proc/self/fd/<py_shim_fd>", RTLD_NOW | RTLD_LOCAL)`).
   - Stage 0 discovers Python (`BLASTIT_PYTHON` -> `$VIRTUAL_ENV` in `dev`/`check` -> `<manifest_dir>/.venv` -> `python3` on `PATH`), resolves its `libpython3.{13,14}.so`, `PYTHONHOME`, and `site-packages`, verifies provenance/ownership in production (just like `host.rs` + `visibility.rs` + `ADR-0010`), and passes pinned `(libpython_fd, py_shim_fd)` into stage 1 **only** when `entrypoint python` (or `BLASTIT_APP`) is active!
   - Remove the `python` Cargo feature split on `crates/blastit` (or make `python` pure-Rust discovery/loader with zero `pyo3` link dependency on `blastit`, on by default alongside `php` and `static`!). Every shipped `blastit` binary supports Python, PHP, and Static out of the box.
2. **Safe Local CLI Handoff (`dev`, `check`, `test`, `fmt`, `lsp` only — NEVER production `blastit`, `--stage=1`, or `--role=dbproxy`!):**
   - Resolution order from `cwd` (only when `BLASTIT_NO_LOCAL_HANDOFF` is unset and `BLASTIT_HANDOFF_DONE` is unset):
     1) `$VIRTUAL_ENV/bin/blastit` (if `$VIRTUAL_ENV` set), else `./.venv/bin/blastit`
     2) `./vendor/bin/blastit`
     3) `./node_modules/.bin/blastit`
   - Safety gates before `execv`: target must be a regular executable file, its `(st_dev, st_ino)` must differ from `/proc/self/exe`, its owner `uid` must equal `geteuid()` or `0`, and neither the binary nor its parent directory may be group/other-writable (`mode & 0o022 == 0`, preventing `/tmp` hijack!). Set `BLASTIT_HANDOFF_DONE=1` before `execv`.
3. **Packaging & `install.sh` (`docs/install.sh` served at `https://offworldnexus.github.io/blastit/install.sh`):**
   - Native `.deb` (`blastit_<ver>_amd64.deb`, `blastit_<ver>_arm64.deb`) and `.rpm` (`blastit-<ver>-1.x86_64.rpm`, `blastit-<ver>-1.aarch64.rpm`) installing into `/usr/bin/{blastit,blastit-lsp}` -> `/usr/share/blastit/{lib,python}`, plus `.tar.gz` (`x86_64-gnu`, `aarch64-gnu`, `x86_64-musl`, `aarch64-musl`).
   - `install.sh` (POSIX `sh`, `SHA256SUMS`-verified):
     * On Debian/Ubuntu (`dpkg` present): attempts `.deb` install (`dpkg -i` when `uid == 0`, or `sudo dpkg -i` when `sudo -n` works or interactive TTY user accepts / `sudo` succeeds; if `--user` is passed or `sudo` is unavailable/declined, falls back cleanly to `~/.local` tarball install!).
     * On Fedora/RHEL/Rocky/Alma (`rpm` present): attempts `.rpm` install (`rpm -Uvh --replacepkgs` via root/`sudo`, with the same clean `--user` / unprivileged fallback to `~/.local`).
     * On Alpine / other Linux (`musl` or no `dpkg`/`rpm`): installs the matching `.tar.gz` into `/usr/local` (if root) or `~/.local`.
4. **Ecosystem Lockfile Wrappers:**
   - **PyPI:** 4 universal `py3-none-manylinux_2_28_{x86_64,aarch64}.whl` and `py3-none-musllinux_1_2_{x86_64,aarch64}.whl` wheels carrying the universal binary + PHP & Python shims + pure-Python `blastit` modules (plus keep `blastit._blastit` pure-Rust helper or CLI subcommands for `fold.py` / `_check_pytest.py` so zero per-CPython-minor wheel matrix is needed!).
   - **Composer (`packages/composer-blastit/`):** `offworldnexus/blastit` Composer package (`composer.json` with `"bin": ["bin/blastit"]`) whose `bin/blastit` PHP/sh wrapper downloads & verifies the exact version's `blastit-<arch>-<libc>.tar.gz` against an embedded SHA-256 table into `vendor/offworldnexus/blastit/.runtime/` (honoring `BLASTIT_RELEASE_BASE_URL` / local cache for offline CI testing!) and `exec`s `blastit` with `BLASTIT_HANDOFF_DONE=1`.
5. **Hero DX:** Landing page & README hero becomes 3 universal lines (`curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh` -> `blastit dev` -> `blastit check`), and all docs drop `uv run` prefixes on `blastit` commands while offering a compact "Lockfile install (`uv` / `composer`)" tab/box where appropriate.

# Acceptance
- `docs/decisions/ADR-0017-universal-distribution.md` and `docs/design/universal-distribution.md` written and reviewed by `review-arch` + `review-ux`
- `uv run --no-project tools/plan.py validate`
