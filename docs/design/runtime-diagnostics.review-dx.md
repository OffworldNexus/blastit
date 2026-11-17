# Review — lens `dx` (human developer experience), node MB.25.1 — round 2

Files reviewed: `docs/local.md` (`## Start` table + one sentence), `docs/reference/log-events.md` (`runtime` table row and the runtime section), `docs/VISION.md` (DX-first standard). This is a re-review: round 1 of this lens was written against the same two pages before the fixes below.

## Round 1 findings and their resolution

1. **MAJOR — `php.ini none` unexplained at the point a PHP developer meets it; dangling "that".** **Resolved.** `docs/local.md:26` now reads `` `php.ini` is the file PHP actually loads; `none` means built-in defaults.`` — the meaning sits one line under the row that raises the question, and the dangling back-reference is gone. The `PHPRC`/`PHP_INI_SCAN_DIR` rule now lives only at `docs/reference/log-events.md:101-102`, and `:102-103` pins the `php.ini none` spelling.
2. **MAJOR — "once per boot" contradicted the reload contract.** **Resolved.** `docs/reference/log-events.md:89-92`: "one line each time the entrypoint resolves: at boot in `blastit dev` and in server mode, and again after a `Blastfile` change that alters it, so the latest line names what is serving." The `When` cell at `:37` says the same ("at boot and after a change that alters it"), so it now agrees with `docs/local.md:33` ("re-reads what changed, then reloads the whole topology").
3. **MINOR — vague lead-in "Boot names what it will run:".** **Resolved.** `docs/local.md:18`: "Startup prints one `runtime` line naming what it will run:" — it says "line", names the event, and does not claim to be first.
4. **MINOR — the `static` row's "neither" was an unresolvable back-reference.** **Resolved.** `docs/reference/log-events.md:98`: "`entrypoint` only: no language runtime is loaded", made airtight by `:100` ("A field a row does not list is absent from the object, never `null`").
5. **MINOR — "what you must act on" mis-described the warns.** **Resolved.** `docs/reference/log-events.md:105`: "`warn` means something needs attention: …" no longer demands action, so `dev.reload.unavailable` and an unused `trait` no longer read as a broken deployment. `unrestricted` joined the list and the postgres item was rephrased.

## Round 2 findings

No `BLOCKER` and no `MAJOR`. Two `MINOR` items remain.

### MINOR — `docs/reference/log-events.md:102-103`: the pretty contract pins two of the three `runtime` lines and leaves the `static` line unpinned

Quoted text:

> `` `pretty` labels the fields `libphp`, `php.ini` and `environment`, and prints `php.ini none` when `php_ini` is `null`. ``

against `docs/local.md:24`:

> ``| no `entrypoint` | `00:11:36.867 INFO  runtime     static  serving static assets only` |``

Why it hurts: `serving static assets only` appears nowhere else under `docs/` (verified by grep). This is the one sentence where the reference promises to describe compacted pretty output, and the one line it omits is exactly the line a static/SPA developer is shown first. MB.25.2 implementing from the reference can legitimately print `static` alone, and the developer then sees a first line that disagrees with the page that taught them to expect it — a doc-vs-tool trap the frozen field list does not force.

Replacement (extend the sentence, do not add a block):

```text
`pretty` labels the fields `libphp`, `php.ini` and `environment`, prints
`php.ini none` when `php_ini` is `null`, and prints `static  serving static
assets only` for a static entrypoint.
```

(Or drop the trailer from `docs/local.md:24` and show `static` alone. Either fixes it; leaving it pinned in neither place invites drift.)

### MINOR — `docs/reference/log-events.md:106-107`: "a `postgres` store no view or trait grants" is a garden path, and not parallel with the item after it

Quoted text:

> `` `egress any`, a `postgres` store no view or trait grants, an unacknowledged `raw` body, an unused `trait`, an `entrypoint php` that no view reaches) ``

Why it hurts: `grants` can be read as a verb ("a store that no view grants" — grants what?) before the reader backs up and reads it as a noun with an elided "that has". Two items later the same sentence spells the relative clause out ("that no view reaches"), so one paragraph teaches two different compressions. A developer scanning the warn list to find "which warning is about my store I never granted anything on" has to re-read. Small, but this paragraph's whole job is making warns legible at a glance.

Replacement: ``a `postgres` store no view reaches`` (matches `docs/reference/blastfile.md:289`, "every view that reaches the store"), or keep both nouns: ``a `postgres` store that no view or trait reaches``.

## What I verified (no finding)

- **Pretty format, byte-exact.** Extracted the three code spans from `docs/local.md:22-24`: timestamp 12 chars (`00:11:36.867`), one space, level 4 chars, exactly 2 spaces, event field exactly 12 columns (`runtime` + 5 spaces), then the summary. All three reproduce the column layout of the existing `docs/local.md:117` `WARN  sql.denied  ` example. Pretty-line lengths 106 / 78 / 65 — all under 160; table rows 150 / 116 / 89, all under the 160-character rule and shorter than the page's existing longest row (162 at `:39`).
- **Growth is earned.** `docs/local.md` 160 → 169 is exactly 9 added lines: 7 for the table (lead-in, blank, header, separator, three rows) and 2 for the blank plus the `php.ini` note. This table is the first-run answer to "did Blastit pick up my app?", its left column is the remedy (a developer seeing `static` when they expected `php` fixes `entrypoint` from the same row), and the note is the only place `php.ini none` is defused. Nothing else on the page moved.
- **One read tells you what runs.** `php 8.5.4  libphp …  php.ini …` names interpreter, version, library and ini; `python 3.14.0  environment /home/you/shop/.venv` names interpreter, version and the exact site-packages root; `static  serving static assets only` tells an SPA author nothing is loaded. The Blastfile cell beside each line closes the loop.
- **Info vs warn is legible in one read.** `docs/reference/log-events.md:89` opens with "`runtime` is `info` (`component: "init"`)" and `:91-92` adds "A resolved runtime is not a problem", which is the exact contrast the `warn` paragraph at `:105` needs; `docs/local.md` shows `INFO` in all three table rows and a `WARN` example at `:117`.
- **It sits naturally beside the existing prose.** The new section follows "The three boot lines" and precedes `## What is never logged`, reuses that heading style, wraps at ~80 columns, and adds no new page, nav entry or fenced block — for this node `git status` shows only `docs/local.md` and `docs/reference/log-events.md` changed under `docs/`.
- **Zero new developer work.** No command, flag, env var or `sudo` is introduced; the line is free output. Adding `unrestricted` to the warn list costs the reader nothing because it was already condemned at `docs/local.md:142-143`.
- **Banned vocabulary absent from the new text.** No `shim`, no retired manifest filename, no `inotify`/`fanotify`, no second PHP transport, no kernel internals in either added block (the only `seccomp` hit, `docs/reference/log-events.md:69`, is untouched pre-existing text). `component: "init"` matches the frozen component, and `entrypoint` values are only `php` / `python` / `static`.

VERDICT: PASS
