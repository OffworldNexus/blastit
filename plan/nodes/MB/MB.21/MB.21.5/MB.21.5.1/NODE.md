---
id: MB.21.5.1
title: Rewrite the documentation site and README for Blastfile and retire reference/manifest.md
kind: task
status: ready
owner: "doc-writer"
priority: 2
depends_on: [MB.21.4.2]
paths: [README.md, "docs/**", ARCHITECTURE.md, zensical.toml]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Rewrite every live page for `Blastfile` — `README.md`, `docs/**` and `ARCHITECTURE.md` — retire
`docs/reference/manifest.md` in favour of `docs/reference/blastfile.md`, and keep every link, anchor and
line budget intact.

# Context

- The language truth is `docs/blastfile.md` + `docs/reference/blastfile.md` (`MB.21.1`); this node points
  pages at them instead of re-explaining a directive.
- Files that carry a complete manifest fence today (must keep a complete, runnable document):
  `README.md`, `docs/index.md`, `docs/getting-started.md`, `docs/local.md`, `docs/frameworks/index.md`,
  `docs/frameworks/{django,flask,php,static-and-spa}.md`. The reference page is the new home for the
  per-key tables that `docs/reference/manifest.md` holds today (groups → traits, ingress, static, PHP,
  proxies → store blocks, grants, routing, boot validation, the safe-function set).
- Retire `docs/reference/manifest.md`, update `zensical.toml`'s nav (the language guide and reference
  were added by `MB.21.1`; the old entry goes), and repoint every inbound link. Measured citations to
  check by clicking, not only by `--strict`: `docs/troubleshooting.md:70`,
  `docs/deploy/{compose-traefik,digitalocean-paas}.md` (`../frameworks/python/django.md#manifest` after
  `MB.20`, `../frameworks/django.md#manifest` today), `docs/local.md`, `docs/ci.md`,
  `docs/frameworks/*`, `docs/reference/{cli,settings}.md`, `ARCHITECTURE.md`.
- Anchor policy: keep the heading anchors the site already links to (`#manifest`, `#pooler`,
  `#bind-parameters-server-side`, `#behind-a-connection-pooler`, `#bootstrap-an-existing-project`,
  `#fill-the-manifest-in-one-pass`) alive at their new home.
- **Do not touch `docs/frameworks/**` structure**: only the manifest fences and the reference links change
  there; `MB.20` owns the regroup and now depends on this node.
- `docs/deploy/**` keeps its pooler guidance: it is a *deployment* concern (PgBouncer / managed pooler),
  never a `Blastfile` knob — say so in one sentence where the pooler is introduced.
- Update `crates/blastit-manifest/tests/docs_fences.rs`'s `FILES` count only if this node adds or removes
  a documented complete document; the strict flip is `MB.21.5.2`.

# Acceptance

```console
$ uvx --offline zensical build --strict
$ make docs-unit
$ make doc-tone
$ wc -l README.md docs/index.md docs/getting-started.md docs/local.md docs/deploy/index.md
$ grep -rn 'blastit\.toml' README.md docs ARCHITECTURE.md --include='*.md' \
    | grep -vE 'docs/blastfile\.md|docs/reference/blastfile\.md|changelog\.md|/decisions/|/design/|/knowledge/|/testing/|/research/|/archive/'
$ grep -rn 'reference/manifest.md' README.md docs ARCHITECTURE.md --include='*.md'
```

1. The two greps print nothing: no live page names the retired file as a path (migration prose in the two
   language pages and the changelog is exempt by the pattern above), and no page links the deleted
   reference.
2. `make docs-check` exit 0 and every documented jump still lands — the author records the clicked list
   in `RESULT.md` (each heading anchor named above resolves on its new page).
3. Every page that shows a complete manifest shows a `Blastfile` (zero `=` outside raw SQL strings) and
   keeps its line budget: guides ≤ 150 lines, framework pages ≤ 120, `README.md` unchanged in length ±10.
4. `docker compose`/`doctl`/`setpriv` recipes are unchanged in behaviour: only the in-app manifest lines
   moved; the deploy pages still show both URLs and the pooler step.
5. `make docs-unit` and `make doc-tone` exit 0.

# Notes

- `python/tests/test_docs_assets.py` refuses a sentence that pairs the runtime name with the word for
  the deep-URL file (read its docstring). When a static page names that file, put the example in a fence.
- Do not "fix" a stale sentence by inventing a directive: if a page needs a capability the language does
  not have, open a `human` request instead.
