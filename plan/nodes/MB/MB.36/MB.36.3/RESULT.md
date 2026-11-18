# MB.36.3 — `crates/blastit-view-python`

## Summary

`blastit-view-python` exists and owns the CPython half of a view process:
`environ`, `input`, `request`, `response`, the `serve` worker pool, the WSGI `WsgiConfig`
and the loaded-file tracker. `crates/blastit-view` is now the **transport only** — one
framing, one credit window, one loaded-file codec, one request-target decoder — and
carries **no `pyo3` at all, normal or dev** (`cargo tree -p blastit-view -e normal|dev`
→ 0). `blastit` wires the new crate through `python = ["dep:pyo3",
"dep:blastit-view-python"]`.

Same frames, same credits, same `DONE`/`RESP_END_COST`: every moved file, every moved
suite and every unit case is accounted for below, and the only changed lines in the
moved suites are the import line and the config type's name (§4.2.2 renames it to
`WsgiConfig`).

## What moved

| Source | Before | After | Cases |
|---|---|---|---|
| `blastit-view/src/environ.rs` | 570 | `blastit-view-python/src/environ.rs` 570 | 11 → 11 (in-file) |
| `blastit-view/src/input.rs` | 568 | `…/src/input.rs` 568 | 11 → 11 |
| `blastit-view/src/request.rs` | 487 | `…/src/request.rs` 487 | 4 → 4 |
| `blastit-view/src/response.rs` | 536 | `…/src/response.rs` 536 | 5 → 5 |
| `blastit-view/src/lib.rs` (loop) | 1 238 | `…/src/serve.rs` 1 135 + `…/src/lib.rs` 62 | 6 → 6 |
| `blastit-view/src/config.rs` (WSGI fields, `validate`, 4 cases) | 118 | `…/src/config.rs` (`WsgiConfig`) 121 | 4 → 4 |
| `blastit-view/src/runtime.rs` (Python half) | 1 088 | `…/src/loaded.rs` (tracker + reporter) 599 | 2 → 2 |
| `blastit-view/tests/serve.rs` | 772 | `…/tests/serve.rs` 772 | 30 |
| `blastit-view/tests/body.rs` | 291 | `…/tests/body.rs` **byte-identical** | 10 |
| `blastit-view/tests/concurrency.rs` | 202 | `…/tests/concurrency.rs` | 4 |
| `blastit-view/tests/environ.rs` | 388 | `…/tests/environ.rs` | 12 |
| `blastit-view/tests/flow.rs` | 302 | `…/tests/flow.rs` | 7 |
| `blastit-view/tests/gil.rs` | 111 | `…/tests/gil.rs` | 1 |
| `blastit-view/tests/support/mod.rs` | 578 | `…/tests/support/mod.rs` | — |
| `blastit/tests/critical_loaded_files.rs` | 443 | `blastit-view-python/tests/critical_loaded_files.rs` 441 | 2 → 2 |

What stayed in `crates/blastit-view` (and why): `wire.rs` (302), `flow.rs` (317),
`loaded.rs` (1 113, now also holding the plain `LoadedReport`), `error.rs` (56),
`config.rs` (35: `route_name` + `frame_limits` only), `sync.rs` (13), `path.rs` (195),
`stream.rs` (37, **new**: `OutputStream`, `OUTPUT_LINE_CAP` and its one case),
`runtime.rs` (451: the trait, `RequestIo`, `RequestEnd`, `flush_response` and their two
cases — `MB.36.5` deletes them) and `php.rs` (2 559, whose only edit is the
`crate::stream::{…}` import). `frame_kind` is an unconditional `pub fn` in
`blastit-view/src/lib.rs` (the PHP worker calls it; the `cfg(any(python, php))` is gone).

`blastit-view/Cargo.toml`: `default = []` (the `python` feature and its default are
deleted), `pyo3` deleted from `[dependencies]` **and** `[dev-dependencies]`.
`blastit-view-python/Cargo.toml`: `pyo3` **without** `auto-initialize` in
`[dependencies]`, **with** it in `[dev-dependencies]`.

Call sites: `crates/blastit/src/zygote.rs` (`WsgiConfig`, `blastit_view_python::serve*`),
`crates/blastit/src/stage1.rs` (`blastit_view_python::LoadedTracker::install`),
`crates/blastit/Cargo.toml`. `cli.rs`, `lib.rs` and `blastit-py` needed no change;
`supervise.rs` builds `blastit_view::LoadedReport`, which is still reachable at that path
because the plain struct is re-exported from `loaded`.

## Evidence

Run in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` with
`PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"` and the Makefile's `RUST_ENV`
(`PYO3_PYTHON`/`PYTHONHOME`/`LD_LIBRARY_PATH` pointing at the uv interpreter).

### 1. `cargo test -p blastit-view-python --locked`

```console
     Running unittests src/lib.rs (…/blastit_view_python-…)
running 43 tests
test result: ok. 43 passed; 0 failed; 0 ignored
     Running tests/body.rs […]        running 10 tests   ok. 10 passed
     Running tests/concurrency.rs […] running 4 tests    ok. 4 passed
     Running tests/critical_loaded_files.rs […] running 2 tests   ok. 2 passed
     Running tests/environ.rs […]     running 12 tests   ok. 12 passed
     Running tests/flow.rs […]        running 7 tests    ok. 7 passed
     Running tests/gil.rs […]         running 1 test     ok. 1 passed
     Running tests/serve.rs […]       running 30 tests   ok. 30 passed
== EXIT 0
```

43 lib cases = `config` 4 + `environ` 11 + `input` 11 + `loaded` 2 + `request` 4 +
`response` 5 + `serve` 6 — the §4.2 column set exactly.

### 2. `cargo test -p blastit-view --locked`

```console
     Running unittests src/lib.rs (…/blastit_view-…)
running 44 tests
test result: ok. 44 passed; 0 failed; 0 ignored
     Running tests/php_native.rs […] running 0 tests (cfg(feature = "php") off)
     Doc-tests blastit_view             ok. 1 passed
== EXIT 0
```

44 = `flow` 8 + `loaded` 22 + `path` 5 + `runtime` 2 + `stream` 1 + `wire` 6. Nothing else
was deleted with the move.

### 3. `cargo test -p blastit --lib --locked` (the standalone, no-`python` build)

```console
running 246 tests
test result: ok. 245 passed; 0 failed; 1 ignored; 0 filtered out
== EXIT 0
```

and with the capability on, `cargo test -p blastit --lib --features python --locked`:
`test result: ok. 279 passed; 0 failed; 2 ignored`.
`cargo check -p blastit --all-targets --features python --locked` is clean.

### 4. The count oracle (`--list`, a floor)

```console
$ for t in serve body concurrency environ flow gil critical_loaded_files; do
    cargo test -p blastit-view-python --test $t --locked -- --list | grep -c ': test$'; done
serve: 30   body: 10   concurrency: 4   environ: 12   flow: 7   gil: 1
critical_loaded_files: 2
```

| suite | design floor (§4.5) | now | verdict |
|---|---|---|---|
| `serve` | 30 | 30 | = floor |
| `body` | 10 | 10 | = floor |
| `concurrency` | 4 | 4 | = floor |
| `environ` | 12 | 12 | = floor |
| `flow` | 7 | 7 | = floor |
| `gil` | 1 | 1 | = floor |
| `critical_loaded_files` | 2 | 2 | = floor, ≥ previous revision |

The previous revision's attribute counts (`git show HEAD:… | grep -cE
'#\[(tokio::)?test\]'`) are 30/10/4/12/7/1 and 2, so no suite fell.

### 5. The four named edits on `critical_loaded_files.rs` = 10 changed lines (budget ≤ 10)

```console
$ diff -u <(git show HEAD:crates/blastit/tests/critical_loaded_files.rs) \
          crates/blastit-view-python/tests/critical_loaded_files.rs
-#![cfg(feature = "python")]
-use blastit_view::{
-    LoadedReport, LoadedTracker, MAX_BATCH_BYTES, ViewConfig, decode, serve_reporting,
-};
+use blastit_view::loaded::{LoadedReport, MAX_BATCH_BYTES, decode};
+use blastit_view_python::{LoadedTracker, WsgiConfig, serve_reporting};
-                ViewConfig::for_route("web"),
+                WsgiConfig::for_route("web"),
-    let src = include_str!("../src/stage1.rs");
+    let src = include_str!("../../blastit/src/stage1.rs");
```

`grep -cE '^[-+][^-+]'` → **10**. Exactly the four named edits, nothing else; both cases
keep every assertion (`the_loaded_file_report_is_exact`,
`the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup`).

### 6. The six moved suites: imports and the renamed config type, nothing else

`diff -u <(git show HEAD:…) <new>` and `grep -cE '^[-+][^-+]'`:

| suite | changed lines | what they are |
|---|---|---|
| `serve.rs` | 22 | the one `blastit_view::{ViewConfig, ViewError}` import + 10 `ViewConfig`→`WsgiConfig` |
| `body.rs` | **0** | byte-identical move |
| `concurrency.rs` | 10 | import + 4 type names |
| `environ.rs` | 6 | import + 2 type names |
| `flow.rs` | 2 | import line only (`ViewError` from the worker crate) |
| `gil.rs` | 4 | import + 1 type name |
| `support/mod.rs` | 10 | import + 4 type names |

Filtering every changed line for anything that is neither the import nor the type name
prints nothing:

```console
$ for f in serve body concurrency environ flow gil support/mod; do
    diff -u <(git show HEAD:crates/blastit-view/tests/$f.rs) \
            crates/blastit-view-python/tests/$f.rs | grep -E '^[-+][^-+]' \
    | grep -v 'ViewConfig\|WsgiConfig\|blastit_view'; done
(no output)
```

### 7. `make critical-guard NODE=MB.36.3`

```console
critical-guard: HEAD..working tree, 31 changed file(s)
node MB.36.3: paths [...]
  D  crates/blastit/tests/critical_loaded_files.rs  [authorized by crates/blastit/tests/critical_loaded_files.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.36.3
```

### 8. `make rs-no-python`

```console
test result: ok. 245 passed; 0 failed; 1 ignored   # cargo test -p blastit --lib
test result: ok. 3 passed                          # --test no_python_build
test result: ok. 44 passed                         # -p blastit-view --no-default-features --lib
cargo clippy -p blastit --all-targets --locked -- -D warnings            Finished
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings  Finished
default build: 0 pyo3 dependencies (no libpython linked)
== EXIT 0
```

### 9. `make check`

```console
$ make check
…
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — 60 packages in the normal closure, none of 12 forbidden; …
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

The protected tier ran the moved suite from its new home:

```console
cargo test -p blastit-view-python  --test critical_loaded_files
running 2 tests
test the_dev_boot_installs_the_tracker_before_the_first_import_and_harvests_after_warmup ... ok
test the_loaded_file_report_is_exact ... ok
test result: ok. 2 passed; 0 failed; 0 ignored
```

The only warning in the whole log is the pre-existing
`warning: doc tests are not supported for crate type(s) 'cdylib' in package 'blastit-py'`.

### 10. The acceptance greps, and `pyo3` leaving the transport

```console
$ test -d crates/blastit-view-python && echo "crate exists"
crate exists
$ test -f crates/blastit-view-python/src/serve.rs && echo "serve moved"
serve moved
$ test ! -e crates/blastit-view/src/environ.rs && test ! -e crates/blastit-view/src/input.rs \
    && echo "no WSGI left behind"
no WSGI left behind
$ test -z "$(grep -n 'default' crates/blastit-view/Cargo.toml | grep -i python)" \
    && echo "python is no longer a default feature"
python is no longer a default feature
$ grep -q 'blastit-view-python' crates/blastit/Cargo.toml && echo "feature wired"
feature wired
$ cargo tree -p blastit-view -e normal --locked
blastit-view v0.2.0 (…)
├── blastit-frame v0.2.0 (…)
│   └── thiserror v2.0.21
└── thiserror v2.0.21
$ echo "pyo3 in blastit-view -e normal: $(cargo tree -p blastit-view -e normal --locked | grep -c pyo3)"
pyo3 in blastit-view -e normal: 0
$ echo "pyo3 in blastit-view -e dev: $(cargo tree -p blastit-view -e dev --locked | grep -c pyo3)"
pyo3 in blastit-view -e dev: 0
$ echo "pyo3 in blastit (default): $(cargo tree -p blastit -e normal --locked | grep -c pyo3)"
pyo3 in blastit (default): 0
$ echo "pyo3 in blastit --features python: $(cargo tree -p blastit -e normal --features python --locked | grep -c pyo3)"
pyo3 in blastit --features python: 5
```

### 11. `git diff --stat -M` and the rename view

```console
$ git diff --stat -M | tail -12
 crates/blastit/Cargo.toml                    |   9 +-
 crates/blastit/src/stage1.rs                 |   2 +-
 crates/blastit/src/zygote.rs                 |  11 +-
 crates/blastit/tests/critical_loaded_files.rs | 443 ---------
 Cargo.lock                                   |  11 +-
 31 files changed, 172 insertions(+), 7249 deletions(-)
```

The destinations are **untracked** in this working tree (I never stage), so the working-tree
diff shows the sources as deletions; rename detection sees the move on the committed range
(`git diff -M --numstat <prev>..<this>`), as §4.5 specifies. The destinations, from
`git status --porcelain`:

```console
?? crates/blastit-view-python/          # Cargo.toml, src/{lib,config,serve,loaded,environ,input,request,response}.rs,
                                       # tests/{serve,body,concurrency,environ,flow,gil,critical_loaded_files}.rs, tests/support/mod.rs
?? crates/blastit-view/src/stream.rs   # OutputStream, OUTPUT_LINE_CAP + its case
```

`cargo metadata --offline --locked` is clean; `cargo fmt --all -- --check` is clean.

## Deviations (all forced, none behavioural)

1. **`Flow::body_left` lost its `#[cfg(test)]`** and is now plain `pub`. Two moved unit
   cases (`input.rs`, `request.rs`) assert it, and they now run from `blastit-view-python`,
   where `blastit-view` is a dependency and `#[cfg(test)]` does not reach. The method's
   body is unchanged; `Credit::available` stays test-only because only `blastit-view`'s own
   tests use it.
2. **Three doc references repointed** because `make rs-doc` runs `-D warnings` and the items
   they named left the crate: `error.rs`'s `[`crate::serve`]` / `[`crate::ViewConfig::validate`]`
   became prose plus `WsgiConfig::validate`; `stream.rs`'s line cap doc names
   `[`crate::RequestIo::output`]`; `runtime.rs`'s `SlotGuard` prose says it lives in
   `blastit-view-python`'s `serve`. No assertion or code path changed.
3. **`blastit-view`'s `[features]` still declares `php` and `default = []`** — the crate is
   *not* feature-less, and this leaf never claimed it should be: `NODE.md` and `ADR-0016`
   required deleting the **`python`** default, which is done (`python` gone from
   `[features]`, `[dependencies]` and `[dev-dependencies]`; `default = []`). `php` stays
   until `MB.36.5` moves `php.rs` into `blastit-view-php` (§4.2.3, §8.4).
   `blastit` keeps `blastit-view = { …, default-features = false }`, which is now a no-op;
   removing it is `MB.36.6`'s feature-matrix leaf.
4. **`blastit-view`'s `ViewConfig` (route_name + frame_limits) has no consumer in this
   leaf** — the WSGI worker uses `WsgiConfig`, `php.rs` uses `PhpRuntimeConfig` — and is
   kept verbatim in shape because §4.2.1 keeps it and `MB.36.4`'s `SpawnContext` is its
   first caller.

## Follow-ups (out of this node's `paths`)

- `ARCHITECTURE.md:542` still lists `blastit-view` as "the Python and PHP worker runtimes"
  with `pyo3`, and `:1942` still says `blastit_view::serve`; `.github/workflows/ci.yml:19`'s
  comment ("blastit-view embeds CPython (pyo3)") is now false. All three belong to
  `MB.36.6`'s living-spec leaf.
- `crates/blastit/src/php_view.rs:13`'s module doc still cites `blastit_view::serve`.
- `scripts/check_lsp_portable.py`'s `SERVING` tuple names `blastit-view` but not
  `blastit-view-static`/`-python`/`-php`; a crate that only `blastit-lsp` could reach
  through one of those is not refused today. Recorded by the lead on **`MB.26.24`**
  (`plan/nodes/MB/MB.26/MB.26.24/NODE.md`: "A fourth item, added by the lead after
  `MB.36.3`"), which extends the tuple and checks it against `crates/`.
- `crates/blastit/src/lib.rs`'s `#[cfg(not(feature = "python"))] use blastit_view as _;`
  is now redundant (`dev_reload` uses `blastit_view::loaded` unconditionally).
- `Makefile`'s `rs-no-python` still spells `cargo check -p blastit-view
  --no-default-features`; it is harmless now (`default = []`) and §4.3 hands its rewrite to
  `MB.36.6`.
- `README.md` is **out of scope and unstaged**. It carries the release-time link rewrite
  that `scripts/build-wheel.sh` (through the docs pipeline) owns; it is not this leaf's
  change, and it was not edited, staged or reverted here.
- `plan/BUDGET.md`, `plan/JOURNAL.md` and `plan/nodes/**/{LOG,NODE}.md` carry the
  harness/lead's own writes (status, attempts, timestamps). I edited none of them and
  staged nothing; `RESULT.md` is the only file this attempt added under `plan/`.

## Post-review corrections (comments and records only)

The reviewer returned **PASS** with three corrections, all of them prose; no source, test,
manifest field or lock entry changed, and nothing was staged.

1. `crates/blastit-view/Cargo.toml`'s `[features]` comment no longer says "empty": it now
   says there is **no WSGI feature and no default feature**, that the embedded CPython
   worker is the crate of its own that removes the `libpython`-by-unification path, and
   that the `php` feature stays until `MB.36.5` moves `php.rs` into `blastit-view-php`.
   The acceptance grep still holds:
   `grep -n 'default' crates/blastit-view/Cargo.toml | grep -i python` → empty.
2. §Deviations 3 restated: `blastit-view`'s `[features]` still declares **`php` and
   `default = []`** — the crate is not feature-less. `NODE.md`/`ADR-0016` required deleting
   the **`python`** default, and that is what was done.
3. The `scripts/check_lsp_portable.py` `SERVING` follow-up is now recorded against
   **`MB.26.24`** (`plan/nodes/MB/MB.26/MB.26.24/NODE.md`: "A fourth item, added by the lead
   after `MB.36.3`"), not against `MB.36.6` or a hypothetical guard leaf.
4. `README.md` is recorded as **out of scope and unstaged**: it carries the release-time link
   rewrite that the docs pipeline through `scripts/build-wheel.sh` owns. It was not edited,
   not staged and not reverted here; it is no longer lumped in with the plan files.

Re-run after the corrections (nothing else was re-executed):

```console
$ cargo fmt --all -- --check
(clean)
$ make critical-guard NODE=MB.36.3
critical-guard: HEAD..working tree, 32 changed file(s)
  D  crates/blastit/tests/critical_loaded_files.rs  [authorized by crates/blastit/tests/critical_loaded_files.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.36.3
$ make check
…
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
$ cargo metadata --offline --locked --format-version 1
(clean)
```

The re-run's protected tier is unchanged — `cargo test -p blastit-view-python --test
critical_loaded_files` → `2 passed; 0 failed` — and the log's only warning is still the
pre-existing `blastit-py` cdylib doc-test notice.
