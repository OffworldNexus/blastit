# Visual & QA Record: Blastfile Highlighter Browser Proof (MB.32.2)

- **Date:** 2026-11-18
- **Auditor:** `visual-checker`
- **Scope:** Browser verification of the `Blastfile` highlighter (`docs/javascripts/blastfile-highlight.js`), token palette (`docs/stylesheets/extra.css`), copy button, line anchors, instant navigation, plain fence preservation, no-JS fallback, and viewport responsiveness across the built Zensical documentation site.
- **Commit / Branch:** `release/0.2.0`
- **Build:** `make docs-check` (`uvx --offline zensical build --strict`) → `No issues found in 0.32s`.
- **Browser:** Google Chrome 157.0.8103.78 via Chrome DevTools Protocol (CDP).
- **Screenshots Directory:** `/tmp/opencode/screenshots/`.

---

## Acceptance Checks

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.32s

$ grep -c 'blastfile-highlight.js' site/index.html
1

$ grep -o 'class="language-text highlight"' site/reference/blastfile/index.html | wc -l
16
```

---

## 1. Computed Token Colours & WCAG AA Contrast Ratios

The code block surface color is `--md-code-bg-color`, mapped via `docs/brand/brand.css` to `--blastit-surface`:
- **Light mode (`default`):** `#fffcf5` (`rgb(255, 252, 245)`)
- **Dark mode (`slate`):** `#1b232c` (`rgb(27, 35, 44)`)

All 13 token classes defined in `docs/stylesheets/extra.css` were sampled via `getComputedStyle()` in a live Chrome browser session on `http://127.0.0.1:44321/reference/blastfile/`. Contrast ratios were calculated against the computed surface color using the standard WCAG relative luminance formula: `(L1 + 0.05) / (L2 + 0.05)`.

### Contrast & Computed Color Matrix

| Token Class | Role / Semantic Target | Light (`default`) Computed RGB | Light Hex | Ratio on `#fffcf5` | Dark (`slate`) Computed RGB | Dark Hex | Ratio on `#1b232c` | WCAG AA (≥ 4.5:1) |
|---|---|---|---|---|---|---|---|---|
| `.k` | Keyword (sections, `use`, grants, SQL keywords) | `rgb(34, 56, 70)` | `#223846` | **11.90:1** | `rgb(243, 201, 151)` | `#f3c997` | **10.29:1** | PASS |
| `.kd` | Block directive (`body`, `root`, `immutable`, slots) | `rgb(129, 55, 28)` | `#81371c` | **8.21:1** | `rgb(235, 154, 108)` | `#eb9a6c` | **7.08:1** | PASS |
| `.kc` | HTTP method (`GET`, `POST`, `DELETE`, …) | `rgb(155, 71, 35)` | `#9b4723` | **6.17:1** | `rgb(255, 158, 203)` | `#ff9ecb` | **8.32:1** | PASS |
| `.kt` | Runtime (`python`, `php`), type annotations | `rgb(90, 47, 112)` | `#5a2f70` | **9.88:1** | `rgb(215, 168, 232)` | `#d7a8e8` | **8.05:1** | PASS |
| `.nf` | View name | `rgb(63, 58, 54)` | `#3f3a36` | **10.96:1** | `rgb(168, 199, 221)` | `#a8c7dd` | **8.98:1** | PASS |
| `.nv` | `$IDENT`, `$1…$n`, `...`, parameter name | `rgb(47, 86, 112)` | `#2f5670` | **7.63:1** | `rgb(143, 214, 180)` | `#8fd6b4` | **9.40:1** | PASS |
| `.s1` | `'…'` raw string (SQL, regex) | `rgb(42, 107, 79)` | `#2a6b4f` | **6.18:1** | `rgb(184, 224, 160)` | `#b8e0a0` | **10.74:1** | PASS |
| `.s2` | `"…"` string, route pattern | `rgb(31, 92, 99)` | `#1f5c63` | **7.40:1** | `rgb(169, 211, 216)` | `#a9d3d8` | **9.82:1** | PASS |
| `.se` | Escape sequence, `$?` placeholder | `rgb(122, 47, 107)` | `#7a2f6b` | **8.38:1** | `rgb(224, 176, 216)` | `#e0b0d8` | **8.59:1** | PASS |
| `.c1` | `#` comment, SQL `--` comment (*italic*) | `rgb(86, 97, 107)` | `#56616b` | **6.18:1** | `rgb(165, 176, 184)` | `#a5b0b8` | **7.18:1** | PASS |
| `.p` | Punctuation (`{`, `}`, `(`, `)`, `,`, `;`) | `rgb(86, 97, 107)` | `#56616b` | **6.18:1** | `rgb(185, 198, 207)` | `#b9c6cf` | **9.10:1** | PASS |
| `.mi` | SQL number (inside `query '…'`) | `rgb(138, 90, 0)` | `#8a5a00` | **5.78:1** | `rgb(240, 192, 122)` | `#f0c07a` | **9.47:1** | PASS |
| `.o` | SQL operator (`=`, `>`, `<`, `*`, etc.) | `rgb(63, 58, 54)` | `#3f3a36` | **10.96:1** | `rgb(203, 213, 220)` | `#cbd5dc` | **10.65:1** | PASS |

- **Minimum contrast:** Light: 5.78:1 (`.mi`), Dark: 7.08:1 (`.kd`).
- **Verdict:** Every single token exceeds the WCAG AA minimum threshold (4.5:1) in both light and dark color schemes.
- **Screenshots:**
  - Light Reference: `/tmp/opencode/screenshots/01-reference-light.png` (`md5: 58c98a2fdedd853abb160fb6bb69ef23`)
  - Dark Reference: `/tmp/opencode/screenshots/02-reference-dark.png` (`md5: 1b4bff01c63dcdc7248b0ca1d440e4cf`)
  - Light Document: `/tmp/opencode/screenshots/03-blastfile-doc-light.png` (`md5: 782f653b893d6c555341f17e8bfe4a26`)
  - Dark Document: `/tmp/opencode/screenshots/04-blastfile-doc-dark.png` (`md5: a4fec269eb9860be4d1b0413ebcf336c`)

---

## 2. Independent Witness: Critical Palette Scoping

In `MB.32.1`, every token rule was scoped through the marker:
`[data-md-color-scheme="…"] .highlight code[data-blastfile-highlighted] .<token>`
to prevent recolouring non-Blastfile code blocks (e.g. `python`, `console`, `dockerfile`).

We verified this directly on `/getting-started/`, which renders:
1. A `Blastfile` fence (`language-text highlight`, classified and marked with `data-blastfile-highlighted="true"`)
2. A Python script fence (`hello.py`, `language-python highlight`, untouched by lexer, no marker)
3. Console blocks (`$ uv run blastit dev`, `language-console highlight`, untouched by lexer, no marker)

### Scoping Comparison (Chrome Computed Styles)

| Fence Type | Scheme | `data-blastfile-highlighted` | Sample Token | Computed RGB | Computed Hex | Source / Theme Rule |
|---|---|---|---|---|---|---|
| **Blastfile** | Light (`default`) | **Yes** (`true`) | `.k` (`app`) | `rgb(34, 56, 70)` | `#223846` | Blastfile palette (`extra.css`) |
| **Python (`hello.py`)** | Light (`default`) | **No** (`false`) | `.k` (`def`) | `rgb(63, 110, 198)` | `#3f6ec6` | Material Pygments default |
| **Console** | Light (`default`) | **No** (`false`) | Text node | `rgb(22, 26, 32)` | `#161a20` | Material default ink |
| **Blastfile** | Dark (`slate`) | **Yes** (`true`) | `.k` (`app`) | `rgb(243, 201, 151)` | `#f3c997` | Blastfile palette (`extra.css`) |
| **Python (`hello.py`)** | Dark (`slate`) | **No** (`false`) | `.k` (`def`) | `rgb(103, 145, 224)` | `#6791e0` | Material Pygments slate |
| **Console** | Dark (`slate`) | **No** (`false`) | Text node | `rgb(241, 236, 224)` | `#f1ece0` | Material slate ink |

- **Proof of Non-Recolouring:** In both light and dark schemes, the Python code block's `.k` token evaluates to Material's native Pygments syntax colors (`#3f6ec6` and `#6791e0`), completely distinct from the Blastfile palette colors (`#223846` and `#f3c997`). Non-Blastfile fences are NOT recoloured.
- **Screenshots:**
  - Scoping Comparison Light: `/tmp/opencode/screenshots/05-scoping-comparison-light.png` (`md5: c2ae4b70315c2227db0b341ff2bfc0fd`)
  - Scoping Comparison Dark: `/tmp/opencode/screenshots/06-scoping-comparison-dark.png` (`md5: cfc6a0176b3a0632d1f54356359351ad`)

---

## 3. Line Spans & Line Number Anchors Intact

On `/blastfile/`, the document fence (`#__code_0 > code`) was verified before and after JavaScript highlighting:

### Line Spans & Anchors Comparison Matrix

| Measurement Condition | Page / Block | Line Count | `<span id="__span-*">` Count | `<a id="__codelineno-*">` Count | Token Spans Count |
|---|---|---|---|---|---|
| **Before Highlighting (No-JS static build, §7)** | `/blastfile/` (Document fence) | **14** | **14** | **14** | **0** |
| **After Highlighting (Live DOM with JS)** | `/blastfile/` (Document fence) | **14** | **14** | **14** | **14** |
| **Unhighlighted Comparison Fence** | `/reference/blastfile/` (EBNF block) | **10** | **10** | **10** | **0** |

- **Line Anchors Click Verification:** In Chrome, dispatching click on line anchor link `#__codelineno-0-1` updated `window.location.hash` to `"#__codelineno-0-1"`.
- **Verdict:** PASS. Line spans and line anchors survive completely intact without markup corruption.
- **Screenshot:** `/tmp/opencode/screenshots/07-anchors-verified.png` (`md5: 3c2bd396ee1d49b6583a09811d987117`). the 14-line Blastfile fence in slate; the 14 `#__span-0-*`/`#__codelineno-0-*` counts are DOM facts (table above) and the click to `#__codelineno-0-1` is the text evidence — no anchors are visible in this capture.

---

## 4. Copy Button: Byte-for-Byte Verification (Against Raw Markdown Fence Source)

On `/blastfile/`, the copy button (`button.md-code__button[title="Copy to clipboard"]`) targeting `#__code_0 > code` was clicked in Chrome.

The raw Markdown source lines (`docs/blastfile.md:7-20`) comprise exactly **264 bytes** including the single trailing newline `\n`. The built `<code>.textContent` is also **264 bytes** including that same trailing newline.

Material for MkDocs / clipboard.js applies `.trimEnd()` (`r.trimEnd()`) when placing text on the clipboard, stripping that single trailing `\n` to yield **263 bytes**. `trimEnd()` removes only trailing whitespace, so any injected token character inside the retained 263 bytes would break the byte equality. Comparing the raw fence source without its final newline against the clipboard text confirms byte-exact identity:

### Byte Comparison Details

- **Raw Markdown Source (`docs/blastfile.md:7-20`, with trailing `\n`):** **264 bytes**
- **Raw Markdown Source (`docs/blastfile.md:7-20`, minus trailing `\n`):** **263 bytes**
- **Built `<code>.textContent` (with trailing `\n`):** **264 bytes**
- **Clipboard Output (`intercepted`):** **263 bytes**
- **Byte Equality (`intercepted === rawFenceSourceNoTrailingNewline`):** **`true`**
- **Token Character Injections:** 0 (no injected classes, spans, or syntax tokens in text)
- **Filename Label Leaks:** `false` (no `Blastfile` header text copied)
- **Line Number Leaks:** `false` (no `__codelineno` or line numbers copied)
- **Pasted Text Illustration:** A scratch editor textarea was populated with the intercepted clipboard text directly below the code block.
- **Verdict:** PASS. The copy button reproduces the raw manifest source byte-for-byte; the 1-byte difference is the single trailing newline stripped by Material's copy handler, not a trim of any manifest content.
- **Screenshot:** `/tmp/opencode/screenshots/08-copy-button-verified.png` (`md5: b283b6e4cba2d443c1dc658b1c5c3e4a`). Captured in dark scheme (`slate`), showing the code block and the scratch editor component displaying the pasted clipboard text.

---

## 5. Instant Navigation (`document$`)

Material for MkDocs uses `document$` for client-side instant navigation swaps without full page reload.

1. **Start Page:** Home (`/`)
   - Initial URL: `http://127.0.0.1:44321/`
   - Initial highlighted Blastfile count: 1
   - Screenshot: `/tmp/opencode/screenshots/09-instant-nav-before.png` (`md5: 722cf347aa7705d5d3ba054f4bd409e4`)
2. **Action:** Click navigation link to "Blastfile reference" (`/reference/blastfile/`).
3. **End Page:** Reference (`/reference/blastfile/`)
   - Swapped URL: `http://127.0.0.1:44321/reference/blastfile/`
   - Highlighted Blastfile count after instant nav swap: **14**
   - Screenshot: `/tmp/opencode/screenshots/10-instant-nav-after.png` (`md5: 58c98a2fdedd853abb160fb6bb69ef23`)
   - *Deterministic Capture Note:* `10-instant-nav-after.png` is legitimately the same deterministic capture of the reference page in light mode as `01-reference-light.png` (`58c98a2fdedd853abb160fb6bb69ef23`), confirming that client-side instant navigation renders the exact same DOM and highlighted styling as direct initial navigation.
- **Verdict:** PASS. The `document$.subscribe(highlight)` entry point colours all newly inserted fences on instant navigation.

---

## 6. Plain Blocks Stay Plain (In Frame)

The four named plain `text` blocks were inspected in Chrome:

| Page | First Line / Content | `data-blastfile-highlighted` | Token Spans (`.k`, `.nv`, `.s1`, etc.) | Monospace Font | Result |
|---|---|---|---|---|---|
| `/reference/blastfile/` | `file        → app? entrypoint? section*` (EBNF grammar) | **No** (`false`) | **0** | `JetBrains Mono, monospace` | PASS |
| `/reference/blastfile/` | `Blastfile:3:9: = is not Blastfile syntax…` (Diagnostic) | **No** (`false`) | **0** | `JetBrains Mono, monospace` | PASS |
| `/how-it-works/` | `/run/blastit/views/<uid>/ 0700…` (Path fragment) | **No** (`false`) | **0** | `JetBrains Mono, monospace` | PASS |
| `/frameworks/php/` | `sudo apt install libphp-embed php-pgsql…` (Console snippet) | **No** (`false`) | **0** | `JetBrains Mono, monospace` | PASS |

- **In-Frame Capture:** `/reference/blastfile/` was scrolled to center the EBNF plain block in full view for `11-plain-blocks-verified.png`.
- **Verdict:** PASS. None of the four plain blocks carry marker attributes or token spans; all remain standard plain monospaced text.
- **Screenshot:** `/tmp/opencode/screenshots/11-plain-blocks-verified.png` (`md5: ad077c8431ec57cb9b87153e27d1e647`). Scrolled directly to the EBNF plain block in light scheme.

---

## 7. No-JS Fallback

JavaScript execution was disabled via CDP (`Emulation.setScriptExecutionDisabled: true`) and `/blastfile/` was loaded fresh:

- **Code presence:** `<code>` element is present and fully visible.
- **Marker presence:** `data-blastfile-highlighted` is `false`.
- **Token spans:** 0 (no runtime DOM mutation occurred).
- **Line spans (`__span-*`):** 14 (rendered by Zensical build).
- **Line anchors (`__codelineno-*`):** 14 (rendered by Zensical build).
- **Text content:** Unchanged 14-line manifest.
- **Layout & Typography:** Preserved monospaced formatting, clean fallback.
- **Verdict:** PASS. The highlighter functions as true progressive enhancement.
- **Screenshot:** `/tmp/opencode/screenshots/12-no-js-fallback.png` (`md5: b100647f2facf22121b4f15696ad0940`). Captured with JavaScript disabled in light scheme.

---

## 8. Narrow Viewport Responsiveness (~390 px)

Chrome viewport was set to 390×844 (mobile emulation):

### Container & Scroll Metrics on `div.highlight code` (390 px Viewport)

| Container / Element | Target Selector | Computed CSS `overflow-x` | `clientWidth` | `scrollWidth` | Max `scrollLeft` | Reachability & Overflow Behavior |
|---|---|---|---|---|---|---|
| **Page Root** | `documentElement` | `visible` | **390 px** | **390 px** | 0 px | **Zero page-level horizontal overflow** (`scrollWidth == clientWidth`) |
| **Code Element** | `div.highlight code` (Shop Blastfile fence) | `auto` | **390 px** | **401 px** | **11 px** | **No text clipping:** At `scrollLeft = 0`, line 8 ends at 384.98 px, which is **5.02 px inside the right edge**. The 11 px difference is container scrollbox padding (`padding-right: 16px`), not text clipping |
| **Pre Wrapper** | `.highlight pre` | `visible` | **390 px** | **390 px** | 0 px | Contained within 390 px layout |
| **Django Page Root** | `documentElement` (`/frameworks/python/django/`) | `visible` | **390 px** | **390 px** | 0 px | **Zero page-level horizontal overflow** |
| **Django Code Block** | `div.highlight code` (long SQL statements in Blastfile fence) | `auto` | **390 px** | **710 px** | **320 px** | Contained cleanly inside `code` scroll container; line 20 (87 chars, width ≈ 678 px) scrolls internally up to 320 px without overflowing the page viewport |

### Mobile Light & Dark Screenshots

Both mobile captures were taken with their respective color schemes actively set and verified:
- **Light Mobile (`default` scheme):** `/tmp/opencode/screenshots/13-mobile-blastfile-light.png` (`md5: 57cd42866d98d8823491b1cf917bd10b`)
- **Dark Mobile (`slate` scheme):** `/tmp/opencode/screenshots/14-mobile-blastfile-dark.png` (`md5: de470013183fa0ae5f8542275a34e0f2`)
- **MD5 Comparison:** Both files exist, differ visibly, and have distinct hashes (`57cd42...` vs `de4700...`).
- **Django Mobile Dark:** `/tmp/opencode/screenshots/15-mobile-django-dark.png` (`md5: 2b27dd603cc01ec1c9ceb8ef6e2294d0`)

---

## Screenshot Inventory & MD5 Hashes

| File | Resolution / Scheme | Description | MD5 Hash |
|---|---|---|---|
| `01-reference-light.png` | 1280×800 / Light | Reference page token palette in default light scheme | `58c98a2fdedd853abb160fb6bb69ef23` |
| `02-reference-dark.png` | 1280×800 / Dark | Reference page token palette in slate dark scheme | `1b4bff01c63dcdc7248b0ca1d440e4cf` |
| `03-blastfile-doc-light.png` | 1280×800 / Light | Document page (`/blastfile/`) in light scheme | `782f653b893d6c555341f17e8bfe4a26` |
| `04-blastfile-doc-dark.png` | 1280×800 / Dark | Document page (`/blastfile/`) in dark scheme | `a4fec269eb9860be4d1b0413ebcf336c` |
| `05-scoping-comparison-light.png` | 1280×800 / Light | Scoping witness on `/getting-started/`: Blastfile vs Python fence (Light) | `c2ae4b70315c2227db0b341ff2bfc0fd` |
| `06-scoping-comparison-dark.png` | 1280×800 / Dark | Scoping witness on `/getting-started/`: Blastfile vs Python fence (Dark) | `cfc6a0176b3a0632d1f54356359351ad` |
| `07-anchors-verified.png` | 1280×800 / Dark | the 14-line Blastfile fence in slate; the 14 `#__span-0-*`/`#__codelineno-0-*` counts are DOM facts (table above) and the click to `#__codelineno-0-1` is the text evidence — no anchors are visible in this capture | `3c2bd396ee1d49b6583a09811d987117` |
| `08-copy-button-verified.png` | 1280×800 / Dark | Document page (`/blastfile/`) in dark scheme (slate) with scratch editor showing pasted text | `b283b6e4cba2d443c1dc658b1c5c3e4a` |
| `09-instant-nav-before.png` | 1280×800 / Light | Instant navigation start: Home page (`/`) before link click | `722cf347aa7705d5d3ba054f4bd409e4` |
| `10-instant-nav-after.png` | 1280×800 / Light | Instant navigation end: Reference page post-swap (deterministic match to `01`) | `58c98a2fdedd853abb160fb6bb69ef23` |
| `11-plain-blocks-verified.png` | 1280×800 / Light | Plain block in frame: EBNF grammar fence (`file → ...`) with 0 token spans | `ad077c8431ec57cb9b87153e27d1e647` |
| `12-no-js-fallback.png` | 1280×800 / Light | Progressive enhancement: `/blastfile/` with JavaScript disabled | `b100647f2facf22121b4f15696ad0940` |
| `13-mobile-blastfile-light.png` | 390×844 / Light | Mobile viewport (390 px) in default light scheme | `57cd42866d98d8823491b1cf917bd10b` |
| `14-mobile-blastfile-dark.png` | 390×844 / Dark | Mobile viewport (390 px) in slate dark scheme | `de470013183fa0ae5f8542275a34e0f2` |
| `15-mobile-django-dark.png` | 390×844 / Dark | Mobile viewport (390 px) `/frameworks/python/django/` with long SQL statements | `2b27dd603cc01ec1c9ceb8ef6e2294d0` |

---

## Summary of Findings

- **BLOCKER:** 0
- **MAJOR:** 0
- **MINOR:** 0
- **Defects:** None.

Every requirement of `MB.32.2` and the epic `MB.32` has been proven, measured, and illustrated in a real browser session.

**VERDICT: PASS**
