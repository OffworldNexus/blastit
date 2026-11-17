---
id: MB.32.2
title: "Browser proof of the Blastfile highlighter: light and dark computed colours at AA, copy text and line anchors intact, instant navigation and plain fences (visual-checker)"
kind: task
status: ready
owner: "visual-checker"
priority: 2
depends_on: [MB.32.1]
paths: ["docs/testing/VISUAL-MB.32.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T12:46"
updated: "2026-11-17T12:46"
---

# Goal

Prove in a real browser, on the **built** site, that `MB.32.1`'s highlighter is right: every
`Blastfile` fence is coloured, the colours are legible in **both** schemes, the copy button still
copies the exact script, the line anchors still work, instant navigation colours the second page, and
the fences that must stay plain are still plain. Record `VERDICT: PASS` (or a numbered defect list)
in `docs/testing/VISUAL-MB.32.md` with screenshots.

`make docs-check` never renders a page: a token painted in the surface colour, a palette scoped to
the wrong scheme, an `innerHTML` rebuild that ate the line spans, or a copy button that now yields
`view products GET` with stray characters would all pass `--strict`.

# Context

- Build and serve exactly as documented: `make docs-check`, then serve `site/`
  (`uvx --offline zensical serve site` or `uv run --no-project python -m http.server --directory site`)
  and open it in Chrome. Do not open the `docs/` Markdown on GitHub as a substitute.
- Pages that carry the fences to check:
  - a **document**: `site/blastfile/index.html` (or the nav's "The Blastfile" page) and
    `site/getting-started/`;
  - the **reference**: `site/reference/blastfile/` — 14 statement lists, one EBNF block
    (`file → app? entrypoint? section*`) and one error block (`Blastfile:3:9: …`) that must stay
    plain;
  - `site/deploy/digitalocean-paas/` — the `postgres` store block titled `Blastfile: the store
    block`;
  - `site/frameworks/python/django/` — the released `immutable regex '…'` line.
- What to measure, in Chrome, per scheme (light first, then the `scheme: slate` toggle, then reload
  to confirm the choice persists) and per page:
  1. **Colour present:** `getComputedStyle` on a comment, a section keyword (`view`), an env var
     (`$DATABASE_URL`), a raw SQL string, an HTTP method and a `{id:int}` parameter is a real token
     colour — not the inherited ink. Report each computed `rgb()`.
  2. **Contrast:** for each of those tokens, compute the ratio against the code block's own
     `background-color` and require ≥ 4.5:1 in both schemes. A ratio under 4.5 is a defect with the
     two colours and the number.
  3. **Anchors intact:** hovering/clicking a line number still yields a `#__codelineno-N-M` anchor
     in the URL, and the rendered block still has the same number of `#__span-*` line spans as
     before highlighting (compare against a page whose fence is not highlighted, e.g. the EBNF
     block).
  4. **Copy button:** click it on a document fence; the clipboard text must equal the script shown,
     byte for byte — same lines, same indentation, no injected token characters, no filename label
     (`Blastfile`) and no line numbers. Paste into a scratch editor to compare; screenshot the
     pasted text.
  5. **Instant navigation:** from the home page, follow a nav link to the reference page **without a
     reload**; the newly inserted fences must be coloured (a `document$` subscription that only runs
     on `DOMContentLoaded` leaves them plain). Screenshot before and after.
  6. **Plain stays plain:** the EBNF block, the `Blastfile:3:9:` error block,
     `docs/how-it-works.md`'s `/run/…` fragments and `site/frameworks/php/`'s `sudo` block carry no
     token spans (`code span.k`, `.nv`, `.s1` count is 0) and are still monospaced and readable.
  7. **No-JS fallback:** with JavaScript disabled the document page still reads as before (copy
     button dead is expected and acceptable; layout, anchors and text must be unchanged).
- Also check a narrow viewport (≈ 390 px): the coloured block must not scroll horizontally where it
  did not before, and long `immutable regex '…'` lines still wrap or scroll as they do today.

# Acceptance

```console
$ make docs-check
$ grep -c 'blastfile-highlight.js' site/index.html
    # 1: the asset is injected on the built page
$ grep -o 'class="language-text highlight"' site/reference/blastfile/index.html | wc -l
    # 16: 14 statement lists + 1 EBNF + 1 error block still render as `text`
```

and in `docs/testing/VISUAL-MB.32.md`: `VERDICT: PASS`, the light/dark screenshots for items 1–7,
the computed colour + contrast table per token per scheme, the clipboard comparison, the
instant-navigation pair, and the plain-block span counts. Any defect is a numbered item with the page,
the token, the computed value and the expected one — reported, not fixed here (this node owns only
the record).

# Notes

- This node writes only `docs/testing/VISUAL-MB.32.md` (excluded from the site). Defects go back to
  `lead` as findings; `MB.32.1` owns the fixes.
- Use the browser's own `getComputedStyle`/clipboard, not a screenshot eyeball, for the colours,
  ratios and copy text — the screenshot is the illustration, the numbers are the proof.
- Same standard as `MB.21.5.4` and `MA.3.3`: a `PASS` with no numbers is not a pass.
