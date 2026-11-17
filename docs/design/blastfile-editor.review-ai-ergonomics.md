# Review: editor DX pages — ai-ergonomics lens

- **Post-fix re-review** (this file replaces the pre-fix review of the same lens).
- Date: 2026-11-17. Node: MB.22.1.
- Reviewed: `docs/local.md` lines 140–158 (the whole `## Editors` section, 20 added lines + one JSON block); `docs/reference/cli.md` line 7 (table row) and lines 67–78 (`## blastit lsp`). No other edits judged.
- Research used: `docs/research/2026-11-16-zed-blastfile-lsp.md` — "TL;DR", "Developer Experience Implications", "Concrete Recommendations" 7 and 8, "Unverified / Open Questions" (registry publication, `format_on_save` + `formatter`, diagnostic text, exit rule).
- Cross-checked: `docs/reference/blastfile.md` Errors/Formatting; `crates/blastit/src/cli.rs:553-561` (positioned vs position-less check output); `python/tests/runnable_docs/inventory.toml` (JSON fence adds no `console` entry).
- Test applied: an agent (fresh context, no web) is told *"set up the editor for this repo"* and must act on these words alone, then verify the result.

## Findings

### 1. `MINOR` — the "Not listed yet" branch has no executable step outside a Blastit checkout
`docs/local.md:142-143`:

> 1. Zed ≥ 0.205: `zed: extensions`, install **Blastfile**. Not listed yet, or in a Blastit checkout: `zed: install dev extension` on `editors/zed` (needs `cargo`).

The fork is now decidable, which fixes the previous MAJOR. But at ship time the registry entry is a human-only follow-up (research Recommendation 8; MB.22 lists the push/PR as out of scope), so "not listed yet" is the state a reader in *their* app repo actually hits — and `editors/zed` exists only in a Blastit checkout. The agent must invent the clone (which repo, from where) before it can run the only available install. One clause fixes it:

> Not listed yet? Clone Blastit, then `zed: install dev extension` and pick `editors/zed` (needs `cargo`, which Zed does not fetch).

If 0.2.0 can ship before the listing, this clause is mandatory; otherwise the whole section's step 1 is a dead end.

### 2. `MINOR` — step 3 gives no create/merge rule; the snippet is a whole document
`docs/local.md:147-149`:

> 3. Turn on format-on-save for this project in `.zed/settings.json`: Zed's default is `off` and an extension cannot set it. Blastit's repo commits that file; if a save changes nothing, add `"formatter": "language_server"` beside the key.

The destination is now stated (previous MAJOR resolved), but an agent that finds an existing `.zed/settings.json` (buffer font, theme, another language) is holding `{ "languages": { "Blastfile": { … } } }` and nothing tells it to keep the rest; writing the block verbatim loses project settings, which is the one destructive act in this section. Add the merge rule to the step, not the JSON:

> … in `.zed/settings.json` at your project root — create the file if it is missing, and keep any keys already there.

### 3. `MINOR` — the format-on-save oracle cannot tell "not wired" from "already canonical"
`docs/local.md:148-149`: "if a save changes nothing, add `"formatter": "language_server"` beside the key."

A save changes nothing in three different states: formatting is off; formatting is on and the file was already canonical; the agent saved without editing. So the only check the page offers cannot confirm the line landed, and an unattended agent has to hand-drive a GUI save to learn nothing. Make the test discriminate and keep it in the same sentence:

> Save a deliberately mis-indented line: it snaps to the canonical layout; if it does not, add `"formatter": "language_server"` beside the key.

This subsumes the research's unverified single-key form without making the fixed JSON block longer.

### 4. `MINOR` — cli.md's diagnostic-parity claim silently drops position-less refusals
`docs/reference/cli.md:75-76`:

> An error carries the sentence `blastit check` prints after `Blastfile:<line>:<col>:`; grants, queries and coverage stay with `blastit check`.

True for positioned errors, false for the whole-file class (missing `root`, unset `$IDENT`, refused `body raw`): `blastit check` prints those as `Blastfile: <sentence>` with no line/column (`crates/blastit/src/cli.rs:553-561`, and `docs/reference/blastfile.md` Errors says "a refusal with no position … names the view and the fix instead"), while the LSP range is `(0,0)`. An agent verifying parity on such a case sees a difference and may "fix" the server to emit a prefix. Wording that covers both:

> An error carries the sentence `blastit check` prints, minus the `Blastfile:` position prefix (the range carries it; a refusal with no position is already one sentence).

### 5. `MINOR` — "using it needs no … `cargo`" still reads against step 1's "(needs `cargo`)"
`docs/local.md:155-157`:

> … using it needs no `npm`, Tree-sitter CLI, `cargo` or `wasi-sdk`.

The previous MAJOR's fix landed in step 1, but this sentence is unscoped: an agent asked "do I need cargo?" finds *yes* and *no* fifteen lines apart and may skip the dev-install prerequisite, then blame the page when Zed's cargo build fails. Scope it to the install it describes (and this also shortens the four-tool checklist):

> The registry install needs nothing else: no `npm`, Tree-sitter CLI, `cargo` or `wasi-sdk`.

### 6. `MINOR` — "Formatting is the canonical layout." states a capability without a way to check it
`docs/reference/cli.md:76-77`. `cli.md` is the reference page an agent greps for behaviour; "canonical layout" is defined nowhere here and no `fmt` section exists on the page. One link makes it verifiable and ties the editor promise to the documented formatter:

> It formats on request to the [canonical layout](blastfile.md#formatting); whether that runs on save is your editor's setting.

### 7. `NIT` — "never run it by hand" and "Start it with no arguments" invite the wrong test
`docs/reference/cli.md:69-72`: an agent verifying the server may run `blastit lsp` in a shell, where it blocks on stdin and hangs a CI step. Say what not to do and give the spawn for a config file separately:

> Your editor spawns it; do not run it in a shell. The command is `blastit lsp` with no arguments — or `uv run blastit lsp` when the CLI is not on `PATH` (that needs `uv add blastit`).

### 8. `NIT` — two prose fragments
`docs/local.md:142` opens a numbered step with the fragment "Zed ≥ 0.205:" (reads as a heading, and the research's only verified bound deserves the object it constrains: "In Zed ≥ 0.205, run `zed: extensions` …"). `docs/local.md:144` says "a squiggle **in** the words" — squiggles sit *under* words.

## What works

- The registry/dev fork is now explicit and self-describing, and the dev path names its `cargo` requirement at the point of use.
- The position-prefix trap is gone from both pages ("the words `blastit check` prints, at the same line and column"; the `Blastfile:<line>:<col>:` sentence in cli.md).
- `cli.md#blastit-lsp` carries every machine fact an agent needs: no-argument spawn, `uv run` + `uv add` prerequisite, stdin/stdout, extensionless attachment, no config/network/privileges, stderr logs, and the exit pair with the "never because a document is broken" guard.
- No blocking command is shown as a `console` fence (`python/tests/test_runnable_docs.py`: 21 passed), and the retired-filename / PHP-fallback / internals guards pass.
- Lean and free of JSON-RPC, framing, UTF-16 or grammar detail; the triad is answered in one line (CI uses `blastit check`, production never runs `blastit lsp`).

VERDICT: PASS
