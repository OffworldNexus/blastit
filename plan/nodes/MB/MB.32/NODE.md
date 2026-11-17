---
id: MB.32
title: "Blastfile syntax highlighting on the Zensical site: one offline lexer, an AA palette in light and dark, asset tests and a browser proof"
kind: epic
status: ready
priority: 1
depends_on: [MB.31]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:46"
updated: "2026-11-17T12:46"
---

# Goal

A developer reading the documentation site sees a **coloured** `Blastfile`: comments, section and
directive keywords, HTTP methods, environment variables, raw SQL strings, route parameters and
punctuation are distinguishable at a glance — in **light** and **dark** scheme, on every page that
carries one, with the copy button and the line anchors still working — because the site ships one
small, offline, self-contained lexer instead of teaching readers raw greyscale text.

Pygments has no `blastfile` lexer, so today every `Blastfile` fence on the site is
`language-text` (design §16.1): a complete manifest is indistinguishable from the one error sentence
on `docs/reference/blastfile.md`, and `$DATABASE_URL` reads like prose. `MB.32` closes that gap
without touching the one-document-fence discipline (`docs_fences.rs`), the nav, or the offline
build.

Non-negotiables, all measured facts:

1. **Zero network.** `uvx --offline zensical build --strict`, `make docs-serve` and
   `.github/workflows/docs.yml` must produce the identical page with no CDN, no font, no fetch.
2. **Never edit the reader's copy source.** `<span id="__span-…">` and `<a id="__codelineno-…">`
   survive untouched, and `code.textContent` stays byte-for-byte what Zensical rendered, so
   `content.code.copy` / `content.code.select` are unchanged.
3. **Highlight only what is a `Blastfile`.** Every complete manifest (12 `text title="Blastfile"`
   fences) and every keyword-led statement list is tokenized; the EBNF grammar, the
   `Blastfile:3:9:` error sentence, the `/run/blastit/views/<uid>/` path fragments and the `sudo`
   console block stay plain.
4. **WCAG AA in both schemes.** Every token colour meets ≥ 4.5:1 against the code surface it sits
   on, in `default` and `slate`, computed from the hex values in the stylesheet — not asserted from
   memory.

# Context

- **Measured tokenizer target:** `editors/zed/languages/blastfile/highlights.scm` (the grammar
  `MB.22.4` shipped) is the vocabulary authority, and `ADR-0012` the syntax authority:
  `#` comments, `'…'` raw strings (SQL, regex), `"…"` escaped strings plus `$?` bind placeholders,
  `$IDENT` env vars, the six section keywords `app entrypoint postgres trait view static`, block
  directives (`body`, `root`, `immutable`, `query`, `preload`, `use`, the store slots `host`,
  `port`, `user`, `password`, `database`, `sslmode`, and the grants `select`, `insert`, `update`,
  `delete`, plus the two `static` answer directives the reference's tables name), the
  HTTP methods `GET POST PUT PATCH DELETE HEAD OPTIONS`, the runtime names `python` / `php`,
  `{param:type}` route parameters, and `{ } ( ) , ;` punctuation.
- **Measured fence inventory (2026-11-17, via `python/tests/runnable_docs/fences.py`):** 33 plain
  and titled `text` blocks on the public pages. 12 are `text title="Blastfile"` documents; 17 are
  keyword-led statement lists (`docs/ci.md:90`, `docs/local.md:127`,
  `docs/deploy/digitalocean-paas.md:69`, and 14 in `docs/reference/blastfile.md`); 4 must stay
  plain — `docs/how-it-works.md:103` (`/run/…`), `docs/frameworks/php/index.md:46` (`sudo …`),
  `docs/reference/blastfile.md:17` (EBNF `file → …`) and `docs/reference/blastfile.md:620`
  (`Blastfile:3:9: …`). The keyword-led rule alone already separates all four; the error/EBNF
  shapes are the belt-and-braces refusals.
- **Rendered shape (`--strict`, measured):** an unknown Pygments name renders
  `<div class="language-text highlight"><span class="filename">Blastfile</span><pre>…`,
  `pygments_lang_class = true`, `line_spans = "__span"`, `anchor_linenums = true`. The filename
  label sits outside `<code>`, so a lexer that walks `code` text nodes cannot disturb it.
- **Wiring:** `zensical.toml` already carries `extra_css = ["brand/brand.css",
  "stylesheets/extra.css"]`; `extra_javascript` is unused and is the seam (a string path under
  `docs/` is copied into `site/` and injected on every page, including instant navigation).
- **Palette:** `docs/brand/brand.css` defines `--blastit-surface` / `--blastit-ink` /
  `--blastit-muted` / `--blastit-accent` per scheme; `docs/stylesheets/extra.css` is currently
  layout-only, which is where the token palette belongs because it must not disturb brand tokens.
- **Order:** `depends_on: [MB.31]` on purpose — `immutable regex '…'` becomes real syntax there, so
  the lexer colours the grammar this release actually ships.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ cargo test -p blastit-manifest --test docs_fences
$ grep -rn 'https\?://' docs/javascripts/blastfile-highlight.js docs/stylesheets/extra.css
    # no output: the site stays offline
```

plus, from `MB.32.2`, a `docs/testing/VISUAL-MB.32.md` carrying `VERDICT: PASS` with light/dark
screenshots of a document page and the reference page, the computed token colours, the copy-button
text comparison and the plain-block proof.

# Notes

- Two leaves on purpose: the lexer + palette + asset tests are one `implementer` session; the browser
  proof is a `visual-checker` session, because `--strict` never renders a page and only a browser
  can show a token that is invisible against the surface.
- The lexer is progressive enhancement: with JavaScript off the page must still read exactly as it
  does today, so nothing in `docs/**` may depend on it for layout, anchors or copy.
- Do not add a Pygments plugin, a Zensical hook, a Node build step or a `docs/requirements.txt`
  dependency: `make docs-check` stays `uvx zensical build --strict` with no extra install.
- `docs/design/**`, `docs/decisions/**` and `docs/testing/**` are excluded from the site; the record
  of *why* the lexer is hand-written belongs in `ARCHITECTURE.md` only if it grows beyond one
  comment block.
