# AI-ergonomics review — MB.24.1 parallel views

Scope: the current diff to `docs/how-it-works.md` and `docs/frameworks/php/index.md` (MB.24.1), read as an AI coding agent asked "why is one PHP route slow, and what do I change?", cross-read against `docs/reference/settings.md` (§Admission and threads, §View process lifecycle and memory), `docs/frameworks/index.md`, `README.md`, `docs/deploy/anything-else.md:8-30`, and `docs/VISION.md`.

## Findings

### BLOCKER

None.

### MAJOR

None.

Round 1's three MAJORs are resolved in the pages: the PHP heading is `## Concurrency`; the budget is "the **shared** `BLASTIT_MAX_WORKERS` budget" in both pages; and the one sentence that could send an agent to the wrong knob now exists ("`BLASTIT_VIEW_MAX_WORKERS` caps one process, not the route"). The remaining `threads` wording lives in `settings.md:68` and `docs/frameworks/index.md:28`, which are follow-ups under this node's scope rule, not page defects — the PHP sentence defends against the row it cites.

### MINOR

**AE-6 (residual) — the request diagram still shows one process that threads, with no scope note** · `docs/how-it-works.md:57,70`

```markdown
participant V as view
...
V->>V: new thread, WSGI environ, app(environ, start_response)
```

The spawn condition is now correct (`no idle view process for this route`), but the only request diagram is the Python path. An agent that parses diagrams before prose concludes a route threads, so `MAX_WORKERS` reads as a cheap thread budget; for PHP the same 16 is 16 processes and 16 upstream DB connections. The paragraph above contradicts it correctly, so this is not blocking. Fix: `participant V as view (Python shown)` — three words, no new prose.

---

**AE-7 (residual) — "why is this route slow on the first requests?" has no answer on either page** · `docs/frameworks/php/index.md:59-64`

Both pages explain how many requests run at once, never that a lazily forked worker makes the first requests pay a fork. `docs/deploy/anything-else.md:27` and `settings.md:114` are the only public places that name the lever, and neither is linked from the concurrency paragraph. Fix, if it fits the one-paragraph cap: close the paragraph with one short clause, *"a route's first requests wait for its fork; mark hot routes `prefork`"*, linking `../../reference/blastfile.md#view-directives`. If that breaks the cap, leave it — an agent following the paragraph's `[settings]` link can still find `BLASTIT_VIEW_SPAWN` one section down.

---

**AE-8 — "inside each view worker" is the last PHP-FPM-shaped noun on the page** · `docs/frameworks/php/index.md:3`

```markdown
Blastit runs `public/index.php` inside each view worker: no PHP server beside it, …
```

The concurrency section says "separate view processes"; the intro says "worker". An agent building an image greps "worker", sees the FPM shape it already knows, and adds a `php-fpm` service or `pm.max_children` — the concurrency paragraph's "no daemon or socket between Blastit and PHP" denies the transport but not the noun. Fix: *"inside the view process Blastit forks"* (keeps the guard-banned tokens out).

---

**AE-9 — the anti-wrong-knob sentence is too compressed to parse** · `docs/frameworks/php/index.md:63-64`

```markdown
`BLASTIT_VIEW_MAX_WORKERS` caps one
process, not the route; there is no daemon or socket between Blastit and PHP.
```

"Caps one process" does not say *of what*: a thread cap, a process count, or an rlimit. This is the sentence an agent must trust to stop reaching for a per-route lever, and a vague qualifier is easy to skim past. Fix: *"`BLASTIT_VIEW_MAX_WORKERS` sizes one view process; it adds no concurrent requests to a route."* (the rlimit it feeds stays a `settings.md` concern).

---

**AE-10 — "bounded by the budget" is the only ceiling stated, but the memory-derived process cap can bind first** · `docs/frameworks/php/index.md:61-62`

`docs/how-it-works.md:137` already says *"a cap on live view processes, derived from memory at boot"*, and `settings.md:116` gives it a row. In a multi-route app that cap can stop a PHP route short of `BLASTIT_MAX_WORKERS`, so an agent raising the budget sees no change and raises it further. Fix (one clause, inside the existing paragraph): *"…or the memory-derived live-process cap, whichever binds first ([settings](../../reference/settings.md#view-process-lifecycle-and-memory))"*. Not blocking: the budget is described as a bound, not a promise of a count, and nothing on the page invites a per-route key.

## Verdict

Answering the lens test: yes — from these two pages an agent learns that one PHP route runs one request per view process, in parallel up to a **shared, process-wide** budget; that `BLASTIT_VIEW_MAX_WORKERS` is not the route lever; and that there is no FastCGI daemon or socket to size. The correct next step (raise `BLASTIT_MAX_WORKERS`, minding the DB connection limit — `settings.md:68`) is one click away at the cited anchor, and no text on either page hints at a route-level `pool`/`workers` key, a second PHP transport, a degraded mode, or a retired manifest format, so nothing tempts an agent toward `unrestricted = true`.

Verified: `make docs-check` → `No issues found` (`--strict`, 0 warnings) with both `reference/settings.md#admission-and-threads` and `../../reference/settings.md#admission-and-threads` resolving; `python/tests/test_docs_assets.py` → 35 passed (no retired PHP vocabulary on the page); `grep -rn "one per route" docs/ README.md --exclude='*.review-*.md'` → only `docs/design/learn-single-pass.md:156,1236` (raw-ingress wording, historical record); `grep -in "fastcgi\|fpm"` over the reviewed public pages → 0 hits. Prose caps hold: two sentences added to `how-it-works.md`, one paragraph to the PHP page.

Follow-ups (outside MB.24.1's paths, not graded): `settings.md:68` still calls the budget "request threads" and `frameworks/index.md:28` links it as "threads"; `settings.md:115`, `docs/deploy/anything-else.md:27` and `docs/troubleshooting.md:92` write `prefork = true` although the DSL is the bare `prefork` directive (`docs/reference/blastfile.md:219`); `anything-else.md:14` ("one route should not hold every thread") is Python-shaped advice for a PHP route.

VERDICT: PASS
