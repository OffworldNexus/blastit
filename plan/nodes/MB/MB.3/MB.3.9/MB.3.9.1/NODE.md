---
id: MB.3.9.1
title: "Rewrite the PHP page Local section for the automatic dev pool (two commands, dx + ai-ergonomics acceptance)"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: []
paths:
  - docs/frameworks/php.md
  - python/tests/runnable_docs/inventory.toml
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - docs/reference/settings.md
  - "docs/design/php-dev-local.review-*.md"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T13:17"
updated: "2026-11-14T14:09"
---

# Goal

Rewrite `docs/frameworks/php.md` for the two decisions that landed on 2026-11-14: the
route is **one line** (no `listen`, no hand-tuned `workers`) and **`blastit dev` spawns and
supervises the pool**. The page is the target DX, written before the code (DX-First law).

# Context

- Owner: "a PHP developer with a Symfony app runs two commands and sees their app." The
  manifest is one line; the second command is `blastit dev`. Every other step is a defect.
- The manifest block becomes exactly:

```toml
[route.fastcgi]
entry = "public/index.php"
```

  Delete the `listen` row from the key table and delete any sentence about matching
  `pm.max_children` (blastit derives the worker count from its own budget). `workers` may
  appear only as an optional advanced override documented as "usually omit".
- Delete the Local-section ceremony: the `php-fpm.conf` block, `mkdir -p run`,
  `php-fpm -F -y …`, and the `uv init --bare` / `uv add` steps, in favour of one honest
  install line. **Do not invent** npm/standalone/package-manager commands — the
  distribution story is `MB.12`'s; keep the install sentence generic and note it in the
  RESULT.
- Say what blastit figures out, briefly and concretely (socket path, worker count,
  `listen.mode`, which `php-fpm` it found), so the reader knows there is no hidden knob —
  one short sentence or a few-row "blastit derives" table, not a lecture.
- **The override is reference material, not quickstart material.** Add exactly one row to
  `docs/reference/settings.md` for `BLASTIT_PHP_FPM` (what it does → default search order
  `php-fpm`, then versioned names in `PATH` → it is env-only, never a manifest key), and in
  `php.md` write one short sentence linking to it for a non-standard install. The page
  itself stays "one line of manifest, one command".
- Keep CI/CD (`blastit check -- <suite>`) and Deployments consistent with the same
  two-command local story. Keep the page lean (guide under ~150 lines).
- Fence accounting: `crates/blastit-manifest/tests/docs_fences.rs` counts this page's
  fences (`MB.3.5.1` bumps the file list to 8). A removed/added fence needs that
  accounting updated, and `make docs-check` (`zensical build --strict`) must stay green.
  A fence that cannot be true until `MB.3.9.4` lands stays **accounted** with a note in
  `python/tests/runnable_docs/inventory.toml`, exactly like the existing entries — never a
  fence that silently claims to have run.

# Acceptance

```console
$ make docs-check
$ make docs-test
$ cargo test -p blastit-manifest --test docs_fences --locked
$ make check
```
- Two fresh critics PASS with no unresolved `BLOCKER`/`MAJOR`: `dx` (fewest steps, zero
  `sudo`, working example first, no kernel trivia) and `ai-ergonomics` (an agent reads the
  page and produces a correct `blastit.toml` plus the one command without guessing).
  Brief both with: "a Symfony developer sees exactly what they must write and what blastit
  figures out — is anything here a knob they should not own?"
- `grep -c 'php-fpm -F' docs/frameworks/php.md` is `0`; the page names exactly one blastit
  command in Local; `grep -c 'listen' docs/frameworks/php.md` finds no manifest key.

# Notes

- `docs/reference/manifest.md`'s FastCGI section is `MB.3.5.2`'s; if it still shows a
  `listen` row when you finish, file a follow-up naming that leaf rather than editing it.
- Store critic reports as `docs/design/php-dev-local.review-<lens>.md` (design-docs skill)
  and resolve every finding before reporting done.
