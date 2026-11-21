---
id: MB.26.3
title: "Pin EditError::source so the editor's error chain is walked by a test"
kind: task
status: superseded
owner: implementer
priority: 4
depends_on: []
paths: ["crates/blastit-manifest/tests/blastfile_errors.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T06:14"
updated: "2026-11-19T00:36"
---

Moved from `MB.21.5.9` (`MB.21`) to `MB.26.3` at the 0.2.0 release gate; scope, `paths`, owner and acceptance are unchanged. Ids quoted inside the text below keep their pre-move numbering — use this node's id in commands.

# Goal

Pin `EditError`'s `std::error::Error::source()` chain: `MB.21.5.3`'s campaign survived both mutants of
`impl Error for EditError::source` (`src/edit.rs:366,367`) because no test ever inspects the chain, so
a caller that walks `source()` (any `anyhow`/`thiserror` boundary in `blastit`) gets no inner error and
nothing fails.

# Context

- `EditError::Source(ManifestErrors)` is the parse/lower refusal the editor wraps; `source()` must
  return it for that arm and `None` for every other arm (the field-less refusals).
- The editor is reached through the PyO3 binding and `blastit check --learn`, whose diagnostics chain
  the two errors; an empty chain is a DX regression, not a crash.

# Acceptance

```console
$ cargo test -p blastit-manifest --test blastfile_errors
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb2157 MUTANTS_JOBS=4 \
  scripts/mutants-run.sh -p blastit-manifest -f crates/blastit-manifest/src/edit.rs
```

1. In `tests/blastfile_errors.rs` (standard tier), assert for one parse refusal fed through
   `apply_edits`: the error is `EditError::Source`, `source()` is `Some`, and its `to_string()` is the
   wrapped `ManifestErrors`' rendering (so the chain is the real inner error, not a placeholder).
2. Assert the negative half: two field-bearing arms (`InvalidRouteName`, `Verification`) return
   `source() == None`.
3. Both `edit.rs:366` and `edit.rs:367` mutants are caught; no timeout.

# Notes

- No protected file is touched. Keep the test in the standard tier: this is API shape, not a policy.
