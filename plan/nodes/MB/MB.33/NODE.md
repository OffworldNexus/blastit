---
id: MB.33
title: "Resolve blastit-lsp without uv on Linux, macOS and Windows: Zed LspSettings -> which(blastit-lsp) -> GitHub Release download, with the exact [[capabilities]] and a guard that fails on extras"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.35]
paths:
  - editors/zed/src/lib.rs
  - editors/zed/extension.toml
  - editors/zed/README.md
  - scripts/check_zed_extension.py
  - scripts/test_check_zed_extension.py
  - docs/local.md
  - docs/reference/cli.md
  - "docs/design/blastfile-lsp-zed.md"
  - "docs/decisions/ADR-0013-blastfile-lsp-and-zed.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:57"
updated: "2026-11-17T13:18"
---

# Goal

The Zed extension stops routing through `uv`, stops assuming Linux, and resolves the **portable**
language server `MB.35` now publishes. A developer with Zed and **no `uv`**, no PyPI package and
no checkout gets colours, SQL diagnostics, Quick Fixes, hover and format-on-save on Linux,
macOS and Windows; a developer who owns a binary can point the editor at that exact file.

Owner mandate (`0.2.0`, 2026-11-17): *"is it gonna get the LSP from uv? because that's super
wonky? especially the package isn't published yet so…"* — plus the cross-platform half: an
editor experience on macOS and Windows even though serving stays Linux-only (`MB.35`).

**Resolution order — exactly this, in this order, nothing else:**

1. **Settings.** `zed::settings::LspSettings::for_worktree("blastit-lsp", worktree)` first:
   `binary.path` wins (`target/release/blastit-lsp`, a tarball, any absolute path),
   `binary.arguments` defaults to `[]` when unset (the portable server takes no arguments; a
   reader who points `path` at the Linux `blastit` binary adds `"arguments": ["lsp"]`, and the
   README says so in one line), and `binary.env` is merged **over** `worktree.shell_env()`. The
   returned `zed::Command` carries `env = shell_env + binary.env`.
2. **`$PATH`.** `worktree.which("blastit-lsp")` → run it with no arguments. Otherwise
   `worktree.which("blastit")` → run `["lsp"]`. Those two names cover the portable archive on
   every platform, `~/.local/bin`, the wheel's console script and `uv tool install blastit` —
   with **zero** `uv run` wrapper and no project sync.
3. **Download once.** `zed::current_platform()` selects one asset from
   `zed::latest_github_release("OffworldNexus/blastit", GithubReleaseOptions { require_assets:
   true, pre_release: false })`:

   | Platform | Asset | Type | Executable |
   |---|---|---|---|
   | `(Os::Linux, Architecture::X8664)` | `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz` | `GzipTar` | `<workdir>/blastit-lsp-<version>/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp`, `make_file_executable` |
   | `(Os::Linux, Architecture::Aarch64)` | `blastit-lsp-aarch64-unknown-linux-gnu.tar.gz` | `GzipTar` | same shape, aarch64 |
   | `(Os::Mac, Architecture::Aarch64)` | `blastit-lsp-aarch64-apple-darwin.tar.gz` | `GzipTar` | same shape, `make_file_executable` |
   | `(Os::Mac, Architecture::X8664)` | `blastit-lsp-x86_64-apple-darwin.tar.gz` | `GzipTar` | same shape, `make_file_executable` |
   | `(Os::Windows, Architecture::X8664)` | `blastit-lsp-x86_64-pc-windows-msvc.zip` | `Zip` | `<workdir>/blastit-lsp-<version>/blastit-lsp-x86_64-pc-windows-msvc/blastit-lsp.exe`, **no** `make_file_executable` |

   `zed::download_file(&asset.download_url, "<workdir>/blastit-lsp-<version>", <type>)` extracts
   into that directory (the API's own words: *"The file will be extracted according to the given
   file type"*). Report through `zed::set_language_server_installation_status(id,
   Downloading)` then `Failed(msg)` on error. A cached binary (`std::fs::metadata` on the
   versioned path) is a hit: the second start does no network I/O.
4. **Anything else.** `(Linux, Architecture::X86)` and any dropped target from `MB.35.1`'s
   verdict → one crisp error naming the supported targets and
   `lsp.blastit-lsp.binary.path`.

`uv` disappears from the editor path in `editors/zed/src/lib.rs`, `editors/zed/README.md`,
`docs/local.md` `## Editors`, `docs/reference/cli.md` `## blastit lsp`, and
`scripts/check_zed_extension.py`. The Python install path (`uv add blastit`, `uv run blastit
dev`) is **not** touched — it is how the package installs, and it was never the complaint.

# Context

- **`MB.35` already owns the design and the artifacts.** `ADR-0013`'s amendment and design §8.3
  were rewritten there (the single-binary rejection is reversed, with the reason); the five
  assets are built and released there; `blastit-lsp` is on `PATH` for anyone who installed it.
  This leaf implements the extension's side and makes the pages' *extension* sentences true. If
  the design's table and this brief disagree, the design wins and this brief is corrected.
- **Why this is a defect, not a preference.** `uv` is not on a working developer's `PATH` in most
  editor setups, the package is not on PyPI yet, and `uv run` inside an editor is two hops of
  resolution for one binary — the "wonky" the owner reported. The `--frozen --no-sync` argv
  (design §8.3, ADR-0013 §6) was a safety patch over a resolution step that should not exist.
- **The artifacts and their layout.** `scripts/build-lsp.sh` (MB.35.5) stages
  `blastit-lsp-<target>/blastit-lsp(.exe)` and archives it; the release job uploads every
  `blastit-lsp-*` asset. The download path above is that layout — verify it against the CI job
  before coding, and if the layout differs, fix this table and the guard together.
- **Zed 2026 gates the download with a capability manifest.** Extensions may only call
  `zed_extension_api::download_file` for hosts listed in `extension.toml`
  (`[[capabilities]] kind = "download_file" host = … path = […]`; schema:
  `crates/extension/src/extension_manifest.rs` `ExtensionCapability`, host function
  `CapabilityGranter::grant_download_file`). `process:exec` gates
  `zed_extension_api::process::Command` — the *extension* spawning a process — not the
  `zed::Command` that `language_server_command` **returns** for the host to spawn. `MB.35.1`
  verified both facts and the redirect hosts; declare **exactly** the capabilities the code can
  reach (github.com and the redirect host it follows) and record the verified answer in the
  design §8.3 row. An unused permission is a wider prompt than the extension needs, a missing
  one makes every download fail with `capability for download_file … is not granted by the
  extension host`.
- **The guard is the proof.** `scripts/check_zed_extension.py` (run by `make zed-check`, inside
  `make check`) asserts 42 properties today, including `lib: which order`
  (`which("blastit")` before `which("uv")`), `lib: argv` (the six `uv` literals), `lib: error
  string` (`uv add blastit` / `uv sync`) and `lib: no capabilities`. Each becomes a property
  about the new order: `lib: which blastit-lsp` (first), `lib: which blastit lsp` (second,
  argv `["lsp"]`), `lib: no uv`, `extension: capabilities` (the exact host set), `lib: asset
  map` (the five rows above, by literal name and type), `lib: zip windows` (`Zip` only on
  `Os::Windows`, `GzipTar` otherwise), `lib: executable` (`make_file_executable` on non-Windows
  only), `lib: cache` (the versioned per-target path), `lib: status` (`Downloading`/`Failed`),
  `lib: platform` (the exact `current_platform` match arms). `scripts/test_check_zed_extension.py`
  asserts the property **names** exactly and `len(properties)`, so the tuple, the count and one
  negative twin per new property move together.
- **DX-first order inside this leaf.** (a) Write the page sentences (`docs/local.md`,
  `docs/reference/cli.md`, `editors/zed/README.md`) so the documented behaviour is fixed first;
  (b) the design/ADR rows were amended by `MB.35.3` — only update a row if the code proves it
  wrong, as a dated amendment, never by rewriting the accepted record; (c) change
  `lib.rs`/`extension.toml` so the pages become true; (d) teach the guard and its negative
  twins. No new crate dependency: `zed_extension_api 0.7.0` already exposes
  `settings::{LspSettings, CommandSettings}`, `current_platform`, `latest_github_release`,
  `download_file`, `make_file_executable` and `set_language_server_installation_status`, so
  `Cargo.toml`/`Cargo.lock` stay untouched.
- **User page shape to keep** (`docs/local.md` step 2, one or two sentences, **no new fence** —
  `MB.32.1`/`MB.35.2` pin the docs' block inventory and `check_settings` requires the existing
  `json` fence to keep `format_on_save: "on"`): *"The extension finds the server itself:
  `blastit-lsp` on your `PATH` (release archive, `~/.local/bin`, or the wheel's `blastit lsp`),
  then downloads the release binary for your platform once. Override with
  `lsp.blastit-lsp.binary.path` in `.zed/settings.json`."*

# Acceptance

Run from the repository root. (`PATH` keeps `uv`/`cargo` reachable; nothing here needs the
network except where noted.)

1. **The guard and its negative twins are green, at the new property count.**
   ```console
   $ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/check_zed_extension.py
   zed-check: OK (N properties checked)
   $ UV_OFFLINE=1 PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/test_check_zed_extension.py
   OK
   ```
   Keep `lib: no process` and `shell_env()` exactly once, and keep `check_settings`'s
   `format_on_save: "on"` assertion.

2. **`uv` is gone from the editor path, everywhere it was an instruction.**
   ```console
   $ ! git grep -n 'which("uv")\|uv run --frozen\|--no-sync' -- editors/zed scripts/check_zed_extension.py
   $ ! git grep -n 'which("uv")' -- docs/design docs/decisions
   $ ! git grep -n 'uv run blastit lsp\|uv add blastit' -- docs/local.md docs/reference/cli.md editors/zed/README.md
   ```
   (Dated record rows in `docs/design/blastfile-lsp-zed.md` and `ADR-0013` keep the old argv
   inside the amendment/history; they must not read as an instruction.)

3. **The wasm builds clean.**
   ```console
   $ cd editors/zed && cargo build --target wasm32-wasip2 --release 2>&1 | tail -2
   Finished `release` profile [optimized] target(s) in …s
   $ cargo build --target wasm32-wasip2 --release 2>&1 | grep -c '^warning'
   0
   ```

4. **The pages and the site still pass.**
   ```console
   $ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project python -m pytest python/tests/test_docs_assets.py -q
   … passed
   $ make doc-tone docs-check docs-test
   ```
   Both targets exit 0 (paste the tails), and every command `MB.35.2` documented is unchanged
   or corrected in the same commit.

5. **The whole gate.** `make check` (it runs `zed-check`, `docs-test`, `docs-check`,
   `plan-validate`, `lsp-portable`) ends green; paste the tails into `RESULT.md`.

6. **Live proof on every platform that can be recorded.**
   - **Linux** (gated): uninstall/rename `uv` on the `PATH` (or run Zed with a scrubbed env),
     `zed: install dev extension` on `editors/zed`, open `./Blastfile`; paste the installation
     status line, one SQL squiggle with its exact `line:col`, and the Quick Fix applied.
   - **macOS and Windows** (recorded if a machine is available, otherwise `NOT RUN` with the
     reason): the same three lines plus which resolution step supplied the server
     (`which blastit-lsp`, `which blastit lsp`, or the download), and the asset name the
     download picked.

# Notes

- **Download hosts:** the release asset's browser URL is `github.com`, and the host follows a
  redirect to `objects.githubusercontent.com`. Declare the hosts the code can actually reach
  (both, unless `MB.35.1` proves the check runs only on the pre-redirect URL) and make the guard
  fail on any host outside that set and on any `npm:install` entry.
- **Keep the resolution in one small function per step** so the guard's static properties stay
  literal and reviewable; no `unwrap`/`expect` on the download path, and every failure becomes
  `LanguageServerInstallationStatus::Failed(<one sentence>)`.
- **Windows specifics:** the `Zip` archive extracts a directory containing `blastit-lsp.exe`;
  `make_file_executable` is a no-op-or-error there, so call it only outside `Os::Windows`.
  `which("blastit-lsp")` must be tried before `which("blastit")` because the portable binary is
  the better answer when both exist.
- Do not touch `editors/zed/Cargo.toml`, `editors/zed/Cargo.lock`, the grammar, the queries,
  `.zed/settings.json`, or any protected test (`crates/*/tests/critical_*.rs`,
  `python/tests/critical/`).
- The `editors/zed` wasm build shares the cargo registry lock with the main workspace: run step
  3 when no other node is compiling.
- Record the capability finding (schema file, enum variant, host function) and the platform
  table in `RESULT.md`; a reversal without the verified citation is the next regression.
