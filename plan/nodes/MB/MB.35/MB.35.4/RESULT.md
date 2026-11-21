# Result MB.35.4

**Status: done.** The language server now lives in `crates/blastit-lsp`; `blastit lsp` is one
delegation behind one `run()`, and nothing in the protocol changed.

## What moved

| Site | Change |
|---|---|
| `crates/blastit/src/lsp/mod.rs` → `crates/blastit-lsp/src/lib.rs` | `git mv` (98 % similarity). Header keeps every line; `#![forbid(unsafe_code)]` added; `use blastit_sqlpolicy::parameterize;` → `use blastit_manifest::parameterize;`; the two comments that named the old paths (`crate::lsp::run` → `crate::run`, `mod.rs` → `lib.rs`) corrected. 12 added / 6 removed lines, body untouched |
| `crates/blastit/src/lsp/hover.rs` → `crates/blastit-lsp/src/hover.rs` | `git mv` (98 %). Two `include_str!("../../../../docs/reference/blastfile.md")` → `../../../…`; the `mod.rs`/`blastit lsp` sentence names `lib.rs` / `blastit-lsp` |
| `crates/blastit-lsp/Cargo.toml` (**new**) | the design's §1.3 manifest verbatim: `[[bin]] blastit-lsp`, `[lib] blastit_lsp`, `[dependencies]` = `blastit-manifest` + `serde_json` only, no other table, no `build.rs` |
| `crates/blastit-lsp/src/main.rs` (**new**) | `#![forbid(unsafe_code)]` + one `fn main` + `std::process::exit(blastit_lsp::run() as i32)`. The panic hook and the `#![deny(…)]` header moved with `run()`/`lib.rs` byte for byte |
| `crates/blastit-lsp/tests/critical_lsp_stdio.rs` (**new, protected**) | the standalone binary's own proof: capability document, a `query` typo as a severity-1 diagnostic at the exact range, `shutdown`+`exit` → `0`, a cleared **and** a poisoned environment, and the one documented argv difference |
| `crates/blastit/src/lib.rs` | `pub mod lsp;` → `pub use blastit_lsp as lsp;` (3-line comment); `cli.rs`, including `hold_none()` before the call, untouched |
| `crates/blastit/Cargo.toml` | one edge: `blastit-lsp = { version = "0.2.0", path = "../blastit-lsp" }` |
| `crates/blastit-manifest/src/lib.rs` | one line, as `ADR-0013` amendment 2 requires: `pub use blastit_sqlpolicy::parameterize;` (+3 comment lines) |
| `Cargo.lock` | one `[[package]] blastit-lsp` entry (8 lines) + `blastit`'s own dependency line. No version moved |
| `crates/blastit/tests/critical_lsp.rs` | **assertions untouched**: 2 `include_str!` literals + the 2-line comment that named them (below) |

`crates/blastit/src/lsp/` no longer exists (the empty directory was removed).

## Acceptance

| # | Command | Verdict |
|---|---|---|
| 1 | `uv run --no-project tools/plan.py validate` | `plan OK` |
| 2 | `cargo build -p blastit-lsp --locked` | green |
| 3 | `cargo test -p blastit-lsp` | 16 + 0 + 3 + 0 passed |
| 4 | `cargo test -p blastit --test critical_lsp` (`timeout 180`) | 52 passed, 0 failed |
| 5 | `cargo clippy -p blastit-lsp -p blastit --all-targets -- -D warnings` | green, zero warnings |
| 6 | `git diff --find-renames --stat` | both files reported as renames (98 %) |
| 7 | `cargo tree -p blastit-lsp --edges normal` | 132 lines, 0 forbidden crates (pasted below) |
| — | `make check` | `EXIT=0` |
| — | `make critical-guard NODE=MB.35.4` | `critical-guard: OK: all 1 protected change(s) are authorized` |

### 1–3

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK

$ cargo build -p blastit-lsp --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
# (the first, non-`--locked` build wrote the lock entry and compiled in 2.00s:
#  "Compiling blastit-lsp v0.2.0 … Finished … in 2.00s")

$ timeout 300 cargo test -p blastit-lsp
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/main.rs (target/debug/build/blastit-lsp/…/blastit_lsp-…)

running 0 tests

     Running tests/critical_lsp_stdio.rs (target/debug/build/blastit-lsp/…/critical_lsp_stdio-…)

running 3 tests
test an_argument_is_ignored_and_end_of_input_exits_one ... ok
test a_poisoned_environment_changes_nothing ... ok
test the_standalone_binary_speaks_the_same_session ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

   Doc-tests blastit_lsp

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The 16 unit tests are the moved ones exactly: `git show HEAD:crates/blastit/src/lsp/mod.rs`
had 14 `#[test]`s and `hover.rs` had 2 — 16 then, 16 now, same names.

### 4 — the delegation, unchanged and green

```console
$ timeout 180 cargo test -p blastit --test critical_lsp
test work_done_progress_is_answered ... ok
test diagnostics_are_capped ... ok
test no_position_of_a_validation_error_is_invented ... ok
test oversized_document_is_not_stored ... ok
test positions_line_split_and_none ... ok

test result: ok. 52 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.22s
```

### 5

```console
$ cargo clippy -p blastit-lsp -p blastit --all-targets -- -D warnings
    Checking blastit-manifest v0.2.0 (…/crates/blastit-manifest)
    Checking blastit-lsp v0.2.0 (…/crates/blastit-lsp)
    Checking blastit-scheduler v0.2.0 (…)
    Checking blastit-env v0.2.0 (…)
    Checking blastit v0.2.0 (…/crates/blastit)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.34s
# a second run is a cache hit ("Checking blastit-lsp … Finished in 0.22s")
```

`RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-lsp -p blastit --no-deps --locked` is also green
(so `cli.rs:576`'s `[`crate::lsp::run`]` intra-doc link still resolves through the re-export).

### 6 — the move is a move

```console
$ git diff --find-renames --stat          # literal acceptance command (worktree vs index)
 Cargo.lock                                         |   9 +
 crates/blastit-lsp/src/hover.rs                    |   8 +-
 crates/blastit-lsp/src/lib.rs                      |  18 +-
 crates/blastit-manifest/src/lib.rs                 |   4 +
 crates/blastit/Cargo.toml                          |   1 +
 crates/blastit/src/lib.rs                          |   6 +-
 crates/blastit/tests/critical_lsp.rs               |   8 +-
 … (the other rows are pre-existing edits in this shared worktree — docs/, plan/, python/; see Follow-ups)

$ git diff HEAD --find-renames --summary
 rename crates/{blastit/src/lsp => blastit-lsp/src}/hover.rs (98%)
 rename crates/{blastit/src/lsp/mod.rs => blastit-lsp/src/lib.rs} (98%)
```

The literal command compares the worktree to the index, and `git mv` staged the two deletions
with the new paths added; `git diff HEAD -M` is the command that shows the pair as a rename. Both
are pasted above. The 18 changed lines of `lib.rs` are the crate doc's first paragraph, the
`parameterize` paragraphs, the `forbid` attribute + its sentence, the import line and the
`crate::run` comment; the remaining 1 585 lines are byte-identical to `HEAD`'s `mod.rs`.

`Cargo.lock`:

```diff
@@ -123,6 +123,7 @@ dependencies = [
  "blastit-ingress",
+ "blastit-lsp",
  "blastit-manifest",
@@ -205,6 +206,14 @@ dependencies = [
+[[package]]
+name = "blastit-lsp"
+version = "0.2.0"
+dependencies = [
+ "blastit-manifest",
+ "serde_json",
+]
+
```

One package entry plus the `blastit` edge that names it; no version moved, nothing else added.

### 7 — the portability evidence

```
$ cargo tree -p blastit-lsp --edges normal | wc -l
132
$ cargo tree -p blastit-lsp --edges normal | grep -Ec '\blibc\b|rustix|\bnix\b|pyo3|blastit-sys|blastit-dbproxy|blastit-scheduler|blastit-http|blastit-view|blastit-py|blastit-php-sapi'
0
```

The 132 lines are pasted at the end of this file. `blastit-manifest` → `blastit-ingress` +
`blastit-sqlpolicy` → `pg_query 6.2.1` (its `cc`/`bindgen` are that crate's **build** deps, so
they are not in a normal tree) and `serde_json`; no privilege crate, no serving crate, no `blastit`.

### The new protected suite's payload

`crates/blastit-lsp/tests/critical_lsp_stdio.rs` asserts the diagnostic, not just its presence:

```console
$ uv run --no-project /tmp/opencode/transcript.py target/debug/blastit-lsp
{"id": 1, "jsonrpc": "2.0", "result": {"capabilities": {"codeActionProvider": true, "documentFormattingProvider": true, "hoverProvider": true, "positionEncoding": "utf-16", "textDocumentSync": {"change": 1, "openClose": true, "save": false}}, "serverInfo": {"name": "blastit", "version": "0.2.0"}}}
{"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"diagnostics": [{"message": "`qery` is not a directive here; a grant block takes `transactions`, `functions`, `select`, `insert`, `update`, `delete` and `query`", "range": {"end": {"character": 12, "line": 4}, "start": {"character": 8, "line": 4}}, "severity": 1, "source": "blastit"}], "uri": "file:///typo", "version": 1}}
{"id": 2, "jsonrpc": "2.0", "result": null}
exit: 0 stderr:
```

`range.start` is `line 4, character 8` — the fixture's `qery` starts at byte 8 of the line (four
spaces of block indent, four of grant-block indent); `range.end.character` is 12, the word's end.
The fixture is `concat!`-built because a `\` continuation strips the next line's indentation, and
the indentation **is** the column this case asserts.

That same script run against `target/debug/blastit lsp` produces a byte-identical transcript
(`diff` empty) — the two binaries answer one session identically, which is the cross-check
`MB.35.6` records in `docs/testing/E2E-MB.35.md`; it is not a substitute for that gate.

### `git status --short` (this node's rows)

```
 M Cargo.lock
RM crates/blastit/src/lsp/hover.rs -> crates/blastit-lsp/src/hover.rs
RM crates/blastit/src/lsp/mod.rs -> crates/blastit-lsp/src/lib.rs
 M crates/blastit-manifest/src/lib.rs
 M crates/blastit/Cargo.toml
 M crates/blastit/src/lib.rs
 M crates/blastit/tests/critical_lsp.rs
?? crates/blastit-lsp/Cargo.toml
?? crates/blastit-lsp/src/main.rs
?? crates/blastit-lsp/tests/
```

`crates/blastit/src/lsp/` appears only as the source half of the two renames.

### The protected file's entire diff (8 lines)

```diff
@@ -2046,11 +2046,11 @@ fn the_lsp_arm_drops_capabilities() {
 fn the_lsp_module_is_a_text_tool() {
-    // §1's entry is `lsp/mod.rs` and its hover data lives beside it as
-    // `lsp/hover.rs`: both are this module, so both carry the same list.
+    // §1's entry is `blastit-lsp/src/lib.rs` and its hover data lives beside it
+    // as `blastit-lsp/src/hover.rs`: both are this module, so both carry the list.
     let sources = [
-        include_str!("../src/lsp/mod.rs"),
-        include_str!("../src/lsp/hover.rs"),
+        include_str!("../../blastit-lsp/src/lib.rs"),
+        include_str!("../../blastit-lsp/src/hover.rs"),
     ];
```

Four literals and the comment that names them; every assertion, every property list and every
`expected` byte is untouched (`the_lsp_module_is_a_text_tool` is green in the 52).

### `make check` and the guard

```console
$ make check
… ok   lib: no process
… ok   lib: no capabilities
… ok   settings: format_on_save
… ok   docs: settings fence
zed-check: OK (47 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
..........................
Ran 26 tests in 9.230s
OK
uv run --no-project tools/plan.py validate
plan OK
EXIT=0

$ make critical-guard NODE=MB.35.4
critical-guard: HEAD..working tree, 30 changed file(s)
node MB.35.4: paths ['crates/blastit-lsp/**', 'crates/blastit/src/lsp/**', 'crates/blastit/src/lib.rs', 'crates/blastit/Cargo.toml', 'Cargo.toml', 'Cargo.lock', 'crates/blastit-manifest/src/lib.rs', 'crates/blastit/tests/critical_lsp.rs']
  M  crates/blastit/tests/critical_lsp.rs  [authorized by crates/blastit/tests/critical_lsp.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.35.4
```

`make check`'s `rs-no-python` and `seam-check` are green: the new crate adds only a
`blastit-manifest` edge to the `blastit` graph, and the standalone binary's ELF is unchanged.

## Follow-ups (out of this node's paths)

1. **`docs/**` still prints the old paths.** `docs/design/blastfile-lsp-zed.md` §1 row 1, §1.2,
   §7.5/S5's table and §12's table quote `crates/blastit/src/lsp/{mod,hover}.rs` as history;
   `ADR-0013`'s pre-amendment text at :38/:174/:193 does the same. The design records the move
   ("the content of every row is unchanged by the move"), so this is a doc leaf's or the lead's
   edit, not a stale source path — no code, script, workflow or e2e harness names `src/lsp`.
2. **For `MB.35.6`'s guard author:** §1.3 row 3 bans the token `extern "C"` in every `src/**` file
   except `main.rs`, and a **doc comment** quoting it counts. My first header sentence quoted it
   and would have turned `make lsp-portable` red; it now says "a foreign declaration needs
   `unsafe`". Worth one sentence in that brief so the next writer does not lose an hour.
3. `crates/blastit/tests/critical_lsp.rs` keeps its 13-token source grep; per §14.2 the
   artifact-level gate (token table, `use`-origin classification, manifest API allowlist) is
   `MB.35.6`'s `scripts/check_lsp_portable.py`, deliberately not this protected file.
4. The design's §1.3 row-3 rule refuses `#[cfg(…)]` other than `#[cfg(test)]` on a test module:
   the moved code has exactly two, both `#[cfg(test)]`, on `mod tests` in `lib.rs` and `hover.rs`.

## Environment notes

- This worktree carries other nodes' uncommitted edits (`docs/**`, `plan/**`,
  `python/tests/test_docs_assets.py` at 06:12, before this attempt began at 06:32). They are not
  mine; the rows above are the whole of this node's change.
- No cargo build collision was hit; every command ran sequentially and no run was attributed to a
  host flake.

## Appendix: `cargo tree -p blastit-lsp --edges normal`

```
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
