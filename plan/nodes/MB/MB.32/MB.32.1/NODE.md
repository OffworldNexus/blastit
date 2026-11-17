---
id: MB.32.1
title: "Ship docs/javascripts/blastfile-highlight.js plus the AA token palette and the asset tests that pin the 33-fence classification"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.31]
paths:
  - "docs/javascripts/blastfile-highlight.js"
  - docs/stylesheets/extra.css
  - zensical.toml
  - python/tests/test_docs_assets.py
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:46"
updated: "2026-11-17T12:46"
---

# Goal

Ship the `Blastfile` highlighter as one offline, dependency-free asset plus the palette that makes it
readable, and pin it with asset tests:

- `docs/javascripts/blastfile-highlight.js` — a self-contained lexer (~200 lines, no imports, no
  fetch, no `eval`) that, for every `<div class="highlight language-text">` whose `<pre><code>` body
  is a `Blastfile` fence, wraps tokens in Pygments/Zensical span classes; every other `text` block is
  left byte-identical.
- `docs/stylesheets/extra.css` — one token palette, scoped `[data-md-color-scheme="default"]` and
  `[data-md-color-scheme="slate"]`, every token ≥ 4.5:1 against the code surface on which it renders.
- `zensical.toml` — `extra_javascript = ["javascripts/blastfile-highlight.js"]` beside `extra_css`.
- `python/tests/test_docs_assets.py` — the record of all three, written so a hand edit to the lexer,
  the palette or a page fails with the number or the fence it produced.

# Context

- **Vocabulary and classes** — take the tokens from `editors/zed/languages/blastfile/highlights.scm`
  and `docs/reference/blastfile.md` §"Layout and tokens", and map them onto the classes Zensical's
  own Pygments CSS already styles (`.k`, `.kd`, `.kn`, `.kt`, `.nf`, `.nv`, `.s1`, `.s2`, `.se`,
  `.c1`, `.p`, `.mi`, `.o`). Do not invent class names: an unstyled class renders as plain text and
  the whole exercise is invisible.
  - section keywords + `use`/store directives → `.k`/`.kd`; block directives (`body`, `root`,
    `immutable`, `preload`, the store slots `host`, `port`, `user`, `password`, `database`,
    `sslmode`, and the two `static` answer directives the reference's tables name) → `.kd`;
    grants/`select insert update delete query` → `.k`; `.kn` for `not`-like modes if you keep them.
  - HTTP methods (`GET POST PUT PATCH DELETE HEAD OPTIONS`) → `.kc` (or `.k` if the palette is
    cleaner); runtime names `python`/`php` → `.kt`.
  - `$IDENT` → `.nv`; `$?` → `.se`; `'…'` raw string (SQL/regex) → `.s1`; `"…"` string → `.s2` with
    `\" \\ \n \t` escapes → `.se`; `# …` → `.c1`; `{id:int}` route parameter name → `.nv`, its
    `:type` → `.kt`; `{ } ( ) , ;` → `.p`.
- **What must not move.** Walk only text nodes inside `code`; never rebuild `code.innerHTML` from a
  string (that destroys `<span id="__span-…">` line wrapping and `<a id="__codelineno-…">`
  anchors). Skip any fence that contains `__codelineno`/`__span` children you cannot re-emit safely
  — the cheapest correct implementation is a text-node walker with `document.createDocumentFragment`,
  or, if that is awkward with line spans, a walker that descends into existing `<span>`s and only
  splits their `Text` children.
- **Injection point.** `zensical.toml`'s `extra_javascript` runs after the theme bundle. Cover both
  entry paths, as `navigation.instant` swaps the document:
  ```js
  if (typeof document$ !== "undefined" && document$.subscribe) { document$.subscribe(highlight); }
  else { document.addEventListener("DOMContentLoaded", highlight); }
  ```
  `highlight` must be idempotent (a `data-blastfile-highlighted` marker on the processed `<code>`),
  because instant navigation fires more than once per page.
- **Fence classification (measured 2026-11-17, `python/tests/runnable_docs/fences.py`).** 33 `text`
  blocks on public pages: 12 `text title="Blastfile"` documents, 17 keyword-led statement lists, 4
  plain (`docs/how-it-works.md:103` `/run/…`, `docs/frameworks/php/index.md:46` `sudo …`,
  `docs/reference/blastfile.md:17` EBNF `file → …`, `docs/reference/blastfile.md:620`
  `Blastfile:3:9: …`). Highlight a `text` block when its first non-blank, non-`#` line begins with a
  `Blastfile` keyword (the six sections plus the block directives), and refuse it when that line
  matches `^\S+:\d+:\d+:` (a diagnostic) or contains `→`/`::=` (grammar). Note
  `docs/deploy/digitalocean-paas.md:69` is titled `Blastfile: the store block` and is classified by
  its `postgres` first line, not by the title — do not match `title="Blastfile` by prefix.
- **Palette.** `docs/brand/brand.css` owns the surfaces (`--blastit-surface` light `#fffcf5`, dark
  `#1b232c`) and ink; `docs/stylesheets/extra.css` is layout-only today and is the right home for
  token colours. Compute ratios in the test from hex, not from comments.

# Acceptance

```console
$ make docs-check
    # --strict green; site/javascripts/blastfile-highlight.js present
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ cargo test -p blastit-manifest --test docs_fences
$ PATH="$HOME/.local/bin:$PATH" uv run --no-project tools/plan.py validate
$ grep -rn 'https\?://\|fetch(\|eval(' docs/javascripts/blastfile-highlight.js
    # no output
```

The new tests in `python/tests/test_docs_assets.py` must each fail on a plausible defect:

1. `zensical.toml` names the asset in `extra_javascript` (drop the entry → red).
2. The asset exists under `docs/` and no `exclude_docs` rule matches it, so Zensical copies it; and
   when `site/` has been built, `site/javascripts/blastfile-highlight.js` is byte-identical to the
   source (a stale copy → red).
3. The lexer has no `http://`/`https://`, `fetch(`, `import `, `eval(` — offline, self-contained.
4. Both entry paths exist (`document$.subscribe` and `DOMContentLoaded`) and the marker line is
   present, so instant navigation cannot skip a page.
5. The anchors guard exists: the source never assigns `innerHTML` on a `code`/`pre` element (a regex
   over the file is enough; the visual leg proves the rendered anchors).
6. Every one of the 33 `text` blocks of the public pages classifies exactly as measured — 12 + 17
   highlighted, the 4 named ones plain — enumerated through `runnable_docs/fences.py` over
   `README.md` and every public `*.md`, so adding a `Blastfile` fence to an unpinned page fails with
   its path and line until the classifier handles it.
7. Every token colour in `extra.css` reaches ≥ 4.5:1 against the code surface of **its own** scheme,
   computed from the hex values (mirror `test_docs_assets.py`'s existing contrast helper), and both
   `[data-md-color-scheme="default"]` and `["slate"]` blocks exist. Pin the ratio per token so a
   palette nudge that drops below AA prints the number it produced.

# Notes

- Keep the file small and commented once at the top with *why* (no Pygments lexer, offline build,
  12-factor docs) — not a lexer tutorial.
- No `type = "module"` needed; a classic script is enough and avoids a fetch graph. If you use one,
  `zensical.toml` needs the dict form — prefer the plain string.
- Do not touch `docs/**/*.md` fence openers, `docs_fences.rs`, `crates/**`, or the nav. If a page
  needs a different opener to be highlighted, the fence rule is wrong — fix the classifier, not the
  page.
- `make docs-check` is the build proof; `MB.32.2` is the render proof. Do not add a headless browser
  to this task.
- Record in `RESULT.md` the exact diff of `zensical.toml`, the contrast table (token, scheme, ratio)
  and the 33-fence classification output.
