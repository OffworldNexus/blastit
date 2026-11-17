---
id: MB.31.1
title: "Document one immutable pattern grammar (bare, view-syntax, regex) and purge the six-glob staircase from every page"
kind: task
status: ready
owner: "doc-writer"
priority: 1
depends_on: [MB.25.2]
paths:
  - docs/frameworks/python/django.md
  - "docs/frameworks/static-and-spa.md"
  - docs/reference/blastfile.md
  - "docs/decisions/ADR-0012-blastfile-dsl.md"
  - "docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md"
  - "docs/design/static-and-spa.md"
  - "docs/design/blastfile-dsl.md"
  - "crates/blastit-manifest/tests/docs_fences.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:33"
updated: "2026-11-17T12:33"
---

# Goal

Write the user-facing contract for the unified `immutable` grammar **before** the parser,
scheduler or editor changes, and delete the six-pattern staircase the owner rejected from
every page that documents it. A Django developer must read one pattern, not six, and must be
able to tell which files earn the year-long cache, which never do, and how to narrow the
match to hashed names only.

The accepted grammar (the whole of it):

| Written | What it does |
|---|---|
| `immutable` | every **exact file** the mount serves earns `Cache-Control: public, max-age=31536000, immutable` |
| `immutable <pattern>[, <pattern>…]` | each `<pattern>` is the **same** pattern `view` takes: `"/assets/*"` (`*` crosses `/` at any depth), literal dots, `{id}`/`{id:int}`/`{id:uuid}`, `/`-anchored |
| `immutable regex '<expr>'` | the raw escape hatch, anchored whole to the resolved URL path |

`index` and `fallback` answers stay `no-cache` **always**, including under bare `immutable`
(S3). Caps are the pattern compiler's: 4096 bytes per pattern, both spellings, 32 patterns
per list.

# Context

- **Why this is the DX debt:** `docs/frameworks/python/django.md:45-56` carries a `static`
  block whose `immutable` list is `"/static/*", "/static/*/*", … "/static/*/*/*/*/*/*"` —
  six patterns to reach `admin/js/vendor/select2/i18n/de.js`, because `ImmutableGlob` made
  `*` stop at `/`. `docs/reference/blastfile.md:397-399` explains the workaround as if it
  were a feature ("`*` in `immutable` stops at `/`, so a nested tree needs one glob per
  depth"), `docs/frameworks/static-and-spa.md:32,54-55` repeats the framing, and
  `crates/blastit/src/lsp/hover.rs:120-124` repeats it a third time on hover.
- **What the reference already promises for `view`** is the grammar to reuse and to name
  once: `docs/reference/blastfile.md` §patterns (`"/products/{id:int}"`,
  `regex '/v[0-9]+/items'`, `MAX_PATTERN_BYTES = 4096`) and
  `docs/decisions/ADR-0012-blastfile-dsl.md` §8.1/§8.3. Do not invent vocabulary: the
  sentences for a misplaced `*`, a non-`/` pattern and a bad regex already exist.
- **Pages and records in scope:**
  - `docs/frameworks/python/django.md` — the fence (line 45) and the paragraph at 66-69;
    use `immutable "/static/*"` (one pattern, legal today, crosses every depth once
    `MB.31.2` lands) and one sentence naming the narrower
    `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'` for a `staticfiles` tree that also
    holds unhashed originals. Keep the page under its current length.
  - `docs/reference/blastfile.md` — the `static` block example (376), the directive table
    row (385), the "`*` stops at `/`" paragraph (397-399), and the caps table row (590);
    state bare `immutable`, the shared pattern syntax, the regex form, the S3 rule, that a
    pattern is at most 4096 bytes (both spellings — the reference has no 256-byte line, and
    the scheduler's silent 256-byte refusal is what `MB.31.3` deletes), and that a `static`
    pattern is refused with the same sentence as a `view` pattern.
  - `docs/frameworks/static-and-spa.md` — the four-directive table row (32) and the
    deep-path paragraph (54-55).
  - `docs/decisions/ADR-0012-blastfile-dsl.md` — one amendment section (§9.5-style, dated
    `2026-11-17`) recording that `immutable` now uses §8.1's pattern compiler, the bare
    form, and that `StaticPolicy.immutable` stops being `Vec<String>`; plus the grammar
    line `static_directive → … | 'immutable' (pattern (',' pattern)*)?` beside the accepted
    `pattern → Str | 'regex' RawStr`.
  - `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md` — an "Amended by
    ADR-0012 §9.5" line on decision 2, which still says "one glob metacharacter (`*`, which
    stops at `/`)". ADRs are records: add the pointer, do not rewrite the history.
  - `docs/design/static-and-spa.md` — a supersession note on §4.5 `R5` (110, 610-620) that
    the matcher is now `pattern::compile` + `routing::compile_pattern`, not `glob()`; the
    date and `MB.31.2` on the note.
  - `docs/design/blastfile-dsl.md` — the EBNF at ~383 and the `static` body row at 414.
  - `crates/blastit-manifest/tests/docs_fences.rs:800-813` — the Django pin asserts
    `mount.policy.immutable.len() == 6, "one pattern per depth"`; change it to the one
    pattern the page now carries (use only the accessor that exists at this commit —
    `Vec<String>`, so `assert_eq!(mount.policy.immutable, ["/static/*"])`). `MB.31.2`
    strengthens it to the typed form.
- **Publish order is deliberate.** The regex form is documented here and becomes executable
  in `MB.31.2`: DX-first means the page states the target. The Django fence itself stays
  legal today (`"/static/*"` is a quoted pattern), so `docs_fences` is green in this commit;
  only the prose claims the depth-crossing behaviour `MB.31.2` ships hours later, on the
  same release branch, before `0.2.0` is tagged.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-manifest --test docs_fences
$ make docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ grep -rn 'stops at `/`\|one glob per depth\|"/static/\*/\*"' \
    docs/frameworks docs/reference docs/decisions/ADR-0012-blastfile-dsl.md
    # only the amended ADR-0007 text and the `view`-pattern contrast may remain
```

and in `RESULT.md`: the `django.md` fence before/after, and the `dx` + `ai-ergonomics`
critic reviews (fresh-context `critic` agents) with their verdicts. The `dx` lens must ask
"can a developer cache nested hashed assets without reading a footnote?" and the
`ai-ergonomics` lens "can an agent pick bare vs pattern vs regex from one table?".

# Notes

- Lean prose: no paragraph about `regex` internals, `REGEX_SIZE_LIMIT` or the two-phase
  compile. One sentence for the security rule (index/fallback are always `no-cache`), one
  for the caps, the table, and the Django example.
- Never document `"*"` inside a quoted pattern anywhere but a trailing `/*`; that is the
  refusal `MB.31.2` inherits, and a page that shows it would teach an error.
- `docs/design/**` and `docs/decisions/**` are excluded from the public site, so their
  notes are for the next agent, not the reader: keep them to the pointer, the date and the
  node id.
- Do not touch `CHANGELOG.md` (release prep owns it) or `crates/blastit/src/lsp/hover.rs`
  (`MB.31.4` owns the hover row and its test asserts every key appears in
  `docs/reference/blastfile.md`).
