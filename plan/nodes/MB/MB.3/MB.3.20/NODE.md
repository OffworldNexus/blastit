---
id: MB.3.20
title: "Held docs pass for MB.3.17: php.md fence and fallback note, design/ADR supersession lines, AUDIT note"
kind: task
status: proposed
owner: "doc-writer"
priority: 2
depends_on: [MB.3.17, MB.12]
paths:
  - docs/frameworks/php.md
  - docs/reference/settings.md
  - "docs/design/**"
  - "docs/decisions/**"
  - "docs/testing/AUDIT-MB.3.4-gate.md"
  - python/tests/runnable_docs/inventory.toml
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T16:09"
updated: "2026-11-14T16:09"
---

# Goal

Finish `MB.3.17`'s **held docs half**: the code half deleted `crates/blastit-fastcgi`, the
`[route.fastcgi]` key and `BLASTIT_ALLOW_EXTERNAL_FASTCGI`; every page that still taught or
sanctioned the external `php-fpm` path is still on disk, and one of them keeps the workspace
red. This node lands those pages after `MB.12`'s four lenses release `docs/frameworks/**`,
`docs/design/**` and `docs/decisions/**`.

# Context

- Owner decision 2026-11-14: **"No fallback please"**. `ADR-0010` is the decision of record.
- The split is recorded in `MB.3.17`'s `# Split` and `RESULT.md`. It was agreed because
  `docs/design/backend-runtimes.md` had already lost 65 KB to a second writer.
- **The code half cannot be green without this node.** `crates/blastit-manifest/tests/docs_fences.rs`
  parses every documented `blastit.toml`: `docs/frameworks/php.md`'s fence still writes
  `[route.fastcgi]`, so `cargo test -p blastit-manifest` (and therefore `make check`) fails with
  `route[0].fastcgi: unknown key`. Nothing else in the workspace fails.
- `docs/reference/settings.md` is already updated: the `BLASTIT_PHP_FPM` row is gone and
  `## Backends` is an empty heading kept only so `docs/frameworks/php.md`'s
  `../reference/settings.md#backends` anchor still resolves.

# Acceptance

```console
$ grep -rn 'route.fastcgi\|ALLOW_EXTERNAL_FASTCGI\|PHP_FPM' docs/ ; echo "docs-grep=$?"   # 1
$ grep -rn 'fastcgi' docs/ --include='*.md' | grep -v 'refuses\|FastCGI param\|deleted\|retired'
$ cargo test -p blastit-manifest --locked        # docs_fences green
$ make check
$ make docs-check
```

- `docs/frameworks/php.md`: the `[route.fastcgi]` fence becomes the `[route.php]` the code
  accepts (after `MB.3.5.4`); the `BLASTIT_PHP_FPM` / `php-fpm` fallback note is gone; the
  page says embedded `libphp` is the only PHP story and names the boot refusal.
- `docs/reference/settings.md`: `## Backends` gets the real setting (`BLASTIT_LIBPHP`) or the
  heading and the `#backends` link go together — never an empty section left behind.
- `docs/design/backend-runtimes.md` (and any `*review-*.md` companion) gains a **superseded**
  line naming `ADR-0010` and `MB.3.17` instead of being silently edited where it sanctioned
  the fallback.
- `docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md` gets a superseded header
  line (`ADR-0010` + `MB.3.17`); `docs/decisions/README.md`'s index agrees.
- `docs/testing/AUDIT-MB.3.4-gate.md` gets a superseded note, not a rewrite.
- `python/tests/runnable_docs/inventory.toml`'s php-page `reason` ("the FastCGI worker that
  serves public/index.php is 0.2.1") is re-worded to the embedded story.

# Notes

- Do not re-add a fallback, a `listen` key, or any sentence offering `uv add blastit` as the
  cure for a host without `libphp`.
- `crates/blastit-php-sapi`'s "refuses the four FastCGI names" sentences stay: they describe
  the CVE surface the shim must refuse, not the deleted transport.
