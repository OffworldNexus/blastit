---
id: MB.31.2
title: "Lower, validate and print static immutable through pattern.rs: bare All, quoted patterns crossing /, regex '...', one refusal set"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.31.1]
paths:
  - "crates/blastit-manifest/src/blastfile/parser.rs"
  - "crates/blastit-manifest/src/blastfile/syntax.rs"
  - "crates/blastit-manifest/src/blastfile/lower.rs"
  - "crates/blastit-manifest/src/blastfile/print.rs"
  - "crates/blastit-manifest/src/blastfile/pattern.rs"
  - "crates/blastit-manifest/src/blastfile/mod.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/routing.rs"
  - "crates/blastit-manifest/src/uid.rs"
  - "crates/blastit-manifest/tests/critical_static_manifest.rs"
  - "crates/blastit-manifest/tests/static_mounts.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_parity.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_print.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit_executor.rs"
  - "crates/blastit-scheduler/tests/common/static_fixture.rs"
  - "crates/blastit-scheduler/tests/critical_static_assets.rs"
  - "crates/blastit-scheduler/src/static_asset.rs"
  - "crates/blastit-scheduler/src/static_response.rs"
  - "docs/design/static-and-spa.md"
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-17T12:33"
updated: "2026-11-17T23:39"
---

# Goal

Make `blastit-manifest` lower and validate `static` `immutable` through the **one** pattern
compiler `view` already uses, in the three shapes `MB.31.1` documented:

```text
static "/static" {
    root "staticfiles"
    immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'   # or:
    immutable "/static/*"                               # or:
    immutable                                           # bare
}
```

Concretely, in `crates/blastit-manifest`:

1. **Parser/CST** (`blastfile/parser.rs`, `blastfile/syntax.rs`): `immutable` takes an
   optional comma list whose items are `pattern` nodes — a quoted string or
   `regex '…'`/`regex "…"` — reusing the view endpoint's `_pattern` item parser and the
   existing pattern CST node. Bare `immutable` with no item is legal. `root`/`index`/
   `fallback` stay one `"…"` string each.
2. **Lowering** (`blastfile/lower.rs:825-890`): each item goes through
   `pattern::compile` / `pattern::compile_regex` at its own position, so the fragment is
   built once, by the same code, with the same escapes and the same `MAX_PATTERN_BYTES`
   (4096) cap. No `str_value` shortcut, no second scanner.
3. **Model** (`model.rs:316-333`): `StaticPolicy.immutable` stops being `Vec<String>` and
   becomes a type that distinguishes **none**, **`All`** (bare) and **`Patterns(…)`**, and
   can answer `matches(url_path)` **without compiling at request time** and without a
   second matcher in `blastit-scheduler`. Expose what `blastit-scheduler` and the printer
   need (pattern text for `fmt`, a match for the cache rule); keep the compiled form
   private if that is possible across the crate boundary — `routing::compile_pattern` is
   `pub(crate)`, so the compile must live here.
4. **Validation** (`validate.rs:480-521`): delete the bespoke per-pattern rows
   (`StaticImmutablePattern` for a non-`/`, `..`, control byte or 256-byte pattern) — those
   refusals are now `PatternNotAnchored` / `PatternStarPosition` / `PatternRegex` /
   `PatternParameter*` / `PatternTooLong`, at the pattern's own `line:col`, exactly as a
   `view` pattern. Keep `MAX_PATTERNS = 32` on the list and nothing else. Remove the dead
   `StaticImmutablePattern` variant and its `error.rs` arms once nothing constructs it.
5. **Printer** (`blastfile/print.rs`, and the keyword/statement behaviour `syntax.rs`
   asserts): `blastit fmt` writes `immutable` bare, `immutable "/a/*"` for one quoted
   pattern, `immutable "/a/*", "/b/*"` for a list, and `immutable regex '…'` for the raw
   form; a formatted file re-parses to the same policy.

# Context

- **The compiler, unchanged:** `crates/blastit-manifest/src/blastfile/pattern.rs`
  (`compile`, `compile_regex`, `Compiled::text`, `Compiled::fragment`,
  `MAX_PATTERN_BYTES`), wrapped `^(?:…)$` by `routing::compile_pattern`
  (`routing.rs:29-37`, `REGEX_SIZE_LIMIT = 1 << 20`, `REGEX_NEST_LIMIT = 64`). A `regex` is
  pre-compiled on its own first, so `x)|(.*` cannot become a catch-all.
- **Today's split path:** `parser.rs:1373-1379` (`comma_list(..., starts_string,
  Parser::string_item)`), `lower.rs:850-853`, `model.rs:332`, `validate.rs:480-521`
  (`MAX_PATTERNS` at :64), `error.rs:525-533` (`StaticImmutablePattern`).
- **Semantics that must survive:** a match is against the **resolved `url_path`** — the
  path the client asked for, prefix restored (`static_asset.rs:243-250,318-340`, design
  §9.4) — never the stripped remainder. `index`/`fallback` answers are never cached
  (S3); that gate is `AnswerKind::Exact` in `static_response.rs:178-197` and belongs to
  `MB.31.3`, but the model must make `All` distinguishable so the scheduler can honour it.
- **Protected test files this leaf owns:** `tests/critical_static_manifest.rs`
  (`:182,470` write `"/assets/*/*"`), `tests/static_mounts.rs` (`:198-250`, the per-depth
  fixture and its doc comment, which also names the page as `docs/frameworks/django.md`
  instead of `docs/frameworks/python/django.md`), `tests/docs_fences.rs` (`:800-813`),
  and — only if lowering parity or `fmt` covers static — `tests/critical_blastfile_parity.rs`
  and `tests/critical_blastfile_print.rs`. They are listed in `paths`, so
  `tools/critical_guard.py` accepts the edits; **never weaken** a traversal, dotfile,
  hidden-file or `.php`-source assertion — only the cache-pattern shape changes.
- `MB.31.1` already set the Django page to one quoted pattern and pinned
  `immutable == ["/static/*"]`; this leaf strengthens that pin to the typed accessor and
  leaves the page text alone. `MB.31.4` owns `crates/blastit/src/lsp/hover.rs` and
  `editors/**`; do not touch either.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest
$ cargo clippy -p blastit-manifest --all-targets -- -D warnings
$ cargo test -p blastit-manifest --test critical_static_manifest \
      --test static_mounts --test docs_fences \
      --test critical_blastfile_parity --test critical_blastfile_print
$ cargo test -p blastit-manifest --doc
$ grep -rn 'StaticImmutablePattern\|256' crates/blastit-manifest/src/validate.rs crates/blastit-manifest/src/error.rs
    # no per-pattern 256-byte ceiling; only the 32-item list cap remains
```

New assertions the leaf must add (in the files above, not a new "tests later" task):

- accepted: bare `immutable`; `immutable "/static/*"` whose fragment matches
  `/static/admin/js/vendor/select2/i18n/de.js`; `immutable "/v1.0/app.css"` (literal dot);
  `immutable "/orders/{id:int}/invoice.pdf"` (a typed segment); `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'`;
  a list of two patterns in written order;
- refused, with the sentence **and** `line:col` of the pattern: `"assets/*"` (not
  anchored), `"/a*b"` and `"/a/*/b"` (star position), `regex '('` (regex does not
  compile), a 4097-byte pattern, and a 33rd list item;
- `fmt` round-trip for all four spellings, byte-identical on a second pass;
- the Django page's mount lowers to exactly one pattern and its fragment is `/static/.*`.

# Notes

- The file count is above the usual 5–8 because one type change ripples through
  `model`/`lower`/`validate`/`print`/`error`: they cannot compile apart. The change budget is
  small (~150 source lines, ~150 test lines) — do not let the leaf grow into a refactor of
  `pattern.rs`, which is correct as written.
- **Do not add a `regex` dependency to `blastit-scheduler`.** The compiled matcher stays in
  `blastit-manifest`; the scheduler consumes it. Two matchers would be the bug this epic
  exists to delete.
- A quoted pattern's trailing `/*` crossing `/` is `TAIL = ".*"` in `pattern.rs`; nothing
  here needs to change it — the point is that `immutable` now reads it.
- Keep `StaticPolicy`'s other fields and its `Default`-less construction untouched, so
  `MB.31.3` is a mechanical switch and `blastit-scheduler` compiles against the new type in
  one step.
