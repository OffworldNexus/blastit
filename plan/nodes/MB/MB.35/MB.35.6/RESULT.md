# RESULT — MB.35.6: make the portability promise enforced, then prove the documented commands

STATUS: done (attempt 4 — the lead's redesign: the `source` property is an **allowlist of `std`
paths**, so the class is closed by construction instead of enumerated)

## Round 4 — the redesign, and the tail it replaces

Rounds 1–3 each added needles and each review found a spelling the list did not have. This round's
two proofs of that tail both **compiled, were fmt-clean and passed the 18-needle guard**:

- `use std::os as o; o::unix::fs::symlink("a", "b")` — `os` was missing from `FORBIDDEN_MODULES`;
- `use std as\ns;` and `use std::\n*;` — the split rule only matched a line ending exactly `usestd`.

A deny-list over spellings has an unbounded tail by construction, so it is gone. `source` now
checks **paths**, and every path must be on a pinned allowlist:

| Mechanism | What it does |
|---|---|
| `code_tokens()` | tokenises each file of `src/`; comments and string literals are trivia, so a doc paragraph quoting `std::` is not a hit while real code always is |
| `parse_use_tree()` | flattens a `use` tree into full paths, so `as` renames and `::{…}` groups are resolved (`use std::{io::{self, Read}, process as p};` → `std::io`, `std::io::Read`, `std::process`); the *path* is what is checked, which is why an alias cannot hide one |
| `collect_std_paths()` | every fully-qualified `std::…` path in the code, plus every `use` tree, filtered to `std` |
| `STD_ALLOWLIST` | the 10 `std` paths the crate may name, each with the reason it needs it — exact paths, **never prefixes** (a row for `std::io` does not allow `std::io::Result`) |
| the allowlist may not hold `std::env` | the guard refuses an `environment` row itself, so the "reads no environment" promise cannot be switched off by editing a list |

The 10 rows, derived from the source and not guessed:

| Path | Why the crate needs it |
|---|---|
| `std::cell::OnceCell` | the per-document cell the session caches before it fills it |
| `std::collections::BTreeMap` | the `documents` map, ordered so a replay is deterministic |
| `std::io` | `io::stdin`/`io::stdout`: the frames are this process's only transport |
| `std::io::Read` | reading one frame's body off stdin |
| `std::io::Write` | writing one frame to stdout |
| `std::panic::AssertUnwindSafe` | the parser/formatter call is wrapped (design §3.3) |
| `std::panic::catch_unwind` | so a panic in the parser declines a request, never kills the session |
| `std::panic::set_hook` | the hook that keeps a panic payload off stderr |
| `std::process::exit` | `main.rs`'s pinned three-line entry point |
| `std::str::from_utf8` | a frame's header block is bytes; `Content-Length` is read as text |

Kept exactly as they were: the `manifest` ceiling, `environment` (now fed by the path scan and
un-allowlistable), `closure`, `identity`, `wiring`, the `#[cfg(test)]`-only `cfg` rule,
`#![forbid(unsafe_code)]`, the pinned `src/main.rs`, and the foreign or unreadable-file needles
(`libc::`, `rustix`, `nix::`, `pyo3`, `blastit_sys`, `extern "C"`, `include!`, `#[path = …]`,
`extern crate`) — 9 of them, none of which is a `std` path.

### The class is closed, not the instances

Every spelling from three rounds is a fixture that **compiles** (`cargo check --offline -p
blastit-lsp` exits 0) and is **refused** with the exact property label: `literal-path`,
`brace-group`, `brace-alias`, `root-alias`, `glob`, `split-root-alias`, `split-glob`, `os-alias`,
`self-item`, `star-item`, `extern-crate`, `include-macro`, `path-attribute`, plus `exact-row`
(no prefix widening) and `allowlist-drift` (a legitimate item with no row). 32 fixtures,
126 assertions.

And the proof that it is a class: a fixture built from a spelling on **no list anywhere** —
`std :: os :: unix :: fs :: symlink("a", "b")`, with the spaces no formatter produces — is
refused, because the allowlist checks the path and not the spelling:

```console
invented-spelling: source: names `std::os::unix::fs::symlink`, which is not one of the 10 pinned `std` paths …
```

### Still honest about what a source scan cannot see

1. **A `std` re-export from a dependency** — `use blastit_manifest::process;` would name no `std`
   path. Checked today: neither `blastit-manifest` nor `blastit-sqlpolicy` has a `pub use std`.
   Closing it in general means checking a dependency's public API — a different guard.
2. **Generated code** — a proc-macro dependency or a `build.rs` could emit a spawn the scan never
   reads. Both are refused `manifest`-side (`build.rs` forbidden, a third dependency a ceiling
   breach), so an escape has to break that promise first.
3. **`closure` is a named-set denylist** (round-2 finding 5): the ceiling makes the direct case
   true, the transitive case is only as good as the list. Freezing the reviewed 60-name closure
   is the recorded follow-up.

## Acceptance

```console
$ PATH="$HOME/.local/bin:$PATH:$HOME/.cargo/bin" uv run --no-project tools/plan.py validate
plan OK
```

```console
$ PATH="$HOME/.local/bin:$PATH:$HOME/.cargo/bin" uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table and no feature; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign spellings, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
exit=0
```

```console
$ UV_OFFLINE=1 PATH="$HOME/.local/bin:$PATH:$HOME/.cargo/bin" uv run --no-project \
    scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 126 assertions over 32 fixtures
exit=0     (10.6s: 15 `cargo check` runs included)
```

```console
$ make lsp-portable
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table and no feature; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign spellings, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 126 assertions over 32 fixtures
exit=0
```

```console
$ cargo tree -p blastit-lsp --edges normal | grep -nE '(^|[^a-z-])(libc|rustix|nix|pyo3|blastit-sys|blastit-php-sapi|blastit-py)( |$)'
exit=1     (no output: none of the six names occurs; the full 132-line tree is in
           docs/testing/E2E-MB.35.md §2)
```

```console
$ grep -n '^check:' Makefile
834:check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test doc-tone manifest-purity zed-check lsp-portable plan-validate ## Full fast unprivileged check
```

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (13 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 17 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked --features python -- -D warnings
… cargo doc --workspace --no-deps --locked --features python
… cargo test --workspace --all-targets --locked --features python        (189 "test result: ok" lines)
… make test-critical / py-lint / docs-unit / docs-check / docs-test
48 fences: 30 executed, 18 accounted, 0 failed · 21.1s
… rs-no-python / seam-check / platform-matrix-check / workflow-check / packaging-gates-test
… doc-tone / manifest-purity
… zed-check / test_check_zed_extension (44 tests, OK)
uv run --no-project scripts/check_lsp_portable.py
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table and no feature; 3 source file(s) naming 10 of 10 pinned `std` paths and none of 9 foreign spellings, with no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
lsp-portable self-test: ok — 126 assertions over 32 fixtures
uv run --no-project tools/plan.py validate
plan OK
exit=0     (3m55s; 189 `test result: ok`, no failure)
```

```console
$ make doc-tone docs-check docs-test
… (docs-check: the site built with --strict, "No issues found")
48 fences: 30 executed, 18 accounted, 0 failed · 23.3s
exit=0
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 1.66s
exit=0
```

## Is the redesign load-bearing?

Three mutations on backup copies, each restored byte-identically (`cmp` clean):

```console
allow-fs        -> lsp-portable self-test: the `literal-path` escape: expected the guard to fail, it exited 0 …
allow-env-exact -> lsp-portable self-test: a clean fixture must be green, got 1:
                   scripts/check_lsp_portable.py: environment: the allowlist holds `std::env::var` —
                   no `std::env` path may be allowed …
drop-use-parser -> lsp-portable self-test: the `brace-group` escape: the refusal does not say
                   'names `std::fs`' — crates/blastit-lsp/src/lib.rs:9: source: names `std`, which is not …
```

The first says the allowlist *is* the gate; the second says the `environment` promise survives an
edit to the list; the third says the `use`-tree flattening is what closes aliases and groups.

## Rounds 1–3, kept because the reviewers saw them

- **Round 1**: the guard, the `lsp-portable` target wired into `make check`, the E2E page with the
  closure, the three byte-identical transcripts and the archive run from a clean directory.
  One red `make check`: my own `cargo build -p blastit` (default features) replacing the shared
  `target/debug/blastit`, restored with `--features python` (`LESSONS.md` line 80).
- **Round 2**: five needles plus the bare-`std::process` ban with `main.rs` pinned by file; the
  `delegation-dropped` / `second-edge` / `phony-dropped` twins; the verdict scoped to Linux
  (`VERDICT: PASS — Linux x86_64 only; 4 legs NOT RUN`); the archive's 30-byte size note with the
  binary's `sha256 8c27938b…`; the closure property documented as a named-set denylist.
  Two attributed reds: a sibling node's concurrent `make check` truncating `target/docs-wheel`, and
  `zed-check` red on their in-flight `editors/zed` edits.
- **Round 3**: the alias/glob/path family closed one spelling at a time (18 needles, 8 twins with
  `cargo check` proofs) — which is exactly the enumeration that this round replaced.

## Deviations

1. **The guard is 755 lines, not the ~200 the brief estimated, and it grew again this round.** The
   redesign bought a closed class instead of a shorter file: a tokenizer, a `use`-tree flattener,
   a path collector and the allowlist. No function exceeds ~90 lines and every line is under 100
   characters.
2. **`VERDICT: PASS` is scoped to Linux x86_64** (`… — Linux x86_64 only; 4 legs NOT RUN`).

## Follow-ups

- **Freeze the reviewed closure** (60 package ids in a checked-in file) to close the transitive
  half of `closure`.
- **A dependency's `std` re-export** is unchecked; the two manifests the crate sees expose none.
- `scripts/platform_matrix.py` keeps printing the four non-Linux legs as unproven until the first
  CI run after the Release PR; `docs/testing/E2E-MB.35.md` §6 must gain that URL and job names.
- `docs/testing/README.md` does not index the `E2E-*.md` records (neither this one nor
  `E2E-MB.34.md`); an index is a one-line edit outside this node's paths.
- The E2E's session driver lives at `/tmp/opencode/mb356/session.py` (a throwaway, like
  `E2E-MB.34.md`'s `compact.py`); a permanent two-name comparison belongs beside
  `scripts/lsp_smoke.py` as a `--compare` mode — a new node, not this leaf.
