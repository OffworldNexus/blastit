---
id: MB.21.3.3
title: Compile Blastfile path patterns and imply HEAD from GET at parse time
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.8]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/blastfile/pattern.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/tests/blastfile_patterns.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_patterns.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:16"
---

# Goal

Compile Blastfile's path patterns into the anchored regexes the scheduler already matches, and
normalize `HEAD` from `GET` at parse time so request matching itself does not change.

# Context

- Design: `docs/design/blastfile-dsl.md` §8 (the pattern → regex table, the refusal matrix, `HEAD`).
- Accepted spellings: `"/products/{id:int}"` (`.` is literal), `{name}` → `[^/]+`, `{name:int}` →
  `[0-9]+`, `{name:uuid}` → the canonical UUID form, trailing `*` → the prefix tail
  (`/assets/*` matches `/assets/a/b.css`), and `regex '<pattern>'` kept verbatim and fully anchored.
  Every result is `^(?:…)$` so `PATH_INFO` still has to match whole (`ADR-0007` behaviour).
- Refusals at `line:col`: an empty pattern, a pattern not starting with `/` (except the prefix `*`
  spelling the design allows), an unknown parameter kind (`{id:float}`), a duplicate parameter name in
  one pattern, `*` anywhere but last, an unescaped `{`/`}` inside a quoted pattern, and a `regex` that
  Rust's `regex` refuses (report the compile error, never a panic).
- **`HEAD` implied by `GET`**: a view declaring `GET` answers `HEAD` too, at parse time, by
  normalizing `methods` into the existing `Option<Vec<String>>` — so retry/matching code is untouched.
  Update `Route::methods`' doc comment (it currently promises the opposite) and note that
  `docs/reference/manifest.md` is repointed by `MB.21.5.1`.
- Tests: `crates/blastit-manifest/tests/critical_blastfile_patterns.rs` (protected) with a table of
  pattern → matching paths, both directions, plus the refusal matrix.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_patterns
$ cargo test -p blastit-manifest --locked
$ make rs-fmt
$ make rs-lint
$ make check
```

1. A table test proves `/a.b` does not match `/axb`, `{id:int}` rejects `12a`, `{id:uuid}` rejects a
   near-miss UUID, `/assets/*` matches a nested file, and a bare `/x` does not match `/xy`.
2. `HEAD` reaches `methods` for a `GET` view (test on the parsed `Manifest`), and a view declaring
   methods explicitly keeps exactly what it declared plus `HEAD` only where `GET` was.
3. Every refusal is asserted with message and `line:col`; a `regex` that does not compile is a clean
   error, never a panic.
4. `make check` exit 0.
5. Files: `src/blastfile/pattern.rs`, the parser hook that stores patterns and methods, `src/model.rs`
   doc-comment update, and the new protected suite (≤ 5 files, ≤ 450 changed lines).

# Notes

- Escape the literal spelling character by character (`.`, `+`, `(`, `)`, `[`, `]`, `$`, `^`, `?`,
  `|`, `\`); do not build a regex out of raw user text.
- `regex` stays the `regex` crate (linear time). Never accept a pattern that a backtracking engine
  would need to interpret.
