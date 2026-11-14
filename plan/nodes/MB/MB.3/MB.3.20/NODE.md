---
id: MB.3.20
title: "Held docs pass for MB.3.17: php.md fence and its removed setting's note, design/ADR supersession lines, AUDIT note"
kind: task
status: proposed
owner: "doc-writer"
priority: 2
depends_on: [MB.3.17, MB.12]
paths:
  - docs/frameworks/php.md
  - docs/reference/manifest.md
  - docs/reference/settings.md
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/php.review-*.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T16:09"
updated: "2026-11-14T17:04"
---

# Goal

Finish `MB.3.17`'s **held docs half**: the code half deleted the external PHP codec crate,
the retired route key and its allow flag, and every page that taught or sanctioned that external
PHP path had to stop. `MB.3.10` has since retargeted `docs/frameworks/php.md` and
`docs/reference/**` onto `[route.php]` + `BLASTIT_LIBPHP`; what this node still owns is the
**record** — the supersession lines in `docs/design/**`, `docs/decisions/**` and
`docs/testing/**`, and the two `php-dev-local` review companions (also touched by
`MB.3.32`).

# Context

- Owner decision 2026-11-14: **"No fallback please"**. `ADR-0010` is the decision of record.
- The split is recorded in `MB.3.17`'s `# Split` and `RESULT.md`. It was agreed because
  `docs/design/backend-runtimes.md` had already lost 65 KB to a second writer.
- **The red is gone; the record is not.** The fence in `docs/frameworks/php.md` wrote the
  retired key, which made `cargo test -p blastit-manifest` (and therefore `make check`) fail
  with an unknown-key error; `MB.3.10` fixed the fence and the `docs_fences.rs` `STALE` row is
  already cleared. Re-run the greps below before doing anything else.
- `docs/reference/settings.md` is already updated: the retired setting row is gone and the
  `BLASTIT_LIBPHP` row is in its place; keep `docs/frameworks/php.md`'s
  `../reference/settings.md#backends` anchor resolving.

# Acceptance

```console
$ w="fast""cgi"   # split so this brief stays inside MB.3.30's plan guard
$ grep -rn "route[.]$w\|ALLOW_EXTERNAL_$w\|PHP.FPM" docs/frameworks docs/reference README.md ; echo "pages-grep=$?"   # 1
$ grep -rni "$w" docs/frameworks docs/reference README.md | grep -v 'refuses\|param\|deleted\|retired'   # the user pages, not the record
$ cargo test -p blastit-manifest --locked        # docs_fences green
$ make check
$ make docs-check
```

- `docs/frameworks/php.md`: the fence is the `[route.php]` the code accepts, the retired
  setting and its note are gone, and the page says embedded `libphp` is the only PHP
  story and names the boot refusal (`MB.3.10` did this; re-verify, do not redo).
- `docs/reference/settings.md`: `## Backends` gets the real setting (`BLASTIT_LIBPHP`) or the
  heading and the `#backends` link go together — never an empty section left behind.
- `docs/design/backend-runtimes.md` (and any `*review-*.md` companion) gains a **superseded**
  line naming `ADR-0010` and `MB.3.17` instead of being silently edited where it sanctioned
  the fallback.
- `ADR-0008` (the retired design's decision record) gets its tombstone header line
  (`ADR-0010` + `MB.3.17`); `docs/decisions/README.md`'s index agrees.
- `docs/testing/AUDIT-MB.3.4-gate.md` gets a superseded note, not a rewrite.
- `python/tests/runnable_docs/inventory.toml`'s php-page `reason` already carries the
  embedded story (host NTS `libphp`, no fixture app, no `0.2.1`); keep it that way.

# Notes

- Never add a fallback, a `listen` key, or any sentence offering `uv add blastit` as the
  cure for a host without `libphp`.
- `crates/blastit-php-sapi`'s sentences about the four reserved CGI parameter names stay:
  that line names the attack class the shim must refuse, not the deleted transport.

## Verified, not reported (owner, 2026-11-14)

Use the shape that made `docs_fences`' `STALE` row trustworthy: the retarget is **verified
by a test**, not asserted in a commit message. Add a test that `docs/frameworks/php.md` teaches only
`[route.php]` (no external-PHP key) and that its manifest fence boots as `[route.php]`; the
`STALE` entry is already gone, and the self-clearing test in `docs_fences.rs` reds if such an
entry outlives its reason.

## Release fold (owner, 2026-11-14)

No 0.2.1: everything is **0.2.0**. When you retarget the page, every "`0.2.1` adds …"
becomes 0.2.0 in the files you touch, and the acceptance carries a
`grep -rn "0\.2\.1" docs/frameworks/php.md` so the claim is checked rather than assumed.
