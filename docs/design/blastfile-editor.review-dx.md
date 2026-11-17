# Blastfile editor support — dx review (post-fix re-review, M1 verified)

- Lens: `dx` — Date: 2026-11-17. Replaces the two earlier reviews; only the editor additions judged.
- Reviewed: `docs/local.md` 140–158 (`## Editors`; neighbours 12–34, 74–163);
  `docs/reference/cli.md` 3–9 (command table), 67–78 (`## blastit lsp`).
- Supporting: `docs/reference/blastfile.md` 43, 555–560, 578 (diagnostic/formatting wording);
  `python/tests/runnable_docs/inventory.toml` inventories `console` fences only, so the
  `json` fence is outside the harness.
- Research: `docs/research/2026-11-16-zed-blastfile-lsp.md` — "TL;DR", "Developer Experience
  Implications", "Concrete Recommendations" (5, 7, 8), "Unverified / Open Questions".
- Language law: clean. No retired manifest filename, no PHP transport, no internals on the pages.

## Resolution

### M1 — `RESOLVED` (was `MAJOR`)
`docs/local.md:147-149` now reads:

> Turn on format-on-save for this project in `.zed/settings.json`: Zed's default
> is `off` and an extension cannot set it. Blastit's repo commits that file; save a
> mis-indented line and it snaps back.

The check is now a positive, deterministic test: a mis-indented line is not canonical, so a
working formatter must change it, and a dead one visibly does not. The false-negative trigger
is gone, and the `"formatter": "language_server"` clause is gone, so the section no longer
contradicts the node's fixed one-key block (`local.md:151-153`, unchanged). An unprobed
one-key form is now a detectable product bug, recorded for MB.22.4, not a silent doc hazard.
Nothing else regressed: `cli.md:1-80` is byte-for-byte the text already judged, and the
`## Editors` neighbours are unchanged.

## Remaining findings

### 2. `MINOR` — the diagnostic promise is broader than the server; the scope caveat is only on the reference page
`docs/local.md:144-146`:

> … a squiggle in the words `blastit check` prints, at the same line and column.

"the words `blastit check` prints" covers missing grants, dead grants and coverage; the LSP
squiggles only the manifest's own errors. That scope lives only at `cli.md:75-76`, which
`local.md` readers never open, and "a squiggle in the words … at the same line" is
ungrammatical. State it once here; keep `cli.md` as the precise wording:

> Open `./Blastfile`: colours, bracket matching, an outline, and the sentence
> `blastit check` prints for the file's own errors, squiggled at the same line and
> column. Grants and coverage stay with `blastit check`.

### 3. `MINOR` — `(needs cargo)` contradicts "no `cargo`" three lines later, and the fallback has no clone step
`docs/local.md:142-143` and `155-157`:

> Not listed yet, or in a Blastit checkout: `zed: install dev extension` on `editors/zed`
> (needs `cargo`). … using it needs no `npm`, Tree-sitter CLI, `cargo` or `wasi-sdk`.

A reader who is not in a Blastit checkout has no `editors/zed`, so the fallback dead-ends; and
`(needs cargo)` collides with the later claim unless "using it" means the LSP. Put the
prerequisite with the action, then the toolchain claim once:

> Not listed yet? Clone Blastit and run `zed: install dev extension` on `editors/zed`;
> that from-source build is the only thing here that needs `cargo`.

End with "Nothing else to install to use either editor: the extension ships its grammar and
its server — no Node, Rust or Tree-sitter toolchain."

### 4. `MINOR` — 18 added lines against a ≤15-line target; name the cuts
`docs/local.md:140-158` is three steps, a JSON fence and a four-sentence trailer; research
recommendation 7 budgets ≤15 lines. Cut "Blastit's repo commits that file" (the other
contributor aside already sits in step 1), the four toolchain names, and `(needs cargo)` once
finding 3 folds it into step 1 — ~15 lines, no instruction lost.

### 5. `MINOR` — hover, the highest-value server feature, is documented nowhere
`docs/local.md:144-146` lists colours, brackets, outline, diagnostics; `cli.md:76` mentions
only formatting. Research §7 and rec 5 ship hover in v1 and call it "the highest value per
line of code" — it answers "what does this grant allow" at the cursor. Undocumented, it does
not exist for users. Add to step 2: "hover a keyword, `unrestricted` included, for what it
allows"; the same half-line belongs in `## blastit lsp`.

### 6. `MINOR` — "Formatting is the canonical layout" defines nothing on this page
`docs/reference/cli.md:76`. A fragment, and "canonical layout" is defined only at
`blastfile.md#formatting`; a client author cannot tell what the server returns. Replace with
"Formatting rewrites the whole document in the [canonical layout](blastfile.md#formatting)."
— same length, and no undocumented command is reintroduced.

### 7. `MINOR` — the spawn prerequisite omits the `PATH` the editor actually sees
`docs/reference/cli.md:69-71`:

> … `uv run blastit lsp` when the CLI is not installed (that needs `uv add blastit`) …

"not installed" conflates "not on `PATH`": a desktop-launched client inherits a different
`PATH` than the shell, and its only symptom is "language server not found". Add: "the client
must see `blastit` (or `uv`) on the `PATH` it inherits; `uv add blastit` is what makes the
`uv run` fallback resolve."

### 8. `NIT` — "Start it" contradicts "you never run it by hand"
`cli.md:69-71`: say "Spawn it with no arguments" — the config author writes the line, the
editor executes it.

### 9. `NIT` — the JSON block does not say it merges
`docs/local.md:151-153`: it is pasted into a file the project may already own, and a
whole-file replacement drops sibling settings. "Add this object to `.zed/settings.json`."

### 10. `NIT` — "save a mis-indented line" reads as saving the line itself
`docs/local.md:148-149`. "mis-indent a line and save" (or "save the file after mis-indenting a
line") names the action and the file unambiguously.

## What works

- Three steps, zero `sudo`, one mandated config line; the Zed floor and both action names
  match the verified extension-API binding.
- Step 3 is now provable on the spot: mis-indent, save, watch it snap back.
- Server resolution (`PATH`, else `uv run`) is stated once and correctly.
- `cli.md` answers spawn, no arguments, stdio, no config, no network, no privileges, stderr,
  `0`-after-`shutdown`, `1`-otherwise — all correct; grants and coverage stay with `check`.
- Other-editor, CI and production sentences are present, one line each, non-contradictory.

VERDICT: PASS
