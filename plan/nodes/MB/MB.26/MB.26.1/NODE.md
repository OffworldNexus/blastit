---
id: MB.26.1
title: "Post-0.2.0 coverage: pin blastfile::read's accepted path so the read bound's mutants die in-crate"
kind: task
status: proposed
owner: implementer
priority: 4
depends_on: []
paths: ["crates/blastit-manifest/tests/critical_blastfile_lexer.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:14"
updated: "2026-11-17T06:14"
---

Moved from `MB.21.5.6` (`MB.21`) to `MB.26.1` at the 0.2.0 release gate; scope, `paths`, owner and acceptance are unchanged. Ids quoted inside the text below keep their pre-move numbering — use this node's id in commands.

# Goal

Give `blastfile::read` an accepted-path oracle inside `blastit-manifest`, so the read bound's
mutants die in this crate instead of only in `crates/blastit/tests/blastfile_discovery.rs`.

# Context

- `MB.21.5.3`'s campaign survived two mutants in `src/blastfile/mod.rs`:
  `262:19 replace > with <` (every small file would be refused `TooLarge`) and
  `259:39 replace + with *` in `take(MAX_SOURCE_BYTES as u64 + 1)` (only a growing file differs).
- `critical_blastfile_lexer.rs` reads exactly `MAX_SOURCE_BYTES` (successful) and nothing smaller, so
  the suite cannot see either. This is an oracle gap, not a code bug: the discovery suite would catch
  the first.
- R3a (`docs/design/blastfile-dsl.md` §4.5/§20) promises "a file over the cap is refused from its
  metadata/size without reading" *and* that an under-cap file reads; only the refusal half is pinned.

# Acceptance

```console
$ cargo test -p blastit-manifest --test critical_blastfile_lexer
$ scripts/mutants-run.sh -p blastit-manifest -f crates/blastit-manifest/src/blastfile/mod.rs
```

1. Add (do not weaken) to `crates/blastit-manifest/tests/critical_blastfile_lexer.rs`:
   - `read` of a ~20-byte regular file returns exactly its text (the accepted half);
   - the same size with mode `000` is `Unreadable` — the deny side of "the size is known before any
     read" for a file *under* the cap (the over-cap mode-`000` case already asserts `TooLarge`);
   - a file of `MAX_SOURCE_BYTES` still reads (the existing case).
2. The `262:19` mutant is caught by the new case. Keep the growing-file case out unless you can stage
   it deterministically (write the file, then extend it between `metadata` and `read` is racy in a
   single-threaded test) — record the `259:39` survivor as untestable-by-design if not.
3. `cargo test -p blastit-manifest` green; no protected assertion removed (additive edit only).

# Notes

- The file is protected: adding tests is always allowed; the node's `paths` name it so
  `tools/critical_guard.py --node` stays green.
- Use the existing `TempDir` and `sized_file` helpers; do not add a new dependency.
