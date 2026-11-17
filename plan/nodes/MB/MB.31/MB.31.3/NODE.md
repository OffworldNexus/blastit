---
id: MB.31.3
title: "Delete ImmutableGlob and glob(): static mounts consume the manifest's matcher, with bare-immutable S3 proof"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.31.2]
paths:
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/src/static_response.rs"
  - "crates/blastit-scheduler/tests/critical_static_resolve.rs"
  - "crates/blastit-scheduler/tests/critical_static_assets.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
  - "crates/blastit-manifest/src/model.rs"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T12:33"
updated: "2026-11-18T00:14"
---

# Goal

Delete the second matcher. `crates/blastit-scheduler/src/static_asset.rs:179-235` carries
`ImmutableGlob` and the recursive `glob()` whose `*` stops at `/`; it is the reason the
Django page needed six patterns. Replace it with the matcher `blastit-manifest` now compiles
(`MB.31.2`), so a `static` mount and a `view` answer a pattern identically:

- `"/assets/*"` matches `/assets/chunks/x.js` — **nested, one pattern**;
- `regex '/static/.+\.[0-9a-f]{12}\.[^/]+'` matches `/static/app.4f1c2a9b7d3e.js` and not
  `/static/app.css`;
- bare `immutable` year-caches every **exact** file the mount serves.

Then keep the security rule intact: an `index` answer and a `fallback` answer are **always**
`no-cache`, under every spelling including bare `immutable` (S3, design §4.5). A year-long
cache is pinned only on an `AnswerKind::Exact` answer whose restored `url_path` matches.

# Context

- **What to remove:** `ImmutableGlob` (`static_asset.rs:179-212`), `glob()`
  (`:214-235`), `MAX_IMMUTABLE_PATTERN_BYTES = 256` (`:56`), and the `filter_map(
  ImmutableGlob::new)` in `StaticRoot::open` (`:336-342`). `StaticRoot.immutable` becomes the
  policy's compiled matcher (moved or borrowed, no boot-time recompilation of a second
  syntax).
- **What must not move:** the cache decision is keyed on `(answer kind, answering path)` in
  `static_response.rs:178-197`; `file.url_path` has the mount prefix restored
  (`static_asset.rs:243-250`), so `static "/assets" { root "public/assets" }` still matches
  `/assets/app.js` and never the stripped `app.js` (design §9.4). `AnswerKind::Exact` is the
  only kind that can be cached.
- **Protected files this leaf owns:** `tests/critical_static_resolve.rs:750-782` (the
  `ImmutableGlob::new` and glob-row tests) and `tests/critical_static_assets.rs` (the cache
  rows and fixture D's `/index.html`). They are in `paths`, so `tools/critical_guard.py`
  accepts the edits. Replace only the pattern semantics: every traversal, NUL, dotfile,
  symlink, `.php`-source, short-read and status-table row keeps its assertion.
- `tests/common/static_fixture.rs` builds the fixtures those suites use; if it carries a
  pattern list, one `"/assets/*"` replaces the per-depth rows.
- `blastit-scheduler` must not gain a `regex` dependency: the compiled matcher comes from
  `blastit-manifest`, and this leaf consumes it. `MB.31.2` lands first; do not edit
  `crates/blastit-manifest/**` here — if the type is awkward to consume, report it as a
  finding instead of forking the matcher.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-scheduler
$ cargo test -p blastit-scheduler --test critical_static_assets \
      --test critical_static_resolve --test critical_rewrite
$ cargo clippy -p blastit-scheduler --all-targets -- -D warnings
$ grep -rn 'ImmutableGlob\|fn glob(\|MAX_IMMUTABLE_PATTERN_BYTES' crates/blastit-scheduler
    # nothing: one matcher in the tree
```

New assertions the leaf must add (in the protected files above):

- `/assets/app.js` **and** `/assets/chunks/vendor/x.js` are `200` +
  `Cache-Control: public, max-age=31536000, immutable` from the single pattern
  `immutable "/assets/*"`; `/assets` itself is `404`, not cached;
- `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'`: `/static/js/app.0123456789ab.js`
  immutable, `/static/js/app.js` and `/static/js/app.0123456789ab.js.map` **not** (one
  hash segment, nothing after it);
- bare `immutable`: an exact file is immutable; `/` (an `index` answer) and
  `/dashboard/settings` (a `fallback` answer) are `no-cache`, with the fixture that makes
  both answers possible in one mount;
- a two-pattern list is an OR: either match earns the year, written order irrelevant;
- the same mount under `static "/assets" { root "public/assets" }` matches on
  `/assets/app.js` (prefix restored), never on the stripped path.

# Notes

- One behaviour deliberately changes and must be stated in `RESULT.md`: a pattern that
  previously stopped at `/` now crosses it. Any test that asserted `/assets/*/*` is an
  artefact of the retired matcher — rewrite it as the one-pattern row, do not keep both.
- `serde`/`Debug` derives on `StaticRoot` must keep compiling: the new matcher has to be
  `Clone` (or borrowed) because `StaticRoot` is built per mount at boot.
- Keep the response header bytes and `NO_STORE_CACHE`/`REVALIDATE_CACHE` constants exactly as
  they are; this leaf changes *which* files earn the immutable value, never the values.
