---
id: MB.34.7
title: "Prove MB.34 end to end: CLI/JSON/LSP agree on line:col, the Quick Fix round trip is clean, and the documented commands run verbatim"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.34.5, MB.34.6]
paths: ["docs/testing/E2E-MB.34.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-17T13:17"
---

# Goal

Prove the epic from the outside: every command the pages now document runs verbatim, the
squiggles land where the pages say, the Quick Fix produces a clean file, and the gates are
green. This leaf writes no production code; it writes the evidence and fixes only what the
evidence exposes in the leaves' own paths.

1. **Runnable docs.** `make docs-check`, `make doc-tone` and `docs-test` are green, and every
   command `MB.34.1` added to `docs/local.md` / `docs/reference/cli.md` / `docs/ci.md` executes
   (the `console` fences are executed by `docs-test`; the prose's inline commands are checked by
   hand and pasted).
2. **CLI parity.** One fixture `Blastfile` with four defects — a `SELCT` typo, a multi-line
   missing `FROM`, an inline literal, a redacted `…` — through `blastit check`, `blastit check
   --json` and the LSP's `didOpen` publish. All three must agree on line **and** column, and the
   JSON finding set must be unchanged in shape (`fix.action` set unchanged).
3. **Quick Fix round trip.** In Zed (recorded, not gated): apply the `"Parameterize SQL for
   Blastfile ($?)"` action on the inline-literal fixture, save, run `blastit check`, expect
   exit `0`; the hover over `$?` and `...` shows the two paragraphs.
4. **Highlighting.** Zed shows SQL keywords, `$?`/`$1…$n` and `...` coloured inside the string
   (screenshot or a copied `tree-sitter parse` tree plus the theme capture list), and a
   lowercase statement is coloured too.
5. **Epic gate inputs.** Run `make check` and `make e2e` as the epic requires, and file the two
   tails.

# Context

- **This is the `MB.34` gate's evidence leaf, not a second implementation.** If a step fails,
  fix it in the owning leaf's paths and note the follow-up here; do not patch `crates/**` or
  `editors/**` from this leaf without listing the path and saying why.
- **The paths of record.** `docs/testing/` is excluded from the public site, so the transcript
  lives in `docs/testing/E2E-MB.34.md` without adding a page or a fence to the reader's site.
- **Build collisions.** Do not start `make check` while another node is compiling (the registry
  lock in `editors/zed` and the Rust workspace is shared); wait for the tree to be free and
  record the command's wall time.
- **No new dependencies, no new tooling.** The LSP transcript uses the existing
  `critical_lsp.rs` harness shape or a small stdlib Python framing script under `/tmp/opencode`;
  nothing new enters `scripts/` or `Cargo.toml`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make doc-tone docs-check docs-test
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ cargo test -p blastit-sqlpolicy -p blastit-manifest
$ cargo test -p blastit --test critical_lsp
$ make zed-check
$ make check
$ make e2e
```

`docs/testing/E2E-MB.34.md` contains, verbatim: the fixture `Blastfile`, the three commands'
outputs (CLI text, `--json` excerpt, LSP publish payload), the four `line:col` values, the Quick
Fix before/after (`blastit check` exit `0`), the Zed hover text, and the `tree-sitter parse`
subtree. It ends with `VERDICT: PASS` or the failing step and its owner leaf.

`make e2e` may be the Docker suites; if the environment cannot run them, say so with the exact
error and mark the item `NOT RUN` — never claim a pass this host did not produce.

# Notes

- **Two facts the result must state explicitly:** (a) the squiggle is not at `0:0` for any of
  the four defects, and (b) no `query` sentence gained a `pg_query` internals lecture — the
  pages stay lean.
- A failure that belongs to `MB.34.3`/`MB.34.4` returns the epic to `ready`, not to a fifth
  attempt here: log it in the owning node and let `lead` re-dispatch.
- Keep the file under ~150 lines; this is evidence, not a tutorial.
