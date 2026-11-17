# AI-ergonomics review — one `immutable` pattern grammar

Verdict: accept

## Findings

### MAJOR 1 — the only client-path-relative directive never says so

`Where:` `docs/reference/blastfile.md:399-400`, `docs/reference/blastfile.md:405`, `docs/frameworks/python/django.md:49-52`

`What:` `root`, `index` and `fallback` are root-relative; `immutable` is the one `static`
directive matched against the **client's** path with the mount prefix re-joined
(`crates/blastit-scheduler/src/static_asset.rs:535-553` `url_path()`, design
`docs/design/blastfile-dsl.md` §9.4 "matches the path the client asked for"). The reference says only "the URL path of the file it answers",
which an agent can read as the file's path below `root`, and proves the prefix rule only by
the Django example. Failure: an agent writes
`static "/static" { root "staticfiles" immutable "/admin/*" }` and every asset silently gets
`no-cache` — `static_response.rs:183-194` requires `AnswerKind::Exact` **and**
`matches(url_path)`, nothing refuses an unmatched pattern, and `blastit check` audits only
`root` (`docs/frameworks/static-and-spa.md:75`). Trial and error is the only signal.

`Fix:` after the intro sentence add: "Unlike `root`, `index` and `fallback`, a pattern matches
the client's URL path, mount prefix included: under `static "/static"` write
`immutable "/static/*"`."

### MAJOR 2 — bare `immutable` has no stated precondition, so the shortest choice is the unsafe one

`Where:` `docs/reference/blastfile.md:404` (table Bare row), `docs/frameworks/static-and-spa.md:32`

`What:` the row says "every exact file the mount serves" but never when that is wrong. The
page itself supplies the counter-example: `collectstatic` keeps unhashed originals beside the
hashed copies (`docs/frameworks/python/django.md:64-66`), and a Vite `dist` carries the unhashed
`publicDir` files at its root. An agent minimizing config writes bare `immutable` for either
mount and pins `admin/css/base.css` or `robots.txt` for a year. The only cue is the third row's
"when only hashed names may be cached", which reads as a `regex` hint, not a warning about the
bare form. NODE's goal ("must be able to tell which files earn the year-long cache") is met for
*what*, not for *choose*.

`Fix:` extend the bare row: "every exact file the mount serves — only when each name changes
with its bytes; a tree with unhashed originals uses a pattern or `regex`".

### MAJOR 3 — the 32-pattern cap lives only in the ADR and design record

`Where:` `docs/reference/blastfile.md:408-410`, `docs/reference/blastfile.md:601`

`What:` `MAX_PATTERNS = 32` per mount survives `MB.31` (`plan/nodes/MB/MB.31/NODE.md:53-56`,
`ADR-0012` amendment, `crates/blastit-manifest/src/validate.rs:64`), but the reference never
states it, and this change removed `immutable` from the `list items` row — so the only limit a
user page now names is 4096 bytes. A generated mount's 33rd pattern is refused by the rule
"at most 32 patterns" (`validate.rs:27`) that appears on no user page. The agent's cap question
("how many lines may I generate?") forces a read of `ADR-0012`.

`Fix:` replace line 408's opening with: "A mount takes at most 32 `immutable` patterns, each at
most 4096 bytes in either spelling; a misplaced `*` or an expression that does not compile is
refused with the same sentence a `view` pattern gets."

### MINOR 4 — the SPA page's one-line table makes it a two-way choice

`Where:` `docs/frameworks/static-and-spa.md:32`

`What:` "bare caches every exact file, otherwise `"/assets/*"`-style patterns" omits
`regex '…'`, so an agent working from the hashed-build page cannot discover the hashed-only
escape hatch from the table, and the row disagrees in arity with the reference's three rows.

`Fix:` "…bare caches every exact file, otherwise a `view` pattern (`"/assets/*"`) or
`regex '…'`; a match gets the year-long cache".

### MINOR 5 — "The one pattern" has no visible antecedent

`Where:` `docs/frameworks/static-and-spa.md:54-55`

`What:` the deep-URL paragraph says "The one pattern crosses `/`, so it covers
`assets/chunks/x.js` too", but the only pattern is in the fence at line 11, outside this
section. An agent skimming the caching section cannot resolve it and may assume any quoted
pattern crosses `/`, including a literal-file pattern like `"/assets/app.js"`.

`Fix:` name it in place: "`immutable "/assets/*"` crosses `/`, so it covers
`assets/chunks/x.js` too."

## What is right

- One grammar, said once: "the same [patterns](#path-patterns) a `view` takes — `regex '…'`
  included" and "the same sentence a `view` pattern gets" — no second dialect to learn.
- The three written forms are unambiguous and copyable verbatim; the Django fence's
  `immutable "/static/*"` genuinely reaches `select2/i18n` (`pattern.rs:141-149`, `TAIL = ".*"`).
- S3 is on the row where the agent chooses: `index`/`fallback` are always `no-cache`, and the
  exact year-long header is quoted.
- The 4096-byte cap is present "in either spelling", and the limits row correctly stopped
  claiming 4096 items for `immutable` (its old cap was 32).

## Review Resolution (re-review 2026-11-17)

Every prior finding is resolved in the current tree; verdict accept.

- **MAJOR 1 — resolved:** `docs/reference/blastfile.md:399-401` now states "request path, mount prefix included" with the `"/static/admin/*"` vs `"/admin/*"` counter-example.
- **MAJOR 2 — resolved:** the bare row (`:404`) now carries "only when each name changes with its bytes; a tree with unhashed originals uses a pattern or `regex`".
- **MAJOR 3 — resolved:** `:408-409` now says "A mount takes at most 32 patterns, each at most 4096 bytes, quoted or `regex '…'`".
- **MINOR 4 — rejected, accepted:** the reference owns the full three-way grammar; bare or `"/assets/*"` is all a hashed Vite `dist` needs, and the page links the reference for the rest.
- **MINOR 5 — resolved:** `docs/frameworks/static-and-spa.md:55` names `"/assets/*"` and `/assets/chunks/x.js`; no unseen antecedent.
- **No new BLOCKER/MAJOR:** the new `"/static/admin/*"` example matches the mount-prefix rule and `static_asset.rs:544-553`; the SPA fence stays `static "/"`, so its `"/assets/*"` is correct in context.
