---
id: MB.21.3.3
title: Compile Blastfile path patterns and imply HEAD from GET at parse time
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.2]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/blastfile/pattern.rs"
  - "crates/blastit-manifest/src/model.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:39"
---

# Goal

Compile Blastfile's path patterns into the anchored regexes the scheduler already matches, and
normalize `HEAD` from `GET` at parse time so request matching itself does not change.

The **public-entry-point** proof of this leaf cannot be written yet: `tests/` may only call
`Manifest::from_blastfile_str`/`parse_blastfile`, and those arrive with lowering in `MB.21.3.8`. This
leaf therefore lands `pattern.rs` with its table and refusal matrix as `#[cfg(test)] mod tests` (plus the
parser hook's own module tests), and `MB.21.3.8` owns `tests/blastfile_patterns.rs` and its protected
twin. That is the same split `MB.21.3.2` uses for `critical_blastfile_parser.rs`. This leaf also has to
come **before** `MB.21.3.8`, because lowering's step 4 compiles each `view` header's patterns (§7.3).

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
- Tests: `#[cfg(test)] mod tests` inside `pattern.rs` (a table of pattern → matching paths, both
  directions) and inside `parser.rs` (the header hook), covering the refusal matrix in the same module.
  The protected integration suite `tests/critical_blastfile_patterns.rs` lands with `MB.21.3.8`.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked blastfile::pattern
$ cargo test -p blastit-manifest --locked
$ make rs-fmt
$ make rs-lint
$ make check
```

1. An in-module table test proves `/a.b` does not match `/axb`, `{id:int}` rejects `12a`, `{id:uuid}`
   rejects a near-miss UUID, `/assets/*` matches a nested file, and a bare `/x` does not match `/xy`.
2. The method normalisation is a tested unit: a `GET` list gains `HEAD`, and a list that declared other
   methods keeps exactly what it declared plus `HEAD` only where `GET` was. `MB.21.3.8` re-asserts the
   same property on the parsed `Manifest`, through the public entry point this leaf does not have.
3. Every refusal is asserted with message and `line:col`; a `regex` that does not compile is a clean
   error, never a panic.
4. `make check` exit 0.
5. Files: `src/blastfile/pattern.rs`, the parser hook that stores patterns and normalised methods,
   `src/model.rs` doc-comment update, and in-module tests only (≤ 4 files, ≤ 450 changed lines). No
   `lower.rs`, no new public item beyond what `MB.21.3.8` will consume.

# Notes

- Escape the literal spelling character by character (`.`, `+`, `(`, `)`, `[`, `]`, `$`, `^`, `?`,
  `|`, `\`); do not build a regex out of raw user text.
- `regex` stays the `regex` crate (linear time). Never accept a pattern that a backtracking engine
  would need to interpret.
