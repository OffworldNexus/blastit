---
id: MB.40.8
title: "Add make install / uninstall (~/.local/share/blastit bundle + symlinks) with embedded PYTHONHOME & .venv auto-discovery"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [Makefile, "crates/blastit/**", "python/blastit/**", "python/tests/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T10:35"
updated: "2026-11-19T11:02"
---

# Goal
Add `make install` and `make uninstall` (`PREFIX ?= $(HOME)/.local`) installing a self-contained, all-features release bundle (`blastit` with `python,php,static`, `blastit-lsp`, `libblastit_php*.so` shims, and bundled `blastit` Python check plugin) under `$(PREFIX)/share/blastit/` with symlinks in `$(PREFIX)/bin/{blastit,blastit-lsp}`, plus automatic embedded `PYTHONHOME` and `.venv` discovery so bare `blastit dev` / `blastit check` works out-of-the-box across Static, PHP, Python `.venv` projects, and Zed.

**HARD RULE FROM OWNER:** Do NOT run bare `make install` into `~/.local` on this machine! Test strictly with `make install PREFIX=/tmp/opencode/test-install` and `make uninstall PREFIX=/tmp/opencode/test-install`!

# Exact Targets (`file:line` pre-diagnosed)
1. **`crates/blastit/build.rs` (gated strictly on `CARGO_FEATURE_PYTHON`!):**
   - When `CARGO_FEATURE_PYTHON` is set and `PYO3_PYTHON` resolves to `<prefix>/bin/python*` holding `libpython<mm>.so` under `<prefix>/lib` or `lib64`, emit:
     - `cargo:rustc-env=BLASTIT_PYTHON_HOME=<prefix>`
     - `cargo:rustc-env=BLASTIT_PYTHON_MM=<major>.<minor>` (parsed from `libpython3.14.so` -> `3.14`).
   - When `CARGO_FEATURE_PYTHON` is NOT set (standalone archive), emit neither so `make rs-no-python` / standalone archives remain 100% free of build-host paths!
2. **Embedded `PYTHONHOME` & `.venv` auto-discovery (`crates/blastit/src/stage0/`, `crates/blastit/src/stage1/`, `crates/blastit/src/config/`):**
   - Across stage 0 -> stage 1 `execve` (`#[cfg(feature = "python")]`): when `host.install() == Install::Binary` and `PYTHONHOME` is not set in the caller's environment, if `option_env!("BLASTIT_PYTHON_HOME")` names an existing directory on disk, include `PYTHONHOME=<prefix>` in stage 1's `execve` `envp` (avoiding `unsafe std::env::set_var` in Rust 2024 while ensuring embedded `Python::initialize()` always finds `encodings` / stdlib even under `env -i`!).
   - When `host.install() == Install::Binary` and the manifest uses `entrypoint python`:
     - Discover active `$VIRTUAL_ENV` (if set and directory) else `<manifest_dir>/.venv` (if directory).
     - If a `.venv` is found:
       - If `<venv>/lib/python<MM>/site-packages` (or `lib64`) exists matching `BLASTIT_PYTHON_MM` (when known), push that `site-packages` directory onto `config.sys_path` (if not already present) and record `<venv>` so the `INFO runtime python` log line names the discovered `.venv`!
       - If `<venv>/lib/python<other>/site-packages` exists for a *different* CPython minor than `BLASTIT_PYTHON_MM`, fail early at stage 0 with a crisp 1-line actionable diagnostic naming both versions and the two fixes (`uv venv --python <MM>` or `uv add blastit && uv run blastit dev`).
3. **`blastit check` with `Host::binary()` (`crates/blastit/src/check.rs`, `python/blastit/_check_pytest.py`):**
   - In `check.rs::python_program`: check `BLASTIT_PYTHON` first, then `Install::Wheel`, then `$VIRTUAL_ENV/bin/python3` (or `python`), then `<manifest_dir>/.venv/bin/python3` (or `python`), then `<BLASTIT_PYTHON_HOME>/bin/python3` (if `pytest` importable or file exists), then `PATH`.
   - In `check.rs::run_pytest`: when `host.install() == Install::Binary`, check if `<resolved_current_exe_dir>/../python/blastit/_check_pytest.py` exists (staged by `make install`!). If it exists, prepend/append `<resolved_current_exe_dir>/../python` to `PYTHONPATH` on the spawned `pytest` command so `-p blastit._check_pytest` works even when `blastit` is NOT installed inside the app's `.venv`!
   - Check `python/blastit/_check_pytest.py` (and `python/blastit/__init__.py` if imported when loading `blastit._check_pytest`): ensure importing `blastit._check_pytest` under a Python `.venv` whose CPython minor differs from `_blastit.*.so` falls back cleanly in pure Python for `probe_ingress` (JSON/form/raw body inspection is trivial in stdlib `json`/`urllib.parse`!) instead of crashing on `ImportError: cannot import name '_blastit'`!
4. **`Makefile`: `install` and `uninstall` targets (`PREFIX ?= $(HOME)/.local`):**
   - `PREFIX ?= $(HOME)/.local`
   - `INSTALL_BINDIR := $(PREFIX)/bin`
   - `INSTALL_LIBDIR := $(PREFIX)/share/blastit/lib`
   - `INSTALL_PYDIR  := $(PREFIX)/share/blastit/python`
   - `install`:
     1. If `[ "$(PREFIX)" = "$(HOME)/.local" ] && [ -L "$(INSTALL_BINDIR)/blastit" ]` and `readlink "$(INSTALL_BINDIR)/blastit"` contains `/uv/tools/blastit`, run `$(UV) tool uninstall blastit || true`.
     2. Build `--release --locked`:
        - `$(RUST_ENV) $(CARGO) build --release --locked -p blastit --features python,php,static -p blastit-lsp -p blastit-py -p blastit-dbproxy`
        - `$(MAKE) php-sapi`
     3. Stage cleanly (`rm -rf "$(PREFIX)/share/blastit"`, `mkdir -p "$(INSTALL_LIBDIR)" "$(INSTALL_PYDIR)/blastit/_bin" "$(INSTALL_BINDIR)"`):
        - Copy `target/release/blastit` and `target/release/blastit-lsp` (`0755`) + any `target/php-sapi/libblastit_php*.so` (`0755`) into `$(INSTALL_LIBDIR)/`.
        - Copy `python/blastit/*.py` into `$(INSTALL_PYDIR)/blastit/`, `target/release/lib_blastit.so` as `$(INSTALL_PYDIR)/blastit/_blastit<EXT_SUFFIX>`, and `target/release/blastit-dbproxy` into `$(INSTALL_PYDIR)/blastit/_bin/blastit-dbproxy` (`0755`).
        - Symlink `ln -sfn "$(INSTALL_LIBDIR)/blastit" "$(INSTALL_BINDIR)/blastit"` and `ln -sfn "$(INSTALL_LIBDIR)/blastit-lsp" "$(INSTALL_BINDIR)/blastit-lsp"`.
     4. Verify `"$(INSTALL_BINDIR)/blastit" --version` and `"$(INSTALL_BINDIR)/blastit-lsp" --help` (or binary presence) and print concise installed paths + capabilities summary.
   - `uninstall`: remove `$(INSTALL_BINDIR)/blastit`, `$(INSTALL_BINDIR)/blastit-lsp`, and `$(PREFIX)/share/blastit`.

# Acceptance
- `make install PREFIX=/tmp/opencode/test-install` (NEVER run without `PREFIX=/tmp/opencode/test-install`!)
- Test `/tmp/opencode/test-install/bin/blastit --version` under `env -i PATH=/usr/bin:/bin`, plus smoke-test `dev` & `check` on a throwaway `.venv` Python app and `make uninstall PREFIX=/tmp/opencode/test-install`
- `make fast-check`
- `make rs-no-python workflow-check packaging-gates-test docs-unit`
