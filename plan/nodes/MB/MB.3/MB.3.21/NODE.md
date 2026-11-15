---
id: MB.3.21
title: "check --learn must not drop a route's executor block: edit.rs expected_routes loses [route.static] today and [route.php] next"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: []
paths:
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/tests/**"
  - "crates/blastit/src/**"
  - "crates/blastit/tests/**"
  - "python/tests/**"
  - "e2e/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T16:36"
updated: "2026-11-14T23:17"
---

# Goal

Stop `blastit check --learn` from destroying a user's `blastit.toml`: an `--learn` write
must leave every route's executor block byte-for-byte intact.

# Context

- **This is an existing bug, not a future one.** `crates/blastit/src/edit.rs`'s
  `expected_routes` (the route-shape the rewriter targets) does not carry
  `Route::static_policy`, so a `[route.static]` block — `root`, `index`, `spa`,
  `immutable` — is dropped from a manifest that `--learn` rewrites today.
- The same function will drop `[route.php]` (`PhpPolicy { entry, workers }`) the moment
  that block exists, which is now (`MB.3.5.4`). A developer who runs the documented
  `blastit check --learn -- <suite>` loop would lose their front-controller block and their
  static root, and the tool would report success.
- The acceptance is a **round trip**, not a code review: rewrite the fixture, diff the
  executor blocks, and assert they are identical. Anything that rewrites a manifest must be
  provably conservative — the manifest is the user's file, not ours.

# Acceptance

```console
$ cargo test -p blastit --locked
$ cargo test -p blastit --test edit_roundtrip --locked
$ make check
```
- A manifest containing `[route.php] entry = …` (optionally `workers`) **and**
  `[route.static] root = … immutable = …` survives `apply_edits` + `write_atomic`
  byte-for-byte on both blocks: same keys, same order, same values, same comments
  placement if the writer preserves them today.
- A route with no executor block is unchanged too (no invented `[route.static]`).
- The additions `--learn` is *supposed* to make (a new grant/query) still land — the fix
  must not turn the rewriter into a no-op. Assert both directions.
- `python/tests/**` (whichever suite owns the CLI-level loop) or an `e2e` row asserts that
  `check --learn` on a php+static fixture leaves both blocks present afterwards — a test at
  the user-visible level, not only at the writer's API.
- Protected: the round-trip test is a **new** `crates/blastit/tests/critical_*` file if the
  data-loss class warrants it; do not edit an existing protected suite.

# Notes

- `MB.3.6.4` (learn/check attribution) points here and must neither fix nor duplicate it.
- If the rewriter cannot be made conservative without redesigning it, report `needs_split`
  with the smaller-safe step (e.g. refuse to rewrite when a route carries an executor block
  it cannot represent, failing closed instead of dropping data) rather than shipping a
  half-fix.
