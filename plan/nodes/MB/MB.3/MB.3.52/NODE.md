---
id: MB.3.52
title: "MB.19 review MINORs: the canonical manifest fence is untested, plus three page corrections"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.19]
paths:
  - docs/frameworks/index.md
  - docs/frameworks/flask.md
  - docs/frameworks/django.md
  - "crates/blastit-manifest/tests/docs_fences.rs"
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-15T08:01"
updated: "2026-11-15T08:21"
---

# Goal

Close the four MINOR findings from MB.19's attempt-2 reviewer. Three are page
precision; one is a real hole: the canonical manifest shape that every framework page
copies is **not registered** with the fence test, so it can drift untested.

# Context

MB.19 (commit b2d0fa2) made `/frameworks/` one intro plus four pages filling the same
nine slots. Its reviewer PASSed and left:

1. `docs/frameworks/flask.md:83-86` — slot 4 is prose only and the page's manifest
   (`:8-25`) carries no `[route.ingress]`, so "the same blocks in the same order" is not
   shown and a reader who posts a form gets `413`. Add `[route.ingress]` with
   `body = "form"` to the `products` route, in the intro's shape.
2. `docs/frameworks/index.md:24-39` — the canonical fence is not registered in
   `crates/blastit-manifest/tests/docs_fences.rs` (`FILES`, `:22-31`) and lacks
   `title="blastit.toml"`, so the shape every page copies can drift untested. Title the
   fence and add the page to `FILES`.
3. `docs/frameworks/django.md:57-58` — "`collectstatic` nests twice under `admin/`" is
   false for vendor assets (`admin/js/vendor/jquery/…`, depth 5), so the three globs leave
   those hashed files revalidated. Say "as deep as the deepest fingerprinted file" and add
   the deeper glob (or drop the claim).
4. `docs/frameworks/index.md:82-83` — "a check that queries the database reports the
   pool's health, not the app's" is ambiguous; it never says what to do. Say: "keep
   database queries out of any health route: the platform check then learns blastit is up,
   not the pool's health".

# Acceptance

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
$ make docs-check
$ make docs-unit
$ make docs-test
```
- `docs/frameworks/index.md`'s canonical fence carries `title="blastit.toml"`, is listed
  in `docs_fences.rs`'s `FILES`, and the suite validates it — paste the test output, and
  prove it is load-bearing by breaking one key in a `/tmp` copy of the page and showing
  the suite reddens (then restore; `git diff --stat` unchanged).
- `flask.md`'s manifest shows `[route.ingress] body = "form"` for the posting route, and
  the page still fits its 120-line budget.
- The django nesting claim is either true of vendor assets or replaced with a sentence
  that is; if you add a glob, say what it matches.
- The health sentence says what to do, not just what happens.
- `make docs-check`, `make docs-unit`, `make docs-test` green (report the
  executed/accounted counts); no page exceeds its budget; no fence unregistered.

# Notes

- Owner is `implementer` rather than `doc-writer` because item 2 needs
  `crates/blastit-manifest/tests/docs_fences.rs`; the three page edits are small prose
  corrections and are authorised under this node's paths.
- Do not re-run the DX/AI critics: MB.19's two rounds already accepted this structure.
