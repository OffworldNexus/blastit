# DX review — one `immutable` pattern grammar (MB.31.1)

Verdict: accept

Answer to the node's question: **yes, in one line and one read.** A Django developer copies
`immutable "/static/*"` from the fence, runs the `collectstatic` command already documented,
and caches a nested file — no `sudo`, no env var, no second command, no footnote, and the
regex is now an option rather than an instruction that contradicts the fence.

Counts (re-check): BLOCKER 0 · MAJOR 0 · MINOR 1 deferred. Every finding below is
resolved unless the resolution table says otherwise.

## BLOCKER

**B1 — the stale sentence survives on a framework page, and the node's own grep catches it.**
`Where:` docs/frameworks/php/laravel.md:36
`What:` ``output; `*` stops at `/`, so this one pattern covers the files under `assets/`.``
After MB.31.2, `*` crosses `/`; after this diff the reference says so. The published set now
contradicts itself on the Laravel page, and the acceptance command
`grep -rn 'stops at \`/\`\|one glob per depth\|"/static/\*/\*"' docs/frameworks …` exits `0`
on this line, so the node cannot show green evidence.
`Fix:` Replace with ``output; `"/build/assets/*"` crosses `/`, so it also covers Vite's
`assets/chunks/…`.`` and add `docs/frameworks/php/laravel.md` to the node's `paths` via `plan.py`.

## MAJOR

**M1 — the match target became ambiguous, and the wrong guess fails silently.**
`Where:` docs/reference/blastfile.md:400
`What:` "matched whole against the URL path of the file it answers". The answering file has two
paths: the client's (`/static/admin/js/…/de.js`) and the root-relative one
(`admin/js/…/de.js`). `StaticFile::url_path` is the former (prefix + resolved remainder,
`crates/blastit-scheduler/src/static_asset.rs:544`), and every example depends on it: the Django
fence is mount `/static` + pattern `/static/*`. A reader who picks root-relative writes a pattern
that matches nothing and gets no error — just `no-cache` forever. The old wording said
"request path"; the diff dropped the word.
`Fix:` `matched whole against the request path, mount prefix included:`.

**M2 — "no pattern" collides with the row's own `Default: none`.**
`Where:` docs/reference/blastfile.md:385
`What:` the row reads ``| `immutable` | none | the files that never change: no pattern means
every exact file the mount serves; …``. In a table with a Default column, "no pattern" means
"directive absent" — which is exactly the `none` in the next cell and means *no caching*. The
new sentence means the opposite: written with no argument.
`Fix:` ``| `immutable` | none | written alone, every exact file the mount serves; otherwise a
pattern list (see [what it caches](#what-immutable-caches)); a match gets `Cache-Control: public,
max-age=31536000, immutable`; an `index` or `fallback` answer is always `no-cache` |``.

**M3 — the depth-crossing example uses a path its own pattern cannot match.**
`Where:` docs/reference/blastfile.md:405
`What:` ``| `immutable "/assets/*", "/fonts/*"` | … so one pattern reaches
`admin/js/vendor/select2/i18n/de.js` |``. `/assets/*` compiles to `/assets/.*` and matches nothing
under `/admin/`. The one place the reference teaches "a trailing `/*` crosses `/`" illustrates it
with a mismatched pattern and a framework path that belongs on the Django page.
`Fix:` ``| `immutable "/assets/*", "/fonts/*"` | each file whose path matches a pattern; a trailing
`/*` crosses `/`, so `"/assets/*"` reaches `/assets/chunks/x.js` |``.

**M4 — the bare form never says when it is wrong.**
`Where:` docs/reference/blastfile.md:404
`What:` ``| `immutable` | every exact file the mount serves |`` documents the mechanism and hides
the consequence: a file edited in place is pinned for a year with no revalidation. MB.31's own
framing is that bare is "a developer's promise that the whole root is content-addressed"; the
reader is never told to make that promise only when it is true. A Django `staticfiles` root with
unhashed originals is the exact counter-example, and it only appears on the Django page.
`Fix:` ``| `immutable` | every exact file the mount serves — only when the whole root is
content-addressed |``.

**M5 — the Django page shows the fence one way and then instructs the other.**
`Where:` docs/frameworks/python/django.md:64-66
`What:` keeping the fence at `immutable "/static/*"` was right, but the follow-up is an imperative:
"`collectstatic` leaves unhashed files beside the hashed copies, **so cache only the hashed names**
with `immutable regex '…'`". The developer now has two answers and no rule for choosing; the one
they just pasted reads as the wrong one. That is two ways to say one thing, and it undercuts the
node's question.
`Fix:` make the regex an option, not the instruction: ``collectstatic` leaves the unhashed copies
beside the hashed ones, so that line pins both; to cache hashed names only, use
`immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'`.``

## MINOR

**m1 — the caps sentence drops the list cap the ADR records.**
`Where:` docs/reference/blastfile.md:408
`What:` "A pattern is at most 4096 bytes, in either spelling" — `validate.rs:64` refuses a 33rd
pattern (`at most 32 patterns`), and the new ADR-0012 amendment states `MAX_PATTERNS = 32`; the
reference mentions only the byte cap, and the limits-table edit removed `immutable` from the
`4 096` list-items row. "either spelling" also asks the reader to decode jargon.
`Fix:` `A list holds at most 32 patterns and each is at most 4096 bytes, quoted or `regex '…'`; a
misplaced `*` or an expression that does not compile is refused with the same sentence a `view`
pattern gets.`

**m2 — the SPA table row is three clauses and one undefined word.**
`Where:` docs/frameworks/static-and-spa.md:32
`What:` ``the files that never change: bare caches every exact file, otherwise `"/assets/*"`-style
patterns; a match gets the year-long cache``. "Bare" is never used or defined on this page; the
other three rows are one clause each, this one is 150 characters covering two forms plus the cache
rule.
`Fix:` ``| `immutable` | files that never change; write it alone to cache every exact file, or with
`"/assets/*"` patterns; a match gets the year-long cache |``.

**m3 — "The one pattern" is a dangling reference, and the path loses its slash.**
`Where:` docs/frameworks/static-and-spa.md:54-55
`What:` "The one pattern crosses `/`, so it covers `assets/chunks/x.js` too." The pattern is two
sentences up; `assets/chunks/x.js` is not a URL when every other path on the page is `/`-prefixed.
`Fix:` ``"/assets/*"` crosses `/`, so `/assets/chunks/x.js` is cached too.``

**m4 — the design record still names the wrong match target.**
`Where:` docs/design/static-and-spa.md:110-112
`What:` R5 keeps "matched **whole** against the **answering file's** URL path (root-relative,
`/`-prefixed) … never against the request path", while `StaticFile::url_path` is prefix + remainder
and the supersession note says only the matcher changes. MB.31.2 can read the surviving sentence
and match `rel`, which silently breaks `immutable "/static/*"` on a non-root mount.
`Fix:` add one clause to the supersession note: ``matching stays against `StaticFile::url_path`
(mount prefix + resolved remainder)``.

## What is right

- The DX question is answered in one line of fence plus three sentences, with no `sudo`, no env
  var, and no new command: `collectstatic --no-input` was already the page's one build step. The
  net change is +3 lines on the Django page and +1 on the SPA page.
- The six-pattern staircase and the "stops at `/`" explanation are gone from the reference, the
  Django page, the SPA page and the design/ADR notes; `glob` no longer appears on any public page.
- Reference §What `immutable` caches is the right shape — three rows, no paragraph on `regex`
  internals — and it keeps the S3 rule in the directive row ("an `index` or `fallback` answer is
  always `no-cache`"), which the SPA page then demonstrates with a real `curl`.
- No invented syntax: bare `immutable`, `"/assets/*"`, `"/fonts/*"`, `{id}`/`{id:int}`/`{id:uuid}`
  and `regex '…'` are exactly what `pattern.rs` `compile`/`compile_regex` accept, and the ADR-0012
  EBNF `'immutable' (pattern (',' pattern)*)?` matches the parser seam. ADR-0007 keeps its history
  and gains only a pointer.

## Resolution (re-check)

Verified in the working tree; the node's acceptance grep for the stale `stops at` phrase now exits `1`.

| Finding | Status |
|---|---|
| B1 | resolved — `laravel.md:36` reads "a trailing `/*` crosses `/` … at any depth"; the file is still outside the node's `paths`, so the plan owner, not the elephant, adds it. |
| M1 | resolved — `reference:400` "request path, mount prefix included", plus the `static "/static"` → `"/static/admin/*"` example. |
| M2 | resolved — row 385 now "written alone it caches every exact file…", so `Default: none` no longer collides with it. |
| M3 | resolved — the depth example is `"/assets/*"` → `/assets/chunks/x.js`. |
| M4 | resolved — bare row now "only when each name changes with its bytes…". |
| M5 | resolved — `django.md:63-66` "that line pins both; to cache hashed names only, use …". |
| m1 | resolved — "at most 32 patterns, each at most 4096 bytes, quoted or `regex '…'`". |
| m2 | resolved — the SPA row no longer says "bare". |
| m3 | resolved — `/assets/*` crosses `/`, so `/assets/chunks/x.js` is cached too. |
| m4 | noted, deferred (MINOR): `design/static-and-spa.md:115-121` now names `StaticFile::url_path`, but R5 above still says "root-relative … never against the request path", so "the prefix-included rule above is unchanged" is false as written. Record only; the operative pointer is now correct. |

No new BLOCKER or MAJOR.
