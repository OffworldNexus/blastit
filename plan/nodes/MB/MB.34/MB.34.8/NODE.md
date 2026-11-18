---
id: MB.34.8
title: "State every query refusal and quote the two hover paragraphs on the pages (design MB.34.2 supersedes the page's silence)"
kind: task
status: done
owner: "doc-writer"
priority: 2
depends_on: [MB.34.2]
paths: [docs/reference/blastfile.md, docs/local.md, docs/troubleshooting.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-18T01:26"
updated: "2026-11-18T05:02"
---

# Goal

Make the four user pages state every refusal `blastit check` can now print, and quote the two
hover paragraphs verbatim, so the page and `hover.rs` cannot drift.

`MB.34.2` (accepted design, authoritative) pinned seven refusal kinds and the hover text; the
pages `MB.34.1` wrote came before the design and are silent about three of them.

1. **`docs/reference/blastfile.md`** — two rows in the `query` refusal table:
   - `query 'SELCT id FROM t'` at the statement's first token, with
     ``the statement does not parse: syntax error at or near "SELCT"``;
   - the `TooLong` row (65 536 characters), in the same voice as the file's existing size cap.
2. **`docs/reference/blastfile.md`** — the two hover paragraphs, quoted verbatim from
   `docs/design/blastfile-lsp-zed.md` §7.6 (`$?` and `...`), so the page and the server's
   `hover.rs` text are one document.
3. **`docs/troubleshooting.md`** — the `query` row gains the same parse sentence for a reader who
   has no editor.
4. **`docs/local.md` `## Editors`** — one clause only: the squiggle sits at the offending token.

No new fenced code block on any page (the inventory is 34/16/18/0; a `text` block may not be
added by this node). `docs/design/**` and `docs/decisions/**` are the design's, not this node's.

Then a fresh-context `critic` reviews the diff through the `dx` lens only (the text is already
accepted prose; this is a consistency fix), and its findings are resolved here.

# Context

- Source of truth: `docs/design/blastfile-dsl.md` §13.4 (the refusal table with offsets) and
  §18.1 (the sentences), `docs/design/blastfile-lsp-zed.md` §7.6 (the two hover paragraphs).
  Quote, do not paraphrase: a second wording is the drift this node exists to prevent.
- `MB.34.1` fixed the same pages and its sentences are accepted; do not reopen them beyond the
  two rows, the one clause and the hover paragraphs.
- `make doc-tone` refuses apology language; `make docs-check` is `--strict`.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make doc-tone docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ git diff --stat -- docs
```

and in `RESULT.md`: the critic report (lens, verdict, findings), the four fence counts before and
after, and the diff of the two rows plus the two hover paragraphs.

# Notes

- The hover paragraphs are the server's contract; if the design's §7.6 wording and the page's
  differ, the design wins and the page is corrected.
- This node changes no Rust and no fence; it is done before `MB.34.5` implements the hover.
