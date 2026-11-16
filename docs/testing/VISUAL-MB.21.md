# Visual & QA Record: Blastfile guide, reference, nav and colour schemes (MB.21.5.4)

- **Date:** 2026-11-16
- **Auditor:** `visual-checker`
- **Scope:** The built documentation pages `docs/blastfile.md` (`site/blastfile/`) and `docs/reference/blastfile.md` (`site/reference/blastfile/`), navigation hierarchy, anchors, tables, code blocks, Mermaid rendering, and brand palettes.
- **Commit:** `release/0.2.0`
- **Build:** `make docs-check` (`uvx --offline zensical build --strict`) → `No issues found` in 0.19 s.
- **Viewports:** Desktop (1440×900, light & dark) and Mobile (390×844 device emulation, light & dark).

## 1. Render audit matrix

| Page | Viewport | Scheme | Artifact | Result | Observation |
|---|---|---|---|---|---|
| `/blastfile/` | Desktop 1440×900 | Light | `.playwright-mcp/blastfile-desktop-light.png` | pass | Clean 3-step workflow above fold; 10-line Blastfile code block with JetBrains Mono, copy button; active nav pill on "The Blastfile"; previous/next links intact. |
| `/blastfile/` | Desktop 1440×900 | Dark | `.playwright-mcp/blastfile-desktop-dark.png` | pass | Dark slate palette (`#121820`), crisp buff ink (`#f1ece0`), warm accent highlights; high contrast; no clipping. |
| `/blastfile/` | Mobile 390×844 | Light | `.playwright-mcp/blastfile-mobile-light.png` | pass | Single-column responsive flow; zero page horizontal overflow (`scrollWidth == clientWidth == 390px`); floating TOC button present. |
| `/blastfile/` | Mobile 390×844 | Dark | `.playwright-mcp/blastfile-mobile-dark.png` | pass | Slate theme on mobile; code blocks contain lines cleanly; contrast and font hierarchy preserved. |
| `/reference/blastfile/` | Desktop 1440×900 | Light | `.playwright-mcp/reference-blastfile-desktop-light.png` | pass | Full grammar reference; left sidebar shows Reference group with 4 active entries; right TOC functional; all 16 tables format with visible borders and aligned columns. |
| `/reference/blastfile/` | Desktop 1440×900 | Dark | `.playwright-mcp/reference-blastfile-desktop-dark.png` | pass | Dark slate surface; table borders distinct; code fences readable; syntax highlighting intact. |
| `/reference/blastfile/` | Mobile 390×844 | Light | `.playwright-mcp/reference-blastfile-mobile-light.png` | pass | All 16 tables wrapped in `.md-typeset__table` scrolling internally; zero page-level horizontal overflow (`scrollWidth == 390px`). |
| `/reference/blastfile/` | Mobile 390×844 | Dark | `.playwright-mcp/reference-blastfile-mobile-dark.png` | pass | Dark mode on mobile; wide tables scroll inside containers without breaking page viewport. |
| Nav Drawer | Mobile 390×844 | Light & Dark | `.playwright-mcp/drawer-reference-mobile-dark.png`, `.playwright-mcp/drawer-reference-mobile-light.png` | pass | Hamburger drawer opens cleanly; Reference section expands into 4 entries: Command line, Blastfile reference, Settings, Log events. No dead or retired links. |
| Mermaid Diagrams | Desktop 1440×900 | Light & Dark | `.playwright-mcp/how-it-works-mermaid-light.png`, `.playwright-mcp/how-it-works-mermaid-dark.png` | pass | Mermaid diagram renders inside closed shadow root; nodes, arrows, and labels clearly drawn and styled with nuthatch palette. |

## 2. Navigation, Anchors, and Links

- **Reference Group Integrity:** Exactly four sub-entries under Reference:
  1. `reference/cli.md` (Command line)
  2. `reference/blastfile.md` (Blastfile reference)
  3. `reference/settings.md` (Settings)
  4. `reference/log-events.md` (Log events)
  The retired `reference/manifest.md` does not appear anywhere. Result: pass.
- **Cross-link & Anchor Verification:**
  - `docs/blastfile.md`: `#the-blastfile`, `#local-development`, `#cicd`, `#deployments` all present and scroll to target. Result: pass.
  - `docs/reference/blastfile.md`: `#stores`, `#traits`, `#view-directives`, `#grants`, `#blastit-check`, `#formatting`, `#limits` all present and scroll to target. Result: pass.
  - Cross-page anchors cited by `MB.21.5.1` (`reference/settings.md` -> `blastfile.md#stores`, `frameworks/django.md` -> `reference/blastfile.md#traits`, etc.) tested and verified. Result: pass.
- **Link Status:**
  - `/blastfile/`: 46 internal links verified, 0 errors, 100% answer HTTP 200. Result: pass.
  - `/reference/blastfile/`: 198 internal links verified, 0 errors, 100% answer HTTP 200. Result: pass.
- **Console Errors:** 0 console errors across all inspected pages. Result: pass.

## 3. Code Blocks and Tables

- **Code Blocks:** All 17 code blocks use monospace font (`JetBrains Mono`, `SFMono-Regular`, `Consolas`, `Menlo`, `monospace`), have functional copy buttons (`Copy to clipboard`), and contain no leaked fence markers (` ``` `). Result: pass.
- **Tables:** All 16 tables in `reference/blastfile.md` fit within container bounds on desktop and scroll within container on narrow viewports without causing viewport scroll. Result: pass.

## 4. Brand Palette & Contrast

- **Light Mode (`default`):** Background `#f4efe3`, surface `#fffcf5`, ink `#161a20` (14:1 contrast), accent `#9b4723` (5:1 contrast). Result: pass.
- **Dark Mode (`slate`):** Background `#121820`, surface `#1b232c`, ink `#f1ece0` (13.5:1 contrast), accent `#eb9a6c` (8.5:1 contrast). Result: pass.

## 5. Verdict

All items pass.
VERDICT: PASS
