# RESULT — MB.3.14 attempt 2

Appended one sentence to the final bullet of `docs/knowledge/LESSONS.md` (the "A design doc is a
single-writer artefact…" bullet, committed in `dc05a48`). No new bullet, no other line, no other file.

## Acceptance

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.25s

$ uv run --no-project tools/plan.py validate
plan OK
```

`make check` deliberately **not** run: another node is mid-edit under `crates/**` (the working tree
shows unrelated in-flight changes to `crates/blastit*`), so a red run there would be meaningless.
`git diff -- docs/knowledge/LESSONS.md` shows exactly one changed line. Nothing staged or committed.

## Final bullet text (as written on that one line)

> **A design doc is a single-writer artefact, and dispatch order — not merge order — is what protects it:** `docs/design/backend-runtimes.md` existed only as an **untracked** file owned by the architect (`MB.12`); a designer was dispatched at the same path "to expand it" while the architect's session was still live, and the architect's ~65 KB version was **irrecoverable** — no backup existed, and `git` could not restore a path that had never been tracked. Check who owns a path and whether that owner's session is still live *before* dispatching a second writer, and prefer a different file (or wait) over a later merge. Write long documents incrementally as `<name>.md.part`, renamed at the end, so a timeout or an overwrite costs one section instead of the whole document, and commit a shared design early: a committed file is recoverable, an untracked one is not. With more than one agent live, stage **explicit file paths**, never a directory or a glob (`docs/**`, `plan/`, `crates/…`): `git add <the files this node wrote>` and check `git status --short` before committing, because a broad stage silently absorbs another node's in-flight file and the commit message then lies about what changed — a commit message must be checkable against `git show --stat`, which is how this very sweep was spotted (twice: `9fab605`, `dc05a48`). (`MB.3.14`).

## Follow-ups

None.
