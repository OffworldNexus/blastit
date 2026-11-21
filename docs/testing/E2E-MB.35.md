# E2E — MB.35 (the portable language server: the guard, the closure, two names, one archive)

Outside-in proof, every tail a command this host ran on 2026-11-18, branch `release/0.2.0`
at `1e96c03` (the release archive built at `ef93363`, zero changes to `crates/blastit-lsp` between
them), cargo 1.100.0, uv 0.13.9, Linux x86_64. The archive
is `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz`, built by `scripts/build-lsp.sh` inside
`quay.io/pypa/manylinux_2_28_x86_64` (the image whose glibc **is** the floor).

**`VERDICT: PASS` — Linux x86_64 only; 4 legs NOT RUN.** The guard and the Linux x86_64
session are proved end to end (§1–§5); the four legs that need a macOS, Windows or aarch64
runner are `NOT RUN` with the exact blocker (§6), and §7 lists what is not proved here. The
`PASS` does not extend to those rows: `scripts/platform_matrix.py` still prints them as declared
and unproven.

## 1. The promise is a gate, not a paragraph (`MB.35.6`)

`scripts/check_lsp_portable.py` reads the crate's manifest, every file under its `src/`,
`cargo metadata`'s normal closure and the `Makefile`, and refuses six named properties. A
refusal is `path:line: property: detail`, so a red run says which promise broke:

| Property | What it refuses | How it is proved (§1.2) |
|---|---|---|
| `manifest` | a third runtime dependency, a `[build-,dev-]dependencies` row, **any** `[target.…]` table, an `optional` dependency, a `[features]` entry, a `build.rs`, or any `[lib]`/`[[bin]]` `path` that resolves outside `crates/blastit-lsp/src/` (or a symlinked `src/`) | `rustix` added; `libc` under `[target.'cfg(windows)'.dependencies]`; `build.rs` created; `[lib].path` and `[[bin]].path` pointed at `../../outside*.rs` (`cargo check` 0, guard 1) |
| `source` | **a token-stream scan and a `std` allowlist, not a per-line regex.** Every `.rs` file under `src/` (refusing symlinked files and directories out of `src/`) is lexed by `code_tokens()` (comments and char literals skipped as trivia, string literals folded into single `"str"` tokens, `r#ident` stripped to `ident`). Over that token stream: `extern crate` and `use` aliases for `std`/`core`/`alloc` and the named foreign crates resolve to a fixed point (`extern\ncrate std as s; s::process::Command::new(..)` resolves to `std::process::Command::new`), every resolved `std`/`core`/`alloc` path must match one of the 10 exact rows of `STD_ALLOWLIST`, every `#[…]`/`#![…]` attribute and macro call (`include!`, `#[path = …]`, `#[cfg(…)]`, `#[cfg_attr(…)]`, `cfg!(…)`) is checked across newlines/comments so only outer `#[cfg(test)]` passes, `#![forbid(unsafe_code)]` must be a real inner attribute on every crate root (not merely prose in a comment), and `src/main.rs` is pinned byte for byte | every escape — literal paths, brace groups, brace/root/module/`self` aliases, globs, split `use`s, single-line and split `extern crate` aliases, single-line and split `include!`/`#[path]`, single-line and split `#[cfg]`/`cfg!`, symlinked files and directories out of `src/`, a doc-comment-only `#![forbid(unsafe_code)]`, `allowlist-drift`, `exact-row`, and a raw-identifier + block-comment spelling on no list — compiles (`cargo check` 0) and is refused naming the path or construct |
| `environment` | any resolved `std::env` path (direct or aliased via `extern crate` / `use`), plus any `std::env` row injected into `STD_ALLOWLIST` itself | `std::env::var("HOME")`; split `extern\ncrate std as s; s::env::var("HOME")`; a temp copy of the guard with `std::env::var` added to `STD_ALLOWLIST` |
| `closure` | a serving or privilege crate anywhere in the normal closure, unfiltered by target; a graph it cannot read | `rustix` reached **transitively** through `blastit-manifest` |
| `identity` | a second copy under `crates/blastit/src/lsp/`, a `lib.rs` that stops delegating, a second `blastit-lsp` edge | a stray `lsp/mod.rs`; the delegation line deleted; a `[dev-dependencies]` edge |
| `wiring` | `lsp-portable` missing from `make check`, or from `.PHONY`, or a target that runs only half the gate | the real `Makefile` with each of the three tokens removed |

```console
$ uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table, no feature and target paths under `src/`; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign constructs, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
exit=0

$ make lsp-portable
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table, no feature and target paths under `src/`; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign constructs, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
exit=0
```

`make check` runs it — the prerequisite is on the one line the guard parses:

```console
$ grep -n '^check:' Makefile
834:check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test doc-tone manifest-purity zed-check lsp-portable plan-validate ## Full fast unprivileged check
```

### 1.1 The boundaries this guard does and does not cross

1. **Ceiling, not floor.** Removing a dependency the code still uses is the **compiler's** red,
   not this guard's: the guard enforces the ceiling (nothing extra), `cargo` enforces the floor.
   Proved by `missing-dependency`, whose `src/lib.rs` calls `serde_json::json!` while
   `[dependencies]` names only `blastit-manifest`:

   ```console
   $ scripts/check_lsp_portable.py --root <fixture>   # guard: green
   $ cargo check --offline -p blastit-lsp              # compiler: red
   error[E0433]: failed to resolve: use of undeclared crate or module `serde_json`
   ```

2. **Never vacuous.** An empty `src/` and an unparseable manifest are refusals (§1.2, `no-source`
   and `missing-manifest`).
3. **Scope of a single-crate lexical scan.** Within `crates/blastit-lsp`'s manifest and `src/`
   token stream, target-path escapes, symlinked files and directories out of `src/`, line splits,
   whitespace, comments, raw identifiers (`r#…`), `use` groups/globs/aliases and `extern crate`
   aliases reduce to the canonical paths and constructs above. What a single-crate token scan does
   **not** read is an allowed dependency's own source: a `pub use std::…` re-export or macro body
   added inside `blastit-manifest` or `serde_json` would name no `std` token in `blastit-lsp`
   (neither exposes one today, and `manifest` forbids a third direct dependency or a `build.rs`).

### 1.2 The guard's own mutations (the twin file, run by `make lsp-portable`)

`scripts/test_check_lsp_portable.py` builds 59 miniature workspaces (`len(trees) == len(cases)` —
every built tree is counted in the banner), plus a temp mutated copy of the guard script itself,
making 229 assertions. Every escape fixture in `compiling_refusals` (43 of the 59 trees, printed
directly in the banner) is first proved to **compile** (`cargo check --offline -p blastit-lsp`
exits 0), and `mid-ident-split` proves `rus\ntix` is rejected by `rustc` itself, so what is refused
is a program Cargo accepts, never a syntax error:

| Case | Mutation | `cargo check` | Red set |
|---|---|---|---|
| `clean` | none | — | `{}` (green) |
| *(mutated guard on `clean`)* | `"std::env::var": "smuggled"` injected into `STD_ALLOWLIST` | — | `{environment}` |
| `third-dependency` | `rustix = "1"` in `[dependencies]` | — | `{manifest}` |
| `target-table` | `libc` under `[target.'cfg(windows)'.dependencies]` | — | `{manifest}` |
| `build-script` | `build.rs` created | — | `{manifest}` |
| `lib-path-outside` | `[lib] path = "../../outside.rs"` spawning `Command::new` while `src/` stays clean | `0` | `{manifest}` |
| `bin-path-outside` | `[[bin]] path = "../../outside_bin.rs"` spawning `Command::new` while `src/` stays clean | `0` | `{manifest}` |
| `literal-path` | `std::fs::read("x")` | `0` | `{source}` |
| `brace-group` | `use std::{fs, net};` | `0` | `{source}` |
| `brace-alias` | `use std::{process as p};` then `p::Command::new("ls")` | `0` | `{source}` |
| `root-alias` | `use std as s;` then `s::process::Command::new("ls")` | `0` | `{source}` |
| `glob` | `use std::*;` then `process::Command::new("ls")` | `0` | `{source}` |
| `split-root-alias` | `use std` / `as` / `s;` on three lines, then `s::process::Command::new("ls")` | `0` | `{source}` |
| `split-glob` | `use std::` / `*;` on two lines, then `process::Command::new("ls")` | `0` | `{source}` |
| `os-alias` | `use std::os as o;` then `o::unix::fs::symlink("a", "b")` | `0` | `{source}` |
| `self-item` | `use std::{self as s};` then `s::process::Command::new("ls")` | `0` | `{source}` |
| `star-item` | `use std::{*, fmt};` | `0` | `{source}` |
| `extern-crate` | `extern crate std as s;` then `s::process::Command::new("ls")` | `0` | `{source}` |
| `split-extern-crate-spawn` | `extern` / `crate std as s;` across lines, then `s::process::Command::new("ls")` | `0` | `{source}` |
| `split-extern-crate-os` | `extern` / `crate std as s;` across lines, then `s::os::unix::fs::symlink("a", "b")` | `0` | `{source}` |
| `split-extern-crate-env` | `extern` / `crate std as s;` across lines, then `s::env::var("HOME")` | `0` | `{environment, source}` |
| `three-line-extern-crate` | `extern` / `/* trivia */` / `crate` / `self` / `as` / `me;` across lines | `0` | `{source}` |
| `split-extern-c` | `pub extern` / `"C"` / `fn c_entry() {}` across lines | `0` | `{source}` |
| `split-libc` | `libc` / `::c()` across lines after a backslash-continued string literal | `0` | `{source}` |
| `split-nix` | `nix` / `::c()` across lines | `0` | `{source}` |
| `foreign-rustix` | `r#rustix` / `::c()` across lines | `0` | `{source}` |
| `foreign-pyo3` | `r#pyo3` / `::c()` across lines | `0` | `{source}` |
| `foreign-blastit-sys` | `r#blastit_sys` / `::c()` across lines | `0` | `{source}` |
| `exact-row` | `std::io::Result` — `std::io` is a row, and a row is one path, never a subtree | `0` | `{source}` |
| `allowlist-drift` | `use std::time::Instant;` — a legitimate item with no allowlist row | `0` | `{source}` |
| `cfg-unix` | `#[cfg(unix)] pub fn only_unix() {}` | `0` | `{source}` |
| `split-cfg-unix` | `#[cfg` / `(unix)]` across lines | `0` | `{source}` |
| `split-hash-cfg` | `#` / `[cfg(unix)]` across lines | `0` | `{source}` |
| `inner-cfg-unix` | `pub mod inner { #!` / `[cfg(unix)] }` across lines | `0` | `{source}` |
| `split-cfg-attr` | `#[cfg_attr` / `(unix, allow(dead_code))]` across lines | `0` | `{source}` |
| `split-cfg-macro` | `cfg` / `!` / `[unix]` across lines | `0` | `{source}` |
| `macro-std-escape` | `macro_rules! u { ($r:ident) => { use $r::process::Command; }; }` invoked as `u!(std)` | `0` | `{source}` |
| `invented-spelling` | `use ::r#std/* trivia */::r#os as o;` then `o :: /* mid-path */ r#unix :: fs :: r#symlink(..)` | `0` | `{source}` |
| `include-macro` | `include!("../../../outside.rs");` | `0` | `{source}` |
| `split-include-macro` | `include` / `!` / `("../../../outside.rs");` across lines | `0` | `{source}` |
| `path-attribute` | `#[path = "<tmp>/outside.rs"] pub mod outside;` | `0` | `{source}` |
| `split-path-attribute` | `#` / `[` / `path` / `=` / `"<tmp>/outside.rs"` / `]` across lines | `0` | `{source}` |
| `inner-path-attribute` | `pub mod outside { #![path = "<tmp>/outside.rs"] }` | `0` | `{source}` |
| `symlink-source` | `src/sneaky.rs` symlinked to `<tmp>/outside.rs` (`pub mod sneaky;`) | `0` | `{source}` |
| `symlink-dir` | `src/sneaky` symlinked to `../vendor_mod` holding `mod.rs` (`sneaky::spawn()`) | `0` | `{source}` |
| `main-not-pinned` | a fourth function added to `src/main.rs`, holding `Command::new` | — | `{source}` |
| `no-forbid` | `#![forbid(unsafe_code)]` attribute deleted while still quoted in a `//!` doc comment | `0` | `{source}` |
| `submodule-forbid-only` | crate-level `#![forbid(unsafe_code)]` moved inside `pub mod sub { … }` | `0` | `{source}` |
| `mid-ident-split` | `rus` / `tix` split mid-identifier across a newline | non-0 | *(compiler rejects)* |
| `environment` | `std::env::var("HOME")` | `0` | `{environment}` |
| `transitive-privilege` | `blastit-manifest` gains `rustix = "1"` | — | `{closure}` |
| `stray-server` | `crates/blastit/src/lsp/mod.rs` created | — | `{identity}` |
| `delegation-dropped` | `pub use blastit_lsp as lsp;` removed from the delegating crate | — | `{identity}` |
| `second-edge` | a `[dev-dependencies] blastit-lsp` edge added beside the real one | — | `{identity}` |
| `wiring-check-dropped` | `lsp-portable` removed from `check:` | — | `{wiring}` |
| `wiring-twins-dropped` | the `test_check_lsp_portable.py` line removed from the recipe | — | `{wiring}` |
| `phony-dropped` | `lsp-portable` removed from the real `.PHONY` list | — | `{wiring}` |
| `no-source` | `crates/blastit-lsp/src/` deleted | — | `{source}` |
| `missing-manifest` | `Cargo.toml` deleted | — | `{manifest}` |
| `missing-dependency` | `serde_json` removed while `src/lib.rs` calls `serde_json::json!` | `E0433` | `{}` (guard green; compiler red) |

The transitive case prints the path, which is the whole point of walking the graph instead of
grepping a lockfile:

```console
crates/blastit-lsp/Cargo.toml: closure: `rustix`, a capability crate (capset, setresuid, seccomp, fork, execve, or a C ABI), is in the normal closure: blastit-lsp -> blastit-manifest -> rustix
```

Split `extern crate` aliases and raw-identifier + block-comment paths resolve to the exact `std`
path they call rather than stopping at surface syntax:

```console
crates/blastit-lsp/src/lib.rs:12: source: names `std::process::Command::new`, which is not one of the 10 pinned `std` paths …
crates/blastit-lsp/src/lib.rs:12: environment: names `std::env::var`, which is not one of the 10 pinned `std` paths …
crates/blastit-lsp/src/lib.rs:11: source: names `std::os::unix::fs::symlink`, which is not one of the 10 pinned `std` paths …
```

And the twin suite is load-bearing across every mechanism. Six targeted mutations of
`scripts/check_lsp_portable.py`, each failing `scripts/test_check_lsp_portable.py` (`exit=1`) and
restored byte-identically (`cmp -s` clean):

```console
drop-allowlist-env-guard -> lsp-portable self-test: a `std::env` row added to `STD_ALLOWLIST`: expected the guard to fail, it exited 0 …
drop-extern-crate-alias  -> lsp-portable self-test: the `split-extern-crate-env` escape: expected exactly ['environment', 'source'] to go red, got ['source'] …
drop-target-path-check   -> lsp-portable self-test: the `lib-path-outside` escape: expected the guard to fail, it exited 0 …
drop-cfg-token-check     -> lsp-portable self-test: the `cfg-unix` escape: expected the guard to fail, it exited 0 …
drop-include-token-check -> lsp-portable self-test: the `include-macro` escape: expected the guard to fail, it exited 0 …
allow-fs-read            -> lsp-portable self-test: the `literal-path` escape: expected the guard to fail, it exited 0 …
```

The first mutation proves the allowlist self-check cannot be deleted silently; the second proves
`extern crate X as Y` alias resolution is what turns `s::env::var` into `{environment}`; rows stay
exact paths, never prefixes (`exact-row`).

## 2. The closure a reviewer reads (`cargo tree`, unfiltered by the guard)

The two dependencies are the whole `[dependencies]` table, and there is no `build.rs`:

```console
$ sed -n '/^\[dependencies\]/,$p' crates/blastit-lsp/Cargo.toml
[dependencies]
blastit-manifest = { version = "0.2.0", path = "../blastit-manifest" }
serde_json = "1.0.154"
```

No serving crate and no privilege crate is reachable, and the greps find nothing. **The direct
case is a ceiling; the transitive case is a named set.** `[dependencies]` cannot grow a third
entry without the `manifest` property refusing by name, but a *new* crate pulled in by
`blastit-manifest` is caught only because the review named the crates it must not pull: a future
`tokio`, say, would arrive through the tree and pass. The list is §1.3's, not a proof of
portability; freezing the reviewed 60-name closure in a checked-in file is the follow-up that
would close it, and it is deliberately not built here.

```console
$ cargo tree -p blastit-lsp --edges normal | grep -nE '(^|[^a-z-])(libc|rustix|nix|pyo3|blastit-sys|blastit-php-sapi|blastit-py)( |$)'
exit=1                          # no output: none of the six names occurs
$ cargo tree -p blastit-lsp --edges normal | grep -c nix
0
```

The tree, verbatim (132 lines; 56 distinct package names, since `thiserror`, `thiserror-impl`,
`itertools` and `syn` each appear at two versions — 60 package ids, which is the number the
guard's walk counts):

```text
blastit-lsp v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit-lsp)
├── blastit-manifest v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit-manifest)
│   ├── blastit-ingress v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit-ingress)
│   │   ├── percent-encoding v2.3.2
│   │   ├── serde v1.0.229
│   │   │   ├── serde_core v1.0.229
│   │   │   └── serde_derive v1.0.229 (proc-macro)
│   │   │       ├── proc-macro2 v1.0.107
│   │   │       │   └── unicode-ident v1.0.27
│   │   │       ├── quote v1.0.47
│   │   │       │   └── proc-macro2 v1.0.107 (*)
│   │   │       └── syn v3.0.9
│   │   │           ├── proc-macro2 v1.0.107 (*)
│   │   │           ├── quote v1.0.47 (*)
│   │   │           └── unicode-ident v1.0.27
│   │   ├── serde_json v1.0.154
│   │   │   ├── itoa v1.0.18
│   │   │   ├── memchr v2.8.3
│   │   │   ├── serde_core v1.0.229
│   │   │   └── zmij v1.0.23
│   │   ├── thiserror v2.0.21
│   │   │   └── thiserror-impl v2.0.21 (proc-macro)
│   │   │       ├── proc-macro2 v1.0.107 (*)
│   │   │       ├── quote v1.0.47 (*)
│   │   │       └── syn v3.0.9 (*)
│   │   └── url v2.5.8
│   │       ├── form_urlencoded v1.2.2
│   │       │   └── percent-encoding v2.3.2
│   │       ├── idna v1.1.0
│   │       │   ├── idna_adapter v1.2.2
│   │       │   │   ├── icu_normalizer v2.4.0
│   │       │   │   │   ├── icu_collections v2.4.0
│   │       │   │   │   │   ├── displaydoc v0.2.7 (proc-macro)
│   │       │   │   │   │   │   ├── proc-macro2 v1.0.107 (*)
│   │       │   │   │   │   │   ├── quote v1.0.47 (*)
│   │       │   │   │   │   │   └── syn v3.0.9 (*)
│   │       │   │   │   │   ├── potential_utf v0.1.6
│   │       │   │   │   │   │   └── zerovec v0.11.9
│   │       │   │   │   │   │       ├── yoke v0.8.4
│   │       │   │   │   │   │       │   ├── stable_deref_trait v1.2.1
│   │       │   │   │   │   │       │   ├── yoke-derive v0.8.4 (proc-macro)
│   │       │   │   │   │   │       │   │   ├── proc-macro2 v1.0.107 (*)
│   │       │   │   │   │   │       │   │   ├── quote v1.0.47 (*)
│   │       │   │   │   │   │       │   │   ├── syn v3.0.9 (*)
│   │       │   │   │   │   │       │   │   └── synstructure v0.14.0
│   │       │   │   │   │   │       │   │       ├── proc-macro2 v1.0.107 (*)
│   │       │   │   │   │   │       │   │       ├── quote v1.0.47 (*)
│   │       │   │   │   │   │       │   │       └── syn v3.0.9 (*)
│   │       │   │   │   │   │       │   └── zerofrom v0.1.8
│   │       │   │   │   │   │       │       └── zerofrom-derive v0.1.8 (proc-macro)
│   │       │   │   │   │   │       │           ├── proc-macro2 v1.0.107 (*)
│   │       │   │   │   │   │       │           ├── quote v1.0.47 (*)
│   │       │   │   │   │   │       │           ├── syn v3.0.9 (*)
│   │       │   │   │   │   │       │           └── synstructure v0.14.0 (*)
│   │       │   │   │   │   │       ├── zerofrom v0.1.8 (*)
│   │       │   │   │   │   │       └── zerovec-derive v0.11.7 (proc-macro)
│   │       │   │   │   │   │           ├── proc-macro2 v1.0.107 (*)
│   │       │   │   │   │   │           ├── quote v1.0.47 (*)
│   │       │   │   │   │   │           └── syn v3.0.9 (*)
│   │       │   │   │   │   ├── utf8_iter v1.0.4
│   │       │   │   │   │   ├── yoke v0.8.4 (*)
│   │       │   │   │   │   ├── zerofrom v0.1.8 (*)
│   │       │   │   │   │   └── zerovec v0.11.9 (*)
│   │       │   │   │   ├── icu_normalizer_data v2.4.0
│   │       │   │   │   ├── icu_provider v2.4.0
│   │       │   │   │   │   ├── displaydoc v0.2.7 (proc-macro) (*)
│   │       │   │   │   │   ├── icu_locale_core v2.4.0
│   │       │   │   │   │   │   ├── displaydoc v0.2.7 (proc-macro) (*)
│   │       │   │   │   │   │   ├── litemap v0.8.3
│   │       │   │   │   │   │   ├── tinystr v0.8.5
│   │       │   │   │   │   │   │   ├── displaydoc v0.2.7 (proc-macro) (*)
│   │       │   │   │   │   │   │   └── zerovec v0.11.9 (*)
│   │       │   │   │   │   │   ├── writeable v0.6.4
│   │       │   │   │   │   │   └── zerovec v0.11.9 (*)
│   │       │   │   │   │   ├── writeable v0.6.4
│   │       │   │   │   │   ├── yoke v0.8.4 (*)
│   │       │   │   │   │   ├── zerofrom v0.1.8 (*)
│   │       │   │   │   │   ├── zerotrie v0.2.5
│   │       │   │   │   │   │   ├── displaydoc v0.2.7 (proc-macro) (*)
│   │       │   │   │   │   │   ├── yoke v0.8.4 (*)
│   │       │   │   │   │   │   └── zerofrom v0.1.8 (*)
│   │       │   │   │   │   └── zerovec v0.11.9 (*)
│   │       │   │   │   ├── smallvec v1.16.4
│   │       │   │   │   └── zerovec v0.11.9 (*)
│   │       │   │   └── icu_properties v2.4.0
│   │       │   │       ├── displaydoc v0.2.7 (proc-macro) (*)
│   │       │   │       ├── icu_collections v2.4.0 (*)
│   │       │   │       ├── icu_locale_core v2.4.0 (*)
│   │       │   │       ├── icu_properties_data v2.4.0
│   │       │   │       ├── icu_provider v2.4.0 (*)
│   │       │   │       ├── zerotrie v0.2.5 (*)
│   │       │   │       └── zerovec v0.11.9 (*)
│   │       │   ├── smallvec v1.16.4
│   │       │   └── utf8_iter v1.0.4
│   │       └── percent-encoding v2.3.2
│   ├── blastit-sqlpolicy v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit-sqlpolicy)
│   │   ├── pg_query v6.2.1
│   │   │   ├── itertools v0.10.5
│   │   │   │   └── either v1.19.0
│   │   │   ├── prost v0.13.5
│   │   │   │   ├── bytes v1.12.1
│   │   │   │   └── prost-derive v0.13.5 (proc-macro)
│   │   │   │       ├── anyhow v1.0.104
│   │   │   │       ├── itertools v0.14.0
│   │   │   │       │   └── either v1.19.0
│   │   │   │       ├── proc-macro2 v1.0.107 (*)
│   │   │   │       ├── quote v1.0.47 (*)
│   │   │   │       └── syn v2.0.119
│   │   │   │           ├── proc-macro2 v1.0.107 (*)
│   │   │   │           ├── quote v1.0.47 (*)
│   │   │   │           └── unicode-ident v1.0.27
│   │   │   ├── serde v1.0.229 (*)
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
```

The design's honest boundary is visible in that tree and is not a defect: `libpg_query` is a
**C** parser inside `pg_query`, and its `cc`/`bindgen` are build dependencies of *that* crate.
The artifact has a byte-identical parse on all five targets only because the C compiles there;
what the crate's own code may not do is reach a privilege call, and §1's `source` property plus
`#![forbid(unsafe_code)]` are that half of the claim.

## 3. One session, two names — `MB.35.3`'s equivalence obligation

Both binaries answer the same framed session with byte-identical frames. The driver
(`/tmp/opencode/mb356/session.py`) sends `initialize` → `initialized` → `didOpen` (a
`Blastfile` with `SELCT`, mis-indented) → `formatting` → `hover` → `codeAction` →
`didChange` (the typo fixed) → `shutdown` → `exit`, prints one `<sent|recv> <json>` line per
frame, normalises `serverInfo.version` to `<version>`, and gives the child **no `PATH`**.

```console
$ uv run --no-project /tmp/opencode/mb356/session.py --cwd /tmp/opencode/mb356/clean \
    <abs>/target/debug/blastit lsp > t1.txt          # exit=0, 0 bytes on stderr
$ uv run --no-project /tmp/opencode/mb356/session.py --cwd /tmp/opencode/mb356/clean \
    <abs>/target/debug/blastit-lsp > t2.txt          # exit=0, 0 bytes on stderr
$ diff -u t1.txt t2.txt ; echo "exit=$?"
exit=0

$ sha256sum t1.txt t2.txt
924f0d2403cba41fd49b2a91560614e4b8d734dd82830907d93938a2fe349349  t1.txt
924f0d2403cba41fd49b2a91560614e4b8d734dd82830907d93938a2fe349349  t2.txt
```

The transcript (t1 == t2, byte for byte):

```text
sent {"id": 1, "jsonrpc": "2.0", "method": "initialize", "params": {"capabilities": {}, "processId": null}}
recv {"id": 1, "jsonrpc": "2.0", "result": {"capabilities": {"codeActionProvider": true, "documentFormattingProvider": true, "hoverProvider": true, "positionEncoding": "utf-16", "textDocumentSync": {"change": 1, "openClose": true, "save": false}}, "serverInfo": {"name": "blastit", "version": "<version>"}}}
sent {"jsonrpc": "2.0", "method": "initialized", "params": {}}
sent {"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {"languageId": "blastfile", "text": "postgres $DATABASE_URL\n\nview products GET \"/products/\" {\n        postgres {\n            query 'SELCT id FROM products'\n        }\n}\n", "uri": "file:///work/Blastfile", "version": 1}}}
recv {"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"diagnostics": [{"message": "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`", "range": {"end": {"character": 24, "line": 4}, "start": {"character": 19, "line": 4}}, "severity": 1, "source": "blastit"}], "uri": "file:///work/Blastfile", "version": 1}}
sent {"id": 2, "jsonrpc": "2.0", "method": "textDocument/formatting", "params": {"options": {"insertSpaces": true, "tabSize": 4}, "textDocument": {"uri": "file:///work/Blastfile"}}}
recv {"id": 2, "jsonrpc": "2.0", "result": [{"newText": "postgres $DATABASE_URL\n\nview products GET \"/products/\" {\n    postgres {\n        query 'SELCT id FROM products'\n    }\n}\n", "range": {"end": {"character": 0, "line": 7}, "start": {"character": 0, "line": 0}}}]}
sent {"id": 3, "jsonrpc": "2.0", "method": "textDocument/hover", "params": {"position": {"character": 15, "line": 4}, "textDocument": {"uri": "file:///work/Blastfile"}}}
recv {"id": 3, "jsonrpc": "2.0", "result": {"contents": {"kind": "markdown", "value": "`query 'SELECT \u2026'` allows one exact statement; repeat the directive for more. It is trusted as written: read it before pasting."}, "range": {"end": {"character": 17, "line": 4}, "start": {"character": 12, "line": 4}}}}
sent {"id": 4, "jsonrpc": "2.0", "method": "textDocument/codeAction", "params": {"context": {"diagnostics": []}, "range": {"end": {"character": 20, "line": 4}, "start": {"character": 15, "line": 4}}, "textDocument": {"uri": "file:///work/Blastfile"}}}
recv {"id": 4, "jsonrpc": "2.0", "result": []}
sent {"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {"contentChanges": [{"text": "postgres $DATABASE_URL\n\nview products GET \"/products/\" {\n        postgres {\n            query 'SELECT id FROM products'\n        }\n}\n"}], "textDocument": {"uri": "file:///work/Blastfile", "version": 2}}}
recv {"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"diagnostics": [], "uri": "file:///work/Blastfile", "version": 2}}
sent {"id": 5, "jsonrpc": "2.0", "method": "shutdown", "params": null}
recv {"id": 5, "jsonrpc": "2.0", "result": null}
sent {"jsonrpc": "2.0", "method": "exit", "params": null}
recv <exit status 0>
recv <stderr ''>
```

The one documented difference is the argument, and it is the documented one — `blastit lsp`
takes `--help` and refuses an unknown flag with `2`, the portable binary ignores any argument
and ends at `1` when stdin closes:

```console
$ target/debug/blastit lsp --help | head -1 ; echo "exit=${PIPESTATUS[0]}"
usage: blastit lsp
exit=0
$ target/debug/blastit lsp --bogus </dev/null ; echo "exit=$?"
blastit lsp: `--bogus` is not an argument: `blastit lsp` speaks LSP on stdin/stdout and reads no file
exit=2
$ target/debug/blastit-lsp --bogus </dev/null ; echo "exit=$?"
exit=1
```

`docs/reference/cli.md` says the portable binary has **no** `--help` and **no** `--version`;
that is executed too, from the released archive (out of the clean directory of §4):

```console
$ /tmp/opencode/mb356/clean/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp --help </dev/null ; echo "exit=$?"
exit=1
$ /tmp/opencode/mb356/clean/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp --version </dev/null ; echo "exit=$?"
exit=1
```

## 4. The archive, from a clean directory, with no `PATH` entry

`MB.35.5`'s script, the shipping Linux invocation (the earlier archive had been deleted from
this shared worktree, so it was rebuilt at `ef93363`: 2000639 bytes against `MB.35.5`'s 2000669,
30 bytes apart, and gzip's own timestamp moves that number anyway — the identity that matters is
the binary's hash, `sha256 8c27938bdcb0fe9060ab837d88174f2acffe2e5b952794b9a02e4780b4e5b7aa`):

```console
$ bash scripts/build-lsp.sh --target x86_64-unknown-linux-gnu \
    --image quay.io/pypa/manylinux_2_28_x86_64 --max-glibc 2_28 --out target/lsp-dist
build-lsp: x86_64-unknown-linux-gnu inside quay.io/pypa/manylinux_2_28_x86_64 (floor 2.28)
cargo 1.100.0 (3acc79c0c 2026-11-05)
build-lsp: building as uid 1000:1000, cache /tmp/build-cargo-home
build-lsp: glibc glibc 2.28
build-lsp: cargo build -p blastit-lsp --release --locked --target x86_64-unknown-linux-gnu
    Finished `release` profile [optimized] target(s) in 4.60s
build-lsp: 0 pyo3 dependencies (no libpython in the graph)
ok   /src/target/lsp-2_28/x86_64-unknown-linux-gnu/release/blastit-lsp: NEEDED libgcc_s.so.1 libpthread.so.0 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
build-lsp: getcap empty (supported unprivileged)
build-lsp: glibc floor 2.28 (readelf --version-info)
build-lsp: floor 2.28 <= --max-glibc 2.28
build-lsp: 2000639 bytes -> target/lsp-dist/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
build-lsp: entries -> blastit-lsp-x86_64-unknown-linux-gnu/{blastit-lsp,LICENSE,SHA256SUMS}
build-lsp: checksum -> target/lsp-dist/SHA256SUMS
exit=0
```

Unpacked into a directory that holds nothing else (`/tmp/opencode/mb356/clean`), with the
page's own `tar -xzf` — the Linux spelling of the `text` block on
`docs/reference/cli.md#blastit-lsp`:

```console
$ cd /tmp/opencode/mb356/clean && tar -xzf <abs>/target/lsp-dist/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
$ find . -maxdepth 2 | sort
.
./blastit-lsp-x86_64-unknown-linux-gnu
./blastit-lsp-x86_64-unknown-linux-gnu/LICENSE
./blastit-lsp-x86_64-unknown-linux-gnu/SHA256SUMS
./blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp
$ cd blastit-lsp-x86_64-unknown-linux-gnu && sha256sum -c SHA256SUMS
blastit-lsp: OK
LICENSE: OK
$ getcap blastit-lsp ; echo "exit=$? (nothing printed = no capability)"
exit=0
```

The same session of §3, driven through the extracted file, with `PATH` removed from its
environment and its working directory holding no `Blastfile`:

```console
$ uv run --no-project /tmp/opencode/mb356/session.py --cwd /tmp/opencode/mb356/clean \
    /tmp/opencode/mb356/clean/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp > t3.txt
$ diff -u t1.txt t3.txt ; echo "diff=$?"
diff=0
$ sha256sum t1.txt t3.txt
924f0d2403cba41fd49b2a91560614e4b8d734dd82830907d93938a2fe349349  t1.txt
924f0d2403cba41fd49b2a91560614e4b8d734dd82830907d93938a2fe349349  t3.txt
```

`MB.35.5`'s smoke is the second opinion on the same bytes:

```console
$ uv run --no-project scripts/lsp_smoke.py --binary /tmp/opencode/mb356/clean/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp
lsp_smoke: ok — /tmp/opencode/mb356/clean/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp: initialize + codeActionProvider, one severity-1 diagnostic at 4:15, shutdown/exit 0, 0 bytes on stderr
exit=0
```

And the resolution step `docs/troubleshooting.md` tells a user to run, in both worlds:

```console
$ ( PATH=/usr/bin:/bin; which blastit-lsp; echo "exit=$?" )
exit=1
$ ( PATH=/tmp/opencode/mb356/clean/blastit-lsp-x86_64-unknown-linux-gnu:/usr/bin:/bin; which blastit-lsp )
/tmp/opencode/mb356/clean/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp
exit=0
```

## 5. The page's "never does" claims, executed rather than read

`docs/reference/cli.md#blastit-lsp` says the server "reads no `./Blastfile`, no `BLASTIT_*`
variable and no configuration". The clean directory now holds a deliberately **broken**
`./Blastfile`, and the child's environment exports three `BLASTIT_*` variables:

```console
$ cat /tmp/opencode/mb356/clean/Blastfile
postgres $DATABASE_URL

view broken "/x" {
    query SELCT

$ export BLASTIT_APP=never_read BLASTIT_URL=postgres://never BLASTIT_ROUTE=also_never
$ uv run --no-project session.py --cwd /tmp/opencode/mb356/clean <the archive's blastit-lsp> > t4.txt
$ diff -u t1.txt t4.txt ; echo "diff=$?"
diff=0
$ sha256sum t4.txt
924f0d2403cba41fd49b2a91560614e4b8d734dd82830907d93938a2fe349349  t4.txt
```

Byte-identical to a session with neither: the diagnostics in the transcript come from the
buffer the *client* sent, and nothing on the host was read to produce them.

## 6. The platform matrix: one leg proved, four `NOT RUN`

`scripts/platform_matrix.py --summary` declares five `lsp` legs and marks exactly one proved:

```console
$ uv run --no-project scripts/platform_matrix.py --summary
| family | declared | built & exercised | dropped (reason) |
|---|---|---|---|
| wheels | 8 | 4 of 8 | — |
| standalone archives | 4 | 2 of 4 | — |
| PHP shims | 12 | 6 of 12 | — |
| lsp assets (editor) | 5 | 1 of 5 | — |
```

| Asset | Runner | Evidence |
|---|---|---|
| `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz` | `ubuntu-24.04` | **PROVED** — §1–§5: built in the shipping manylinux image, floor measured 2.28, unpacked, driven through six LSP methods twice, `sha256sum -c` and `getcap` clean |
| `blastit-lsp-aarch64-unknown-linux-gnu.tar.gz` | `ubuntu-24.04-arm` | `NOT RUN` — no aarch64 runner on this host, and no aarch64 C toolchain (`command -v aarch64-linux-gnu-gcc` prints nothing), so even a cross build of `libpg_query` is not possible here; `build-lsp.sh` refuses a foreign `--target` by design, because an artifact nothing executed is a claim, not a proof |
| `blastit-lsp-aarch64-apple-darwin.tar.gz` | `macos-latest` | `NOT RUN` — no macOS host, no CI runner, and no network to dispatch the workflow from here. Only the first `.github/workflows/ci.yml` run after the Release PR can prove it |
| `blastit-lsp-x86_64-apple-darwin.tar.gz` | `macos-15-intel` | `NOT RUN` — same blocker |
| `blastit-lsp-x86_64-pc-windows-msvc.zip` | `windows-latest` | `NOT RUN` — same blocker, plus the `LIBCLANG_PATH` finding of `MB.35.1` §Q1 that only a Windows runner can confirm |

The `lsp` job that will produce this evidence exists in `.github/workflows/ci.yml` (`MB.35.5`)
and runs `scripts/build-lsp.sh` and `scripts/lsp_smoke.py` per leg. It has never run: a
workflow file on a branch is not a CI run, and no URL is pasted here to pretend otherwise.

## 7. What this record does not prove

1. **No Zed.** `docs/local.md#editors`' install step (`zed: install dev extension`), the
   `format_on_save` merge and the squiggle are `NOT RUN`: Zed is not installed on this host and
   its extension runtime is not reproducible outside it. §3 drives the same protocol Zed
   drives, which is everything but the editor's own UI.
2. **No macOS, no Windows, no Apple silicon.** §6.
3. **No signing or notarisation.** `xattr -d com.apple.quarantine` is a user-side step in the
   page; nothing on Linux can execute it.
4. **The wheel and the serving binary are out of scope.** This page proves the editor's server;
   `blastit lsp`'s capability drop and the serving path are `MB.35.4`'s suites and the E2E
   records of their own milestones.

VERDICT: PASS — Linux x86_64 only; 4 legs NOT RUN. Every command this page documents was
executed on Linux x86_64 (§1–§5); aarch64, both macOS legs, Windows and the editor itself are
recorded as `NOT RUN` with their blockers (§6, §7) rather than as passes.
