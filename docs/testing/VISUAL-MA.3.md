# Visual QA Report: Zensical Built Site (Milestone MA.3)

- **Date:** 2026-11-13
- **Auditor:** `visual-checker`
- **Scope:** Built documentation site (`site/` via `zensical.toml` and `make docs-check`)
- **Base Commit:** `dba348c`
- **Viewports Tested:** Desktop 1440x900 (Light & Dark), Mobile 390x844 (Light & Dark)

## 1. Viewport & Page Audit Matrix

| Page | Viewport | Status | Visual Observation |
|---|---|---|---|
| `/` (Index) | 1440x900 Light | PASS | Nuthatch hero mascot, 3-step zero-sudo command box above fold, clear brand palette |
| `/` (Index) | 1440x900 Dark | PASS | Crisp slate/navy palette, white logo icon visible in header, no contrast clipping |
| `/` (Index) | 390x844 Mobile | PASS | Single column layout, no horizontal overflow, hamburger drawer opens cleanly |
| `/getting-started/` | 1440x900 Light | PASS | Clean 3-step zero-sudo workflow (Install, 2 files, Run), no walls of prose |
| `/getting-started/` | 1440x900 Dark | PASS | High-contrast syntax highlighting on code blocks, no text clipping |
| `/getting-started/` | 390x844 Mobile | PASS | Code blocks scroll internally, body `scrollWidth == clientWidth` (no horizontal scroll) |
| `/local/` | 1440x900 Light | PASS | Scannable tables, bolded warnings against production data, clean layout |
| `/ci/` | 1440x900 Light | PASS | One-command summary (`blastit check`), table fits perfectly, no overflow |
| `/reference/cli/` | 1440x900 Light | PASS | Sidebar nav with active highlight pill, command tables cleanly aligned |
| `/reference/manifest/` | 1440x900 Light | PASS | Top-level keys table formatted cleanly, code annotations aligned |
| `/reference/manifest/` | 390x844 Mobile | PASS | Multi-column table scrolls within its container via `extra.css`, no viewport break |
| `/how-it-works/` | 1440x900 Light | PASS | Both Mermaid diagrams rendered into SVG (topology + sequence), clear typography |
| `/how-it-works/` | 1440x900 Dark | PASS | Both Mermaid diagrams rendered with dark theme adaptation, no inverted labels |
| `/how-it-works/` | 390x844 Mobile | PASS | Diagrams scale to viewport width, tables scroll internally, no overflow |
| `/deploy/fair-queuing/` | 1440x900 Light | PASS | Mermaid flow diagram rendered with custom nuthatch palette node fills |
| `/troubleshooting/` | 1440x900 Light | PASS | Two-column Symptom/Fix table clearly readable, code pills aligned |

## 2. Rendering & Diagram Verification

- **Mermaid Shadow Roots:** Inspected visually via browser screenshot tools. All three Mermaid diagrams (`how-it-works.md` process topology, `how-it-works.md` request sequence, `deploy/fair-queuing.md` two-level flow diagram) draw successfully as SVG graphs with nodes, connectors, and text labels (not unrendered code blocks).
- **Navigation & Reader Journey:** Zero 404s across all internal links. Page sequence matches developer progression (Home -> Getting started -> Local -> CI/CD -> Deployments -> Frameworks -> Reference -> How it works -> Security -> Troubleshooting -> Changelog).
- **Console & Network:** 0 network errors, 0 runtime console errors.

## 3. Defects & Minor Observations

- **[MINOR] Header Logo Contrast in Light Mode:** `docs/brand/logo.svg` has a hardcoded `fill="#fbfaf6"` (paper white). In dark mode (`[data-md-color-scheme="slate"]`), the logo contrasts against the dark header. In light mode, the header background is `#f4efe3` / semi-transparent white, making the white cutout low-contrast relative to the `blastit` text title. (Non-blocking: text header provides clear branding; follow-up can apply SVG color inversion or `mascot-head-ink.svg` for light scheme).

## 4. Verdict

VERDICT: PASS
