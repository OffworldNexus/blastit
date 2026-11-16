---
id: MB.21.3.0
title: "Model parity (a): Manifest name/mounts/php/equivalent, StaticMount, Route.sandbox, MountTable, uid by sandbox, StaticPolicy::fallback"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.2]
paths:
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/routing.rs"
  - "crates/blastit-manifest/src/uid.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/parse.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/tests/routing.rs"
  - "crates/blastit-manifest/tests/validate.rs"
  - "crates/blastit-manifest/tests/uid.rs"
  - "crates/blastit-manifest/tests/parse.rs"
  - "crates/blastit-manifest/tests/critical_manifest_011.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit_executor.rs"
  - "crates/blastit-manifest/tests/edit_branches.rs"
  - "crates/blastit-manifest/tests/grants.rs"
  - "crates/blastit-manifest/tests/common/mod.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T02:15"
updated: "2026-11-16T03:05"
---

# Goal

Land the **model half** of the Blastfile cutover in `crates/blastit-manifest`, so that the parser
`MB.21.3.1`–`MB.21.3.8` build has the model Blastfile needs while the retired reader still drives the
running product:

- `Manifest` gains `name: Option<String>`, `mounts: Vec<StaticMount>` and `php: Option<PhpPolicy>`,
  with `name()`, `mounts()`, `php()` and the crate-internal builders `with_name`/`with_mounts`/
  `with_php` (design §4.3).
- `Manifest::equivalent(&self, other) -> bool` compares every field **except** `route_lines` (§4.3,
  used by `fmt`'s postcondition, `--learn`'s `verify`, and the parity oracle).
- `StaticMount { prefix, policy }`, exported from the crate root.
- `StaticPolicy` is **not** touched in this leaf: `spa` stays exactly as it is and the new
  `fallback: Option<String>` field, with the removal of `spa: bool`, is `MB.21.3.7`'s (§9.3). Every
  `StaticPolicy { … }` literal in the workspace lives in a file of that leaf's list (the scheduler's
  `route.rs` and its `static_fixture.rs`, `critical_rewrite.rs`, the two manifest suites), so adding the
  field here would leave the workspace red outside this leaf's paths.
- `Route` gains `sandbox: u32` plus `Route::with_sandbox`; `UidLayout` and `check_uids` number by
  **sandbox**, so several endpoints of one `view` share one uid (§2.3, §7.3).
- `routing::MountTable` and `MountTable::lookup`: segment-boundary prefix matching, longest prefix wins
  (`/assets` never matches `/assets-secret`) (§9.2).

This leaf is **design `MB.21.3.0a`**. `docs/design/blastfile-dsl.md` §21 splits model parity into
`3.0a` (model/routing/uid/validate) and `3.0b` (the scheduler and `blastit` call sites plus the three
suites that assert the old accessors). The plan tool has no letter-suffixed ids, so the two halves are
`MB.21.3.0` and `MB.21.3.7`, chained by `depends_on`.

# Context

- Design: §4.3 (the only model changes, and the removal list), §9.1–§9.5 (mounts, `fallback`, the
  `url_path` rule), §4.4 (`equivalent`), §2.3/§7.3 (`UidLayout` by sandbox), §21 (this leaf's paths
  and acceptance). ADR-0012 records the decisions; a design that re-opens one is out of scope.
- **Removal vs. a green tree.** `Route::static_policy()`, `Route::with_static`, `Route::php()` and
  `Route::with_php` are deleted by the *pair*, not here: §4.3's "compile errors are the work list" is
  `MB.21.3.7`'s job. Here you land the new surface and rewrite this crate's own tests. Where a removal
  would break a crate outside this leaf, keep one **temporary inherent method** that reads the new
  field, name every such bridge in `RESULT.md`, and `MB.21.3.7` deletes it.
- **The bridge this leaf keeps.** The per-view `static`/`php` fields and their four accessors stay as
  they are, and `src/parse.rs` fills them *and* the new `Manifest.mounts`/`Manifest.php` from the same
  TOML source, so today's runtime reads one answer either way. `RESULT.md` names each bridge with its
  deletion owner (`MB.21.3.7`), and no new code outside this crate may start reading the old accessors.
- **The retired reader keeps working.** `src/parse.rs` is deleted by `MB.21.4.5`, so in this leaf it
  must produce the new model: a `[route.static]` block becomes a `StaticMount` whose `prefix` is that
  mount's own literal path; `spa = true` lowers to `fallback = Some(index)`. The assertions in
  `critical_static_manifest.rs` (rewritten by `MB.21.3.7`) then still describe today's behaviour.
- If a suite cannot stay true without a rule the design does not pin, stop and open a `human` request:
  do not invent a mapping, and do not weaken the suite.
- `#![deny(missing_docs)]` is on: every new public item needs a doc comment, and the crate-level doctest
  in `lib.rs` names the accessors that move.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked
$ make rs-fmt
$ make rs-lint
$ cargo check --workspace --all-targets --locked
$ make check
```

1. `cargo check --workspace --all-targets --locked` and `make rs-lint` (`-D warnings`) exit 0: the other
   crates still build, through a bridge where required, and every bridge is named in `RESULT.md` with
   the accessor it replaces.
2. `tests/uid.rs` plus the module doctest prove the new numbering: two `Route`s with the same `sandbox`
   get one uid, the next sandbox gets the next uid, and a mount owns no uid.
3. `Manifest::equivalent` ignores `route_lines` and honours every other field: a test mutates one field
   at a time and asserts inequality, and a document whose only difference is a line number asserts
   equality.
4. `MountTable` matches only on a segment boundary and prefers the longest prefix (`/assets`,
   `/assets/v2`, `/assets-secret` table test, both directions).
5. `StaticPolicy` and every existing literal of it compile unchanged: this leaf adds no field to it and
   the retired reader keeps its `spa` handling byte for byte. The new `fallback: Option<String>` field and
   the removal of `spa: bool` are `MB.21.3.7`'s (§9.3), whose `# Acceptance` restates this item.
6. `make check` exit 0, and no TOML code path was removed: `grep -n '^toml' crates/blastit-manifest/Cargo.toml`
   still prints the dependency.

# Notes

- `crates/blastit-manifest/src/lib.rs` and `crates/blastit-manifest/tests/common/mod.rs` are in `paths`
  although §21's row omits them: `StaticMount`/`MountTable` must be exported from the crate root
  (the crate-level docs and doctests name the moving accessors), and `tests/common/mod.rs` is the
  shared harness every suite in this crate is built on.
- Do **not** rename `Manifest::version()`, `Manifest::global_groups()` or `PhpPolicy::workers`: §4.3
  keeps them, always `1`, always empty, always `None`, so `--json`, `effective_grants` and the
  scheduler need no change.
- `StaticOutcome::fallback_missing` and the runtime precedence are **not** here: the rename lands with
  `MB.21.3.7` (the enum lives in `blastit-scheduler`), the precedence with `MB.21.4.2`.
