---
id: MB.21.4.5
title: "Erase the retired manifest format from Rust: delete the reader, the TOML synthesis in stage1, the toml dependency and every remaining mention"
kind: epic
status: ready
priority: 1
depends_on: [MB.21.4.4]
paths:
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/src/routing.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/blastfile/mod.rs"
  - "crates/blastit-manifest/tests/parse.rs"
  - "crates/blastit-manifest/tests/common/mod.rs"
  - "crates/blastit-manifest/tests/critical_manifest_011.rs"
  - "crates/blastit-manifest/tests/critical_static_manifest.rs"
  - "crates/blastit-manifest/tests/critical_php_manifest.rs"
  - "crates/blastit-manifest/tests/blastfile_errors.rs"
  - "crates/blastit-manifest/tests/validate.rs"
  - "crates/blastit-manifest/tests/mutation_gaps.rs"
  - "crates/blastit-manifest/tests/routing.rs"
  - "crates/blastit-manifest/tests/grants.rs"
  - "crates/blastit-manifest/tests/uid.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - "crates/blastit-manifest/tests/static_mounts.rs"
  - "crates/blastit-manifest/Cargo.toml"
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/infer_routes.rs
  - crates/blastit/src/php_view.rs
  - crates/blastit/tests/critical_check_engine.rs
  - "crates/blastit-env/src/**"
  - "crates/blastit-env/tests/**"
  - "crates/blastit-sqlpolicy/src/learn.rs"
  - "crates/blastit-sqlpolicy/src/tables.rs"
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/**"
  - Cargo.toml
  - Cargo.lock
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T02:16"
updated: "2026-11-16T16:58"
---

# Goal

Erase the retired manifest format from the last Rust paths in the tree.

Delete, in one serialized commit:

- `crates/blastit-manifest/src/parse.rs` — the retired reader, with no replacement and no compatibility
  shim.
- `Manifest::from_toml_str` and everything in `src/model.rs` that exists only to serve it.
- The TOML synthesis in `crates/blastit/src/stage1.rs::learn_routing_table` — `--learn` splices the
  CST editor's output (`MB.21.3.5`), it never re-synthesises a document.
- The `toml` dependency (`crates/blastit-manifest/Cargo.toml`) and its `Cargo.lock` entries. Workspace
  `Cargo.toml` carries none; confirm `toml_edit` never existed rather than assuming it.
- Every remaining Rust mention: `from_toml_str` call sites, `mod parse`, the v1 patcher entry in
  `src/edit.rs`, the v1 suite `tests/parse.rs`, and `docs_fences.rs`'s contiguous retired opening.

`Cargo.toml` and `Cargo.lock` are in `paths` because this is **the only leaf in `MB.21` allowed to
touch them**; nothing else may add, remove or bump a dependency. Run it strictly alone.

# Context

- Design: §15.2 (the erasure, R27–R29), §21's `MB.21.4.5` row, and §4.1's note that `parse.rs` is
  deleted with no replacement. `MB.21.4.4` has already rewritten every fixture, script and Makefile
  stage, and `MB.21.4.3` every Python reader; this leaf only touches what they left behind.
- Measured remainder `from_toml_str` touches at planning time: `src/{model,lib,edit,routing,validate}.rs`,
  `tests/{parse,common/mod,mutation_gaps}.rs`, `tests/docs_fences.rs`, and `crates/blastit-env/src/`
  (doc examples and unit tests) plus `crates/blastit-env/tests/environment.rs`. Everything else was
  moved by `MB.21.3.7`, `MB.21.4.1`–`MB.21.4.4`. §21's row listed six paths; the additions here are the
  files that cannot compile without the reader, which §21's own rule allows ("the plan's own `paths`
  plus the additions the leaf cannot be green without"). Each is listed with its reason in `RESULT.md`.
- `tests/parse.rs` is the retired reader's suite. Delete it with its subject, and name in `RESULT.md`
  which suite now carries each cross-document refusal it covered (`tests/validate.rs`,
  `tests/critical_manifest_011.rs`, and the `critical_blastfile_*` suites). No refusable document may
  become unasserted.
- `crates/blastit-env`'s doc examples and unit tests build a `Manifest` from a literal: rewrite them to
  `Manifest::from_blastfile_str` with a `Blastfile` literal, keeping the same wiring assertions.
- `docs_fences.rs` still recognises the retired opening until `MB.21.5.2` flips it strict: build the
  opening from two fragments (the literal `"blastit"` and the literal `".toml"`, joined with `concat!`)
  so the tree never carries the contiguous name.

## Children

| Node | What | Depends |
|---|---|---|
| `MB.21.4.5.1` | Retire the v1 parser suite (delete it) and translate the protected 0.1.1 schema suite, one row per refusal re-homed | — |
| `MB.21.4.5.2` | The protected static-mount and php-entrypoint suites prove the same grammar on `static` / `entrypoint php` | — |
| `MB.21.4.5.3` | The last manifest suites (`validate`, `mutation_gaps`, `routing`, `grants`, `uid`, `common`) and `docs_fences` counting the retired opening instead of parsing it | `5.1` |
| `MB.21.4.5.4` | `RoutingTable::for_learn(&manifest)` — no synthesised document — plus the boot helpers and the `blastit-env` / `blastit-sqlpolicy` examples | — |
| `MB.21.4.5.5` | The scheduler's 45 fixtures on `Blastfile`, and the deletion of its `Source::{Toml, Blastfile}` switch | `MB.21.4.9` |
| `MB.21.4.5.6` | The erasure, alone: the reader, the retired patcher, `from_toml_str`, the two version variants and the `toml` dependency | `5.1`–`5.5` |

**Ordering.** `5.1`, `5.2` and `5.4` are path-disjoint and start immediately (three in flight); `5.3` waits
for `5.1`, which deletes the third user of `tests/common/mod.rs`; `5.5` gates on `MB.21.4.9`, which is
rewriting `crates/blastit-scheduler/tests/critical_ingress_enforcement.rs` today — `5.5` is the single leaf
that owns that file, so the two can never collide. `5.6` is last and strictly alone: it is the only leaf
that owns `crates/blastit-manifest/Cargo.toml`, `Cargo.toml` and `Cargo.lock`, so no other child may add,
remove or bump a dependency. Deterministic order: `5.1` → (`5.2` ∥ `5.3` ∥ `5.4`, with `5.5` as soon as
`MB.21.4.9` closes) → `5.6`.

**Every child obeys:** zero new dependencies, zero compatibility shim, no fallback reader; a child whose
`paths` name a protected file (`5.1`, `5.2`, `5.4`, `5.5`) says why in its `# Goal` and runs
`critical_guard.py --node` before reporting; the `# Acceptance` above stays the epic gate and no child
weakens an item of it (`5.6` is where the greps, `cargo tree` and the `Cargo.lock` diff are proven; `5.3`
is the one child that narrows a check, because `docs_fences` can no longer execute the retired opening once
the reader is gone — it keeps the counts pinned and `MB.21.5.1` restores them as executed fences, recorded
in that child's `RESULT.md`). The per-route `static`/`php` model pair (§4.3) is *not* in this epic: it is a
recorded follow-up blocked on `MB.21.5.1`, and `5.6` names it.

# Acceptance

```console
$ grep -rnE 'from_toml_str|toml::|toml_edit' crates/ --include='*.rs'
$ grep -rnE 'blastit[.]toml' crates/ --include='*.rs'
$ cargo tree -p blastit-manifest --locked --edges normal | grep -ci toml
$ git diff --stat Cargo.toml Cargo.lock
$ make check
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.5
```

1. The first two greps print nothing — no reader entry point, no `toml::` path, no retired file name in
   any Rust file — and `cargo tree … | grep -ci toml` prints `0`.
2. `crates/blastit-manifest/src/parse.rs` is gone, `git grep -n 'mod parse' crates/` prints nothing, and
   `tests/parse.rs` is gone with each of its refusals re-homed (named in `RESULT.md`).
3. `make check` exit 0 (including `test-critical`, `rs-test`, `rs-lint` and `docs-unit`), and
   `critical_guard.py --node MB.21.4.5` is clean: no protected test was weakened to make the deletion
   fit.
4. `git diff --stat Cargo.toml Cargo.lock` shows only the dependency removal — no version bumps, no
   other crate's dependencies moved.
5. `blastit-env`'s suites assert the same wiring through `Blastfile` literals, and `docs_fences.rs` still
   validates every documented document it validated before.

# Notes

- Do not add a compatibility shim, a fallback reader or a `--manifest`-format switch: after this leaf
  there is exactly one reader, and a file that does not parse is a file that does not parse.
- `crates/blastit/src/stage1.rs` keeps its behaviour: the routing table it writes comes from the CST
  editor, and the learned document is spliced, not regenerated.
- Docs still carry the retired fences until `MB.21.5.1`; keep `docs-unit` green by renaming the file the
  harness stages, never by editing a page.
