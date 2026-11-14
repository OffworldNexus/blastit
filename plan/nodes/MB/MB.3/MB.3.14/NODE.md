---
id: MB.3.14
title: "Record the design-doc collision in LESSONS: one writer per design doc, incremental .part writes, dispatch order"
kind: task
status: active
owner: "doc-writer"
priority: 1
depends_on: []
paths: [docs/knowledge/LESSONS.md]
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-14T14:43"
updated: "2026-11-14T14:52"
---

# Goal

Record the 2026-11-14 design-document collision in `docs/knowledge/LESSONS.md`, with the
rule that prevents a repeat. Three lessons, one bullet.

# Context

`docs/design/backend-runtimes.md` existed only as an untracked file. The architect (owning
`MB.12`) had written it; a designer dispatched by this lead wrote the same path while the
architect's session was still live, and the architect's ~65 KB version was **irrecoverable**
— no backup was made and `git` could not help because the path was never tracked.

The bullet must say:
1. **One writer per design document.** A design doc is a single-writer artefact.
2. **Write long documents incrementally** (`<name>.md.part`, renamed at the end), so a
   timeout or an overwrite costs one section rather than the whole document; and commit a
   shared design early, because a committed file is recoverable and an untracked one is not.
3. **Dispatch order, not merge order, prevents this.** Never point a designer at a path an
   architect owns "to expand it" while that architect is still running — check who owns the
   path and whether that session is live *before* dispatching, and prefer a different file
   (or wait) over a later merge.

# Acceptance

```console
$ make docs-check
$ make check
```
- Exactly one new bullet appended to `docs/knowledge/LESSONS.md`, in the file's existing
  style (bold lead-in, then the concrete facts: the path, the loss, the two rules).
- No existing bullet edited or reordered; no `docs/**` page touched.
- The bullet names the specific failure (untracked + two writers + no recovery) rather than
  a generic "be careful with docs" sentence.

# Notes

- `LESSONS.md` is excluded from the public site; keep it factual and short.
- Do not mention any external client or sibling project (§11 of `AGENTS.md`).
