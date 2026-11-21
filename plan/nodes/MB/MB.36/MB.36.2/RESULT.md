# MB.36.2 — `crates/blastit-view-static`

## Summary

`blastit-view-static` exists and owns the whole static view kind. It is a
**behaviour-preserving move**: `static_asset.rs` (resolver), `static_response.rs`
(head builder) and `rewrite.rs` (precedence) left `crates/blastit-scheduler`,
which keeps dispatch and is now a consumer through a **default** `static`
feature over an optional dependency. The one request-target decoder moved to
`blastit-view::path` (D3) and is re-exported by the scheduler, so there is
exactly one `percent_decode` in the tree. The three protected suites and
`common/static_fixture.rs` moved with the code; `critical_rewrite.rs` stayed and
had only its one `#[path]` line repointed. No status code, header or precedence
rule changed: `rewrite.rs` and `static_response.rs` are **byte-identical**, and
`git diff` against the previous blobs shows the only edits are the ones the
design's §4.2.4 table names (plus one setter, explained under Evidence).

Three deviations from the design's literal wording, all forced and recorded:

1. `StaticRoot::max_bytes` was `pub(crate)`, so the scheduler's
   `root.max_bytes = cfg.static_max_bytes;` cannot cross a crate boundary. It
   became `StaticRoot::with_max_bytes` — the same assignment, mirroring the
   existing `with_prefix` builder. Behaviour is identical; the field stays
   private so no caller can reach behind the resolver.
2. The three doc references to `[`crate::path`]` in `static_asset.rs` now name
   `[`blastit_view::path`]`: `make rs-doc` runs with `RUSTDOCFLAGS="-D warnings"`,
   and the decoder is no longer in this crate.
3. The crate's normal dependencies are `blastit-view` (transport, D3),
   `blastit-manifest` (the mount policy), `rustix` (`openat2`/`fstat`/`open`)
   **and `thiserror`**, which the §4.2.4 row omits: the resolver's
   `StaticRefusal` derives `std::error::Error` through it. The only
   dev-dependency is `rustix` with the `process` feature, for the fixture's
   non-root precondition (`geteuid`); no `tokio`, no `pyo3`, no `blastit-php-sapi`.

`blastit-scheduler/src/scheduler.rs` gates the mount dispatch behind
`feature = "static"` (`#[cfg]` on the responder's items, on `Request`'s static
fields and on `Resolution`'s two mount-refusal variants), and `run()` now
refuses a manifest with a mount in a `static`-less build with
`SchedulerError::StaticNotCompiled` before it serves a channel — and that
branch is **executed**: `crates/blastit-scheduler/tests/static_feature_off.rs`
drives `run` in the `static`-less build (deny: a mount is refused; allowed: the
same boot without a mount keeps serving and is not refused), and the deny row
was proved red by deleting the refusal and restored byte-identically. The
**scheduler** compiles in both configurations with zero warnings
(`cargo check -p blastit-scheduler --no-default-features` and the default
build).

Attempt 2 also closes the **release artifact path**: `crates/blastit/src/stage1.rs`
named `blastit_scheduler::static_asset::STATIC_MAX_FILE_BYTES` unconditionally,
so this leaf's optional dependency broke
`cargo build -p blastit --no-default-features --features php` — the configuration
`scripts/build-standalone.sh` and the publish workflow build. That one field
assignment now carries `#[cfg(feature = "static")]` and a `static`-less build
takes `SchedulerConfig::default()`'s honest `0` through the literal's existing
`..SchedulerConfig::default()`; the 64 MiB ceiling is still spelled once.

## Evidence

Every command was run in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`
with `PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"`.

### 1. The new crate and its three suites

```console
$ cargo test -p blastit-view-static --locked
     Running unittests src/lib.rs (.../blastit_view_static-...)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/critical_static_assets.rs (...)
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/critical_static_entry.rs (...)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/critical_static_resolve.rs (...)
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
== EXIT 0
```

### 2. The count oracle (`--list`, a floor)

```console
$ for t in critical_static_assets critical_static_resolve critical_static_entry; do
    cargo test -p blastit-view-static --offline --test $t -- --list | grep -c ': test$'; done
critical_static_assets: 20
critical_static_resolve: 18
critical_static_entry: 6
```

| suite | design floor (§4.5) | previous revision (`HEAD`, `grep -cE '#\[(tokio::)?test\]'`) | now | verdict |
|---|---|---|---|---|
| `critical_static_assets` | 14 | 20 | 20 | ≥ floor, = previous, no case lost |
| `critical_static_resolve` | 18 | 18 | 18 | ≥ floor, = previous, no case lost |
| `critical_static_entry` | 6 | 6 | 6 | ≥ floor, = previous, no case lost |

The suites `#[path]`-include a fixture with **0** `#[test]`s
(`git show HEAD:crates/blastit-scheduler/tests/common/static_fixture.rs | grep -cE '#\[(tokio::)?test\]'`
→ `0`), so `--list` equals the attribute count in every case; the "14" column is
stale for `critical_static_assets` (the file grew in a later leaf), and the
oracle's rule — "≥ the column **and** ≥ the previous revision's `--list`" — holds.

The moved suites lost nothing: the whole diff against their previous blobs is
the `blastit_scheduler::` → `blastit_view_static::` prefix, 4 / 1 / 3 lines:

```console
$ git show HEAD:crates/blastit-scheduler/tests/critical_static_assets.rs | diff -u - crates/blastit-view-static/tests/critical_static_assets.rs
-use blastit_scheduler::rewrite::{Rewrite, decide};
+use blastit_view_static::rewrite::{Rewrite, decide};
-use blastit_scheduler::static_asset::{
+use blastit_view_static::static_asset::{
-use blastit_scheduler::static_response::{StaticResponse, http_date};
+use blastit_view_static::static_response::{StaticResponse, http_date};
-fn read_body(body: blastit_scheduler::static_response::StaticBody) -> Vec<u8> {
+fn read_body(body: blastit_view_static::static_response::StaticBody) -> Vec<u8> {
$ git show HEAD:crates/blastit-scheduler/tests/critical_static_resolve.rs | diff -u - crates/blastit-view-static/tests/critical_static_resolve.rs
-use blastit_scheduler::static_asset::{
+use blastit_view_static::static_asset::{
$ git show HEAD:crates/blastit-scheduler/tests/critical_static_entry.rs | diff -u - crates/blastit-view-static/tests/critical_static_entry.rs
-use blastit_scheduler::rewrite::{Rewrite, decide};
+use blastit_view_static::rewrite::{Rewrite, decide};
-use blastit_scheduler::static_asset::{
+use blastit_view_static::static_asset::{
-use blastit_scheduler::static_response::{StaticBody, StaticResponse};
+use blastit_view_static::static_response::{StaticBody, StaticResponse};
$ git show HEAD:crates/blastit-scheduler/tests/common/static_fixture.rs | diff -u - crates/blastit-view-static/tests/common/static_fixture.rs
-use blastit_scheduler::static_asset::{Backend, STATIC_MAX_FILE_BYTES, StaticRoot};
+use blastit_view_static::static_asset::{Backend, STATIC_MAX_FILE_BYTES, StaticRoot};
```

### 3. The scheduler, now a consumer

```console
$ cargo test -p blastit-scheduler --locked
test result: ok. 45 passed; ...   # lib (incl. path-free units, `slot_releasable`, …)
test result: ok. 11 passed; ...   # admission
test result: ok. 10 passed; ...   # critical_blastfile_parity
test result: ok. 7 passed;  ...   # critical_charge
test result: ok. 16 passed; ...   # critical_ingress_enforcement
test result: ok. 32 passed; ...   # critical_rewrite  ← stayed, #[path] repointed
test result: ok. 9 passed;  ...   # ctl_channel
test result: ok. 17 passed; ...   # fairness
test result: ok. 6 passed;  ...   # flow
test result: ok. 2 passed;  ...   # head_allocs
test result: ok. 13 passed; ...   # isolation
test result: ok. 18 passed; ...   # lifecycle
test result: ok. 14 passed; ...   # memory
test result: ok. 4 passed;  ...   # mount_precedence
test result: ok. 20 passed; ...   # pool
test result: ok. 14 passed; ...   # routing
test result: ok. 1 passed;  ...   # doctests
== 17 × "test result: ok", 0 failures
```

`critical_rewrite.rs` is unchanged except its one line:

```console
-#[path = "common/static_fixture.rs"]
+#[path = "../../blastit-view-static/tests/common/static_fixture.rs"]
```

### 4. The CLI suite, green at the leaf's end state

```console
$ cargo test -p blastit --test critical_static_cli --locked
running 11 tests
...
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
== EXIT 0
```

11, not 12: the file's twelfth `#[test]` is `#[cfg(feature = "python")]`
(`a_mixed_tree_without_an_app_still_names_blastit_app`), and this command runs
the default (`php, static`) configuration. The file is untouched
(`git diff --stat HEAD -- crates/blastit/tests/` is empty) and
`blastit`'s `default = ["php", "static"]` is what keeps it green.

### 5. `blastit-view` after the decoder moved in

```console
$ cargo test -p blastit-view --locked
test result: ok. 87 passed; ...   # lib: 82 → 87, the five moved `path::tests`
test result: ok. 10 passed; ...   # body
test result: ok. 4 passed;  ...   # concurrency
test result: ok. 12 passed; ...   # environ
test result: ok. 7 passed;  ...   # flow
test result: ok. 1 passed;  ...   # gil
test result: ok. 0 passed;  ...   # php_native (no host libphp; #[ignore] kept)
test result: ok. 30 passed; ...   # serve
test result: ok. 1 passed;  ...   # doctests
== EXIT 0

$ cargo test -p blastit-view --locked --lib -- --list | grep -c '^path::tests::'
5
path::tests::control_bytes_are_recognised
path::tests::percent_decoding_handles_every_case
path::tests::script_name_is_normalised_once
path::tests::script_name_is_stripped_at_a_segment_boundary
path::tests::target_is_cut_at_the_first_question_mark
```

### 6. The guard

```console
$ make critical-guard NODE=MB.36.2
uv run --no-project tools/critical_guard.py --base HEAD --node MB.36.2
critical-guard: HEAD..working tree, 22 changed file(s)
node MB.36.2: paths [...]
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [authorized by crates/blastit-scheduler/tests/critical_rewrite.rs]
  D  crates/blastit-scheduler/tests/critical_static_assets.rs  [authorized by crates/blastit-scheduler/tests/critical_static_assets.rs]
  D  crates/blastit-scheduler/tests/critical_static_entry.rs  [authorized by crates/blastit-scheduler/tests/critical_static_entry.rs]
  D  crates/blastit-scheduler/tests/critical_static_resolve.rs  [authorized by crates/blastit-scheduler/tests/critical_static_resolve.rs]
critical-guard: OK: all 4 protected change(s) are authorized by node(s) MB.36.2
```

### 7. The three path proofs

```console
$ test -d crates/blastit-view-static && echo "crate exists"
crate exists
$ test -z "$(ls crates/blastit-scheduler/src/static_*.rs 2>/dev/null)" && echo "static left the scheduler"
static left the scheduler
$ test -z "$(grep -rn 'openat2\|ResolveFlags' crates/blastit-scheduler/src)" && echo "no resolver left behind"
no resolver left behind
```

### 8. `cargo tree -p blastit-view-static -e normal`

```console
$ cargo tree -p blastit-view-static -e normal
blastit-view-static v0.2.0 (.../crates/blastit-view-static)
├── blastit-manifest v0.2.0 (...)
│   ├── blastit-ingress v0.2.0 (...)
│   │   ├── percent-encoding v2.3.2
...
$ cargo tree -p blastit-view-static --depth 1 -e normal
blastit-view-static v0.2.0 (...)
├── blastit-manifest v0.2.0 (...)
├── blastit-view v0.2.0 (...)
├── rustix v1.1.5
└── thiserror v2.0.21

$ cargo tree -p blastit-view-static | grep -cE 'pyo3|blastit-php-sapi'
0

$ cargo tree --workspace -i blastit-view-static -e normal
blastit-view-static v0.2.0 (...)
└── blastit-scheduler v0.2.0 (...)
    └── blastit v0.2.0 (...)
        └── blastit-py v0.2.0 (...)
```

No `pyo3`, no `blastit-php-sapi`, and the edge runs
`blastit → blastit-scheduler → blastit-view-static`, never the reverse. Both
edges into `blastit-view` are `default-features = false`:

```console
$ grep -n 'blastit-view = ' crates/blastit-scheduler/Cargo.toml crates/blastit-view-static/Cargo.toml
crates/blastit-scheduler/Cargo.toml:32:blastit-view = { version = "0.2.0", path = "../blastit-view", default-features = false }
crates/blastit-view-static/Cargo.toml:20:blastit-view = { version = "0.2.0", path = "../blastit-view", default-features = false }
$ grep -n 'blastit-scheduler = ' crates/blastit/Cargo.toml
54:blastit-scheduler = { version = "0.2.0", path = "../blastit-scheduler", default-features = false }
```

### 9. The `static` feature and the boot refusal

```console
$ cargo check -p blastit-scheduler --no-default-features       # zero warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.33s
$ cargo clippy -p blastit-scheduler --lib --no-default-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.38s
$ cargo clippy -p blastit-scheduler -p blastit-view-static -p blastit-view --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.35s
```

`run()` in a `static`-less build:

```rust
#[cfg(not(feature = "static"))]
if !cfg.mounts.is_empty() {
    return Err(SchedulerError::StaticNotCompiled);
}
```

**A refusal nobody executes is not a check**, so that branch has its own suite:
`crates/blastit-scheduler/tests/static_feature_off.rs` opens with
`#![cfg(not(feature = "static"))]` (the default build compiles it to nothing and
no other scheduler suite changes) and uses the package's own harness
(`mod common;` → `World`, which builds the cfg from the parsed `Blastfile`, a
real `http.chan` socketpair and the in-memory `CtlChannel`; nothing in `common`
is behind `static`, so nothing had to be re-implemented) to drive
`blastit_scheduler::run` directly:

* **deny** — `static "/site" { root "public" }` (the smallest mount the grammar
  allows, and `run` never opens the root) → `World::finish()` is `Err(
  SchedulerError::StaticNotCompiled)`, the message contains `--features static`
  and echoes neither `public` nor `/site` (no path, no manifest content);
* **allowed** — `view app "/*" {}` with `cfg.mounts` empty → `run` is **not**
  refused: the run loop is still serving when a 300 ms window closes
  (`timeout(...)` is `Err(Elapsed)`), which is the only shape this case can take
  because that loop ends when its peers go away, not `Ok(())`.

Both rows run in the one configuration that has the branch:

```console
$ cargo clippy -p blastit-scheduler --no-default-features --test static_feature_off --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.09s

$ cargo test -p blastit-scheduler --no-default-features --test static_feature_off --locked
     Running tests/static_feature_off.rs (...)
running 2 tests
test a_manifest_with_a_mount_is_refused_before_any_channel_is_served ... ok
test the_same_boot_without_a_mount_is_not_refused ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s
== EXIT 0
```

The same target in the default build lists **0** tests, so `make check`'s
workspace run is unchanged:

```console
$ cargo test -p blastit-scheduler --test static_feature_off
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`make check`'s workspace run says the same (`:2681` of its log):
`Running tests/static_feature_off.rs` → `running 0 tests` → `ok` — and it is
still `EXIT=0` with the new target in the tree.

**The deny row can fail** (a test that cannot fail is worse than none). With the
`if !cfg.mounts.is_empty()` block deleted from `scheduler.rs`, the deny case
goes red on its 2 s deadline and its accepted twin stays green — so the failure
is "not refused", not "any failure":

```console
$ cargo test -p blastit-scheduler --no-default-features --test static_feature_off   # refusal deleted
running 2 tests
test the_same_boot_without_a_mount_is_not_refused ... ok
test a_manifest_with_a_mount_is_refused_before_any_channel_is_served ... FAILED
---- a_manifest_with_a_mount_is_refused_before_any_channel_is_served stdout ----
thread '...' panicked at crates/blastit-scheduler/tests/static_feature_off.rs:55:10:
a mount must be refused at boot, not served: Elapsed(())
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
error: test failed, to rerun pass `-p blastit-scheduler --test static_feature_off`
```

Restored byte-identically (`cp` from a copy taken before the edit, then `cmp`),
and green again:

```console
$ cmp /tmp/opencode/scheduler.rs.before crates/blastit-scheduler/src/scheduler.rs
$ echo $?
0
$ cargo test -p blastit-scheduler --no-default-features --test static_feature_off --locked
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
== EXIT 0
$ cargo check -p blastit-scheduler --no-default-features --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.80s
```

and `SchedulerError::StaticNotCompiled` names the fix without leaking anything:

```console
"this build cannot serve a static mount: it was compiled without the `static` feature
 (rebuild with `--features static`, which `blastit`'s default features already include)"
```

### 10. Moves, not deletions

```console
$ git status --porcelain | grep -E 'scheduler/(src|tests)'
 M crates/blastit-scheduler/src/config.rs
 M crates/blastit-scheduler/src/error.rs
 M crates/blastit-scheduler/src/lib.rs
 D crates/blastit-scheduler/src/path.rs
 D crates/blastit-scheduler/src/rewrite.rs
 M crates/blastit-scheduler/src/scheduler.rs
 D crates/blastit-scheduler/src/static_asset.rs
 D crates/blastit-scheduler/src/static_response.rs
 D crates/blastit-scheduler/tests/common/static_fixture.rs
 M crates/blastit-scheduler/tests/critical_rewrite.rs
 D crates/blastit-scheduler/tests/critical_static_assets.rs
 D crates/blastit-scheduler/tests/critical_static_entry.rs
 D crates/blastit-scheduler/tests/critical_static_resolve.rs
?? crates/blastit-view-static/

$ git diff --stat -M HEAD -- crates/
 crates/blastit-scheduler/Cargo.toml                |   18 +
 crates/blastit-scheduler/src/config.rs             |    8 +
 crates/blastit-scheduler/src/error.rs              |   10 +
 crates/blastit-scheduler/src/lib.rs                |   12 +-
 crates/blastit-scheduler/src/path.rs               |  190 ---
 crates/blastit-scheduler/src/rewrite.rs            |  167 ---
 crates/blastit-scheduler/src/scheduler.rs          |  152 +-
 crates/blastit-scheduler/src/static_asset.rs       |  751 ----------
 crates/blastit-scheduler/src/static_response.rs    |  332 -----
 .../tests/common/static_fixture.rs                 |  271 ----
 crates/blastit-scheduler/tests/critical_rewrite.rs |    2 +-
 .../tests/critical_static_assets.rs                | 1499 --------------------
 .../tests/critical_static_entry.rs                 |  376 -----
 .../tests/critical_static_resolve.rs               | 1242 ----------------
 crates/blastit-view/src/lib.rs                     |    4 +
 crates/blastit/Cargo.toml                          |   23 +-
 16 files changed, 192 insertions(+), 4865 deletions(-)
```

The destination is **untracked** in this worktree, so `git diff` cannot pair the
two sides before the commit — per design §4.5 the rename proof belongs to the
lead's committed range (`git diff -M --numstat <prev>..<this>`). The byte-level
proof, run here, is stronger:

```console
$ git show HEAD:crates/blastit-scheduler/src/rewrite.rs         | cmp -s - crates/blastit-view-static/src/rewrite.rs && echo byte-identical
byte-identical
$ git show HEAD:crates/blastit-scheduler/src/static_response.rs | cmp -s - crates/blastit-view-static/src/static_response.rs && echo byte-identical
byte-identical
$ git show HEAD:crates/blastit-scheduler/src/static_asset.rs | diff -u - crates/blastit-view-static/src/static_asset.rs
-use crate::path::{path_of_target, percent_decode};
+use blastit_view::path::{path_of_target, percent_decode};
-/// (`R7`): NUL and the C0 range, exactly [`crate::path`]'s rule.
+/// (`R7`): NUL and the C0 range, exactly [`blastit_view::path`]'s rule.
-/// (`crate::path`) answers `400` for a malformed escape or invalid UTF-8
+/// (`blastit_view::path`) answers `400` for a malformed escape or invalid UTF-8
+    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self { … }   # replaces one `pub(crate)` field write
-/// One decode already happened (`crate::path`). The decoded path must begin
+/// One decode already happened (`blastit_view::path`). The decoded path must begin
$ git show HEAD:crates/blastit-scheduler/src/path.rs | diff -u - crates/blastit-view/src/path.rs
+//! (the one decoder paragraph)
-pub(crate) fn path_of_target  → +pub fn path_of_target
-pub(crate) fn percent_decode   → +pub fn percent_decode
-pub(crate) fn strip_script_name → +pub fn strip_script_name
-pub(crate) fn is_control_byte  → +pub fn is_control_byte
```

### 11. `make check`

```console
$ make check                        # full log: 5432 lines, exit 0
...
protected critical tests hold no skip construct          # :3755 — no skip in the critical tier
...
cargo clippy -p blastit --all-targets --locked -- -D warnings
    Checking blastit-view-static v0.2.0 (...)
    Checking blastit-scheduler v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.34s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
default build: 0 pyo3 dependencies (no libpython linked)  # :5310 — the standalone artifact
...
doc-tone: ok — 30 user-facing page(s), no banned phrase
manifest-purity: ok — 17 source file(s), no std::env
...
zed-check: OK (59 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
............................................
----------------------------------------------------------------------
Ran 44 tests in 9.866s

OK
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table, no feature and target paths under `src/`; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign constructs, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

The only warning in the whole log is pre-existing and unrelated
(`doc tests are not supported for crate type(s) `cdylib` in package `blastit-py``).

### 12. Lock file, formatting, metadata

```console
$ git diff Cargo.lock
+ "blastit-view",
+ "blastit-view-static",          # blastit-scheduler's new edges
+[[package]]
+name = "blastit-view-static"     # the one new workspace member

$ cargo metadata --offline --locked --format-version 1 > /dev/null && echo "metadata clean"
metadata clean
$ cargo fmt --all && cargo fmt --all -- --check && echo "FMT CLEAN"
FMT CLEAN
```

### 13. The release artifact path: `--no-default-features --features php`

`scripts/build-standalone.sh:112,115` and `.github/workflows/publish-pypi.yml:250`
build exactly this configuration, and this leaf broke it by moving
`static_asset` behind the scheduler's `static` feature while
`stage1.rs` still named `blastit_scheduler::static_asset::STATIC_MAX_FILE_BYTES`.
The fix is one `#[cfg]` (the whole diff of that file):

```console
$ git diff crates/blastit/src/stage1.rs
@@ -1441,6 +1441,12 @@ fn scheduler_config(
         manifest_dir: config.manifest_dir.clone(),
+        // A build without the responder cannot resolve that constant, and needs
+        // no ceiling: it serves no file and `run` refuses any mount at boot
+        // (`SchedulerError::StaticNotCompiled`). The update syntax below then
+        // supplies `SchedulerConfig::default()`'s own feature-less value, so the
+        // ceiling is still spelled once (design §4.6).
+        #[cfg(feature = "static")]
         static_max_bytes: blastit_scheduler::static_asset::STATIC_MAX_FILE_BYTES,
```

Evidence, after `cargo clean -p blastit-view -p blastit-view-static
-p blastit-scheduler -p blastit` so nothing is served from a cached unit:

```console
$ cargo check -p blastit --no-default-features --features php --locked
    Checking blastit-scheduler v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.02s
== EXIT 0

$ cargo build -p blastit --no-default-features --features php --locked   # what the release scripts run
   Compiling blastit v0.2.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.78s
== EXIT 0

$ cargo check -p blastit --no-default-features --locked
warning: variable does not need to be mutable          # pre-existing, stage1.rs:346
warning: fields `socket` and `source` are never read   # pre-existing, supervise.rs:226
warning: `blastit` (lib) generated 2 warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.77s
== EXIT 0

$ cargo build -p blastit --locked                        # default (php, static)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.59s
== EXIT 0
```

Both warnings are in code this leaf does not touch (the diff above is
`stage1.rs`'s only hunk, and `supervise.rs` is not in this leaf at all), so they
are the base behaviour of a feature-less `blastit`, not new; nothing gates
`-D warnings` on this configuration.

**A false alarm worth recording:** the first attempt at this check failed with
`no `path` in the root` at `blastit-scheduler/src/lib.rs:67` while
`cargo check -p blastit-scheduler --no-default-features` passed. The cause was a
**stale `blastit-view` rmeta** in this worktree's `target/debug/build/…` layout
(its dep-info did not even list `path.rs`); `touch crates/blastit-view/src/lib.rs`
made it pass and `cargo clean -p …` above removed the doubt entirely. No source
change was involved, and `git status` never showed an extra file.

## Files

Created:

- `crates/blastit-view-static/Cargo.toml`
- `crates/blastit-view-static/src/lib.rs`
- `crates/blastit-view-static/src/static_asset.rs` (from
  `crates/blastit-scheduler/src/static_asset.rs`; one `use`, three doc links,
  and `with_max_bytes`)
- `crates/blastit-view-static/src/static_response.rs` (byte-identical move)
- `crates/blastit-view-static/src/rewrite.rs` (byte-identical move)
- `crates/blastit-view-static/tests/critical_static_assets.rs` (move, imports)
- `crates/blastit-view-static/tests/critical_static_resolve.rs` (move, imports)
- `crates/blastit-view-static/tests/critical_static_entry.rs` (move, imports)
- `crates/blastit-view-static/tests/common/static_fixture.rs` (move, one import)
- `crates/blastit-scheduler/tests/static_feature_off.rs` — the `static`-less
  boot refusal, executed: deny (a mount is refused) and its accepted twin (the
  same boot without a mount keeps serving)
- `crates/blastit-view/src/path.rs` (from `crates/blastit-scheduler/src/path.rs`;
  four helpers `pub`, one doc paragraph)

Edited:

- `crates/blastit-view/src/lib.rs` — `pub mod path;`
- `crates/blastit-scheduler/src/lib.rs` — `pub use blastit_view::path;`, the
  `#[cfg(feature = "static")] pub use blastit_view_static::{…}`
- `crates/blastit-scheduler/src/scheduler.rs` — `#[cfg(feature = "static")]` on
  the responder's use lines, `StaticRead`/`StaticReadMsg`, the `Request` static
  fields, `Resolution::{MountRefused, Hidden}` and their arms, `MountHead`,
  `StaticHead`, `Resolution::mount`, `static_head`, and the ten dispatch
  functions; a `#[cfg(not(feature = "static"))]` stub for the three the view
  path calls; `run()` opens roots through `with_max_bytes` and refuses a mount
  without the feature
- `crates/blastit-scheduler/src/config.rs` — `static_max_bytes`'s default is
  feature-gated
- `crates/blastit-scheduler/src/error.rs` — `SchedulerError::StaticNotCompiled`
- `crates/blastit-scheduler/Cargo.toml` — `static` (default) + optional
  `blastit-view-static` + `blastit-view` (`default-features = false`)
- `crates/blastit-scheduler/tests/critical_rewrite.rs` — the one `#[path]` line
- `crates/blastit/Cargo.toml` — `static = ["blastit-scheduler/static"]`,
  `default = ["php", "static"]`, scheduler edge `default-features = false`
- `crates/blastit/src/stage1.rs` — the one field assignment gated, so the
  `--no-default-features --features php` release build resolves again (§13)
- `Cargo.lock` — one new workspace member and the scheduler's two new edges

Deleted (moved):

- `crates/blastit-scheduler/src/{path,rewrite,static_asset,static_response}.rs`
- `crates/blastit-scheduler/tests/common/static_fixture.rs`
- `crates/blastit-scheduler/tests/critical_static_{assets,resolve,entry}.rs`

## Follow-ups

1. **`docs/knowledge/LESSONS.md` (out of this leaf's `paths`, for the lead).**
   `tokio::select!` accepts **no** attributes on its branches (the macro has no
   `$($(#[$meta])*)?` capture), so a feature-gated branch cannot be `#[cfg]`-ed
   in place. The fix used here: give the channel a cfg'd message type
   (`type StaticReadMsg = StaticRead;` / `= ();`), stub the handler, and hold
   the sender for the whole run so the arm simply never completes. Leaves
   `MB.36.3`–`.5` will meet the same wall in `blastit-view`'s `serve` loop.
2. **Every other scheduler test target needs `static`**
   (`cargo check -p blastit-scheduler --no-default-features --all-targets`
   still fails: `critical_rewrite`, `mount_precedence` and friends import the
   responder). `static_feature_off.rs` is the one that builds in both (its crate
   is `#![cfg(not(feature = "static"))]`); the lib plus that target are the
   feature-less-buildable surface, which is why the leaf's acceptance names that
   **one** target. `MB.36.6`'s `make rs-no-default` builds `-p blastit`, which
   does not compile another package's tests, so this is not a gate today. If a
   future gate wants `--no-default-features --all-targets`, the static suites
   need `#![cfg(feature = "static")]`.
3. **Fixed here (attempt 2), not deferred.** `crates/blastit/src/stage1.rs:1444`
   wrote `blastit_scheduler::static_asset::STATIC_MAX_FILE_BYTES`
   unconditionally; with `static_asset` behind the scheduler's `static` feature
   that made `cargo build -p blastit --no-default-features --features php` — the
   command `scripts/build-standalone.sh:112,115` and
   `.github/workflows/publish-pypi.yml:250` run — fail to compile. **This leaf
   introduced that regression** (at base `4aa39d9` both `cargo check` and
   `cargo build -p blastit --no-default-features` were EXIT=0, as the reviewer
   confirmed); an earlier draft of this file wrongly claimed the build was
   already broken, and that claim is withdrawn. The fix is one `#[cfg]` on that
   field assignment (evidence in §13); nothing else in `stage1.rs` changed.
4. **`MB.36.6` — the feature matrix, now genuinely the only open piece:**
   `make rs-no-default` (the new guard), the wheel matrix, and
   `scripts/build-standalone.sh`'s own spelling. The *build* those scripts run
   with `--no-default-features --features php` compiles again as of this leaf
   (§13), but that archive still ships **without** the responder and therefore
   refuses any `static` mount at boot, so ADR-0016 decision 6's five-site edit
   (`--features php,static`) and `Makefile:209`'s stale "default features are
   `php`" comment remain for that leaf.
5. **Documents that still name the old paths (out of `paths`, for the epic's
   living-spec leaf `MB.36.6`):** `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md:21`
   and `docs/design/static-and-spa.md` (`:287`, `:390`, `:419`, `:449`, `:1132`)
   still say `crates/blastit-scheduler/src/{static_asset,static_response,rewrite}.rs`.
   They are records of where the code was; the design of record for this move is
   `ADR-0016` + `docs/design/view-crates-architecture.md`, whose §4.2.4 row
   should also gain the two amendments this leaf had to make (below).
6. **§4.2.4 amendment the design owner must fold in (kept, not reverted):**
   `StaticRoot::max_bytes` was `pub(crate)`, so the scheduler's
   `root.max_bytes = cfg.static_max_bytes;` cannot cross the crate boundary this
   leaf drew. It became `StaticRoot::with_max_bytes` — the same assignment,
   mirroring the existing `with_prefix` builder, and the field stays private
   (a setter is safer than a `pub` field; the review lens agreed). The table row
   still says "byte for byte except its one `use crate::path` line"; the real
   diff is that line, three `blastit_view::path` doc links (required by
   `make rs-doc`'s `-D warnings`), and this one setter. `MB.36.6` holds
   `docs/design/view-crates-architecture.md` and can correct the row.
7. **A second WSGI-side decoder, deliberately left alone:**
   `crates/blastit-view/src/environ.rs` carries its own lenient
   `percent_decode`/`strip_script_name` for PEP 3333's `PATH_INFO` rebuild
   (`%zz` passes through, `+` untouched). It is a different contract from the
   request-target decoder this leaf centralised, it is not in `paths`, and
   `MB.36.3` moves the file; the design's D3 sentence is about the routing
   decoder. Worth one look there so the two do not drift.
