---
id: MB.35.2
title: "Document blastit-lsp for Linux, macOS and Windows: install, run, what the editor gets, and what stays Linux-only"
kind: task
status: done
owner: "doc-writer"
priority: 1
depends_on: [MB.35.1, MB.32.1]
paths:
  - docs/reference/cli.md
  - docs/local.md
  - "docs/getting-started.md"
  - editors/zed/README.md
  - docs/troubleshooting.md
  - python/tests/test_docs_assets.py
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T07:06"
---

# Goal

Write the user-facing contract for the portable language server **before** the crate exists, and
have it reviewed through the `dx` and `ai-ergonomics` goldfish lenses. One new fact per page,
no new navigation, and no promise the research verdict cannot keep.

1. **`docs/reference/cli.md`** — a new `## blastit-lsp` section immediately after
   `## blastit lsp`, stated in the same voice:
   - what it is: the same server as `blastit lsp`, packaged as its own binary for Linux, macOS
     and Windows;
   - how to run it: `<path>/blastit-lsp`, no arguments, stdin/stdout, extensionless `Blastfile`;
   - what it **never** does: read `./Blastfile`, read a `BLASTIT_*` variable, open a socket,
     serve an application, or hold a privilege — `blastit` itself remains the Linux-only server;
   - exit codes `0`/`1` identical to `blastit lsp`;
   - one line on where to get it: the GitHub Release asset for the platform, or `blastit lsp`
     on Linux;
   - one line for `binary.path`: point it at `blastit-lsp`, or at `blastit` with
     `"arguments": ["lsp"]`.
   Keep `## blastit lsp` and say in one sentence that it delegates to the same implementation.
2. **`docs/local.md` `## Editors`** — the install step for a Mac/Windows developer:
   `blastit-lsp` on `PATH` (release archive), the Zed extension finds it, diagnostics/format
   work; Linux readers keep the one-sentence `blastit` path. State plainly that macOS/Windows
   give the **editor** experience only — serving a build is Linux-only — and where that boundary
   is explained (`docs/deploy/`).
3. **`docs/getting-started.md`** — one sentence in `## 1. Install`: the editor's language server
   is a separate download for macOS/Windows; serving an app is Linux. No new fence.
4. **`editors/zed/README.md`** — the extension's own two lines: `LspSettings` override, then
   `blastit-lsp` on `PATH`, then `blastit lsp`, then the release download; link
   `docs/reference/cli.md#blastit-lsp`. Do not document the asset URLs twice — one source of
   truth, the CLI page.
5. **`docs/troubleshooting.md`** — one row: *"the extension cannot find the server"* → check
   `which blastit-lsp` on the `PATH` Zed captured, or set `lsp.blastit-lsp.binary.path`; on
   macOS/Windows a login-shell `PATH` is not a GUI app's `PATH`, so a shell-installed binary may
   need the absolute path.

The non-Linux commands are **`text` blocks without a `$` prompt** (the docs harness executes
`console` fences on Linux, and a `$ ` prompt in a non-`console` block is a scanner error). Every
`text` block you add is pinned as plain in `python/tests/test_docs_assets.py` — see below.

Then a fresh-context `critic` (`dx` + `ai-ergonomics`) reviews the diff; record `VERDICT: PASS`,
the lens, and each finding's resolution in `RESULT.md`.

# Context

- **The research verdict bounds the promise.** `MB.35.1` says which targets build. If it says
  `GO WITH DROPPED TARGETS`, the page names the dropped platform and what it loses; it never
  implies a macOS/Windows download that does not exist.
- **`docs-test` executes every `console` fence.** `python/tests/runnable_docs/fences.py`
  refuses a `$ ` prompt outside `console` and executes `console` commands in a clean directory
  on Linux, so a Windows `powershell` line in a `console` fence is a red gate. Use `text`.
- **`MB.32.1` pins the docs' block inventory.** `python/tests/test_docs_assets.py` enumerates
  every public `text` block and asserts which are highlighted vs plain. This leaf lists that
  test file and updates the pinned inventory for each new block (they are plain: a `curl` or
  `Expand-Archive` line starts with no `Blastfile` keyword), with a comment naming the page and
  line. A block that classifies as a `Blastfile` document is a red test — fix the block, not the
  classifier.
- **Keep `## blastit lsp` true.** It says `uv run blastit lsp` when the CLI is not installed;
  `MB.33` repairs that page's `uv` instructions separately. Do not duplicate that work here:
  state the delegation, not the extension's resolution order.
- **Do not touch** `docs/design/**`, `docs/decisions/**`, `ARCHITECTURE.md`, `editors/zed/src/**`,
  `.github/**`, or any `crates/**` file.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ make doc-tone docs-check docs-test
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
$ git diff --stat -- docs editors/zed/README.md python/tests/test_docs_assets.py
```

plus, in `RESULT.md`: the critic report (lens, verdict, findings), the one-paragraph `## blastit-lsp`
text, and the exact pinned inventory diff (`33 -> N` with each new block's page:line and why it
is plain).

The leaf is done when a Mac or Windows reader can answer from these pages: *what do I download,
where do I put it, what will work, and what will not* — and a Linux reader is not told anything
changed for serving.

# Notes

- **Lean.** Five pages touched, one new section, one sentence each elsewhere. No architecture
  story, no Rust crate names, no `pg_query`.
- The Zed README and `docs/reference/cli.md` must agree on the **order**; repeat the order once,
  not the asset names.
- If the research says macOS x86_64 is dropped, the page says `aarch64` macOS only — an honest
  sentence beats a broken download.
