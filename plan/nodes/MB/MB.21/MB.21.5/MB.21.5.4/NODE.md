---
id: MB.21.5.4
title: "Browser proof of the Blastfile pages, nav and both colour schemes (visual-checker)"
kind: task
status: ready
owner: "visual-checker"
priority: 3
depends_on: [MB.21.5.2]
paths: ["docs/testing/VISUAL-MB.21.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Prove with a browser that the two new pages are readable and navigable: the nav entries, the grammar
tables, the long `Blastfile` examples and the cross-links render in both colour schemes, on the built
site — not on a screenshot of the source.

# Context

- Zensical renders Mermaid inside a closed shadow root and `--strict` never renders a page
  (`LESSONS.md`), so this check is the only honest proof of layout.
- Build and serve with the documented targets (`make docs-check`, then a local serve), browse
  `docs/blastfile.md` and `docs/reference/blastfile.md` plus the top-level nav entry, at desktop and a
  narrow viewport, in light and dark scheme.
- Look specifically at: the language example on the guide (no horizontal scroll, no clipped line), the
  reference's tables (they are wide: confirm they scroll inside their own container rather than the
  page), the code fences' syntax highlighting contrast, the "next" links from `docs/getting-started.md`
  and the frameworks pages into the reference, and that the retired reference page's old URLs no longer
  appear in the nav.
- Report to `docs/testing/VISUAL-MB.21.md`: one line per page/viewport/scheme, the screenshot paths, and
  a pass/fail per item with the actual observation. File a defect as a new node if anything is wrong;
  do not fix `docs/**` here (it is `MB.21.5.1`'s lane).

# Acceptance

```console
$ make docs-check
$ ls docs/testing/VISUAL-MB.21.md
$ grep -c 'pass' docs/testing/VISUAL-MB.21.md
```

1. `VISUAL-MB.21.md` records both pages at desktop and narrow widths, light and dark, with a pass/fail
   per item above and the screenshot path for each.
2. Tables do not overflow the viewport; no code block is clipped or horizontally scrolled at the narrow
   width; text contrast is legible in both schemes.
3. The nav exposes the guide and the reference at the expected level, and every link from another page
   cited by `MB.21.5.1` lands on the intended heading.
4. Anything that fails is filed as a follow-up node with the page, the viewport and the observation.
5. `make docs-check` exit 0 (a visual pass never excuses a broken build).

# Notes

- Use the real built site, not a raw Markdown preview: the theme's CSS is what a reader sees.
- Keep screenshots out of the repository unless the existing convention stores them under `docs/testing/`;
  otherwise cite the tool's own path.
