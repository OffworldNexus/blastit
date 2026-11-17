---
id: MB.33
title: "Resolve blastit lsp without uv: Zed LspSettings -> which(blastit) -> GitHub Release download, with the exact [[capabilities]] the host grants and a guard that fails on extras"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.32]
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
updated: "2026-11-17T12:57"
---

# Goal

The Zed extension stops routing through `uv`. `editors/zed/src/lib.rs` resolves `blastit lsp`
the way shipping Zed language extensions do — `LspSettings` → `which("blastit")` → one
GitHub-Release download — so a developer with Zed and **no `uv`**, no published PyPI package and
no checkout still gets colours, diagnostics and format-on-save, and a developer who owns
`target/release/blastit` can point the editor at that exact file.

Owner mandate (`0.2.0`, 2026-11-17): *"is it gonna get the LSP from uv? because that's super
wonky? especially the package isn't published yet so…"*

**Resolution order — exactly this, in this order, nothing else:**

1. **Settings.** `zed::settings::LspSettings::for_worktree("blastit-lsp", worktree)` first:
   `binary.path` wins (`target/release/blastit`, a tarball, any absolute path), `binary.arguments`
   defaults to `["lsp"]` when unset, and `binary.env` is merged **over** `worktree.shell_env()`.
   The returned `zed::Command` carries `env = shell_env + binary.env`.
2. **`$PATH`.** `worktree.which("blastit")` → run `<blastit> lsp` directly. That one name already
   covers the standalone tarball, `~/.local/bin/blastit`, the wheel's console script and
   `uv tool install blastit` — with **zero** `uv run` wrapper and no project sync.
3. **Download once.** `zed::current_platform()`; on `(Os::Linux, Architecture::X8664 | Aarch64)`
   take the asset `blastit-x86_64-unknown-linux-gnu.tar.gz` / `blastit-aarch64-unknown-linux-gnu.tar.gz`
   from `zed::latest_github_release("OffworldNexus/blastit", zed::GithubReleaseOptions { require_assets: true, pre_release: false })`,
   `zed::download_file(&asset.download_url, "<workdir>/blastit-<version>", zed::DownloadedFileType::GzipTar)`,
   `zed::make_file_executable("<workdir>/blastit-<version>/blastit-<target>/blastit")`, and report
   through `zed::set_language_server_installation_status(id, Downloading)` then `Failed(msg)`.
   A cached binary (`std::fs::metadata` on the versioned path) is a hit: the second start does no
   network I/O.
4. **Anything else.** `(Mac, …)`, `(Windows, …)` or `(Linux, Architecture::X86)` → one crisp error
   that Blastit targets Linux x86_64/aarch64, naming `lsp.blastit-lsp.binary.path`.

`uv` disappears from the editor path in `editors/zed/src/lib.rs`, `editors/zed/README.md`,
`docs/local.md` `## Editors`, `docs/reference/cli.md` `## blastit lsp`,
`docs/design/blastfile-lsp-zed.md`, `ADR-0013` and `scripts/check_zed_extension.py`. The Python
install path (`uv add blastit`, `uv run blastit dev`) is **not** touched — it is how the package
installs, and it was never the complaint.

# Context

- **Why this is a defect, not a preference.** `uv` is not on a working developer's `PATH` in most
  editor setups, the package is not on PyPI yet, and `uv run` inside an editor is two hops of
  resolution for one binary — the "wonky" the owner reported. The `--frozen --no-sync` argv
  (design §8.3, ADR-0013 §6) was a safety patch over a resolution step that should not exist.
- **The release artifact already exists and is named for this.** `scripts/build-standalone.sh`
  (`name="blastit-$target"`, `tar -C "$stage" -czf "$out/$name.tar.gz" "$name"`) produces
  `blastit-<target>.tar.gz` whose root directory is `blastit-<target>/` holding `blastit`;
  `.github/workflows/publish-pypi.yml` uploads every `blastit-*.tar.gz` to the GitHub Release
  (`gh release create "$RELEASE_TAG" dist/*`). The download path in the Goal above is that layout.
- **Zed 2026 gates the download with a capability manifest.** Extensions may only call
  `zed_extension_api::download_file` for hosts listed in `extension.toml`
  (`[[capabilities]] kind = "download_file" host = … path = […]`; schema:
  `crates/extension/src/extension_manifest.rs` `ExtensionCapability`, host function
  `CapabilityGranter::grant_download_file`). `process:exec` gates
  `zed_extension_api::process::Command` — the *extension* spawning a process — not the
  `zed::Command` that `language_server_command` **returns** for the host to spawn. Verify both
  facts against the schema (and `https://zed.dev/docs/extensions/capabilities`) before editing
  `extension.toml`, declare **exactly** the capabilities the code can reach, and record the
  verified answer in the design doc §8.3 row: an unused permission is a wider prompt than the
  extension needs, a missing one makes every download fail with
  `capability for download_file … is not granted by the extension host`.
- **The guard is the proof.** `scripts/check_zed_extension.py` (run by `make zed-check`, inside
  `make check`) currently asserts 42 properties, including `lib: which order` (`which("blastit")`
  before `which("uv")`), `lib: argv` (the six `uv` literals), `lib: error string` (`uv add
  blastit` / `uv sync`) and `lib: no capabilities`. Each of those four becomes a property about
  the new order. `scripts/test_check_zed_extension.py` asserts the property **names** exactly and
  `len(properties) == 42`, so the tuple, the count and one negative twin per new property move
  together.
- **DX-first order inside this leaf.** (a) Write the page sentences (`docs/local.md`,
  `docs/reference/cli.md`, `editors/zed/README.md`) so the documented behaviour is fixed first;
  (b) amend the design §8.3 + ADR-0013 §6 with a dated amendment (the accepted text stays as the
  record; the amendment states why the `uv` fallback is gone and what the capability trade-off is
  now); (c) change `lib.rs`/`extension.toml` so the pages become true; (d) teach the guard and
  its negative twins. No new crate dependency: `zed_extension_api 0.7.0` already exposes
  `settings::{LspSettings, CommandSettings}`, `current_platform`, `latest_github_release`,
  `download_file`, `make_file_executable` and `set_language_server_installation_status`, so
  `Cargo.toml`/`Cargo.lock` stay untouched.
- User page shape to keep (`docs/local.md` step 2, one sentence, **no new fence** — `MB.32.1`
  pins the docs' fence classification and `check_settings` requires the existing `json` fence to
  keep `format_on_save: "on"`): *"The extension finds the server itself: `blastit` on your `PATH`
  (tarball, `~/.local/bin`, wheel or `uv tool install blastit`); with none, it downloads the
  Linux release binary from GitHub once. Override with `lsp.blastit-lsp.binary.path` in
  `.zed/settings.json`."*

# Acceptance

Run from the repository root. (`PATH` keeps `uv`/`cargo` reachable; nothing here needs the network
except where noted.)

1. **The guard and its negative twins are green, at the new property count.**
   ```console
   $ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/check_zed_extension.py
   zed-check: OK (N properties checked)
   $ UV_OFFLINE=1 PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/test_check_zed_extension.py
   OK
   ```
   New properties must include, at least: `extension: capabilities` (the exact allowed set, extras
   red), `lib: settings first` (`for_worktree("blastit-lsp"` before `which("blastit")`),
   `lib: which blastit` (`"lsp"` argv), `lib: no uv` (`which("uv")` absent, no `--no-sync`, the
   error string names no `uv` command), `lib: github release` (`OffworldNexus/blastit`,
   `require_assets: true`, `pre_release: false`, the two `<arch>-unknown-linux-gnu` asset names,
   `GzipTar`, `make_file_executable`, `Downloading`/`Failed`), `lib: asset layout`
   (`blastit-<version>/blastit-<target>/blastit`), `lib: platform` (`current_platform` match plus
   the non-Linux error naming Linux x86_64/aarch64), `docs: no uv lsp form` (`docs/local.md`,
   `docs/reference/cli.md`, `editors/zed/README.md`). Keep `lib: no process` and
   `shell_env()` exactly once.

2. **`uv` is gone from the editor path, everywhere it was an instruction.**
   ```console
   $ ! git grep -n 'which("uv")\|uv run --frozen\|--no-sync' -- editors/zed scripts/check_zed_extension.py
   $ ! git grep -n 'which("uv")' -- docs/design docs/decisions
   $ ! git grep -n 'uv run blastit lsp\|uv add blastit' -- docs/local.md docs/reference/cli.md editors/zed/README.md
   ```
   (Dated record rows in `docs/design/blastfile-lsp-zed.md` §12 and `ADR-0013` keep the old argv
   inside the amendment/history; they must not read as an instruction. §8.3 and its table rows are
   rewritten, not annotated.)

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
   $ make doc-tone docs-check
   ```
   Both targets exit 0 (paste the tails).

5. **The whole gate.** `make check` (it runs `zed-check`, `docs-test`, `docs-check`, `plan-validate`)
   ends green; paste the tails into `RESULT.md`.

6. **Live proof, recorded not gated** (no `uv` on `PATH`): `zed: install dev extension` on
   `editors/zed`, open `./Blastfile`, and paste the installation status line plus one squiggle
   into `RESULT.md`. Say which of the three resolution steps supplied the server.

# Notes

- `download_file` hosts: the release asset's browser URL is `github.com`, and the host follows a
  redirect to `objects.githubusercontent.com`. Declare the hosts the code can actually reach
  (both, unless the schema proves the check runs only on the pre-redirect URL) and make the guard
  fail on any host outside that set and on any `npm:install` entry.
- Keep the resolution in one small function per step so the guard's static properties stay
  literal and reviewable; no `unwrap`/`expect` on the download path, and every failure becomes
  `LanguageServerInstallationStatus::Failed(<one sentence>)`.
- Do not touch `editors/zed/Cargo.toml`, `editors/zed/Cargo.lock`, the grammar, the queries,
  `.zed/settings.json`, or any protected test (`crates/*/tests/critical_*.rs`,
  `python/tests/critical/`).
- The `editors/zed` wasm build shares the cargo registry lock with the main workspace: if another
  node is compiling (`MB.23.5.1` was active when this node was written), run step 3 after it
  finishes rather than in parallel.
- Record the capability finding (schema file, enum variant, host function) in the design doc
  §8.3 row and in the ADR amendment; a reversal without the verified citation is the next
  regression.
