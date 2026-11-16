---
id: MB.22.1
title: "Write the editor setup and blastit lsp DX docs (Zed extension, diagnostics, format-on-save) with dx + ai-ergonomics critics"
kind: task
status: ready
owner: "doc-writer"
priority: 1
depends_on: [MB.21]
paths:
  - docs/local.md
  - docs/editors.md
  - docs/reference/cli.md
  - zensical.toml
  - python/tests/runnable_docs/inventory.toml
  - "docs/design/blastfile-editor.review-dx.md"
  - "docs/design/blastfile-editor.review-ai-ergonomics.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T09:16"
updated: "2026-11-16T09:16"
---

# Goal

Write the **user-facing editor documentation first**, before any LSP or extension design
exists. A developer installs the Blastfile extension in Zed and, with zero project config,
gets colours, live errors in the same words `blastit check` prints, and format-on-save after
one documented JSON line. Any other editor can point its LSP client at `blastit lsp` on
stdin/stdout in three lines.

Preferred shape (lean, research recommendation 7): add **≤ 20 lines** to `docs/local.md`
under an `## Editors` heading. A separate `docs/editors.md` is allowed only if the page stays
≤ 120 lines and is registered in `zensical.toml` nav **and**
`python/tests/runnable_docs/inventory.toml`. In both shapes, add the `blastit lsp` entry to
`docs/reference/cli.md`.

The docs must show, exactly:

- **Local machine, zero `sudo`:** (1) Zed ≥ 0.205, `zed: extensions` → install Blastfile;
  contributors use `zed: install dev extension` and pick `editors/zed`. (2) Open `./Blastfile`
  — colours, bracket matching, outline, squiggles; the extension starts `blastit lsp` itself
  (from `PATH`, or `uv run blastit lsp` when the CLI is not installed). (3) The one line that
  turns on formatting for this language:

  ```json
  { "languages": { "Blastfile": { "format_on_save": "on" } } }
  ```

  Say that the Blastit repo already commits `.zed/settings.json`, so contributors get it for
  free, and that Zed's default for a language it does not special-case is `off`.
- **Other editors:** `blastit lsp` speaks LSP on stdin/stdout; point the client at it. No
  `npm`, no Tree-sitter CLI, no `cargo`, no `wasi-sdk` on the user's machine — those are
  extension-build requirements, not user requirements.
- **CI/CD:** the editor shows the same `Blastfile:<line>:<col>` sentence that `blastit check`
  fails on, so there is no separate editor step in CI. One workstation paragraph, no new
  pipeline.
- **Deployments:** nothing runs `blastit lsp` in production; one sentence, no new preset.

Then pass fresh-context `dx` and `ai-ergonomics` goldfish critics. They write
`docs/design/blastfile-editor.review-dx.md` and
`docs/design/blastfile-editor.review-ai-ergonomics.md`, each ending `VERDICT: PASS`.

# Context

- Source of truth: `docs/research/2026-11-16-zed-blastfile-lsp.md`, sections
  "Developer Experience Implications" and "Concrete Recommendations" (7). The reported traps
  are verified there; do not invent a second installation path.
- `docs/local.md` is the page a developer reads before writing a route. The editor is part of
  that first hour; a separate page is acceptable only if it stays lean.
- `docs/reference/cli.md` is the command table; `blastit lsp` belongs there beside `dev`,
  `check`, `fmt` and the prod binary.
- **Language rules:** never mention the retired manifest filename; never describe a PHP
  alternative transport in any sentence; keep the page free of grammar/JSON-RPC internals.
- **Fences:** a `console` fence is executed by the runnable-docs harness, so every new one
  needs an `inventory.toml` entry with a mode and a reason. `blastit lsp` blocks on stdin and
  would hang the harness: show it as prose or a `json`/`text` snippet, never as `$ blastit lsp`
  in a `console` fence, unless the inventory records a non-executing mode and reason.
- Do not edit `docs/blastfile.md` or `docs/reference/blastfile.md`: `MB.21` owns their content.
  Do not edit `docs/local.md` sections outside the editor addition.

# Acceptance

```console
$ grep -rn "install dev extension\|format_on_save\|blastit lsp" docs/local.md docs/editors.md docs/reference/cli.md 2>/dev/null | head
$ ls docs/design/blastfile-editor.review-dx.md docs/design/blastfile-editor.review-ai-ergonomics.md
$ grep -n "VERDICT" docs/design/blastfile-editor.review-dx.md docs/design/blastfile-editor.review-ai-ergonomics.md
$ make docs-check
$ make docs-unit
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ uv run --no-project tools/plan.py validate
```

1. A developer with Zed installed goes from reading to colours + diagnostics + format-on-save
   in ≤ 3 steps, with zero `sudo` and no project config beyond the one JSON line.
2. `docs/reference/cli.md` names `blastit lsp`, says an editor spawns it (not the user), what
   it reads (stdin/stdout, no config, no network, no privileges), where logs go (stderr), and
   its exit behaviour (0 after `shutdown`, 1 otherwise).
3. Both critic files end `VERDICT: PASS`; every BLOCKER/MAJOR is either fixed in the page or
   answered in `RESULT.md` with the section that changed.
4. The new prose mentions neither the retired manifest filename nor a PHP alternative
   transport; it teaches no kernel or protocol internals.
5. If a new page was created, `zensical.toml` nav and `inventory.toml` carry it and
   `make docs-check`/`make docs-unit` are green; if the section shape was chosen, nav is
   unchanged and the only inventory change (if any) is a fence entry.

# Notes

- Keep the section scannable: a 3-step list, one code block, one sentence for other editors.
  No screenshots only; a screenshot may accompany text but never replace a command.
- `format_on_save` is a Zed setting the extension cannot set for the user. Do not promise it
  works out of the box; promise the one line and the committed repo file.
- The extension version compatibility sentence is "Zed ≥ 0.205" (the `zed_extension_api
  0.7.0` pairing verified in the research). Do not guess a newer number.
- If the critic finds the section is doing too much, cut the "other editors" paragraph to one
  sentence and link to `docs/reference/cli.md#blastit-lsp`.
