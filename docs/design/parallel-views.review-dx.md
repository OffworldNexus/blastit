# DX review — MB.24.1 parallel views

Scope: the uncommitted `docs/how-it-works.md` and `docs/frameworks/php/index.md` changes for MB.24.1, human-DX lens, whole pages read against `docs/reference/settings.md#admission-and-threads`/`#view-process-lifecycle-and-memory`, `docs/security.md:130-164`, `docs/deploy/anything-else.md:12-27`, `crates/blastit-view/src/php.rs:1-34` and `crates/blastit/src/php_view.rs:72`; `make docs-check` prints `No issues found`, and both links land on `id="admission-and-threads"` in the built site.

## Findings

### F1 · MINOR (follow-up, `docs/reference/settings.md:68` — outside this node's paths)

> `BLASTIT_MAX_WORKERS` … "live request **threads** across every view"

Both pages now link a process-based PHP explanation to the one row a PHP developer will open to check the definition, and the row (plus `docs/deploy/anything-else.md:14`, "one hot view takes the whole budget") speaks only of threads and of a view holding the budget. The knob and default are right, so this is not a defect in the two pages and not this node's file.

**Exact fix (separate node):** "live request slots across every view (a Python thread, or a PHP worker process)". Do not edit `settings.md`/`deploy/` from MB.24.1.

### F2 · MINOR (follow-up, `docs/frameworks/php/index.md:57` — outside the concurrency paragraph this node may touch)

> "We support supported PHP versions: 8.3, 8.4 and 8.5."

A duplicated word in the section above the one this node owns; it predates the diff and is unrelated to concurrency. The node's "touch only the concurrency paragraph" rule keeps it out of this change.

**Exact fix (when the page is next opened):** "We support PHP 8.3, 8.4 and 8.5."

## Verdict

Every round-1 finding is resolved, and nothing new broke:

- **F1 (was MAJOR)** — `docs/how-it-works.md:46` now says "as many requests at once as the shared `BLASTIT_MAX_WORKERS` budget allows": an upper bound, no count, one link. The memory-derived live-process cap that can bind lower is still stated on the same page (`:137`, "a cap on live view processes, derived from memory at boot") and both knobs are named at the link target, so the page no longer promises a number. Naming the knob is the right call for a developer grepping "concurrency"; dropping it for "the worker budget" would only move the name into the link.
- **F2 (was MAJOR)** — `## Concurrency` replaces `## One request at a time`; `grep -rn "one-request-at-a-time" docs/ README.md` is empty, so no inbound anchor breaks, and the H2 text is the only structural touch (all framework sections and `## Where next` intact).
- **F3-F6** — the mermaid label and table row both read "one or more per route" (no `1..N`); the spawn condition is "no idle view process for this route"; "PHP runs each concurrent request in its own view process" names no forking actor; the uid/ACL note is now its own paragraph, so the rule is a clean three-sentence block.
- **Constraints** — `how-it-works.md` gains exactly two sentences; the PHP page gains one four-line paragraph. No pool/LRU/`idle_timeout` internals, no kernel lecture, no promised worker count, no second transport or degraded mode, no retired manifest name (`Blastfile` only). The replacement ending, "there is no daemon or socket between Blastit and PHP", carries the single-transport claim with the intro's "no PHP server beside it" and does not use the forbidden vocabulary (`python/tests/test_docs_assets.py`: 35 passed); the PHP paragraph still reads leanly to a developer who greps "concurrency" — parallel, the bounding knob, the knob that is *not* it, and nothing to tune.
- **Verification** — `make docs-check` (Zensical `--strict`): `No issues found`; `site/how-it-works/index.html` → `../reference/settings/#admission-and-threads` and `site/frameworks/php/index.html` → `../../reference/settings/#admission-and-threads`, both resolving to `id="admission-and-threads"`; the rendered pages carry the new label, spawn condition and `id="concurrency"`.

No BLOCKER and no MAJOR remain; the two MINORs are out-of-scope follow-ups.

VERDICT: PASS
