---
id: MB.31
title: "One pattern grammar for static immutable: bare immutable, view-syntax patterns and regex '...' (kill the six-glob staircase)"
kind: epic
status: done
priority: 1
depends_on: [MB.25.2]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:33"
updated: "2026-11-18T00:37"
---

# Goal

`Blastfile` has **one** pattern grammar, documented in `docs/reference/blastfile.md` and
compiled by `crates/blastit-manifest/src/blastfile/pattern.rs`: a quoted `"/assets/*"`
whose trailing `/*` crosses `/` at any depth, `{name}`/`{name:int}`/`{name:uuid}`
segments, `/`-anchored, plus `regex '<expr>'` as the escape hatch. `view` uses it.

`static "<prefix>" { immutable … }` does not. An `ImmutableGlob` written at `MB.2.3`
gives `*` stop-at-`/` semantics, no `regex '…'`, and no bare form, so the owner had to
write this — six patterns to reach Django's `admin/js/vendor/select2/i18n/<lang>.js`:

```text
static "/static" {
    root "staticfiles"
    immutable "/static/*", "/static/*/*", "/static/*/*/*",
              "/static/*/*/*/*", "/static/*/*/*/*/*", "/static/*/*/*/*/*/*"
}
```

Owner: *"This is absolutely not legal or acceptable. Use a regexp or something, this is so
not okay."* That six-pattern staircase is in `docs/frameworks/python/django.md`,
`docs/reference/blastfile.md` and the `blastit lsp` hover text: one absurdity, documented
three times, on the page a Django developer reads first.

Unify `immutable` with the pattern grammar `view` already has, in three shapes:

1. **`immutable`** — bare, no arguments. Every **exact file** this mount serves earns
   `Cache-Control: public, max-age=31536000, immutable`. An `index` answer and a
   `fallback` answer stay **`no-cache`**, always: security invariant **S3** (`ADR-0007` §5,
   design §4.5) is that a year-long cache must never be pinned on a shell that changes.
2. **`immutable <pattern>[, <pattern>…]`** — each pattern is the **same** syntax `view`
   accepts and goes through the **same** compiler: `"/assets/*"` crosses `/` at any depth;
   `"/v1.0/app.css"` has a literal dot; `{id:int}`/`{id:uuid}` work; `regex '<expr>'` is
   the raw escape hatch, anchored whole to the resolved URL path.
3. **Django's actual need**, one line instead of six:
   `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'`.

Caps become the pattern compiler's caps, not a second set: `MAX_PATTERN_BYTES = 4096` per
pattern for both spellings, `MAX_PATTERNS = 32` on the list, and a refusal is the sentence
a `view` pattern already gives (`PatternNotAnchored`, `PatternStarPosition`,
`PatternRegex`, …) — not `static immutable "assets/*": must start with /`. The 256-byte
`MAX_IMMUTABLE_PATTERN_BYTES` ceiling and the recursive `glob()` matcher in
`crates/blastit-scheduler/src/static_asset.rs` are deleted, so there is exactly one matcher
and one refusal set in the tree.

Mandatory for `0.2.0`; `depends_on: [MB.25.2]`.

# Context

- **The compiler to reuse, unchanged:** `crates/blastit-manifest/src/blastfile/pattern.rs`
  (`compile`, `compile_regex`, `Compiled::fragment`, `MAX_PATTERN_BYTES`).
  `crates/blastit-manifest/src/routing.rs`'s `compile_pattern` is the anchored
  `^(?:…)$` wrapper (`REGEX_SIZE_LIMIT`); `regex` text is pre-compiled on its own first, so
  an unbalanced `)` can never be rescued into a catch-all (§17.3/§17.4).
- **Today's split path, and every site that encodes it:**
  - parse: `crates/blastit-manifest/src/blastfile/parser.rs:1373` — `immutable` accepts only
    `string` items through `comma_list`;
  - lower: `crates/blastit-manifest/src/blastfile/lower.rs:850` — `str_value` into
    `Vec<String>`;
  - model: `crates/blastit-manifest/src/model.rs:332` — `StaticPolicy.immutable: Vec<String>`;
  - validate: `crates/blastit-manifest/src/validate.rs:480-521` — bespoke
    `StaticImmutablePattern` rows, `MAX_PATTERNS` at :64;
  - scheduler: `crates/blastit-scheduler/src/static_asset.rs:179-235` — `ImmutableGlob` +
    recursive `glob()`; `static_response.rs:178-197` — `cache_control` gates on
    `AnswerKind::Exact`.
- **Docs that repeat the staircase:** `docs/frameworks/python/django.md:45-56,66-69`,
  `docs/reference/blastfile.md:385,397-399`, `docs/frameworks/static-and-spa.md:32,54-55`,
  `crates/blastit/src/lsp/hover.rs:120-124`, and the design records
  `docs/design/static-and-spa.md:86,610-620` and `ADR-0007` §2.
- **Tests pinned to the old semantics:** `crates/blastit-manifest/tests/docs_fences.rs:800-813`
  (`immutable.len() == 6`), `.../static_mounts.rs:198-250` (the per-depth fixture),
  `critical_static_manifest.rs:182,470` (`"/assets/*/*"`), and
  `crates/blastit-scheduler/tests/critical_static_resolve.rs:756-781` (glob rows and
  `ImmutableGlob::new`). These `critical_*` files are protected: each leaf that edits one
  lists it in `paths`, and no traversal / dotfile / hidden-file / `.php` assertion may be
  weakened — only the cache-pattern assertions change.
- **Editor side:** `editors/tree-sitter-blastfile/grammar.js` already has `immutable` as a
  `keyword` and `regex_raw_string` as its own node, but `_value` does not admit
  `regex_raw_string`, so `immutable regex '…'` is an `ERROR`. Bare `immutable` already parses
  (`_values` is optional). `editors/zed/extension.toml` pins
  `[grammars.blastfile] rev = "053b56e…"` and `make zed-check` proves
  `git cat-file -e <rev>:editors/tree-sitter-blastfile/src/parser.c`, so the regenerated
  `src/parser.c` and the new `rev` cannot land in the same commit — the `MB.22.4` →
  `MB.22.5` two-step, recorded in `docs/knowledge/LESSONS.md`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest -p blastit-scheduler
$ cargo test -p blastit --test critical_lsp
$ cargo test -p blastit-manifest --test docs_fences --test static_mounts
$ make docs-check
$ make zed-check
$ make check
```

and, pasted into the epic `RESULT.md`, one live `blastit dev` transcript over a mount whose
root holds `staticfiles/admin/js/vendor/select2/i18n/de.js`:

- `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'` (or bare `immutable`) →
  `200` + `Cache-Control: public, max-age=31536000, immutable` for
  `/static/admin/js/vendor/select2/i18n/de.0123456789ab.js` — **nested, one pattern**;
- `/static/` (an `index` answer) and a `fallback` answer → `Cache-Control: no-cache`
  even under bare `immutable` (S3);
- the owner's quoted six-pattern block from `django.md` no longer appears anywhere:
  `grep -rn '"/static/\*/\*"' docs crates editors` prints nothing.

# Notes

- **Order is the DX-first order and it is not cosmetic.** `MB.31.1` (docs + ADR-0012 note)
  lands first so the grammar is decided in prose a developer reads, then the manifest
  compiler, then the scheduler matcher, then the editor surfaces. `MB.31.3` and `MB.31.4`
  are path-disjoint and may run in parallel after `MB.31.2`.
- **`immutable` is not a security boundary; S3 is.** The bare form is a developer's promise
  that the whole root is content-addressed. It may never reach an `index` or `fallback`
  answer (`AnswerKind::Exact` only), and the protected test that proves it stays
  (`critical_static_assets.rs` cache rows).
- **One compiler, so one refusal.** After this epic a `static` pattern with a misplaced `*`
  or a bad regex fails with the same sentence and the same `line:col` as a `view` pattern.
  Do not add a second sentence for `static`; delete `StaticImmutablePattern`'s per-pattern
  rows and keep only the list-length cap.
- `crates/blastit-manifest/tests/static_mounts.rs:10,228` calls the page
  `docs/frameworks/django.md`; the path is `docs/frameworks/python/django.md` — correct it
  while the fixture is rewritten.
- `CHANGELOG.md` (line 28) and `ARCHITECTURE.md` are release-prep surfaces (`MB.3.8`);
  this epic records the change in `ADR-0012` and leaves the changelog line as a follow-up
  note for the 0.2.0 entry.
