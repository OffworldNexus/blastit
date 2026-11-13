---
id: MA.2.2.7
title: Prove write_atomic never writes through a planted temp symlink
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_manifest_edit.rs"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T14:56"
updated: "2026-11-12T15:08"
---

# Goal

Prove `write_atomic` never writes through a planted file or symlink at its
`.<filename>.<pid>.tmp` name, and that a stale temp file is replaced rather than followed.

# Context

- Audit `docs/testing/AUDIT-MA.2.2.md`, mutant `mf.write_atomic_create_new_off`
  (`create_new(true)` -> `create(true)`): it survived `critical_manifest_edit` and the whole
  crate. `MA.2.2.2`'s RESULT claims "a symlink planted at that name is refused, never written
  through", and no test exercises it.
- Proven in a scratch copy: with the mutant, a symlink at the temp name is followed and its
  target is overwritten with the manifest content; on the tip the link is unlinked and the
  manifest is written (`VICTIM: "victim\n"`).
- The rustdoc says the planted name is "refused"; the code removes a stale temp file and
  retries. Keep the security property (never write through) and assert it; do not assert an
  error return that the tip does not produce.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_manifest_edit --locked
```

- New: create `blastit.toml`, a `victim.txt`, and a symlink `.<name>.<pid>.tmp -> victim.txt`;
  `write_atomic` succeeds, `victim.txt` is byte-identical, the manifest holds the new content,
  and no `.tmp` entry remains.
- New (denied twin): a **regular file** planted at the temp name is replaced, its old content
  is gone from the directory, and no `.tmp` entry remains — the retry path is exercised, not
  a silent write-through.
- Keep `a_failed_write_removes_its_temp_file` untouched.

# Notes

- Test-only node; use a unique directory per test (the existing `TempDir` helper).
- If the retry branch is judged acceptable documentation drift, also fix the `write_atomic`
  rustdoc sentence "a planted symlink at that name is refused" in a separate implementer node.
