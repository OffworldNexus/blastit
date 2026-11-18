# RESULT MB.33 — resolve `blastit-lsp` without `uv` on Linux, macOS and Windows

Attempt 2/3 (attempt 1 was reviewed: one MAJOR and three MINORs, all fixed below). Base `ef93363`,
branch `release/0.2.0`. Every command was run from the repository root with
`PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"`, on this Linux host.

**Status: done.** The extension resolves settings → `which("blastit-lsp")` →
`which("blastit") lsp` → the binary it unpacked earlier → one GitHub Release download, in that
order. The cache answers **before** any network call, the manifest declares **no**
`[[capabilities]]`, the child gets a `PATH`/`SYSTEMROOT` allowlist, and the three page sentences
match the code.

## Attempt 2: the review's findings

| # | Finding | Fix |
|---|---|---|
| 1 | **MAJOR** `latest_github_release` ran before the cache hit, so an offline or rate-limited machine that already had `blastit-lsp-<version>/<target>/blastit-lsp` got `Failed(…)` and no server; NODE.md:65 and design §8.3a required "a second start does no network I/O", and the dated amendment recorded the code instead of fixing it | The cache is now the **first** thing `from_release` does, on a **version-free** `blastit-lsp/blastit-lsp-<target>/blastit-lsp(.exe)`, with `std::fs::metadata(…).is_ok_and(\|entry\| entry.is_file())` and an early `return Ok(…)`. `latest_github_release` runs only on a miss. `lib: cache` asserts the hit precedes the release call (twin: the cache block moved below it → red). The version-free directory keeps `download_file` and the relaunch path on one name, which is what the old `<version>` segment was for; design §8.3a's two path columns, the `<version>` paragraph and the amendment now say that, and `docs/reference/cli.md` states the promise the code keeps ("unpacked once and then reused from disk — a later start needs no network at all") |
| 2 | **MINOR** `lib: cache` only ordered `std::fs::metadata(` before `zed::download_file(`, so an unused probe beside an unconditional download stayed green | The predicate is pinned verbatim in `CACHE_HIT` (the way `ENV_ALLOWLIST` is), and the property now requires it inside `from_release`, a `return Ok(` between it and the release call, and the download after the release. The self-test carries its own copy of the literal, so weakening the guard's constant turns every cache case red |
| 3 | **MINOR** `lib: arguments` pinned only `vec!["lsp".to_string()]`, so `args: vec!["--stdio".to_string()]` elsewhere stayed green | `lib: arguments` now reads **every** `args:` value and requires them all to be `Vec::new()` (three arms: the `blastit-lsp` on `$PATH`, the cached binary, the unpacked one) or exactly `["lsp"]` once. Twin: `--stdio` in an arm → red |
| 4 | **MINOR** no twin for honouring `binary.env` | `test_a_binary_env_is_red` adds `let _ = binary.env;` to `from_settings` → exactly `{"lib: env allowlist"}` |

## What changed

| File | Change |
|---|---|
| `editors/zed/src/lib.rs` | `from_release` is now: asset → cache hit (early return) → `Downloading` → release → asset lookup → `download_file(…, EXTRACT_DIR, …)` → `make_file_executable`; `EXTRACT_DIR = "blastit-lsp"` replaces `release_directory(version)`, `executable_path(asset)` loses its version parameter, and `failed()` reports `Downloading` then `Failed` so every `Err` carries both statuses. Eight host unit tests, one of them rewritten (`the_cache_path_is_version_free_and_shared_with_the_extract_directory` replaces the versioned-path case) |
| `editors/zed/extension.toml` | untouched — nothing to declare (the gate is a user setting) |
| `editors/zed/README.md` | unchanged this attempt: the four-row table and the "Until the release …" paragraph's removal stand |
| `docs/local.md` `## Editors` step 2 | "downloaded once and reused from disk after that" |
| `docs/reference/cli.md` `## blastit-lsp` | "unpacked once and then reused from disk — a later start needs no network at all" replaces "once per Blastit release"; no project-file settings alternative (the setting is `zed: open settings`) |
| `docs/design/blastfile-lsp-zed.md` §8.3a | the order table's third row, the `<version>` paragraph, the five path cells, the download-destination row and the dated amendment now state the version-free directory and the cache-first rule, with its cost (a cached binary is not refreshed until the work directory is cleared) |
| `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` | untouched — decisions 4 and 5 already describe this order, no capability, a relative destination, unconditional `make_file_executable` and the ignored `binary.env` |
| `scripts/check_zed_extension.py` | `lib: cache`, `lib: arguments`, `lib: executable`, `lib: download destination` and `lib: status` tightened; `EXTRACT_DIR`, `CACHE_HIT` and `ALLOWED_ARGV` pinned as module constants |
| `scripts/test_check_zed_extension.py` | three new twins, three rewritten; the `CACHE_HIT` literal is duplicated on purpose |

### The resolution order, in the code's own shape

```rust
if let Some(command) = from_settings(worktree) { return Ok(command); }   // 1. settings
if let Some(command) = from_path(worktree)     { return Ok(command); }   // 2. $PATH
from_release(language_server_id, worktree)                              // 3. cache, then download
```

```rust
// from_release, in order:
let (os, architecture) = zed::current_platform();
let Some(asset) = asset_for(os, architecture) else { … };                // five pairs, no default
let command = executable_path(&asset);                                   // "blastit-lsp/<target>/<exe>"
if std::fs::metadata(&command).is_ok_and(|entry| entry.is_file()) {      // cache, before any network
    return Ok(zed::Command { command, args: Vec::new(), env: env_allowlist(worktree) });
}
zed::set_language_server_installation_status(id, &Downloading);
let release = zed::latest_github_release(REPOSITORY, RELEASE_OPTIONS)?;  // only on a miss
… find the asset by name …
zed::download_file(&asset_url.download_url, EXTRACT_DIR, asset.file_type)?;
zed::make_file_executable(&command)?;                                    // unconditional
```

### The child's environment

`env_allowlist()` reads `worktree.shell_env()` **once** and keeps only `ENV_ALLOWLIST =
["PATH", "SYSTEMROOT"]`. The settings' `env` map is never read, `initialization_options` /
`workspace_configuration` are not implemented, and no `Command { env: … }` is the whole shell
environment — asserted by `lib: env allowlist`, proved by `test_a_binary_env_is_red`.

## Property and test counts (honest)

| | attempt 1 | attempt 2 |
|---|---|---|
| guard properties (`zed-check: OK (N properties checked)`) | 59 | **59** (same names; five of them are stricter) |
| negative self-tests (`Ran N tests`) | 41 | **44** (3 added, 3 rewritten) |
| `editors/zed` host unit tests (`cargo test`) | 8 | 8 (one replaced) |

The tightened properties: `lib: cache` (cache hit before `latest_github_release`, predicate pinned),
`lib: arguments` (whitelist over every `args:` value), `lib: executable` (the path shape is read out
of `executable_path`'s body, not the whole file), `lib: download destination` (`EXTRACT_DIR` pinned
and passed to `download_file`), `lib: status` (`Downloading` twice: before the fetch and in the
failure helper).

## Acceptance

### 1. The guard and its negative twins, at the new property count

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/check_zed_extension.py
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (59 properties checked)
$ UV_OFFLINE=1 PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/test_check_zed_extension.py
----------------------------------------------------------------------
Ran 44 tests in 8.257s

OK
```

The ten twins worth pasting (from `/tmp/opencode/mb33_twins2.py`, which calls the guard's own cases
and then prints the guard's stderr for the doctored tree):

```console
$ # test_a_release_lookup_before_the_cache_is_red
[guard exit 1]
FAIL lib: cache: the unpacked `blastit-lsp/<target>/blastit-lsp` must be a hit BEFORE `latest_github_release`: an offline or rate-limited machine with the binary on disk gets a server (§8.3a)
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_no_cache_check_is_red
[guard exit 1]
FAIL lib: cache: the unpacked `blastit-lsp/<target>/blastit-lsp` must be a hit BEFORE `latest_github_release`: an offline or rate-limited machine with the binary on disk gets a server (§8.3a)
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_a_foreign_argument_is_red
[guard exit 1]
FAIL lib: arguments: every `args:` must be `Vec::new()` (three arms: `blastit-lsp` on `$PATH`, the cached binary, the unpacked one) or exactly ["lsp"] once for `which("blastit")`; got ['vec!["--stdio".to_string()]', 'vec!["lsp".to_string()]', 'Vec::new()', 'Vec::new()']
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_a_binary_env_is_red
[guard exit 1]
FAIL lib: env allowlist: the child env must be the literal PATH/SYSTEMROOT allowlist: never `binary`'s env map, never `Command { env: worktree.shell_env() }`, and no settings payload (§8.3a)
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_a_declared_capability_is_red
[guard exit 1]
FAIL extension: capabilities: [[capabilities]] [{'kind': 'download_file', 'host': 'github.com', 'path': ['**']}] must not be declared: the download gate is the user's setting (`host = "*"` by default), never this manifest
FAIL lib: no capabilities: a [capabilities] block is inert for download_file and names a host that fails closed: ['editors/zed/extension.toml']
zed-check: 2 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_a_dropped_path_step_is_red
[guard exit 1]
FAIL lib: order: the three steps must be one call each, in this order: from_settings( -> from_path( -> from_release( (a download-first extension is red)
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_a_misspelt_asset_is_red
[guard exit 1]
FAIL lib: asset map: the five asset names must be §8.3a's verbatim: got ['blastit-lsp-x86_64-unknown-linux-gnu.tar.gz', 'blastit-lsp-aarch64-unknown-linux-gnu.tar.gz', 'blastit-lsp-aarch64-apple-darwin.tar.gz', 'blastit-lsp-x86_64-darwin.tar.gz', 'blastit-lsp-x86_64-pc-windows-msvc.zip']
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_a_cfg_gated_make_file_executable_is_red
[guard exit 1]
FAIL lib: executable: one joined path `<extract dir>/<target>/<executable>`, `blastit-lsp.exe` only on Windows, and one unconditional make_file_executable (§8.3a)
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_an_absolute_download_destination_is_red
[guard exit 1]
FAIL lib: download destination: download_file takes a directory relative to the extension's work directory — the one `EXTRACT_DIR` constant the relaunch path also starts with — never an absolute path (§8.3a)
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # test_a_missing_downloading_status_is_red
[guard exit 1]
FAIL lib: status: report Downloading before the fetch, then Failed(<one sentence>) — a silent failure is an editor that never starts the server
zed-check: 1 of 59 properties failed; the extension would not install (design §8, §10, §12.4)

$ # baseline (no mutation)
[guard exit 0]
zed-check: OK (59 properties checked)
```

The capability twin is the one mutation that legitimately produces **two** reds (the parsed manifest
and the text grep over `editors/zed/**`), and its case says so — the convention
`test_missing_sql_capture_is_red` already uses. `lib: which order`, `lib: order`,
`lib: arguments`, `lib: zip windows` (`GzipTar` on Windows), `lib: failure` (a message without the
hint) and `lib: asset url` (a CDN host) have their own cases in the same suite.

### 2. `uv` is gone from the editor path

```console
$ ! git grep -n 'which("uv")\|uv run --frozen\|--no-sync' -- editors/zed scripts/check_zed_extension.py
$ echo $?
0
$ ! git grep -n 'uv run blastit lsp\|uv add blastit' -- docs/local.md docs/reference/cli.md editors/zed/README.md
$ echo $?
0
```

The guard detects the old spellings as **regular expressions** so it does not itself carry the
literals this grep hunts for; `test_a_uv_step_is_red` inserts `let _uv = worktree.which("uv");` and
turns `lib: no uv` red.

**The item's `docs/design docs/decisions` grep cannot pass, by design, and was not forced:**

```console
$ git grep -n 'which("uv")' -- docs/design docs/decisions | wc -l
10
$ git grep -n 'which("uv")' -- docs/design docs/decisions | cut -d: -f1 | sort -u
docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
docs/design/blastfile-lsp-zed.md
docs/design/blastfile-lsp-zed.review-security.md
docs/design/portable-lsp-arch.review-consistency.md
```

Every hit is accepted record: design §8.3's superseded block and rows, ADR-0013 §6's history, and
two immutable review reports. Design §8.3a's own row states the rule — *"The check must be
**positive** … because a repo-wide `git grep which("uv") -- docs/design docs/decisions` can never
pass while the accepted record still describes the old order"* (`docs/design/blastfile-lsp-zed.md`
§8.3a). The positive half is `lib: no uv`, `lib: which order` and `lib: arguments`.

### 3. The wasm builds clean

```console
$ cd editors/zed && cargo build --target wasm32-wasip2 --release 2>&1 | tail -2
   Compiling blastit-zed v0.1.0 (…/editors/zed)
    Finished `release` profile [optimized] target(s) in 0.32s
$ cargo build --target wasm32-wasip2 --release 2>&1 | grep -c '^warning'
0
$ echo $?
1          # `grep -c` exits 1 when it counts zero matches; the count is 0
```

`rustup target list --installed` includes `wasm32-wasip2`, so this is a real build. The pure table is
also unit-tested on the host:

```console
$ cd editors/zed && cargo test 2>&1 | tail -13
running 8 tests
test tests::an_unbuilt_pair_has_no_asset ... ok
test tests::every_asset_name_carries_its_own_target_triple ... ok
test tests::every_supported_pair_has_one_asset ... ok
test tests::the_cache_path_is_version_free_and_shared_with_the_extract_directory ... ok
test tests::the_environment_allowlist_is_exactly_path_and_systemroot ... ok
test tests::the_failure_sentence_names_the_five_targets_and_the_setting ... ok
test tests::the_relaunch_path_is_the_archives_own_directory ... ok
test tests::only_windows_unpacks_a_zip_and_only_windows_ships_an_exe ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`the_cache_path_is_version_free_and_shared_with_the_extract_directory` is the new one: it pins
`blastit-lsp/<target>/<exe>` as three segments starting with `EXTRACT_DIR`, which is the path the
cache check and `download_file` share. (`editors/zed/Cargo.toml`/`Cargo.lock` are untouched.)

### 4. The pages and the site still pass

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project python -m pytest python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 2.03s
$ make doc-tone
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.20s
$ make docs-test
PASS  docs/reference/cli.md  fence 1  dev-flags             4.3s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 2  check-flags           5.2s  each check flag runs
PASS  docs/reference/cli.md  fence 3  serve-flags           0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                 0.0s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked every command, flag and snippet (…)

48 fences: 30 executed, 18 accounted, 0 failed · 31.7s
```

No fence was added or removed (`EXPECTED_FENCES` unchanged), and `check_settings`' `json` fence
still carries `format_on_save: "on"`.

### 5. The whole gate

```console
$ make check                # full log: /tmp/opencode/mb33-attempt2-make-check.log, 189 `test result: ok`
…
124 passed, 65 subtests passed in 2.87s                    # docs-unit
uvx --offline zensical build --strict
Build started
No issues found                                          # docs-check (--strict)
Build finished in 0.20s
…
48 fences: 30 executed, 18 accounted, 0 failed · 31.7s    # docs-test
…
workflow-check: ok — 449 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 56 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7)
doc-tone: ok — 30 user-facing page(s), no banned phrase
manifest-purity: ok — 17 source file(s), no std::env
manifest-purity self-test: ok — 82 assertions, 37 fixtures proved, 17 real source file(s)
…
ok   lib: download destination
ok   lib: cache
ok   lib: status
ok   lib: failure
ok   lib: asset url
ok   lib: no capabilities
ok   settings: format_on_save
ok   docs: settings fence
zed-check: OK (59 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
............................................
----------------------------------------------------------------------
Ran 44 tests in 8.625s

OK
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no
build script, no target table and no feature; 3 source file(s) with none of 15 banned spellings and
no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home;
`make check` runs this target
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 73 assertions over 23 fixtures
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

```console
$ make zed-check
ok   docs: settings fence
zed-check: OK (59 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
............................................
----------------------------------------------------------------------
Ran 44 tests in 12.133s

OK
$ echo $?
0
```

`make check` reported zero `test result: FAILED` and zero `make: ***` lines (grep count 0/0).

### 6. Live proof

- **Linux**: `NOT RUN (no Zed binary on this host, and the wasm loads only inside Zed's extension
  host)` — the command is `zed: install dev extension` → `editors/zed`, open `./Blastfile`, in
  Zed ≥ 0.205.
- **macOS / Windows**: `NOT RUN (no macOS or Windows machine available to this attempt)`.

## Follow-ups (out of this node's `paths`)

1. **`docs/knowledge/LESSONS.md`**, three lines this node earned: (a) a Rust string continued with
   `\` at end-of-line makes a guard that reads a literal back out of source see `macOS \` +
   `x86_64`, so normalise `.replace("\\\n", "")` first; (b) a TOML `[[capabilities]]` is an array
   of tables and `^\s*\[capabilities\]` never matches it; (c) `grep -c '^warning'` exits **1** when
   it counts zero, so a `| grep -c` acceptance line always shows a non-zero status.
2. **Cache refresh**: a cached `blastit-lsp` is never replaced when a newer Blastit release ships,
   because learning the version needs the network the cache-first rule avoids. Clearing the
   extension's work directory is the refresh; a future node could compare a stored version stamp
   against a *successful* release lookup when one happens (never in front of the cache).
3. **`docs/reference/cli.md` "work offline"**: it now names the manual-asset path, which is
   unchanged and correct; the extension's own offline behaviour is the cache-first rule above.
