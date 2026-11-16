---
id: MB.21.1
title: "Write the Blastfile basic guide and complete language reference, register them, pass dx + ai-ergonomics critics"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: []
paths:
  - docs/blastfile.md
  - docs/reference/blastfile.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/blastfile-docs.review-dx.md"
  - "docs/design/blastfile-docs.review-ai-ergonomics.md"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T01:31"
---

# Goal

Write the two pages that define `Blastfile` for a developer — **before** any design or code:

* `docs/blastfile.md` — the **basic guide** (≤ 150 lines): one working `Blastfile`, then Local
  (zero `sudo`, `blastit dev`, `--learn`), CI/CD (`blastit check` completeness), Deployments
  (presets fail closed).
* `docs/reference/blastfile.md` — the **complete language reference**: grammar, tokens, all four
  sections, every view directive, path patterns, static mounts, precedence, `blastit fmt`, the error
  contract, and the `blastit.toml` → `Blastfile` mapping.

Register both in `zensical.toml` and in `python/tests/runnable_docs/inventory.toml`, then pass
fresh-context `dx` and `ai-ergonomics` goldfish critics on the final revision.

This node has **zero code dependencies and starts immediately**; it is the DX gate the whole epic
hangs from. The language is settled in `plan/nodes/MB/MB.21/NODE.md` — copy it, do not re-litigate it.

# Context

- The settled spec, the v1 capability map and the out-of-scope list (no `pooler`, no vendor knob, no
  Milestone C/D proxy keyword) live in `MB.21`'s body. This page set documents **only what `0.2.0`
  runs**: `app`, `entrypoint python|php`, `postgres`, `trait`/`use`, `view`, `static`, `fmt`.
- The parser does not exist yet, so no command on these pages can run. Every `console` fence gets
  `mode = "static"` with a `reason` (≥ 15 characters, real words) that **names `MB.21.5.2`** and says
  what the gate checks instead. `make check` therefore stays green before the lexer lands, and
  `MB.21.5.2` flips these entries to live execution.
- **Guide shape** (lean, one canonical way per task): a 10-line `Blastfile` first, then three short
  sections — Local, CI/CD, Deployments — each ≤ 25 lines with one command block, then one link to the
  reference. No kernel or WASM internals (those belong in `how-it-works.md` / `security.md`), no four
  alternative spellings, no apology language (`make doc-tone` refuses it).
- **Reference shape** (complete, scannable, tables over prose): `#` comments and `;`/newline
  terminators; `"..."` escaped vs `'...'` raw strings; `$IDENT` (`[A-Z_][A-Z0-9_]*`) and "a literal
  where `$VAR` is required is a `line:col` syntax error"; the four sections; both view headers; the
  pattern table (`{name}`, `{name:int}`, `{name:uuid}`, trailing `*`, `regex`); directives (`body`,
  `use`, `egress any`, `prefork`, `postgres … unrestricted`, the grant directives); static mounts and
  the four-step precedence rule; `blastit fmt [--check] [PATH]`; the error contract (`file:line:col`);
  and the migration table (one row per v1 key, each row's new spelling).
- **Do not drop real capability:** the reference must carry `preload`, the named-store form, the
  split `host`/`port`/`user`/`password`/`database` block, `immutable`, `index`, `prefork`,
  `egress any`, `transactions`, `functions`, table/column grants and `query '…'`.
- **Open question you must flag, not invent:** `[route.php] workers` has no Blastfile spelling yet.
  Document the language as settled and put one `> **Migration note**` line in the reference saying the
  per-view concurrency cap is settled by `MB.21.2`; do not promise a keyword.
- **Prose ban (`python/tests/test_docs_assets.py`):** read that file's docstring first — it refuses a
  sentence that pairs the runtime name with the word for the deep-URL file, and it counts `route` as a
  runtime word too. Language examples go in fences.
- Critics are fresh-context `critic` subagents with the `dx` and `ai-ergonomics` lenses (skill
  `design-docs`), writing `docs/design/blastfile-docs.review-dx.md` and
  `docs/design/blastfile-docs.review-ai-ergonomics.md`. Loop until both say `VERDICT: PASS`.

# Acceptance

```console
$ wc -l docs/blastfile.md docs/reference/blastfile.md
$ grep -n 'blastfile.md' zensical.toml
$ grep -c '```console' docs/blastfile.md docs/reference/blastfile.md
$ make docs-check
$ make docs-unit
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ grep -nE 'gzip|brotli|zstd|WASM|multipart|mysql|redis|mongodb|sqlite|pooler|digitalocean' docs/blastfile.md docs/reference/blastfile.md
$ grep -nE '^[[:space:]]*(app|entrypoint|postgres|trait|use|view|static|body|egress|prefork|transactions|functions|select|insert|update|delete|root|index|immutable|preload)[[:space:]].*=' docs/blastfile.md docs/reference/blastfile.md
$ grep -rn 'VERDICT' docs/design/blastfile-docs.review-*.md
```

1. `docs/blastfile.md` is **≤ 150 lines** and carries a working 10-line `Blastfile` before any caveat;
   `docs/reference/blastfile.md` is complete — every keyword in the settled spec has a row or a
   section, and every v1 key in the capability map has a migration row.
2. Both pages are in the site nav (`< 150`-line guide under the top level, reference under
   `Reference`) and `uvx --offline zensical build --strict` reports no issue.
3. **No TOML syntax leaks into a language example:** the last `grep` above prints nothing (its
   keyword list is every directive that must never take `=`; `query` is excluded on purpose because
   raw SQL contains `=` as data). No `version` header appears anywhere as syntax.
4. **No false claim:** the first `grep` prints nothing — zero Milestone C/D or vendor vocabulary, and
   no `pooler` knob inside `Blastfile`. `blastit.toml` is mentioned only in migration prose.
5. Every published console fence is accounted for in `python/tests/runnable_docs/inventory.toml` with
   `mode = "static"` and a `reason` naming `MB.21.5.2`; `make docs-unit` and `make check` are green.
6. Both critic files end in `VERDICT: PASS` for the final revision; their BLOCKER/MAJOR findings are
   either fixed or answered in `RESULT.md` with the exact line changed.

# Notes

- Paths stay narrow on purpose: this node owns the two new pages, the nav, and the inventory entries.
  Rewriting the *other* pages (and `README.md`) is `MB.21.5.1`, after the parser exists.
- Suggested inventory reasons name the cutover so a reader of a `static` fence learns why it is not
  executed: `"Blastfile examples run only once the DSL parser lands: MB.21.5.2 flips this fence to live execution."`
- If a critic asks for a feature the spec does not have (a second way to name a store, a `version`
  header, an inline secret), answer with the spec, not with new syntax.
