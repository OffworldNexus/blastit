# Cross-platform `blastit-lsp`: `pg_query` per target, Zed's API, release assets, dependency portability

- Node: MB.35.1   - Author: researcher   - Date: 2026-11-17

Decision memo for `MB.35` (a portable `blastit-lsp`; `blastit` itself stays Linux-only). Four
questions, each answered from source, a quoted CI/release fact, or a command run here.

## TL;DR

- **Windows MSVC builds, proven upstream.** `pg_query.rs`'s own CI ran `cargo build` **and**
  `cargo test` on `windows-latest` at the `6.2.1` tag, all Windows jobs green, **no `LIBCLANG_PATH`**
  set. Needs only `cl.exe` and `libclang.dll` (`C:\Program Files\LLVM`); pin
  `LIBCLANG_PATH=C:\Program Files\LLVM\bin` for determinism, not correctness.
- **Both Apple targets are buildable, and a native Intel runner exists** — but unproven for the Rust
  wrapper. `macos-latest` is arm64; `macos-15-intel`/`macos-26-intel` are standard, free-on-public, GA
  with no retirement notice. `pg_query.rs` has **no macOS CI**, so this is the one leg `MB.35.5` must
  *prove*. Zed still ships `Zed-x86_64.dmg` (v1.28.1).
- **No macOS/Windows trapdoors in Zed's API.** `current_platform()` reports the **host** arch (Intel
  Mac = `Os::Mac` + `X8664`); `Zip` **extracts** into the path you pass and keeps no archive;
  `make_file_executable` is a **no-op `Ok(())`** off Unix; `which("blastit-lsp")` resolves an
  on-`PATH` `blastit-lsp.exe` via `PATHEXT`.
- **`gh release create dist/*` uploads bytes unchanged**, and the asset URL 302s to
  **`release-assets.githubusercontent.com`**, not `objects.githubusercontent.com`. Zed's
  `download_file` gate is a **user setting** (default `host = "*"`) checked pre-flight on the
  original URL: `MB.33` declares **no** capability; naming the CDN host would fail closed.
- **`lsp/`'s dependencies are portable today**: `std` + `blastit-manifest` + `blastit-sqlpolicy` +
  `serde_json`, with no `libc`/`rustix`/`nix`/`pyo3`/`blastit-sys` anywhere in the closure.
- **Two packaging consequences:** archive entries extract **verbatim** (so `blastit-lsp.exe` must be
  at the zip root), and a downloaded binary is launched only through the relative `Command.command`
  that `path_from_extension` joins — `which` can never find it.

## Developer Experience Implications

- **The editor does the work.** `download_file` + `path_from_extension` mean zero install steps: no
  `PATH` edit, no `chmod`, no unzip. The minimum stays one `[language_servers.blastit-lsp]` block,
  plus one `lsp.blastit-lsp.binary.path` setting for overrides.
- **The binary contract is the DX contract.** `blastit-lsp` must start and speak LSP on stdio with no
  project, no Python, no privileges; everything Linux-only stays out and `make check` refuses it.
- **Windows is not a reduced mode** — `pg_query` builds and tests pass on MSVC, so the same SQL
  diagnostics ship; no "Windows without parsing" downgrade is needed.
- **`make_file_executable` is a no-op on Windows**, so the install path has no `if cfg!(windows)`:
  call it unconditionally, then launch — one code path, three platforms.
- **Failures must be legible.** If a target is ever dropped, the extension says "no `blastit-lsp` for
  `<os>/<arch>`" with the release URL — never a server that registers and validates nothing.

## 1. `pg_query 6.2.1` per target

| Target | Runner / evidence | Result | `LIBCLANG_PATH` / toolchain |
|---|---|---|---|
| `x86_64-pc-windows-msvc` | `windows-latest` = Server 2025, LLVM 20.1.8 | **Proven** — upstream `cargo build`+`test` green at the `6.2.1` tag | none required; set `C:\Program Files\LLVM\bin` |
| `aarch64-apple-darwin` | `macos-latest` (= arm64 macOS 26) | **Buildable** — C lib proven on `macos-latest`; Rust wrapper has no upstream macOS job | none (clang-sys hardcodes Xcode/CLT paths) |
| `x86_64-apple-darwin` | `macos-15-intel` (GA, standard, free on public repos) | **Buildable, unproven** — Tier 2 target; native leg is the only `cargo test`-grade evidence | none |
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04`, manylinux_2_28 | **Proven** by the matrix (`MB.3.18`) | image clang/libclang |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | **Declared, unbuilt** (`--summary`) | same |

### 1.1 `x86_64-pc-windows-msvc`

Two build-time requirements, both on the runner:

- **A C compiler.** `pg_query-6.2.1/build.rs:46-57` globs the C sources into `cc::Build`, adds the
  Windows include dirs at `:61-66` (`port/win32`, plus `port/win32_msvc` for MSVC) and compiles at
  `:67`. `cc` finds `cl.exe` through VS Build Tools, listed on the image as *Visual Studio
  Enterprise 2026* with `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`
  (<https://raw.githubusercontent.com/actions/runner-images/main/images/windows/Windows2025-VS2026-Readme.md>).
  **No `make`/`cmake`/`perl`/`protoc`:** the Makefile is only *copied* (`build.rs:29`) and `protoc`
  is probed then skipped (`:77`, `:104-106`).
- **`libclang.dll` for `bindgen` 0.66.1** (`build.rs:70-74`); default feature `runtime` loads it
  **inside the build-script process**. `clang-sys 1.9.1` searches `LIBCLANG_PATH` only if set
  (`clang-sys-1.9.1/build/common.rs:264-279`), else globs `C:\Program Files*\LLVM\lib` and the
  sibling `bin` (`:180-194`, `:249-257`) for `libclang.dll` (`build/dynamic.rs:151-154`); no registry
  lookup exists in 1.9.1.

**The LLVM path the image actually has:** the Windows 2025 README lists `- LLVM 20.1.8`;
`images/windows/scripts/build/Install-LLVM.ps1` extracts into `C:\Program Files\LLVM` on the ARM64
branch (`:9`) and only there adds `…\bin` to the machine `PATH` (`:29`) — the x64 branch shells out
to `choco install llvm` (`:31-33`), so `PATH` is **not guaranteed**. Pin
`LIBCLANG_PATH=C:\Program Files\LLVM\bin` — the **`bin`** directory holding `libclang.dll`, not `lib`.

**Prior art at job level:** run `36765631562` ("Release 6.2.1 (#84)", `conclusion: success`) has
`ci (stable, windows-latest)`, `ci (nightly, windows-latest)`,
`ci (stable-x86_64-pc-windows-gnu, windows-latest)` and `ci (stable-i686-pc-windows-msvc, …)` all
`success` (<https://api.github.com/repos/pganalyze/pg_query.rs/actions/runs/36765631562>); the plain
`windows-latest` job passes **no `--target`**, so it builds the host triple. MSVC shims ship
(`win32_msvc/*`, `Cargo.toml:17-26`) and `pg_config.h` carries `_WIN32`/`_MSC_VER` blocks.

### 1.2 Apple targets

Runner facts, quoted from
<https://docs.github.com/en/actions/reference/runners/github-hosted-runners>:

> `macOS | 4 | 14 GB | 14 GB | Intel | macos-15-intel, macos-26-intel`
> `macOS | 3 (M1) | 7 GB | 14 GB | arm64 | macos-latest, macos-15, macos-26, xcode-27`

So `macos-latest` is **arm64** and both Intel labels are **standard** runners, free on public repos;
`macos-14` retired 2026-11-02, so it is not an option. The only published Intel horizon is issue
#13045: *"available from now until August 2027. This will be the last available x86_64 image"* — a
claim already superseded by `macos-26-intel` (GA, issue #13739). No macOS 15 Intel retirement
announcement exists as of 2026-11-18; August 2027 is a floor on the guarantee, not a shutdown date.

**Why native Intel, not cross.** `x86_64-apple-darwin` is **Tier 2 with Host Tools** (Tier 2:
*"Automated tests are not always run so it's not guaranteed to produce a working build"*,
<https://doc.rust-lang.org/nightly/rustc/platform-support.html>). Cross-compiling works mechanically —
clang gets `--target=x86_64-apple-darwin` (`cc-1.11.2/src/lib.rs:3223-3247`) and one `macosx` SDK
covers both arches (`cc-1.11.2/src/target/apple.rs:10`) — but *running* the tests needs Rosetta 2,
capped at *"macOS 27 or earlier"* (<https://support.apple.com/en-us/102527>). Native `macos-15-intel`
is the only honest `cargo test` evidence, and it is free here.

**What the build needs:** the same two things, already present.

- The Makefile's `Darwin` branch (`libpg_query/Makefile:22-32`) is **dead code for Cargo** — it only
  names the shared library, while `build.rs:46-67` compiles via `cc` and `:21` links
  `static=pg_query`. Nothing macOS-hostile in it.
- `libclang` without env vars: clang-sys searches `llvm-config --prefix`'s `bin`/`lib`/`lib64`
  **first** (`clang-sys-1.9.1/build/common.rs:282-289`), then `xcode-select --print-path` (`:291-297`),
  then the hardcoded `DIRECTORIES_MACOS` — where `/Library/Developer/CommandLineTools/usr/lib` and
  `/Applications/Xcode.app/…/XcodeDefault.xctoolchain/usr/lib` come before `/usr/local/opt/llvm*/lib`,
  which is **last** (`:167-172`). The macOS 15 image ships Xcode 16.4 (default) and CLT 16.4
  (`images/macos/macos-15-Readme.md`). Footgun stands, for the search-order reason: a brew `llvm`
  puts its own `llvm-config` (and keg) ahead of Xcode, so do not `brew install llvm` in the job.

**Prior art, precisely:** `pganalyze/pg_query.rs`'s only workflow (`main.yml`) is `ubuntu-latest`×3 +
`windows-latest`×4 — **zero macOS runners**
(<https://api.github.com/repos/pganalyze/pg_query.rs/contents/.github/workflows>). The C library's CI
does have `build_macos: runs-on: macos-latest` (arm64) running `make` — Apple-Silicon C evidence
only. This is the one leg the epic must *produce* rather than cite.

### 1.3 The two Linux targets — cited, not re-derived

`scripts/platform_matrix.py:182-187` declares the four legs; `ARCHES` (`:60`) maps `x86_64 →
ubuntu-24.04`, `aarch64 → ubuntu-24.04-arm`, and `LIBCS["gnu"]["wheel_tag"]` is `manylinux_2_28`
(`:65`, `:67`). `.github/actions/build-wheel/action.yml` builds inside
`quay.io/pypa/manylinux_<tag>_<arch>`, so the C sources are compiled by the image's toolchain.
`uv run --no-project scripts/platform_matrix.py --summary` says: archives `x86_64 gnu` and
`x86_64 musl` **proved by MB.3.18**; `aarch64 gnu`/`aarch64 musl` **declared, unbuilt**. So
`x86_64-unknown-linux-gnu` is proven and `aarch64-unknown-linux-gnu` is CI-built on a release PR but
has never been *executed* — acceptance must not call it proven.

### 1.4 The boundary, stated honestly

The parser is the same code on all five targets, so there is no fallback to design. If a target ever
fails, the honest shape is: drop it from `scripts/platform_matrix.py` with `dropped_because` **and**
from the extension's target table, so `MB.33` prints "no `blastit-lsp` for `<os>/<arch>`" — never a
`#[cfg(target_os)]` stub that returns "no diagnostics", which is a validator that approves
everything.

## 2. Zed API facts (`zed_extension_api 0.7.0`, pinned)

Crate binds **`./wit/since_v0.6.0`** (`src/extension_api.rs:333-340`), so all facts come from
`wit/since_v0.6.0/`; public re-exports are `src/extension_api.rs:18-40`.

| Question | Answer | Citation |
|---|---|---|
| `current_platform()` variants | `os = {mac, linux, windows}`; `architecture = {aarch64, x86, x8664}` — no 32-bit ARM, no riscv | `platform.wit:3-20` |
| Is an Intel Mac `X8664`? | **Yes.** Host maps `env::consts::OS`/`ARCH`; `"x86_64" => X8664`, else `Err("unsupported architecture")` | `crates/extension_host/src/wasm_host/wit/since_v0_8_0.rs:870-887` (main; `since_v0_6_0.rs:406-409` forwards) |
| `download_file(…, Zip)` — extract or keep? | **Extracts**, archive not kept: `extract_zip`/`extract_tar_file` into the destination | WIT doc `extension.wit:53-57`; `since_v0_8_0.rs:1109-1134` |
| Where? | `<data-dir>/extensions/work/<extension-id>/<file-path>`, symlink-escape-checked, created on demand | `extension_host.rs:380` (`work_dir = extensions_dir.join("work")`), `since_v0_8_0.rs:1079-1087`, `wasm_host.rs:765-800` |
| Zip layout | Entries joined **verbatim** — no top-dir stripping, so `blastit-lsp.exe` must be at the archive root | `crates/util/src/archive.rs:20-47` |
| `make_file_executable` on Windows | **No-op, `Ok(())`** (`#[cfg(not(unix))]`); Unix does `chmod 0o755` | `crates/util/src/fs.rs:94-111` |
| `which("blastit-lsp")` | WIT: "path to the given binary name, if one is present on `$PATH`"; host goes `WorktreeDelegateAdapter → LspAdapterDelegate::which → which_in(name, shell_PATH, worktree_root)`, and `which` 8 expands each candidate with the `PATHEXT` extensions on Windows, so `"blastit-lsp"` resolves an on-`PATH` `blastit-lsp.exe` | `extension.wit:66-77`; `language_extension/src/extension_lsp_adapter.rs:28-53`; `project/src/lsp_store.rs:17162-17173`; `which-8.0.0/src/finder.rs:193-238` (`# Append the extension`), `src/sys.rs:163-173` (`env_windows_path_ext`); `Cargo.toml:1011` (`which = "8"`) |
| Launching a **downloaded** binary | `Command.command` is resolved by `Extension::path_from_extension` = `work_dir.join(path)`, so the *relative* string used in `download_file` is the launch path; `which` can never find it (not on `PATH`) | `crates/extension/src/extension.rs:56-62`; `extension_lsp_adapter.rs:191-221` |
| Redirects in `download_file` | Followed: `get(&url, …, true)` → `RedirectPolicy::FollowAll` | `since_v0_8_0.rs:1093-1096`; `crates/http_client/src/http_client.rs:128-138` |

**Consequences for `MB.33`** (facts, not design):

1. `Os::Mac` + `X8664` is a real reported combination; the target table needs that key.
2. `Zip`/`GzipTar` take a **directory** path: `"blastit-lsp.zip"` becomes a directory of that name,
   so the binary lands at `<work>/<id>/blastit-lsp.zip/blastit-lsp.exe` unless the zip root is bare.
3. `make_file_executable` must still be called on Linux/macOS after extraction; it is safe to call
   unconditionally on Windows.
4. `which` searches `$PATH`; a downloaded artifact is launched **only** through the relative
   `Command.command`, never through `which`.
5. On Windows a WASI-shaped `/C:\…` path has its leading `/` stripped before the join
   (`extension_lsp_adapter.rs:191-221`) — another reason to return relative paths.

## 3. GitHub Release mechanics

**Upload is byte-for-byte.** The job is `.github/workflows/publish-pypi.yml:807-841`: it verifies
every archive's checksum (`:782-805`), then `gh release upload "$RELEASE_TAG" dist/* --clobber` on a
re-run or `gh release create "$RELEASE_TAG" dist/* --verify-tag --notes-file notes.md`. `gh` POSTs
the file handle as the body with `ContentLength = asset.Size` (`cli/cli
pkg/cmd/release/shared/upload.go`), and the REST docs say *"GitHub expects the asset data in its raw
binary form… send the raw binary content of the asset as the request body"*
(<https://docs.github.com/en/rest/releases/assets>). `Content-Type` is guessed client-side
(`.zip → application/zip`, `.tar.gz → application/x-gtar`) and stored as sent; the API's
`digest: sha256:…` equals `sha256sum` of the download. Caveats that bite a release job, all from the
same REST page: *"GitHub renames asset filenames that have special characters, non-alphanumeric
characters, and leading or trailing periods"*; *"If you upload an asset with the same filename as
another uploaded asset, you'll receive an error and must delete the old file before you can
re-upload"*; and for the re-run path, `gh`'s own warning: *"When using `--clobber`, existing assets
are deleted before new assets are uploaded. If the upload fails, the original assets will be lost."*
(<https://cli.github.com/manual/gh_release_upload>).

**The browser URL does not go to `objects.githubusercontent.com`** — the brief is stale. Measured
here on 2026-11-18:

```console
$ curl -sSI https://github.com/cli/cli/releases/download/v2.105.0/gh_2.105.0_linux_amd64.tar.gz
HTTP/2 302
location: https://release-assets.githubusercontent.com/github-production-release-asset/…&sig=…
$ curl -sS -o /dev/null -L -w 'num_redirects=%{num_redirects} code=%{http_code} …'
num_redirects=1 code=200   # final host release-assets.githubusercontent.com
```

No docs page names the CDN host; the REST page only says *"API clients should handle both a 200 or
302 response"*. `codeload.github.com` (git archives) is not in this chain.

**Zed's `download_file` gate is a user setting, not an extension declaration.** The granter is built
from `ExtensionSettings::granted_capabilities` (`extension_settings.rs:45-64` ←
`extension.granted_extension_capabilities`) at `wasm_host.rs:678-682`, and the shipped default
grants everything: `{ "kind": "download_file", "host": "*", "path": ["**"] }`
(`assets/settings/default.json:2413-2417`). `grant_download_file` iterates only those capabilities
(`capability_granter.rs:49-65`) — the **manifest is never consulted**, so a `[[capabilities]]` block
is inert for downloads and `MB.33` needs none. Matching is exact-host or `*` plus per-segment path
globs (`capabilities/download_file_capability.rs:6-42`; `fake-github.com` does not match
`host = "github.com"`). The check runs on the **pre-redirect** URL (`since_v0_8_0.rs:1076,1093-1096`),
so `host = "github.com"` suffices and naming the CDN host would fail closed. Do not oversell it:
`http_client::fetch` performs no capability check at all (`since_v0_8_0.rs:633-650`) — this gate is
a guardrail, not an egress sandbox.

## 4. Dependency portability

### 4.1 `cargo tree -p blastit --edges normal -i blastit-manifest`

```console
blastit-manifest v0.2.0 (…/crates/blastit-manifest)
├── blastit v0.2.0 (…/crates/blastit)
├── blastit-env v0.2.0 (…/crates/blastit-env)
│   └── blastit v0.2.0 (…/crates/blastit)
└── blastit-scheduler v0.2.0 (…/crates/blastit-scheduler)
    └── blastit v0.2.0 (…/crates/blastit)
```

### 4.2 `cargo tree -p blastit-manifest --edges normal` (133 lines; the `icu_*`/`zerovec` subtree elided)

```console
blastit-manifest v0.2.0 (…/crates/blastit-manifest)
├── blastit-ingress v0.2.0 (…/crates/blastit-ingress)
│   ├── percent-encoding v2.3.2
│   ├── serde v1.0.229
│   ├── serde_json v1.0.154
│   ├── thiserror v2.0.21
│   └── url v2.5.8
│       ├── form_urlencoded v1.2.2
│       ├── idna v1.1.0            # → icu_normalizer/icu_properties/zerovec…
│       │   ├── smallvec v1.16.4
│       │   └── utf8_iter v1.0.4
│       └── percent-encoding v2.3.2
├── blastit-sqlpolicy v0.2.0 (…/crates/blastit-sqlpolicy)
│   ├── pg_query v6.2.1
│   │   ├── itertools v0.10.5        # → either v1.19.0
│   │   ├── prost v0.13.5            # → bytes, prost-derive
│   │   ├── serde v1.0.229
│   │   ├── serde_json v1.0.154
│   │   └── thiserror v1.0.69
│   ├── serde v1.0.229
│   └── serde_json v1.0.154
├── regex v1.13.1                    # → aho-corasick, memchr, regex-automata, regex-syntax
├── serde v1.0.229
└── thiserror v2.0.21
```

`grep -c -E 'libc|rustix|nix|pyo3'` over the full output returns **0**. `pg_query`'s own `cc`,
`bindgen`, `fs_extra`, `glob`, `prost-build` are build-time only — the reason §1 exists.

### 4.3 What `lsp/` imports

`crates/blastit/src/lsp/mod.rs` (1591 lines) + `hover.rs` (337): `std` (`io`, `cell`,
`collections`, `panic`), `blastit_manifest::{blastfile, Manifest, ManifestError,
ManifestWarning, Position, QuerySite, UidLayout}`, `blastit_sqlpolicy::parameterize`,
`serde_json::{Map, Value, json}`. `grep -n 'crate::' crates/blastit/src/lsp/*.rs` matches **one
comment** (`mod.rs:62`), no import. `cfg(unix)|cfg(windows)|std::os::unix|std::os::windows` over
`blastit-manifest/src`, `blastit-sqlpolicy/src`, `blastit-ingress/src`, `blastit/src/lsp/`: **no
matches**.

### 4.4 Crates that are Unix-only (must stay out of `blastit-lsp`)

| Crate | Why not portable | Citation |
|---|---|---|
| `blastit-sys` | `libc` + `rustix` (564 matching lines); fanotify, caps, seccomp, pidfd | `Cargo.toml:19-20`; `src/{fanotify,caps,privs,pid,mem,proc}.rs` |
| `blastit-dbproxy` | `rustix` net/fs (25 references) | `Cargo.toml:27` |
| `blastit-scheduler` | `rustix` net/process/fs | `Cargo.toml:18` |
| `blastit` (binary) | `libc`, `rustix`, `blastit-sys`, optional `pyo3`/`blastit-php-sapi` | `Cargo.toml:37-56` |
| `blastit-py`, `blastit-view` | `pyo3` (embedded CPython) | `blastit-py/Cargo.toml:54`; `blastit-view/Cargo.toml:33,39` |
| `blastit-php-sapi` | `dlopen`-only libphp ABI shim, consumed as a Linux `.so` | `Cargo.toml:12-18` |

`blastit-http` has two `#[cfg(unix)]` blocks (`src/lib.rs:126`, `src/server.rs:224`) but is **not**
in the `lsp` path and must not become one.

## Risks (one line each, and the decision it forces)

| Risk | Decision |
|---|---|
| Windows is proven only by upstream's CI (which builds the submodule; the `.crate` compiles its vendored copy of the same tag) | `MB.35.5` adds the leg **and** a smoke transcript; no "works in theory" note |
| `LIBCLANG_PATH` unset by default; clang-sys 1.9.1 has no registry fallback | Pin `C:\Program Files\LLVM\bin` in the job; document it in `MB.35.2` |
| No upstream macOS job for the Rust wrapper | Both Apple legs are the epic's only *unproven* builds — build and smoke them before the release PR |
| `macos-15-intel` carries an "until August 2027" note | Keep the Intel leg in the matrix with a `dropped_because` note, so dropping it later is one edit and one honest count |
| Archive entries extract verbatim into the `download_file` path | Fix the archive root now (`blastit-lsp.exe` / `blastit-lsp`), or `MB.33` hard-codes a directory name |
| `aarch64-unknown-linux-gnu` declared, not proved | Acceptance says "declared, unbuilt" until a smoke run exists |
| `download_file`'s gate is a user setting and `fetch` bypasses it | `MB.33` declares no capability and claims no egress enforcement |
| Bundling `pg_query` needs `cc` + `libclang` on every build host | `MB.35.2` names the C toolchain in "build it yourself"; prebuilt artifacts stay the default |
| `MB.34`'s promise is SQL diagnostics | If a target is dropped, it is documented as *no SQL diagnostics for `<target>`* in the extension's message — never a stub |

## Unverified / Open Questions

- **Apple has no CI precedent** (see §1.2): everything there is source + runner docs. Settle it with
  one `macos-latest` and one `macos-15-intel` job running `cargo test -p blastit-lsp`; `MB.35.5` owns
  that evidence and this node has no macOS host.
- `libclang.dylib` presence at clang-sys's hardcoded paths is not in the image README; one `ls -l` in
  the first macOS job settles it (fallback: `LIBCLANG_PATH` from `xcode-select -p`).
- Which `cc`/`clang-sys` versions the upstream Windows job resolved (`cc = "1.0.83"` is a caret
  requirement, `pg_query-6.2.1/Cargo.toml:73-74`); green either way.
- Whether August 2027 still binds `macos-15-intel` now that `macos-26-intel` is GA.
- If a future Zed re-validates redirect hops against a declaration, `MB.33` would need a host list;
  nothing in today's manifest format provides one.

## Sources

- `~/.cargo/registry/src/*/pg_query-6.2.1/{build.rs,Cargo.toml}`, `libpg_query/Makefile`,
  `libpg_query/src/postgres/include/port/win32{,_msvc}/`; `…/clang-sys-1.9.1/build/{common,dynamic}.rs`;
  `…/zed_extension_api-0.7.0/src/extension_api.rs`, `wit/since_v0.6.0/{platform,extension}.wit`.
- Zed `main`: `crates/extension_host/src/{extension_host.rs,wasm_host.rs,capability_granter.rs,
  extension_settings.rs,wasm_host/wit/since_v0_8_0.rs}`, `crates/extension/src/{extension.rs,
  capabilities/download_file_capability.rs}`, `crates/language_extension/src/extension_lsp_adapter.rs`,
  `crates/project/src/lsp_store.rs`, `crates/util/src/{fs.rs,archive.rs}`, `Cargo.toml` (`which = "8"`),
  `assets/settings/default.json`.
- `https://api.github.com/repos/pganalyze/pg_query.rs/actions/runs/36765631562` and
  `…/contents/.github/workflows`; `https://raw.githubusercontent.com/pganalyze/libpg_query/master/
  .github/workflows/ci.yml`.
- `https://raw.githubusercontent.com/actions/runner-images/main/images/{windows/Windows2025-VS2026,
  macos/macos-15}-Readme.md`; `https://docs.github.com/en/actions/reference/runners/github-hosted-runners`;
  `https://doc.rust-lang.org/nightly/rustc/platform-support.html`;
  `https://support.apple.com/en-us/102527`; `https://docs.github.com/en/rest/releases/assets`;
  `https://cli.github.com/manual/gh_release_upload`.
- `https://api.github.com/repos/zed-industries/zed/releases/latest` (`Zed-x86_64.dmg` still shipped);
  `https://api.github.com/repos/zed-industries/zed/contents/crates/extension_host/src` (module list).
- Measured here (2026-11-18): `curl -sSI`/`-L` on the `cli/cli` v2.105.0 asset (302 →
  `release-assets.githubusercontent.com`, `num_redirects=1 code=200`); the two `cargo tree` commands.
- Repo: `.github/workflows/publish-pypi.yml`, `.github/actions/build-wheel/action.yml`,
  `scripts/platform_matrix.py`, `crates/*/Cargo.toml`, `crates/blastit/src/lsp/*.rs`.

VERDICT: GO — five targets planned; Windows and Linux `x86_64` proven, both Apple legs unproven
until `MB.35.5` builds and smokes them.
Conditions, so `MB.35.3` does not read this as "nothing left to prove":

1. **Windows** is proven by upstream CI at the `6.2.1` tag: set `LIBCLANG_PATH=C:\Program Files\LLVM\bin`,
   build on the Windows runner, smoke the artifact.
2. **macOS (both)** is buildable per docs+source but **unproven for the Rust wrapper**: `MB.35.5`
   must build, test and smoke on `macos-latest` and `macos-15-intel`. If a leg fails there, drop it
   **with** `dropped_because` and an extension message naming the missing target — never a stub.
3. **Linux x86_64** proven (`MB.3.18`); **aarch64** declared-but-unbuilt, and acceptance must say so.
4. **`MB.33` declares no `download_file` capability** and targets `github.com` only; the
   `objects.githubusercontent.com` instruction in the brief is stale.
