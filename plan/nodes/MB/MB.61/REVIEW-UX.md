# REVIEW-UX — MB.61 (`docs/local.md`, `docs/index.md`)

Lens: developer experience, terminal/docs craft. Base `76b1bcb`.
Scope reviewed: the one docs-only diff (7 added lines in `docs/local.md`,
2 changed in `docs/index.md`) plus surrounding pages and `LESSONS.md`.

Verdict: **CHANGES** (1 × [MAJOR], 2 × [MINOR], 3 × [NIT]).

## What is already right (no action)

- Caveat placement: the explanation lands inside the existing collapsed
  `??? note "Developing on macOS or Windows?"` (`docs/local.md:18`), still after
  the first `blastit dev` console block — no open `!!! warning`, no caveat before
  command 1. Unchanged structure, correct instinct.
- Single source: `grep -rn "kernel facilit" docs/ README.md` → only
  `docs/local.md:24` (mechanism list) and `docs/index.md:38` (one pointer clause).
  No third copy, no mechanism list on the front page.
- Link resolves: `[why](local.md)` is a same-directory page with a nav entry
  (`zensical.toml:26`); `make docs-check` → `Build started / No issues found /
  Build finished in 0.21s` (strict link + anchor + nav clean).
- Tone: "Not packaging:" is evidence, not apology; no diary heading, no platform
  matrix, no "coming soon"; "**for now**" is the node-mandated hedge, not a
  roadmap promise. Mermaid (`local.md:55`, `index.md:93`) untouched. Added lines
  are 76–83 cols, consistent with the file's wrap.

## Findings

### [MAJOR] `docs/local.md:25` — "which production enforces:" wrongly implies production owns all four mechanisms, and contradicts this page 8 lines earlier

The added sentence reads:

```
    equivalent, which production enforces: a per-`view` uid split (`setresuid`,
    ... `openat2`-confined static reads ... seccomp
    for forged outgoing data, and `SCM_CREDENTIALS` between roles.
```

Only the uid split is production-only. `crates/blastit/src/stage1/spawn.rs:430-447`
shows `blastit dev` substituting `Privileges::Shared` for `Privileges::DropTo` and
*still* installing `Filter::Seccomp(SeccompRole::View { egress })` (`seccomp` is
only off under an explicit `BLASTIT_SECCOMP=0`), and `docs/local.md:13-14`
already tells the same reader "The credential barrier and the grants are real"
in dev — the credential barrier *is* `SCM_CREDENTIALS`. So the new sentence
retracts the page's own security claim in passing: a careful reader gets
"dev has no seccomp and no credential check", which is false.

Minimal fix — scope "production" to the uid split, keep every other word:

```markdown
    Not packaging: serving rests on Linux kernel facilities with no portable
    equivalent. In production a per-`view` uid split (`setresuid`,
    `CAP_SETUID`/`capset`) is the isolation boundary; static reads are confined by
    `openat2` (`RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS`, with a verified-walk
    fallback on older kernels); a seccomp filter refuses sockets a route did not
    grant; roles identify each other with `SCM_CREDENTIALS`. Porting means
    replacing them — a design project, not a recompile — so serving stays Linux
    **for now** while the editor ships native everywhere.
```

### [MINOR] `docs/local.md:26-27` — the rewrap splits the `RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS` pair at its slash, so it renders with a space

Line 26 ends `(`RESOLVE_BENEATH`/` and line 27 starts `RESOLVE_NO_SYMLINKS``.
Markdown folds the newline to a space, so the built page shows
`(RESOLVE_BENEATH/ RESOLVE_NO_SYMLINKS` — a stray space inside a flag pair, the
one place the wrap break is not a word boundary (every other break in this
paragraph, and in lines 20-22, is between words).

Minimal fix: keep the pair on one line — the [MAJOR] replacement above already
does, e.g. end line 1 after "confined by" and carry
`` `openat2` (`RESOLVE_BENEATH`/`RESOLVE_NO_SYMLINKS`, with a verified-walk ``
whole.

### [MINOR] `docs/local.md:27-28` — "seccomp for forged outgoing data" is undecodable for a first-time reader

Nothing is "forged": the filter refuses creating `AF_INET`/`AF_INET6` sockets
for a route without `egress` (`crates/blastit-sys/src/seccomp.rs:39-44`,
`SeccompRole::egress`), i.e. it stops a view opening its own connection instead
of using the route's proxy. A macOS/Windows reader cannot map "forged outgoing
data" onto anything they have seen in this page, so the item costs words without
adding evidence.

Minimal fix: "a seccomp filter refuses sockets a route did not grant" (as in the
[MAJOR] block).

### [NIT] `docs/local.md:30` — "while the editor ships native everywhere" is a repeat

`docs/local.md:21-22` already says `blastit-lsp` ships native Linux, macOS and
Windows, inside the same note, 8 lines up. Optional trim: end the paragraph at
`so serving stays Linux **for now**.` — the misconception is already dead by
then; keeping the clause is defensible only if you want the contrast spelled
out twice.

### [NIT] `docs/index.md:38` — link text `why` alone

Renders "…rests on Linux kernel facilities (why)." Generalises badly (a screen
reader hears "why, link") and scans as vague next to the other front-page links
(`install reference`, `Local`, `CI/CD`).

Minimal fix: `([why Linux](local.md))` — same clause length, one word of context.

### [NIT] follow-up, outside this node's `paths` — `docs/reference/cli.md:121-123`

Same public page family still carries a rival one-line answer: "serving an app
needs Linux, as root or with the capability set
([deployments](../deploy/index.md))". It does not contradict the new paragraph,
but a reader who arrives at the CLI reference still concludes packaging gap, and
it is now the only place in `docs/` that says "why" without pointing at the
authoritative explanation. Worth a follow-up node: point it at `local.md` and
drop the capability-set detail (`grep -rn "kernel facilit" docs/` stays exactly
two hits).
