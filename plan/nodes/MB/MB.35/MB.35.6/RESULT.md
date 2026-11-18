# RESULT — MB.35.6: make the portability promise enforced, then prove the documented commands

STATUS: done (attempt 5 — `expert`: every remaining per-line regex in `source` moved onto the
`code_tokens()` stream with fixed-point `extern crate` + `use` alias resolution, `[lib]`/`[[bin]]`
target-path containment enforced in `scan_manifest`, and the allowlist self-check proved by a
guard-mutating negative twin; 59 fixtures, 229 assertions, 43 cargo-checked)

## Root-Cause Diagnosis (Attempt 5 — `expert`)

Attempts 1–3 chased a spelling denylist. Attempt 4 inverted `std` paths to `STD_ALLOWLIST`, **but**
left half the scanner on compacted individual lines (`"".join(line.split())`) and assumed every
compiled file already lived under `src/`:

1. **`extern crate`, `include!`, `#[path = …]` and `NEEDLES` were per-line regexes (`BLOCKER`).**
   `extern\ncrate std as s;` bypassed `re.compile(r"externcrate")`, and `collect_std_paths` only
   inspected paths whose literal root token was `"std"`, so `s::process::Command::new(..)`,
   `s::env::var(..)` and `s::os::unix::fs::symlink(..)` compiled (`cargo check` exit 0) while the
   guard exited 0. Along the way, `code_tokens()` also failed to strip `r#` on raw identifiers
   (`r#std`, `r#os`), mis-lexed `b"\"…"` escaped byte strings as raw strings, and checked
   `#![forbid(unsafe_code)]` via raw substring (`in text`) — which `src/lib.rs`'s line-40 `//!`
   doc comment satisfied even when the real attribute on line 44 was deleted.
2. **`scan_manifest` never inspected `[lib].path` or `[[bin]].path` (`MAJOR`).** Pointing `[lib]`
   or `[[bin]]` at `../../outside.rs` (containing `Command::new`) while leaving clean files in
   `src/` compiled cleanly and bypassed `scan_sources` completely; symlinks inside `src/` had the
   same shape.
3. **`cfg_predicates` ran on a single compacted line (`MAJOR`).** `#[cfg\n(unix)]` and
   `cfg\n!(unix)` bypassed `_CFG_OPEN` on each half-line.
4. **No negative twin tested `STD_ALLOWLIST`'s own `std::env` self-check (`MAJOR`).** Deleting the
   `for allowed_path in STD_ALLOWLIST:` loop left all 126 assertions green.
5. **Two `MINOR`s.** The docstring/E2E prose overstated single-line literal matching and
   unscoped "by construction" coverage, and the self-test banner printed `len(cases)` (`32`) while
   building two extra uncounted trees (`34`).

## Fix — Token-Shaped Guard + Target-Root Containment

| Mechanism | What changed in `scripts/check_lsp_portable.py` |
|---|---|
| `code_tokens()` | Lexes once per `.rs` file: skips `//` and nested `/* … */` comments plus `'…'`/`b'…'` char literals, folds `"…"`/`b"…"`/`c"…"` and `(b\|c)?r#*"…"#*` raw strings into single `"str"` tokens (so prose inside strings never triggers identifier/path rules while `#[cfg(target_os = "linux")]` keeps its predicate), and normalizes `r#ident` raw identifiers to `ident`. |
| `scan_file_tokens()` | Runs **all** `source` and `environment` checks on `code_tokens()` — zero per-line regexes remain: (1) parses `extern crate X [as Y]`, FFI `extern`, and `use` trees (`_joined` fixed from `[:-4]` to `removesuffix("::self")`); (2) resolves `extern crate` + `use` crate/module aliases for `std`/`core`/`alloc` and `{libc, rustix, nix, pyo3, blastit_sys}` to a fixed point; (3) parses `#[…]` and `#![…]` brackets and `ident !` macro calls on tokens so `#[\ncfg\n(unix)]`, `cfg\n!(unix)`, `include\n!("…")`, `#[\npath\n=\n"…"]`, `extern\ncrate`, `extern\n"C"` and real `#![forbid(unsafe_code)]` inner attributes are token-exact across arbitrary whitespace, newlines and comments. |
| `scan_manifest()` + symlink checks | Verifies `[lib].path` and every `[[bin]].path` are relative `.rs` paths resolving inside `crates/blastit-lsp/src/` (`Path.resolve().is_relative_to(...)`), refuses a symlinked `src/` or any symlinked `.rs` inside `src/`, and checks `#![forbid(unsafe_code)]` on every target root. |
| `test_check_lsp_portable.py` | Every built workspace tree lives in `cases` (`len(trees) == len(cases) == 59`, `229` assertions, `43` cargo-checked with `cargo clean --offline -p blastit-lsp` before each check). Adds compiling (`cargo check` exit 0) escape fixtures for `lib-path-outside`, `bin-path-outside`, `split-extern-crate-spawn`, `split-extern-crate-os`, `split-extern-crate-env`, `three-line-extern-crate`, `split-extern-c`, `split-libc`, `split-nix`, `foreign-{rustix,pyo3,blastit-sys}`, `split-include-macro`, `split-path-attribute` (`#\n[\npath\n=\n…\n]`), `inner-path-attribute`, `split-cfg-unix`, `split-hash-cfg`, `inner-cfg-unix`, `split-cfg-attr`, `split-cfg-macro`, `macro-std-escape`, `symlink-source`, `symlink-dir`, doc-comment-only `no-forbid`, `submodule-forbid-only`, and raw-identifier + block-comment `invented-spelling`, plus `mid-ident-split` (`rus\ntix` rejected by `rustc`) and a `mutated_allowlist_guard` twin that injects `"std::env::var"` into a temp copy of the guard and asserts `{environment}`. |

### Verbatim Refusals on the Closed Classes (each `cargo check --offline -p blastit-lsp` exit 0)

```console
=== lib-path-outside (cargo check=0, guard exit=1) ===
crates/blastit-lsp/Cargo.toml: manifest: [lib] sets `path = '../../outside.rs'`, which resolves outside `crates/blastit-lsp/src` — every library and binary target must live inside `src/` so the source scan reads every compiled root

=== bin-path-outside (cargo check=0, guard exit=1) ===
crates/blastit-lsp/Cargo.toml: manifest: [[bin]] (blastit-lsp) sets `path = '../../outside_bin.rs'`, which resolves outside `crates/blastit-lsp/src` — every library and binary target must live inside `src/` so the source scan reads every compiled root

=== split-extern-crate-spawn (cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/lib.rs:8: source: contains `extern crate` — `extern crate` introduces a second crate binding outside `use`
crates/blastit-lsp/src/lib.rs:12: source: names `std::process::Command::new`, which is not one of the 10 pinned `std` paths — use an allowlisted path, or add a row to `STD_ALLOWLIST` in `scripts/check_lsp_portable.py` with the reason this crate needs it

=== split-extern-crate-os (cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/lib.rs:8: source: contains `extern crate` — `extern crate` introduces a second crate binding outside `use`
crates/blastit-lsp/src/lib.rs:12: source: names `std::os::unix::fs::symlink`, which is not one of the 10 pinned `std` paths — use an allowlisted path, or add a row to `STD_ALLOWLIST` in `scripts/check_lsp_portable.py` with the reason this crate needs it

=== split-extern-crate-env (cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/lib.rs:8: source: contains `extern crate` — `extern crate` introduces a second crate binding outside `use`
crates/blastit-lsp/src/lib.rs:12: environment: names `std::env::var`, which is not one of the 10 pinned `std` paths — use an allowlisted path, or add a row to `STD_ALLOWLIST` in `scripts/check_lsp_portable.py` with the reason this crate needs it

=== split-include-macro (cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/lib.rs:8: source: contains `include!` — `include!` compiles a file's text into the binary at build time, which is a path this scan cannot follow

=== split-path-attribute (cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/lib.rs:9: source: contains `#[path` — a `#[path = …]` module compiles a file outside `src/`, which this scan never reads — the same unreadable-file class as `include!`

=== split-cfg-unix & split-cfg-macro (cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/lib.rs:8: source: carries `cfg` predicate `unix` — the only conditional compilation this crate may have is a `#[cfg(test)]` test module
crates/blastit-lsp/src/lib.rs:9: source: carries `cfg` predicate `unix` — the only conditional compilation this crate may have is a `#[cfg(test)]` test module

=== symlink-source (cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/sneaky.rs: source: resolves outside `crates/blastit-lsp/src` — symlinks out of `src/` are refused

=== invented-spelling (`use ::r#std/* trivia */::r#os as o; o :: /* mid-path */ r#unix :: fs :: r#symlink(..)`, cargo check=0, guard exit=1) ===
crates/blastit-lsp/src/lib.rs:8: source: names `std::os`, which is not one of the 10 pinned `std` paths …
crates/blastit-lsp/src/lib.rs:11: source: names `std::os::unix::fs::symlink`, which is not one of the 10 pinned `std` paths …
```

### Load-Bearing Proof (6 Guard Mutations, Restored `cmp`-Identical)

```console
drop-allowlist-env-guard (exit=1) -> lsp-portable self-test: a `std::env` row added to `STD_ALLOWLIST`: expected the guard to fail, it exited 0 …
drop-extern-crate-alias  (exit=1) -> lsp-portable self-test: the `split-extern-crate-env` escape: expected exactly ['environment', 'source'] to go red, got ['source'] …
drop-target-path-check   (exit=1) -> lsp-portable self-test: the `lib-path-outside` escape: expected the guard to fail, it exited 0 …
drop-cfg-token-check     (exit=1) -> lsp-portable self-test: the `cfg-unix` escape: expected the guard to fail, it exited 0 …
drop-include-token-check (exit=1) -> lsp-portable self-test: the `include-macro` escape: expected the guard to fail, it exited 0 …
allow-fs-read            (exit=1) -> lsp-portable self-test: the `literal-path` escape: expected the guard to fail, it exited 0 …
cmp: byte-identical
```

### Scope Statement (Classes Still Outside a Single-Crate Lexical Scan)

1. **Re-exports or macro bodies inside an allowed dependency:** `use blastit_manifest::…` naming a
   hypothetical future `pub use std::…` inside `blastit-manifest` or `serde_json` emits no `std`
   token in `crates/blastit-lsp/src/` (verified today: neither exposes one, and `manifest` forbids
   both a third direct dependency and `build.rs`).
2. **Transitive `closure` outside the 12 named crates:** `closure` checks the named serving and
   privilege crates of design §1.3; freezing all 60 package IDs in a checked-in allowlist is a
   recorded follow-up.

## Acceptance Evidence

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
exit=0
```

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table, no feature and target paths under `src/`; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign constructs, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
exit=0
```

```console
$ UV_OFFLINE=1 PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project \
    scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
exit=0
```

```console
$ make lsp-portable
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table, no feature and target paths under `src/`; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign constructs, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
exit=0
```

```console
$ cargo tree -p blastit-lsp --edges normal | tail -n 20
│   │   │   ├── serde_json v1.0.154 (*)
│   │   │   └── thiserror v1.0.69
│   │   │       └── thiserror-impl v1.0.69 (proc-macro)
│   │   │           ├── proc-macro2 v1.0.107 (*)
│   │   │           ├── quote v1.0.47 (*)
│   │   │           └── syn v2.0.119 (*)
│   │   ├── serde v1.0.229 (*)
│   │   └── serde_json v1.0.154 (*)
│   ├── regex v1.13.1
│   │   ├── aho-corasick v1.1.5
│   │   │   └── memchr v2.8.3
│   │   ├── memchr v2.8.3
│   │   ├── regex-automata v0.4.18
│   │   │   ├── aho-corasick v1.1.5 (*)
│   │   │   ├── memchr v2.8.3
│   │   │   └── regex-syntax v0.8.11
│   │   └── regex-syntax v0.8.11
│   ├── serde v1.0.229 (*)
│   └── thiserror v2.0.21 (*)
└── serde_json v1.0.154 (*)
# 132 lines total (full tree in docs/testing/E2E-MB.35.md §2); grep for
# (libc|rustix|nix|pyo3|blastit-sys|blastit-php-sapi|blastit-py) exits 1 (0 matches).
```

```console
$ grep -n '^check:' Makefile
834:check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test doc-tone manifest-purity zed-check lsp-portable plan-validate ## Full fast unprivileged check
```

```console
$ make doc-tone docs-check docs-test
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.23s
…
PASS  docs/reference/cli.md  fence 1  dev-flags             2.2s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 2  check-flags           4.1s  each check flag runs
PASS  docs/reference/cli.md  fence 3  serve-flags           0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                 0.0s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked every command, flag and snippet (The e2e harnesses need Docker and root; the runnable-docs gate is unprivileged and Docker-free, so only the command names and the scripts they name are checked.)

48 fences: 30 executed, 18 accounted, 0 failed · 27.6s
exit=0
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 1.67s
exit=0
```

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (5 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 7 changed file(s)
  no change to a protected critical test
…
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; reaches: served 6 of 12 shims, 0 probed, 0 refused, 6 unbuilt; 12 served once the aarch64 legs run; 0 leg(s) dropped
UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py
workflow-check: ok — 449 checks over 6 workflows and 1 actions, 0 problem(s)
UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 56 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7)
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
uv run --no-project scripts/check_manifest_purity.py
manifest-purity: ok — 17 source file(s), no std::env
uv run --no-project scripts/test_manifest_purity.py
manifest-purity self-test: ok — 82 assertions, 37 fixtures proved, 17 real source file(s)
uv run --no-project scripts/check_zed_extension.py
zed-check: OK (59 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
Ran 44 tests in 11.626s — OK
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table, no feature and target paths under `src/`; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign constructs, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

## Deviations

1. **Guard size:** `scripts/check_lsp_portable.py` is a complete token-stream scanner, `use`/`extern crate` fixed-point alias resolver, target-path validator and `cargo metadata` closure walker rather than the ~200-line grep estimated in `NODE.md`, which is what closes the token-shaped escape class instead of chasing line spellings.
2. **Scoped E2E verdict:** `VERDICT: PASS — Linux x86_64 only; 4 legs NOT RUN` in `docs/testing/E2E-MB.35.md`, matching `scripts/platform_matrix.py` (`1 of 5` proved locally; macOS/Windows/aarch64 `NOT RUN` with exact runner blockers).

## Follow-ups

- Freeze the reviewed 60-package normal closure in a checked-in file to turn the transitive half of `closure` from a 12-crate denylist into an allowlist.
- Check allowed dependencies' public re-exports if `blastit-manifest` ever grows `pub use` surfaces beyond `blastfile`, `parameterize`, and model types.
- Record the first post-Release-PR `.github/workflows/ci.yml` `lsp` matrix URL and job names in `docs/testing/E2E-MB.35.md` §6 so `platform_matrix.py` can advance from `1 of 5` to `5 of 5`.
- Index `E2E-*.md` files in `docs/testing/README.md` (outside this node's `paths`).
