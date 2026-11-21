# RESULT — MB.32.2

**STATUS: done** (attempt 4/3; final precision wording applied)

Complete browser proof of the `Blastfile` highlighter (`MB.32.1`), scoped token palette, and progressive enhancement on the built Zensical documentation site. All measurements conducted in Google Chrome 157 via Chrome DevTools Protocol (CDP).

## Files touched (within node scope)

| File | Change |
|---|---|
| `docs/testing/VISUAL-MB.32.md` | Comprehensive visual QA report, contrast matrix, scoping witness, mobile scroll metrics, and screenshot inventory |
| `plan/nodes/MB/MB.32/MB.32.2/RESULT.md` | Acceptance execution tails, evidence summary, and screenshot MD5 hashes |

No production pages, no stylesheets, no JS assets, and no tests touched.

---

## Acceptance Commands & Output Tails

### 1. `make docs-check`

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.32s
```

### 2. Injected highlighter asset verification

```console
$ grep -c 'blastfile-highlight.js' site/index.html
1
```

### 3. Plain and highlighted code fence integrity

```console
$ grep -o 'class="language-text highlight"' site/reference/blastfile/index.html | wc -l
16
```

---

## Reviewer Text Precision Updates (Attempt 4)

1. **[MAJOR] Anchors Caption Wording:**
   - Updated caption of `07-anchors-verified.png` verbatim to: "the 14-line Blastfile fence in slate; the 14 `#__span-0-*`/`#__codelineno-0-*` counts are DOM facts (table above) and the click to `#__codelineno-0-1` is the text evidence — no anchors are visible in this capture."

2. **[MAJOR] Django Mobile Container Measurement & Label:**
   - Dropped the `immutable regex` label from the Django 390 px mobile table row.
   - Measured and reported real metrics from `div.highlight code`: `clientWidth = 390px`, `scrollWidth = 710px`, `maxScrollLeft = 320px` (accommodating line 20's 87 characters at width ≈ 678 px).
   - Relabeled `15-mobile-django-dark.png` caption: "Mobile viewport (390 px) `/frameworks/python/django/` with long SQL statements".

3. **[MINOR] `trimEnd()` Injected Token Rationale:**
   - Added explicit explanation: "`trimEnd()` removes only trailing whitespace, so any injected token character inside the retained 263 bytes would break the byte equality."

---

## Screenshot Inventory (`/tmp/opencode/screenshots/`)

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
