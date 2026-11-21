---
id: MB.21.5.8
title: "Give P4's postcondition oracle a direct test and a truthful section for unresolved grants"
kind: task
status: superseded
owner: implementer
priority: 3
depends_on: []
paths:
  - "crates/blastit-manifest/src/blastfile/mod.rs"
  - "crates/blastit-manifest/src/blastfile/write.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit.rs"
  - "crates/blastit-manifest/tests/edit_branches.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T23:04"
updated: "2026-11-17T06:14"
---

# Goal

Close P4's two oracle holes found by `MB.21.5.3`'s audit (`docs/testing/AUDIT-MB.21.md` §5.3):

1. `BlastfileEdits::apply_to` (`src/blastfile/mod.rs:167`) is the public name of the postcondition
   oracle and **no test calls it**; `critical_manifest_edit.rs` reaches the same code through
   `apply_edits`. A test that calls it pins the API a caller (`fold`, `check --learn`) may use.
2. `write.rs::first_difference` (`write.rs:281`) has no `unresolved_grants` arm, so a P4 mismatch caused
   by an unresolved grant is reported as "the top-level keys".
   `critical_manifest_edit.rs:856` currently **pins** that wording, so the fix is a two-file change.

# Context

- Design `docs/design/blastfile-dsl.md` §14.5 (P4) and §13.1 (grants on an undeclared store);
  `Manifest::equivalent` excludes source positions only.
- The node that owns the editor's behaviour is `MB.21.3.5`; this node owns only the oracle gap, so it
  changes the *message* and adds a test, not the splice.
- Changing the protected assertion is authorised by this node's `paths` and justified here: the pinned
  section name becomes the truthful one, and the assertion stays at least as strong (it names the exact
  section, as it already does).

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_manifest_edit
$ cargo test -p blastit-manifest --test edit_branches
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb2156 MUTANTS_JOBS=4 \
  scripts/mutants-run.sh -p blastit-manifest -f crates/blastit-manifest/src/blastfile/write.rs
```

1. Add `unresolved_grants` to `first_difference`'s chain (after `routes`/`groups`/`proxies`/`mounts`,
   before the catch-all branch) and return "the unresolved grants" for the audit's fixture; update
   `critical_manifest_edit.rs:856`'s expected section in the same commit, with a comment saying which
   model field moved.
2. Add `a_second_difference_arm_names_its_section`-style coverage for at least two distinct arms (the
   unresolved-grant case and one other), so the chain is not pinned at one point.
3. Add a test that `BlastfileEdits::apply_to(&manifest)` equals the manifest the edited text lowers to
   for one fixture with a new view and one with a grant (the oracle's own definition).
4. The `write.rs` campaign's `first_difference` mutants are all caught; report the survivors.

# Notes

- If `Manifest` exposes the unresolved grants only through `validate`, build the "intended" model with
  the unresolved grant present instead of adding a public accessor; do not widen the model's API for a
  message.
