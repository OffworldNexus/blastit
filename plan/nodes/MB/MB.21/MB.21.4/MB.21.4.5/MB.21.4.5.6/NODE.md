---
id: MB.21.4.5.6
title: "Delete the reader, the retired patcher and the model surface they filled, and drop the toml dependency - alone"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.5.1, MB.21.4.5.2, MB.21.4.5.3, MB.21.4.5.4, MB.21.4.5.5]
paths:
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/blastfile/mod.rs"
  - "crates/blastit-manifest/Cargo.toml"
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T16:58"
updated: "2026-11-16T16:58"
---

# Goal

The retired format no longer exists in Rust. `src/parse.rs` and the retired patcher inside `src/edit.rs`
are deleted, `Manifest::from_toml_str` and `Manifest::parse_and_validate` are gone, `mod parse` is gone,
the two version error variants are gone, and `toml` leaves `crates/blastit-manifest/Cargo.toml` with its
`Cargo.lock` entries.

One reader remains: `blastfile::read` → `Manifest::from_blastfile_str`. No shim, no fallback, no
`--manifest`-format switch.

# Context

- Design §15.2's rows for `MB.21.4.5`, §4.1 ("`parse.rs` is deleted, no replacement"), `R28`. This is the
  **only** leaf in `MB.21` that owns `Cargo.toml` / `Cargo.lock`; nothing else may add, remove or bump a
  dependency.
- It runs strictly alone, after every caller is gone (`MB.21.4.5.1`–`MB.21.4.5.5`). The diff is
  deletion-dominated — 1277 lines of `parse.rs` plus the retired patcher and its ~580-line test module in
  `edit.rs`, against fewer than 200 authored lines — which is why the 600-line budget does not apply here.
  Say exactly that in `RESULT.md` beside `git diff --stat`.
- **Keep** in `src/edit.rs`: `EditError` (the Blastfile writer and four suites match on its variants —
  prune only variants that neither it nor a suite uses) and `write_atomic` (boot, `fmt`, `--learn` and
  `dev_learn` call it). **Delete**: `ManifestEdits`, `ProposedRoute`, `ProposedGrant`, the plan and
  `apply_edits`, their private helpers, and the test module that covered them. Nothing outside `edit.rs`
  names them any more.
- `src/model.rs` loses `from_toml_str`, `parse_and_validate` and the doc paragraphs that describe them.
  `src/lib.rs` loses `mod parse` and its crate-doc example is rebuilt as a `Blastfile` (`postgres
  $DATABASE_URL`, one view with a `query`) with every assertion it makes about `mounts`, `php`, `name`,
  `sandbox` and `equivalent` kept. `src/error.rs` loses `MissingVersion` / `UnsupportedVersion`.
  `src/validate.rs` has one unit fixture built by the reader (and a helper that builds a per-route static
  block): build the mount equivalent through the model, or delete the case naming the family that carries
  it — the mount rules are `MB.21.4.5.2`'s.
- `src/blastfile/mod.rs`'s docs link `crate::edit::ManifestEdits`; a broken intra-doc link fails `rs-doc`
  with warnings denied. Reword to name the one reader.
- **Stay**: `Manifest::version()` (always `1`; §4.3 keeps it), `global_groups` (always empty), `PhpPolicy`,
  `StaticPolicy`, `Route::static_policy` / `php` and `Proxy::output`. The per-route pair is unreachable
  from a `Blastfile` and `docs_fences`' retired-page predicate still reads it until `MB.21.5.1`; its
  removal is a follow-up, recorded in `RESULT.md` with the paragraph below.
- Cargo: delete `toml = "1.1.8"` from `crates/blastit-manifest/Cargo.toml`; the workspace `Cargo.toml`
  carries no `toml`/`toml_edit` (confirm, do not assume); `Cargo.lock` loses the `toml*` packages when
  cargo rewrites it. No version bump, no other crate's entry touched.

# Acceptance

```console
$ test ! -e crates/blastit-manifest/src/parse.rs
$ grep -rnE 'from_toml_str|Manifest::parse_and_validate|toml::|toml_edit|^mod parse;' crates/ --include='*.rs'
$ grep -rnE 'blastit[.]toml' crates/ --include='*.rs'
$ cargo tree -p blastit-manifest --locked --edges normal | grep -ci toml
$ git diff --stat Cargo.toml Cargo.lock crates/blastit-manifest/Cargo.toml
$ make check
$ make critical-guard BASE=<sha before this node> NODE=MB.21.4.5.6
```

1. The first command exits `0`; the two greps print nothing; `grep -ci toml` prints `0`.
2. `git diff --stat` shows only the dependency removal — no version bumps, no other crate's dependencies
   moved. Paste it.
3. `make check` exits `0` (including `test-critical`, `rs-test`, `rs-lint`, `rs-doc`, `docs-unit`,
   `docs-check`), and `critical-guard` is clean: no protected test was weakened to make the deletion fit.
4. `RESULT.md` carries `git diff --stat`, one line per deleted type/function naming the leaf that removed
   its last caller, and the follow-up paragraph for the per-route `static`/`php` pair (`§4.3`'s removal,
   blocked on `MB.21.5.1` rewriting the two pages `docs_fences` reads it from).

# Notes

- `docs/` still carries the retired fences until `MB.21.5.1`; `docs-unit` stays green because
  `MB.21.4.5.3` made `docs_fences` count them instead of parsing them. Do not edit a page.
- Do not remove `UidLayout`, `Manifest::assemble` or the `#[doc(hidden)]` builders the integration suites
  use to build an expected model field by field.
- If a caller you did not expect still compiles against `from_toml_str`, stop: that is an unowned path, and
  the answer is a `## Follow-ups` entry naming the file, not a shim.
