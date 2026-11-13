# AI-ergonomics review — Blastit 0.1.1 user documentation

Lens: **`ai-ergonomics`**. Subject: the public docs for Python WSGI + PostgreSQL 0.1.1, written
docs-first. Read-only review; this file is the only artefact.

Sources read: the 16 pages named in the brief, `docs/VISION.md`, `plan/nodes/MA/MA.1/NODE.md`,
`docs/reference/log-events.md`, and `docs/design/ma-lean-dx.review-dx.md` (only to skip findings it
already raised). No code is judged; the documented contract is.

## The trace this review judges

An agent is handed a 50-route Django/Flask app with PostgreSQL and told "adopt Blastit, get CI
green". The only documented path is:

1. hand-write a manifest that already boots (`version`, `app`, `[proxy.db]`, at least one `[[route]]`);
2. `blastit check --learn`, which mutates `blastit.toml`;
3. read `git diff`, "tighten" what learn guessed;
4. `blastit check --json`, parse stdout, apply every `fix` string, repeat until exit `0`.

Every finding below is a place where that loop is undefined, lossy, or self-contradictory.

## BLOCKER

### B1 — `--json`'s stdout guarantee is contradicted by the reference and by pytest
`docs/ci.md:59`: "`--json` prints exactly one object on stdout; progress and the summary go to
stderr." `docs/reference/log-events.md:3` and `docs/reference/settings.md:40`: "stdout is the only
output". The same page says `blastit check` "runs `pytest -q`" (`docs/ci.md:9`), and `pytest -q`
writes its own progress and summary to **stdout**; the docs never say Blastit captures it.

Failure mode: the agent's standard move, `blastit check --json > out.json; jq . out.json`, sees
either pytest noise before the object (exit 1 path) or logs on stdout if `BLASTIT_LOG_FORMAT` is
`json` (production default, `settings.md:36`). The one contract the AI workflow rests on is not
enforceable as written. Exit `2` is worse: the page promises an object unconditionally but never
says whether a config error produces one or nothing.

Fix: state verbatim, in `reference/cli.md` and `ci.md`: "under `--json`, stdout carries exactly one
JSON document and nothing else; pytest output, progress, the human summary and `learn:` lines are
captured to stderr; on exit 2 stdout is either empty or `{"ok":false,"error":{...}}` — pick one and
test it." Remove "stdout is the only output" or scope it to server mode.

### B2 — The finding schema is an example, not a contract: no version, no closed enum, no severity, prose `fix`
`docs/ci.md:43-57` shows four `kind` values in a sample and calls it a day. Nothing declares the
`kind` set stable or exhaustive, there is no `schema_version`, no `severity`, and `fix` is a
sentence: `"add to [route.catalog.db].queries"`, `"delete it"`, `"cover it or delete it"`.
`"sql": "SELECT …"` (`:51`) is a Unicode ellipsis; if that is literal output the agent pastes an
invalid grant, and if it is illustrative the docs never say machine output is never truncated.

Failure mode: the agent must regex English to decide the edit, cannot safely branch on unknown
future kinds, cannot rank a `dead_grant` below a `missing_grant`, and cannot verify it handled
every kind. The `findings` array also has 4 entries while the human summary says "5 problems"
(`ci.md:34`), so `len(findings)` is not the problem count.

Fix: publish a versioned schema in `reference/` (e.g. `check.schema.json`), with
`schema_version`, `ok`, `phase`, per-proxy counts, and findings of a closed enum
(`unrouted_request`, `missing_grant`, `dead_grant`, `unexercised_route`, `missing_ingress`,
plus `ingress_mismatch`/`proxy_unused` below). Replace `fix` with machine data:
`"fix": {"action":"add_query","table":"[route.catalog.db].queries","value":"SELECT …"}` and an
`"unsafe": true` flag on destructive suggestions. Never truncate `sql`.

### B3 — Exit codes cannot tell a broken test suite from manifest drift
`docs/ci.md:62-68`: `1` is "a finding, or the suite failed". Those need opposite repairs: fix the
app/tests vs edit `blastit.toml`. The same page then says no tests makes pytest exit `5`
(`ci.md:78-80`), a code absent from the table. A `--learn` run that fails the suite is documented to
still write the manifest and "exits `0` if the suite passed" (`ci.md:89-91`), leaving the exit
semantics of the write case undefined.

Failure mode: `if rc == 1` tells the agent nothing; it applies manifest edits to a code bug, or
rewrites tests to satisfy a grant. A wrapper that treats "not 0/1/2" as infrastructure failure
misfiles pytest's `5`.

Fix: separate codes (`0` clean, `1` findings, `2` config, `3` suite failed, or keep `1` but add
`"phase":"suite"` + `"suite_exit":5` to the JSON), and document how pytest `2/3/4/5` are mapped.

### B4 — `--learn` writes `tables` grants while the rest of the docs teach exact `queries`
`docs/local.md:73`: "learn: 2 routes, 977 statements, **2 table grants** added". `docs/frameworks/django.md:99`
repeats "2 table grants added". But the grant model taught everywhere else is exact statements:
`local.md:106-111` ("Replace the empty list with the normalised statement"), `flask.md:69-70` ("A
`queries` entry is the statement as it goes on the wire"), `manifest.md:163-167`.

Failure mode: one pass converts 977 observed statements into a handful of `tables` grants, i.e. a
materially broader permission model than the docs describe. `blastit check` then passes clean, so
the agent cannot tell it produced a wide grant; the "least privilege, one statement at a time"
narrative is silently false. Conversely, if learn actually writes `queries`, the printed
"table grants" is wrong. Either way the agent cannot predict the diff it is asked to review.

Fix: decide and state it. Recommended: `--learn` writes exact normalised `queries` by default and
prints `learn: N queries added`; add `--prefer-tables` (already a `fold` flag, `cli.md:84`) for the
broad mode, with a warning. Show the exact diff grammar (routes first, then grants, sorted).

### B5 — The ingress enum cannot express a form or upload, and the machine `fix` prescribes `json` for every body route
`docs/reference/manifest.md:84-88` offers exactly three codecs: `json`, `none`, `raw`. Yet
`docs/ci.md:54` emits `"fix": "[route.ingress] body = \"json\""` for a route named `upload`
(`ci.md:32`) and for any route that "took a body". A Django admin login or any Flask form POST is
`application/x-www-form-urlencoded`; `body = "json"` makes it a `400` (`manifest.md:86`). The only
remaining choice is `raw`, the "loud opt-out" (`manifest.md:88`). The docs' own Django quickstart
(`django.md:80-82`) routes `/admin/.*` with `unrestricted` but no ingress, so its first form POST
hits exactly this trap.

Failure mode: an agent applies the supplied `fix` verbatim, CI goes green (presence-only check, see
M9), and the deployed app 400s all form posts — or the agent reaches for `body = "raw"` to make CI
pass, which is the security-relevant opt-out. This is the single most likely route to weakening the
manifest.

Fix: add `form` and `multipart` (or a `codec` list with `schema`) to the ingress enum before
claiming Python WSGI support. Make `missing_ingress` findings carry observed content types and do
not prescribe a codec in `fix`; suggest the codec that matches observation, or `"action":
"choose_codec"` with candidates.

### B6 — Test-fixture SQL has no documented grant home, so "fixtures do not change" is undefined
`docs/ci.md:74-76`: "The test database is prepared first with the real credentials, then the suite
runs through the proxy: your fixtures and your `DATABASE_URL` do not change." Fixtures issue
`TRUNCATE`, seed `INSERT`s, `BEGIN`/`ROLLBACK` and savepoints during the suite. Grants are per route
(`manifest.md:214-219`), and the proxy attributes statements by the per-route socket
(`manifest.md:126-135`). A pytest process is not a route, so the docs never say:

- which route owns fixture SQL,
- whether `--learn` records it (and where), or
- how the agent grants it without attaching test-only `TRUNCATE`/`DELETE` grants to a production route.

Failure mode: the first real CI run produces findings the agent cannot place; the obvious "fix" is
to add fixture statements to the route under test or to `unrestricted`, which hands a production
route the cleanup SQL (including deletes). "Your fixtures do not change" is false in the only
dimension that matters: their SQL now needs grants.

Fix: specify the attribution model. Either a synthetic `[route]`-less test principal with its own
grant block, or a documented `[test]` section consumed only by `blastit check` and refused in
production. Show the pytest-django `--create-db` / per-test rollback path and where those grants
live. Do not claim fixtures are unchanged until this exists.

## MAJOR

### M1 — `unrouted` is a parallel list, has no fix, and packs method and path into one string
`docs/ci.md:49`: `"unrouted": ["GET /robots.txt"]`, while `findings` (`:50-55`) has no
`unrouted_request` entry. The human summary counts it as one of "5 problems" (`:34`).

Failure mode: an agent that iterates `findings` and applies each `fix` — the documented loop
(`ci.md:60`, "Every finding names the route and the fix, so one edit closes it") — never fixes the
unrouted request, and cannot parse `"GET /robots.txt"` unambiguously when a path contains a space.

Fix: put unrouted requests in `findings` as `{"kind":"unrouted_request","method":"GET","path":"/robots.txt","fix":{"action":"add_route_or_widen_path"}}`, and keep the top-level array only as a convenience view.

### M2 — The one combination an agent needs (`--learn` + `--json` + inspect-before-write) is undocumented
`--json` and `--learn` are each documented (`cli.md:40-44`) but never together. With `--json`,
progress goes to stderr; `--learn` prints `learn: 2 routes, … → blastit.toml` (`local.md:73`) with
no statement of which stream that uses under `--json`. `--learn` writes straight to the manifest:
`dev` "on exit" (`local.md:86-88`), `check` after the suite (`ci.md:89-91`). There is no dry run,
no print-to-stdout, no `--output`, and no statement that writes are atomic, idempotent, or skipped
when the suite fails.

Failure mode: the agent cannot obtain a machine-readable *proposal* without mutating a
possibly read-only checkout, cannot re-run learn safely, and a killed `dev --learn` may or may not
have written. "Review the diff" assumes a human at a terminal.

Fix: define `blastit check --learn --json` as "one object on stdout: `{"proposed":{routes,grants}, "wrote": bool}`", add `--dry-run`, document atomic replace + idempotency + the failed-suite write policy per command, and state that `dev --learn` writes on `SIGINT` and `SIGTERM` (not `SIGKILL`).

### M3 — "Bootstrap an existing project" assumes a manifest that already boots; learn cannot discover the parts that make it boot
`docs/ci.md:84-87` says only "record everything, write what was missing". But a grant on an
undeclared proxy aborts the boot (`manifest.md:15`), `[[route]]` is required (`manifest.md:17`), and
`input` variable names are environment wiring learn cannot infer. Learn also cannot invent `app`.

Failure mode: an agent with a 50-route app and no `blastit.toml` runs the documented bootstrap and
gets exit `2` before a single request is observed. It then guesses the skeleton — often omitting
`[proxy.db]` or putting `[route.db]` before `[[route]]`.

Fix: put the minimal seed manifest in "Bootstrap" (the six-line `local.md:36-50` block), plus a
two-column table: "learn discovers: routes, queries/tables, transaction use" / "you must write:
`app`, `preload`, `[proxy.<name>]`, `input` names, `path` boundaries".

### M4 — Route inference by first path segment collapses distinct trust domains and can emit invalid names
`docs/local.md:88-89`: "it names routes from the first path segment and matches the rest with
`.*`". For a 50-route app, `/products/` and `/products/1/reviews` become one route with
`/products/.*`; `/api/v1/users` becomes `api`. `name` must match `[a-z0-9][a-z0-9_-]*`
(`manifest.md:61`), but no sanitisation/dedup rule is stated for `/`, `/Product`, `/__debug__`, or
a UUID segment.

Failure mode: learn produces a manifest that boots but has fewer, broader sandboxes than the app
has routes; check is green and the agent declares success. It also risks a duplicate-name or
invalid-name boot abort on real paths, with no documented repair.

Fix: normalise observed paths into templates (numeric/UUID segments become a parameter), one route
per distinct template, sanitise/dedupe names deterministically, and emit `learn.route_inferred`
events (or JSON entries) naming each generated regex. Say plainly that learned routes are
proposals, not security boundaries.

### M5 — A suite that never touches PostgreSQL reports clean; there is no observation sanity check
`docs/ci.md:9-11` claims a clean run "means no path, grant or route was forgotten", but the JSON
(`ci.md:44-48`) has only totals. If the suite overrides `DATABASES` to SQLite, or the app reads a
different URL, `statements` is `0` and a grant-free manifest can pass. `--learn` then writes
nothing and reports success.

Failure mode: the agent's strongest signal — `"ok": true` — is reachable with a completely
unobserved database. The VISION promise ("zero missing SQL grants") is vacuously true.

Fix: add per-proxy observation counts and a `proxy_unused` finding when a declared proxy observed
zero statements, plus a `no_observations` finding when the suite made zero requests. Make them
failures, not warnings, so "clean" means "observed".

### M6 — Destructive fixes and no "explicitly unexercised" escape push an agent to delete routes and grants
`docs/ci.md:52-53`: `"delete it"` for a dead grant, `"cover it or delete it"` for an unexercised
route. `ci.md:19-20` repeats it in prose. There is no baseline, no `[route] exercised = false`, no
`--allow-unexercised`, and no JSON severity to say "review, do not auto-apply".

Failure mode: adopting Blastit on a 50-route app with 30 untested routes, the cheapest path to
green is to delete 30 `[[route]]` entries — which turns those production paths into Blastit's 404
(`manifest.md:246`) — and to delete grants whose code path is untested but live. The agent is
following the documented fix.

Fix: document an explicit, warned baseline (`[route] exercised = false` or `--baseline PATH`) that
keeps the route and reports it as a warning, and make destructive suggestions
`{"action":"remove","unsafe":true}` rather than the default. State that deleting a route changes
production routing.

### M7 — `blastit check` offers no pytest selection or passthrough
`docs/ci.md:9` and `cli.md:42` define the command as `pytest -q` with no arguments. No `--`, no
`-k`, no marker, no `BLASTIT_PYTEST_ARGS`.

Failure mode: on a 50-route app the agent must run the whole suite for every single-route fix, and
cannot use markers to isolate a route; project-specific pytest plugins cannot be disabled. It will
invent `blastit check -- -k products` (a plausible guess) and get an unknown-flag error.

Fix: document a passthrough (`blastit check -- -k products`) or an env var, and state what is
captured vs forwarded. If unsupported, say so and name the workaround (run pytest yourself and
feed the log to `fold`).

### M8 — The Django quickstart recommends `unrestricted = true`, explicitly contradicting the local guide
`docs/frameworks/django.md:80-82`: "An area you cannot enumerate — Django's own admin — needs its
own route with `unrestricted = true`". `docs/local.md:117` says: "Do not reach for
`unrestricted = true` first: it gives away every statement on that proxy, including the ones
reading password hashes."

Failure mode: the first manifest an agent copies contains the exact escape hatch it was told to
avoid, on the route that handles admin login. Its natural generalisation is "un-enumerable area →
unrestricted", which is how a 50-route app ends up with several unrestricted routes.

Fix: move admin out of the quickstart into a boxed last resort showing the boot warning cost and a
narrow `path`; show the group-based alternative first. If admin is genuinely unenumerable, say what
it costs in the same paragraph as `local.md:117`.

### M9 — `missing_ingress` checks presence, not whether the declared codec matches observed bodies
`docs/ci.md:21` and `manifest.md:71-72` fail only when a body route has **no** `[route.ingress]`.
A route declaring `body = "json"` that actually receives form posts passes the audit and 400s at
runtime (`manifest.md:86`).

Failure mode: combined with B5, the agent's "fix" makes CI green and production red; the audit
cannot catch the mistake it induced.

Fix: add `ingress_mismatch` from observed `Content-Type` vs declared codec, and make it part of
the 4-way completeness claim.

### M10 — Top-level `groups` is a silent TOML footgun
`docs/reference/manifest.md:28-29` says "`app`, `preload`, `version` and `groups` come before any
`[table]`", but `groups` is also a route key (`manifest.md:54,64`). A `groups = ["session"]` line
written after a `[[route]]` silently binds to that route instead of erroring.

Failure mode: an agent generating a manifest from tables will place globals after the routes (or
after `[proxy.db]`) and get a valid file with the wrong scope: grants attached to one route instead
of all, or a global group that silently grants nothing. Both pass boot, so the error surfaces only
as a denial at runtime.

Fix: require globals before any table with a precise parse error ("`groups` at line N is not
top-level; move it above the first `[table]`"), or introduce a `[blastit]` table for globals. Add a
`manifest.warning` when a route-level `groups` names a group that is only meaningful globally.

### M11 — `frameworks/index.md` puts three assignments in one TOML block
`docs/frameworks/index.md:7-11` is a single ```toml fence containing `app = "myshop.wsgi:application"`,
`app = "app:app"`, and `app = "wsgi:application"`, differentiated only by comments. A literal
agent copies all three and gets a duplicate-key parse error, then guesses which to keep.

Fix: three separate fences with prose ("Django:" / "Flask:" / "anything else:"), or one placeholder
`app = "<module>:<callable>"`.

## MINOR

- **`preload` type is asymmetric with `path`.** `manifest.md:62` explicitly allows a bare string for
  `path`, but `preload` is shown only as an array (`:13,:24`) while `BLASTIT_PRELOAD` is
  comma-separated (`settings.md:12`). An agent will try `preload = "myshop.urls"`. State the accepted
  TOML types, or accept the string.
- **`fold` contradicts "every command reads `./blastit.toml`".** `cli.md:10` says every command
  honours `BLASTIT_MANIFEST`, but `cli.md:76-82` requires `--manifest PATH` for
  `blastit-manifest fold`. Say whether `fold` reads the env var; if not, exclude it from "every
  command". Scripted repair using `fold` otherwise fails.
- **`learn:` output does not resolve the path.** `local.md:73` and `django.md:99` print
  "→ blastit.toml" even when `BLASTIT_MANIFEST` points elsewhere (`settings.md:13`). An agent
  parsing logs edits the wrong file. Print the absolute resolved path.
- **Empty `BLASTIT_*` wins and clears manifest values.** `settings.md:3-4` says a set variable wins
  "even when empty". An agent generating CI env from optional variables
  (`BLASTIT_PRELOAD: ${{ vars.PRELOAD }}`) can silently erase `preload`/`app` and get a boot abort.
  Document the empty-string behaviour next to the variable table and suggest omitting empty vars.
- **Discovery is cwd-only, not upward.** `local.md:13` and `settings.md:13` say `./blastit.toml`;
  no page says Blastit does not search parent directories like Cargo/npm. A monorepo agent will
  assume upward discovery. State it, and state that `dev`/`check` have no `--manifest` flag (env
  only) so the guess is corrected before it fails.

## Verdict

The human-facing prose is lean, but the machine contract an agent must run on is not yet a contract:
`--json` output, exit codes, finding kinds, fix semantics, ingress codecs and `--learn` writes are
each ambiguous or self-contradictory, and the docs' own fixes lead to `raw`/`unrestricted` or to
deleted routes — fix B1–B6 before MA.5 treats these pages as the design source of truth.
