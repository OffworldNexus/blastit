---
lens: ai-ergonomics
target: docs/frameworks/php.md
status: review
---

# `php.md` — ai-ergonomics review

## Round 3 (2026-11-14) — verification after the bootstrap and ingress edits

**Verdict:** FAIL — 0 BLOCKER, 1 MAJOR. The one-pass bootstrap is now scriptable (`check --learn -- <suite>`) and the DB split is PostgreSQL-only; the remaining MAJOR is that the page's own route order makes the documented second-route split dead.

**Method:** I re-derived the manifest and commands from `docs/frameworks/php.md` alone (149 lines), then checked each claim against `docs/reference/manifest.md` (§Ingress, §Static, §Proxies, §Grants, §Effective grants, §Routing), `docs/reference/settings.md` §Backends, `docs/local.md` §Fill the manifest in one pass, and `docs/design/php-fastcgi.md` §4.10. I did not read the round-1/2 text except to map the findings.

### What the page alone produces

```toml
version = 1

[[route]]
name = "app"
path = ['.*']

[route.static]
root = "public"
immutable = ["/assets/*"]

[route.fastcgi]
entry = "public/index.php"

[route.ingress]
body = "form"
```

For a Symfony app with a JSON API, `:37-38` adds a second route:

```toml
[[route]]
name = "api"
path = [???]                    # the page gives no value
[route.fastcgi]
entry = "public/index.php"
body = "json"                   # the page omits the [route.ingress] table
```

For PostgreSQL, `:42-46` adds:

```toml
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[route.db]                      # a grant; the page gives no key
```

```console
$ uv tool install blastit
$ blastit dev
$ curl -i http://127.0.0.1:8080/users/42
$ blastit check -- vendor/bin/phpunit
$ blastit check --learn -- vendor/bin/phpunit   # once, never the gate
$ blastit check --json -- vendor/bin/phpunit
$ blastit --preset compose-traefik
```

**Correct and complete?** The single-route form app is: it boots, takes a form POST, serves `public/` assets, and `check --learn` then `check` is a complete one-pass bootstrap for routes and queries. The DB bootstrap is complete: an empty `[route.db]` is a declaration that still gets a listener (`stage0.rs:1364-1376`; `grants.rs:142-149` parses it), so `check --learn` can record queries. The multi-codec split is **not** usable as written — the second route is placed after the `.*` catch-all (N1), and its `path` value is missing.

### Round-2 disposition

| id | status | where / why |
|---|---|---|
| M9 | resolved | `:114-118` "Bootstrap once, never as the gate: `--learn` records instead of denying: `blastit check --learn -- vendor/bin/phpunit`"; `:44-46` "writes the queries you exercised. Review the diff, then run `check` without `--learn`" |
| m20 | partially | `:36` "`path` entries are regexes, `immutable` entries are globs" — the label is there, but `:37-38` still gives no `path` value (`/api/.*`) and never says "matched whole", so `path = ['/api/*']` remains possible |
| m21 | resolved | `:39` "A body that does not match the codec is a `400`; no `[route.ingress]` at all is a `413`" |
| m22 | partially | `:44-46` "writes the queries you exercised. Review the diff" implies an in-place edit, but the file is not named and "in place" is not stated; `:114` "Bootstrap once, never as the gate" covers the CI warning. `:44` also says `dev --learn` "proposes a missing `root`" while `manifest.md:342` says it adds queries too — a simplification, not a wrong action |
| m23 | resolved | `settings.md:26` now "the PHP Blastit runs for a `[route.fastcgi]` route; environment-only, never a manifest key"; `:146` mirrors "the PHP Blastit runs" |
| m24 | carried | `:146` "The image needs the PHP Blastit runs" — the version claim is gone, but "The image" still has no antecedent (own `Dockerfile` vs one the preset generates) |
| m2 | carried | `:55` `uv tool install blastit`; no "requires `uv`" |
| m3 | carried | `:60` `127.0.0.1:8080`; no `--port` |
| m4 | carried | `:32` gives `entry` "relative to `blastit.toml`"; `root`'s no-leading-`/`/no-`..` rules are still absent |
| m5 | carried | `:12` `name = "app"`; the `[a-z0-9][a-z0-9_-]*` grammar is still absent |
| m10 | carried | `:120` names `BLASTIT_URL`; what else the suite gets is still unstated |
| m19 | partially | `:82-84` now gives `/assets/*` for `public/assets/` and `/build/*` for Encore, "matched whole", "`*` stops at `/`"; the example `:17` is still `/assets/*`, and "a glob that matches nothing is not an error" is still absent (the `Cache-Control` probe at `:86-91` verifies a match) |

### New findings

#### N1 — MAJOR — the second route is appended after the `.*` catch-all, so first-match makes it dead

- `docs/frameworks/php.md:13` — `path = ['.*']`
- `docs/frameworks/php.md:37-38` — "a JSON API beside the form app is a second `[[route]]` with its own `name`, `path`, `[route.fastcgi] entry` and `body = "json"` — first match wins."
- `docs/reference/manifest.md:308` — "Routes are tried in order; the first match of `path` (and `methods`) wins."; `:313` — "A catch-all last".

**Wrong outcome:** the agent appends the JSON route after the canonical catch-all (the page calls it "second"), so `.*` matches every request first; the JSON route never runs, JSON POSTs hit `body = "form"` and `400`, and the cheapest fix is to flip the catch-all to `json` (breaking forms) or add `raw`/`allow_raw` to it — the loud opt-out the page warns about. The page states "first match wins" but never says "before the catch-all", and its example order contradicts it.

**Replacement (net +2 words, paid for by deleting "beside the form app"):** "a JSON API is a `[[route]]` **before** the catch-all, with its own `name`, `path` (`/api/.*`), `[route.fastcgi] entry` and `[route.ingress] body = "json"`". That also closes m20's missing worked regex.

### Verdict

VERDICT: FAIL — 0 blocker, 1 major

---

## Round 2 (2026-11-14) — after the ingress and mechanism-neutrality edits

**Verdict:** FAIL — 0 BLOCKER, 1 MAJOR, 13 MINOR, 3 NIT. The canonical manifest now takes a POST and the page is mechanism-neutral (no pool, `pm.max_children`, socket or `listen.mode`; only the H1/nav still say PHP-FPM). One MAJOR remains: the page has no scriptable one-pass bootstrap, so an agent still fixes `missing_grant` one query per request.

**Method:** I wrote the manifest and commands below from `docs/frameworks/php.md` alone, then opened `docs/reference/settings.md` §Backends, `docs/reference/manifest.md` (path/ingress/static/proxies), `docs/ci.md` and `docs/deploy/index.md` to check each inference. I did not read `php-fastcgi.md`.

### What the page alone produces

```toml
version = 1

[[route]]
name = "app"
path = ['.*']

[route.static]
root = "public"
immutable = ["/assets/*"]

[route.fastcgi]
entry = "public/index.php"

[route.ingress]
body = "form"
```

```console
$ uv tool install blastit
$ blastit dev
$ curl -i http://127.0.0.1:8080/users/42
$ blastit dev --learn          # request an asset, Ctrl-C
$ blastit check -- vendor/bin/phpunit
$ blastit check --json -- vendor/bin/phpunit
$ blastit --preset compose-traefik
```

This boots, takes a form POST, and the split rule at `:37-38` is enough to add the `/api/.*` route. B1 is gone.

### Round-1 disposition

| id | status | where / why |
|---|---|---|
| B1 | resolved | `:23-24` shows `[route.ingress] body = "form"`; `:37-38` states one codec per route and the first-match split |
| M1 | resolved | `:69-70` links `settings#backends`; no `BLASTIT_PHP_FPM=` assignment, no `PATH` claim |
| M2 | resolved | `:49` states 8.2+; `:67` "it does not check your PHP version" |
| M3 | resolved | `:65-67` "does not install dependencies, migrate, warm caches or read `.env`" |
| M4 | resolved | `:69-72` shows the `./blastit.toml:12: …` boot error; the FPM-specific `503`/`502` are correctly dropped |
| M5 | carried (external) | `manifest.md:13` still says `app` is "required when any route is not static" |
| M6 | resolved | `:43-45` proxy + `--learn` |
| M7 | resolved | `:39-41` `raw` + `allow_raw` scoped to the upload path, "never the catch-all" |
| M8 | carried (external) | `manifest.md` still has no `[route.fastcgi]` section |
| m1 | resolved | `:36` "`[route.static]` is optional" |
| m2 | carried | `uv` still an unstated prerequisite for `:53` |
| m3 | carried | `--port` still absent; `check`'s fixed `:8080` has no documented escape either |
| m4 | carried | `root`'s "relative, no leading `/`, no `..`" rules still absent |
| m5 | carried | `name` grammar still absent |
| m6 | resolved | `:34` "a value above the budget Blastit derives is a boot error" |
| m7 | resolved | `:121-123` "`check` exits `2` without it" |
| m8 | resolved | `:127-128` "one document on stdout, everything else on stderr" |
| m9 | carried | `:135` still shows only `unexercised_route`, no `missing_ingress`/`declare_ingress` shape |
| m10 | carried | neither the app's nor the suite's environment is stated |
| m11 | resolved | `:4-5` "the `--learn` static-root proposal" |
| m12 | resolved | heading removed; `:95` is prose |
| m13 | resolved | `:126-127` "is a finding (exit `1`)" |
| m14 | resolved | `:146` "image must contain PHP … write access to its `var/`" |
| m15 | resolved | `:113` "With `vendor/` installed" |
| m16 | carried | `:95-96` still omits "request a URL that maps to a real file" |
| m17 | carried (external) | H1 `:1` and `zensical.toml:37` still say PHP-FPM |
| m18 | resolved | `:65` "runs PHP for the route itself … no server to start beside it" |
| m19 | carried | `:18` still `/assets/*`; a glob that matches nothing is still silent |
| n1 | carried | the "restart" sentence is gone, but whether a PHP edit needs a restart is still unstated |
| n2 | carried | `:74` demo body still unlabelled |
| n3 | carried | `:62` `dbproxy` still unexplained |

### New findings (round 2)

#### M9 — MAJOR — no scriptable one-pass bootstrap; `--learn` is presented as interactive

- `docs/frameworks/php.md:3-5` — "`0.2.1` adds it, with `check -- <suite>` and the `--learn` static-root proposal."
- `docs/frameworks/php.md:44-45` — "`blastit dev --learn` writes the grants you exercise"
- `docs/frameworks/php.md:113-119` — "With `vendor/` installed, one command starts the app on loopback, runs your suite and audits every request and query it saw: `blastit check -- vendor/bin/phpunit`"

**Wrong outcome:** the intro scopes `--learn` to the static root; the only grant-learning shown is `dev --learn`, which needs a human clicking (or a second shell pointed at 8080, which the page never mentions). The CI section runs the suite *without* `--learn`, so a missing grant is denied at the proxy, the request 500s at its first query, and the next query in that request is never reached. An agent bootstrapping a 50-route Symfony app fixes one query per run — the whack-a-mole `VISION.md:25` promises to remove — and by the third round the cheapest edit is `unrestricted = true` on the route.

**Replacement:** one line in CI/CD after the first `check`:

> Bootstrapping an existing app: `blastit check --learn -- vendor/bin/phpunit` records every route and query and writes the grants the manifest lacks; re-run without `--learn` for the clean audit.

And name both forms in the proxies paragraph: "`blastit dev --learn` (click through) or `blastit check --learn -- <suite>` (run the suite) writes the grants you exercise." If `--learn` is `dev`-only for a FastCGI manifest, say so and show the two-shell recipe (`blastit dev --learn` + `BLASTIT_URL=http://127.0.0.1:8080 vendor/bin/phpunit`) — do not leave the agent to invent it.

#### m20 — MINOR — `path` is a regex, `immutable` is a glob; neither is labelled

- `docs/frameworks/php.md:14` — `path = ['.*']`
- `docs/frameworks/php.md:38` — `path = ['/api/.*']`
- `docs/frameworks/php.md:84-86` — "`immutable` values are served-path globs … matched whole, and `*` stops at `/`"

**Wrong outcome:** an agent carries the glob habit from `immutable` and writes `path = ['/api/*']`; that regex matches `/api` and `/api//`, not `/api/users`, so the JSON route never matches and its POSTs fall to the catch-all and 413/400.

**Replacement:** "`path` entries are regexes matched against the whole request path (`.*`, `/api/.*`); `immutable` entries are globs — do not mix them."

#### m21 — MINOR — the runtime status for a codec mismatch is unstated

- `docs/frameworks/php.md:39` — "A route with no `[route.ingress]` answers a body with `413`."

**Wrong outcome:** the agent's route has `body = "form"` and a JSON client; the answer is `400` (`manifest.md` ingress table), not the `413` the page teaches, so the agent debugs "missing ingress" instead of "wrong codec".

**Replacement:** "A body that does not match the route's codec is a `400`; a body to a route with no `[route.ingress]` is a `413`."

#### m22 — MINOR — `--learn` "writes" vs "proposes"; the file side effect is unstated

- `docs/frameworks/php.md:44-45` — "writes the grants you exercise"
- `docs/frameworks/php.md:95-96` — "proposes `root` on Ctrl-C"

**Wrong outcome:** an agent cannot tell whether `blastit.toml` is edited in place. It runs `--learn` inside a CI audit (dirty tree, grants written from a partial run) or forgets to review the diff.

**Replacement:** "`--learn` edits `blastit.toml` in place: it writes the proxy grants you exercised and proposes the static `root`. Review and commit the diff; never run `--learn` in the CI audit."

#### m23 — MINOR — the `BLASTIT_PHP_FPM` row still describes the spawn mechanism

- `docs/reference/settings.md:26` — "| `BLASTIT_PHP_FPM` | `php-fpm`, then versioned names on `PATH` | the PHP binary Blastit starts for a `[route.fastcgi]` route; environment-only, never a manifest key |"

**Wrong outcome:** the row is the one place that goes false if libphp wins ("the PHP binary Blastit starts"), and it is now the row the page links for the failure path. The variable and its env-only status are fixed; the description is not.

**Replacement:** "the PHP runtime Blastit uses for a `[route.fastcgi]` route; environment-only, never a manifest key", and mark the discovery default "(spawn mode)" until the mechanism decision lands.

#### m24 — MINOR — "The image" has no antecedent

- `docs/frameworks/php.md:146` — "The image must contain PHP, and each route's uid needs write access to its `var/`."

**Wrong outcome:** an agent asked to deploy cannot tell whether it edits its own `Dockerfile` or one the preset generates; `deploy/index.md` (the link) does not define "the image" either.

**Replacement:** "Your app image (the `compose-traefik` recipe's `Dockerfile`) must contain PHP 8.2+, and each route's uid needs write access to its `var/`."

### External (recorded, not counted as this page's findings)

- `docs/ci.md` is still pytest-shaped: its bootstrap example is `uv run blastit check --learn` with no `-- <suite>`.
- `docs/reference/manifest.md` has no `## FastCGI` section and a stale `app` row (`:13`).
- The H1 (`php.md:1`) and the nav (`zensical.toml:37`) still say "PHP-FPM".
- `manifest.md#ingress` / `#proxies` are the page's link targets for keys the page does not show (the `[route.ingress]` values beyond `form`, the `[proxy.<name>]` block shape).

---

## Round 1 (original review)

**Test (the owner's):** an agent reads the page and produces a correct `blastit.toml` plus the one command, without guessing — a Symfony developer sees exactly what they must write and what Blastit figures out.

**Method:** fresh context. I wrote the manifest and command sequence below from `docs/frameworks/php.md` alone, *then* opened `settings.md`, `manifest.md`, `php-fastcgi.md` §1.1–§1.4 and its amendment banner, and `VISION.md` to check each inference. Every inference is a finding. I did not read any other page.

**Revision note:** the page was edited while this review ran. Every finding below is against the current text (read 2026-11-14). The earlier `entry` ambiguity is fixed at `:31` and is not counted; the earlier `KernelBrowser` gap is fixed at `:119-121`.

---

## 0. What I produced from the page alone

### 0.1 First-pass `blastit.toml`

```toml
version = 1

[[route]]
name = "app"
path = ['.*']

[route.static]
root = "public"
immutable = ["/assets/*"]

[route.fastcgi]
entry = "public/index.php"
```

### 0.2 First-pass commands

```console
$ uv tool install blastit
$ blastit dev
$ curl -i http://127.0.0.1:8080/users/42

$ blastit dev --learn        # request an asset, Ctrl-C
$ blastit check -- vendor/bin/phpunit
$ blastit check --json -- vendor/bin/phpunit
$ blastit --preset compose-traefik
```

### 0.3 What I had to guess to make it actually work

```toml
version = 1

[[route]]
name = "api"
path = ['/api/.*']
[route.ingress]
body = "json"

[[route]]
name = "app"
path = ['.*']
[route.ingress]
body = "form"

[route.static]
root = "public"
immutable = ["/build/*"]

[route.fastcgi]
entry = "public/index.php"
```

Every added or changed line above is an inference the page did not state. The findings below are those inferences, graded.

---

## 1. Inference log

| # | What I had to infer | Finding |
|---|---|---|
| I1 | A POST needs `[route.ingress] body = "…"`; form and JSON need two routes | B1 |
| I2 | Which PHP binary and version, and how to point at it | M1, M2 |
| I3 | `blastit dev` does not install deps / migrate / read `.env` | M3 |
| I4 | What a failed `blastit dev` prints | M4 |
| I5 | `app` is omitted, not forbidden | M5 |
| I6 | A Doctrine app needs a `[proxy.*]` and `--learn` grants | M6 |
| I7 | A multipart upload needs `raw` + `allow_raw`, scoped | M7 |
| I8 | The reference's `[route.fastcgi]` section | M8 |
| I9 | `[route.static]` is optional | m1 |
| I10 | `uv` is a prerequisite | m2 |
| I11 | `--port` exists | m3 |
| I12 | `root` is relative, no leading `/` | m4 |
| I13 | `name` has a grammar | m5 |
| I14 | An over-budget `workers` is a boot error, not a clamp | m6 |
| I15 | `check` needs `-- <suite>` | m7 |
| I16 | `--json` stdout is exactly one document | m8 |
| I17 | What a `missing_ingress` finding looks like | m9 |
| I18 | What environment the suite gets | m10 |
| I19 | What "the PHP `--learn` write" writes | m11 |
| I20 | "Find the asset paths" finds a root, not paths | m12 |
| I21 | `(1)` is the exit code | m13 |
| I22 | The image needs `php-fpm`; `var/` must be writable | m14 |
| I23 | `composer install` precedes `check` | m15 |
| I24 | `--learn` needs a real file, and `public/assets/` is not Encore's path | m16 |
| I25 | The title's "PHP-FPM" is the mechanism | m17 |
| I26 | "starts PHP … stops it" is a process model | m18 |
| I27 | `*` does not cross `/`; a glob that matches nothing is silent | m19 |
| I28 | Whether a PHP edit is picked up | n1 |
| I29 | The `curl` body is the demo app, not Blastit | n2 |
| I30 | The banner's `dbproxy` is a process, not a configured proxy | n3 |

---

## 2. BLOCKER

### B1 — the canonical manifest cannot take a POST, and the page never shows the ingress key or the one-codec-per-route rule

`docs/frameworks/php.md:35-36`

> A request with a body (POST, PUT, PATCH) needs `[route.ingress]` (`form` or `json`; `raw` also needs `allow_raw`), or it is a `413`.

The example at `:9-22` has no `[route.ingress]`, so every POST to the canonical manifest is a `413`. The page names the values but never the key (`body = "…"`), the table placement, or the fact that a route carries exactly one codec (`docs/reference/manifest.md:95-100`). A Symfony app with a login form **and** a JSON API cannot use one catch-all route: `body = "form"` 400s JSON, `body = "json"` 400s the form. The only escape is `raw` + `allow_raw` on the catch-all — the loud opt-out (see M7).

**Reader/agent gets wrong:** it copies the example, POSTs to the login form, gets `413`, and has no documented next step. It cannot produce a correct manifest in one pass.

**Replacement:** show the block and the split rule:

```toml
[route.ingress]
body = "form"     # or "json"; one codec per route
```

"A route that takes both form and JSON bodies needs two routes with disjoint `path` prefixes (routes are first-match, in order)."

---

## 3. MAJOR

### M1 — the page still hands the developer an FPM-specific env var as a knob, ahead of the design's own discovery contract

`docs/frameworks/php.md:57-59`

> PHP it cannot find is one variable — `BLASTIT_PHP_FPM=/opt/php/bin/php-fpm` ([settings](../reference/settings.md#backends)).

Three problems in one sentence. (a) It is false if Blastit embeds libphp: there is no `php-fpm` binary and no `PATH` lookup. (b) The brief fixes `BLASTIT_PHP_FPM` as env-only and a *reference* row, not a page knob; the page restates the assignment. (c) The design explicitly defers "the discovery order, cleanup and the error contract" to `MB.3.9.3` (`docs/design/php-fastcgi.md:34-36`), so the page promises a remedy the design has not pinned.

**Reader/agent gets wrong:** it treats `BLASTIT_PHP_FPM` as a normal local setting, and on a libphp build it looks for a binary that does not exist.

**Replacement:** "If Blastit cannot find the PHP it needs, the boot names what it looked for and [settings](../reference/settings.md#backends) says how to point it at yours." No assignment, no `PATH` claim, no mechanism.

### M2 — "PHP 8.2 or newer" is unverifiable from the page, and `php` is not `php-fpm`

`docs/frameworks/php.md:40-41`

> PHP 8.2 or newer on the machine (or in the container) that runs Blastit, and Blastit installed once.

**Reader/agent gets wrong:** the page never says how to check, never says Blastit does not check, and never distinguishes the `php` CLI (which Composer uses) from the `php-fpm` binary (which Blastit uses). A machine with `php` 8.3 and `php-fpm` 8.1 passes the developer's check and fails at the first request with a Symfony parse error. The design has no version gate, so nothing catches it.

**Replacement:** "PHP 8.2 or newer, both the `php` CLI and `php-fpm`; check with `php -v` and `php-fpm -v`. Blastit does not check the version for you."

### M3 — the page never says what `blastit dev` will *not* do

`docs/frameworks/php.md:56-57`

> Blastit runs PHP for the route itself: one command, no server to start beside it, nothing left running after Ctrl-C.

**Reader/agent gets wrong:** "one command" reads as "Blastit sets up the app". It does not run `composer install`, migrations or a cache warmup, and it does not read `.env` for the app (the app's own Dotenv does). It does pass the environment through minus `BLASTIT_*` (`php-fastcgi.md:583-589`), but the page never says that either. An agent that skips `composer install` gets a 500 with no hint; an agent that sets `APP_ENV=test` cannot tell whether it reached PHP.

**Replacement:** a three-line "What `blastit dev` does not do" list: "It does not run `composer install`, migrations or a cache warmup, and it does not read `.env`; your app does. It passes your environment through, minus `BLASTIT_*`."

### M4 — no failure path: the page shows only 200s

`docs/frameworks/php.md:63-69`

> `$ curl -i http://127.0.0.1:8080/users/42` … `HTTP/1.1 200 OK`

**Reader/agent gets wrong:** the design pins exactly one fatal diagnostic shape, `./blastit.toml:<line>: …` (`php-fastcgi.md:87, 397`), plus `503` for a pool that is not up and `502` mid-exchange (`R20`). The page shows none of them. An agent whose `entry` is wrong, whose PHP is missing, or whose port 8080 is taken has no string to match and no next step. This is the owner's "what to do when it fails" question, unanswered.

**Replacement:** one bad-manifest example and one pool example: "A manifest Blastit cannot use stops the boot before it binds: `./blastit.toml:12: route "app" (index 0): fastcgi entry "index.php": must be under static root "public"`. Fix that line. A pool that is not up answers `503`; a pool that dies mid-response answers `502`."

### M5 — "needs no `app`" is right, but the linked reference says the opposite

`docs/frameworks/php.md:26`

> A `[route.fastcgi]` route needs no `app`: the front controller is the app, and `[route.ingress]` applies to it.

`docs/reference/manifest.md:13` says `app` is "required when any route is not static", and its route table (`:68-78`) has no `[route.fastcgi]` row. The design's reconciliation says `app` is "accepted and ignored" on a manifest with any `[route.fastcgi]` (`php-fastcgi.md:595-601`). The page is now correct; the reference is stale. An agent following the page's last link (`php.md:145`) is told to add an `app` the page says it does not need, and cannot tell whether setting one is fatal.

**Replacement (reference, not page):** `manifest.md:13` → "required when any route declares neither `[route.static]` nor `[route.fastcgi]`"; on a manifest with any `[route.fastcgi]`, `app` is accepted and ignored. Optionally add "(ignored if set)" to `php.md:26`.

### M6 — a Doctrine app fails on its first query and the page never mentions the proxy or `--learn` for SQL

`docs/frameworks/php.md:34`

> Nothing else to declare for this route: Blastit starts PHP from `entry` and stops it when `blastit dev` exits (Ctrl-C).

**Reader/agent gets wrong:** "Nothing else to declare for this route" is scoped to the FastCGI block, but the manifest example is the whole file and has no `[proxy.*]`. A route with no grant for a proxy gets no listener (`manifest.md:285-286`), so a Symfony app with Doctrine cannot connect. The VISION's "single-pass wire learning" (`VISION.md:25`) is exactly the fix — `blastit dev --learn` records queries and writes the grants (`manifest.md:342-343`) — but the page's `--learn` section is only about static assets. An agent will either hardcode a `DATABASE_URL` (bypassing the proxy) or conclude Blastit does not support its database.

**Replacement:** one sentence + link: "A route that talks to PostgreSQL/MySQL/Redis declares a `[proxy.<name>]`; a route with no grant gets no listener. `blastit dev --learn` records the queries you exercise and writes the grants — see [the manifest](../reference/manifest.md#proxies)."

### M7 — a file upload pushes the agent to `raw` + `allow_raw` on the catch-all

`docs/frameworks/php.md:36`

> (`form` or `json`; `raw` also needs `allow_raw`)

**Reader/agent gets wrong:** Symfony file uploads are `multipart/form-data`; there is no multipart codec (`manifest.md:105-106`), so the only answer is `raw`. The page mentions `raw` in a parenthesis with no warning that it is the loud opt-out, flagged at boot (`manifest.md:100, 333`), and that it must be scoped. An agent hitting a 413 on upload will add `raw` + `allow_raw` to the catch-all route and silently disable decoding for every request — the PHP-page version of "just set `unrestricted = true`".

**Replacement:** "A multipart upload has no codec yet: use `raw` with `allow_raw = true` on a route scoped to the upload path only. `raw` is the loud opt-out, flagged at boot; never put it on the catch-all."

### M8 — reference: `manifest.md` has no `[route.fastcgi]` documentation, so the page's verification link is a dead end

`docs/reference/manifest.md:68-78` (route key table) and `:108-145` (static section) contain no `[route.fastcgi]` row or section. `manifest.md:145` already uses the term "front controller" without defining it.

**Reader/agent gets wrong:** the page's last line links `[the manifest](../reference/manifest.md)` (`php.md:145`). An agent following it to verify `entry`/`workers` finds no `[route.fastcgi]` and a top-level `app` row that says the opposite of `php.md:26`. This is transient — `MB.3.5.2` specifies the three edits (`php-fastcgi.md:592-608`) — but until it lands, the page and its reference disagree.

**Replacement (reference, not page):** apply the three §4.1 edits, especially `manifest.md:13` and the new `## FastCGI` section. Do not patch the page around a stale reference.

---

## 4. MINOR

### m1 — `[route.static]` is still presented as part of the canonical manifest

`docs/frameworks/php.md:16-18` versus `:97` — the example includes `[route.static]`; the learn section says "Started without `[route.static]`". **Gets wrong:** the agent copies the block into an app with no built assets (or a non-`public` root) and either boot-fails (`manifest.md:327-330`: a missing root stops the boot) or serves files it did not mean to. **Replacement:** "`[route.static]` is optional: include it only when you have built assets under `public/`; without it every path reaches the front controller."

### m2 — `uv` is an unstated prerequisite

`docs/frameworks/php.md:44` — `uv tool install blastit`. **Gets wrong:** an agent on a machine without `uv` cannot run the only install line. **Replacement:** "Requires `uv` (one line to install); then `uv tool install blastit`."

### m3 — `--port` is not shown

`docs/frameworks/php.md:51` — "127.0.0.1:8080". **Gets wrong:** an agent with 8080 busy has no documented escape. **Replacement:** "`--port N` changes it; `PORT` is the deployment form."

### m4 — `root`'s path rules are not on the page

`docs/frameworks/php.md:17` — `root = "public"`. **Gets wrong:** the agent writes `/public` or `../public` and hits a boot error (`manifest.md:128`). **Replacement:** "`root` is relative to `blastit.toml`, no leading `/`, no `..`."

### m5 — route `name` grammar is not on the page

`docs/frameworks/php.md:13` — `name = "app"`. **Gets wrong:** the agent writes `name = "Symfony App"` and hits a boot error (`manifest.md:70`). **Replacement:** "`name` is `[a-z0-9][a-z0-9_-]*`; it appears in logs and `check --json`."

### m6 — an over-budget `workers` is a boot error, not a clamp

`docs/frameworks/php.md:32` — "It can only lower this route's concurrency below what Blastit derives, never raise it". **Gets wrong:** an agent setting `workers = 32` expects a silent lower and gets a boot error (`php-fastcgi.md:356`: `FastCgiWorkersOverBudget`, naming both numbers). **Replacement:** "omit it; a value above the budget is a boot error naming both numbers. The boot logs the derived value."

### m7 — `check` requires `-- <suite>` for a FastCGI manifest

`docs/frameworks/php.md:113` — `blastit check -- vendor/bin/phpunit`. **Gets wrong:** the agent runs `blastit check` and gets exit `2`. **Replacement:** "For a FastCGI manifest `-- <suite>` is required; without it `check` exits `2` naming the route."

### m8 — `--json` stdout contract is not stated

`docs/frameworks/php.md:127` — `blastit check --json -- vendor/bin/phpunit`. **Gets wrong:** the agent pipes to `jq` without knowing the human line moved to stderr. **Replacement:** "With `--json`, stdout is exactly one JSON document; everything else goes to stderr."

### m9 — the JSON example never shows the `missing_ingress` finding an agent must fix

`docs/frameworks/php.md:131` — the only finding is `unexercised_route`. **Gets wrong:** the agent fixing a POST has no example of the `declare_ingress` fix. **Replacement:** add one line: `{"kind":"missing_ingress","severity":"error","route":"app","fix":{"action":"declare_ingress","codecs":["form"],"allow_raw":false}}`.

### m10 — the suite's environment is not stated

`docs/frameworks/php.md:117` — "`check` puts the front controller on loopback as `BLASTIT_URL=…`". **Gets wrong:** the agent's shell variables (`APP_ENV`, `DATABASE_URL`) are dropped (`php-fastcgi.md:100-101`: `env_clear` + prepared env). **Replacement:** "The suite gets `BLASTIT_URL` and the app's prepared environment; other shell variables are not passed."

### m11 — "the PHP `--learn` write" is ambiguous

`docs/frameworks/php.md:4-5` — "(`0.2.1` adds it, with the PHP `--learn` write and `check -- <suite>`.)". **Gets wrong:** the agent expects `--learn` to generate `[route.fastcgi]`; it writes `[route.static]` only (`php-fastcgi.md:1431-1438`). **Replacement:** "the `--learn` static-root proposal for a PHP app".

### m12 — "Find the asset paths" finds a root, not paths

`docs/frameworks/php.md:95` — "## Find the asset paths". **Gets wrong:** the agent expects path discovery; the section teaches the static root. **Replacement:** "Find the static root" or "Teach Blastit where the assets live".

### m13 — `(1)` is ambiguous

`docs/frameworks/php.md:123` — "is a finding (`1`)". **Gets wrong:** the agent reads it as "one finding", not the exit code. **Replacement:** "is a finding (exit `1`)".

### m14 — the deployment image and Symfony's `var/` ownership are unstated

`docs/frameworks/php.md:142` — "each route's PHP runs as that route's uid, with nothing shared between routes." **Gets wrong:** the agent builds an image without `php-fpm`, or deploys and gets a 500 because `var/cache` and `var/log` are not writable by the route uid. **Replacement:** "The image must contain `php-fpm`; give each route's uid write access to its `var/` (or warm the cache at build)."

### m15 — "one command" in CI skips `composer install`

`docs/frameworks/php.md:109` — "Your image has PHP, Composer and Blastit; one command starts the app…". **Gets wrong:** the agent expects `blastit check` to install `vendor/`. **Replacement:** "with `vendor/` installed (`composer install`), one command…".

### m16 — `--learn`'s example path is not Encore's, and the precondition is unstated

`docs/frameworks/php.md:104` — "Request any file under `public/assets/`; Ctrl-C writes `[route.static] root = "public"`." **Gets wrong:** a Symfony/Encore app's files are under `public/build/`; an agent requests `/assets/...`, gets no proposal, and does not know learn only proposes when the URL maps to a real file (`php-fastcgi.md:1435-1438`). **Replacement:** "Request a URL that maps to a real file under your static root (`public/assets/` or `public/build/`); Ctrl-C writes `root`. It proposes `root` only — add `immutable` yourself."

### m17 — the title names the mechanism under re-decision

`docs/frameworks/php.md:1` — "# PHP-FPM, Symfony and Laravel". **Gets wrong:** if libphp wins, the title is false before the reader reaches a sentence. **Replacement:** "# PHP, Symfony and Laravel" until the mechanism decision lands.

### m18 — "starts PHP … stops it" is a process-model claim

`docs/frameworks/php.md:34` — "Blastit starts PHP from `entry` and stops it when `blastit dev` exits (Ctrl-C)." **Gets wrong:** under libphp, Blastit does not start a separate PHP process; it loads the SAPI into the route's process. **Replacement:** "Blastit runs the route's front controller from `entry` and tears it down when `blastit dev` exits."

### m19 — the `immutable` example is still `/assets/*`, and `*` does not cross `/`

`docs/frameworks/php.md:18` and `:71-72` — the example uses `/assets/*`; the prose correctly adds "or `/build/*` for Encore's `public/build/`". **Gets wrong:** a Symfony reader copies the example, gets no match, and no warning; a nested `public/build/images/x.png` needs a second glob because `*` stops at `/` (`manifest.md:131`). **Replacement:** show `/build/*` in a Symfony variant (or make the example framework-neutral), and add "`*` does not cross `/`; a glob that matches nothing is not an error — check the `Cache-Control` header on one real file."

---

## 5. NIT

### n1 — whether a PHP edit is picked up is unstated

`docs/frameworks/php.md:105` — "restart `blastit dev` (the manifest is read once)". **Gets wrong:** the agent knows to restart for a manifest change but not whether a PHP edit needs one. **Replacement:** state it (and pin `opcache.validate_timestamps` in the pool) or drop the implication.

### n2 — the first `curl` body is not labelled as the demo app

`docs/frameworks/php.md:61-68` — "A front controller that answers `/users/42`:" then `{"route":"user.show","id":"42"}`. **Gets wrong:** the agent may think Blastit injects `route` into JSON. **Replacement:** label it "the demo app" like the home-route example at `:82`.

### n3 — the banner's `dbproxy` looks like a configured proxy

`docs/frameworks/php.md:53` — "http · scheduler · dbproxy · 1 route". **Gets wrong:** the agent may think it must declare a proxy. **Replacement:** no change needed if the banner is understood as a process list; a parenthetical would help.

---

## 6. Mechanism-neutrality audit

The page passes the hard part: it contains **no** `pm.max_children`, no `listen`, no socket path, no per-route pool count, no pool file, and no `php-fpm -F -y`. `workers` is correctly a route cap, not a pool directive. That is exactly right for a page that must survive the spawn-vs-embed decision.

The sentences that would be false under libphp:

| Line | Text | Severity |
|---|---|---|
| `:1` | "# PHP-FPM, Symfony and Laravel" | m17 |
| `:57-59` | "PHP it cannot find … `BLASTIT_PHP_FPM=…`" | M1 |
| `:34` | "starts PHP … stops it" | m18 |

Everything else is mechanism-neutral: "one command, no server to start beside it" (`:56`), "a query string is data, never PHP options" (`:82`), "a hostile request parameter is refused" (`:92`), "each route's PHP runs as that route's uid" (`:142`), and the `?-d` black-box test (`:85`) all hold under either mechanism. The `:92` rewrite from "FastCGI param" to "request parameter" is the right move — keep it.

---

## 7. Reference disagreements (reported, not assumed)

1. **`app`** — `manifest.md:13` says "required when any route is not static"; `php.md:26` says "needs no `app`"; the design's reconciliation (`php-fastcgi.md:595-601`) says "accepted and ignored". The page is right; the reference is stale. **Counted as M5.**
2. **No `[route.fastcgi]` section** — `manifest.md` route table `:68-78` and static section `:108-145` do not document the key the page is about. **Counted as M8.**
3. **`BLASTIT_PHP_FPM`** — `settings.md:26` correctly owns the row ("environment-only, never a manifest key"); the page restates it as a knob. **Counted as M1.**
4. **`--learn`** — `php.md:4` "the PHP `--learn` write" versus `php-fastcgi.md:1431-1438` (static root only, no PHP-specific line). **Counted as m11.**
5. **Discovery/error contract** — `php.md:57` promises a `PATH` remedy; the design's amendment banner defers "the discovery order, cleanup and the error contract" to `MB.3.9.3` (`php-fastcgi.md:34-36`). **Counted as M1.**

Positives that match the design exactly: the `--json` document (`php.md:131` = `php-fastcgi.md:1460`), the three-line banner (`php.md:51-53` = `php-fastcgi.md:72-75`), and the `check` summary line (`php.md:114` = `php-fastcgi.md:62`). Do not regress these.

---

## 8. The owner's two questions

**"Is anything here a knob they should not own?"** Yes, one: `BLASTIT_PHP_FPM` (M1). It is env-only, reference-only, and mechanism-specific; the page should link, not assign. Everything else on the page is legitimately the developer's: `entry`, optional `workers`, `root`, `immutable`. `workers` is correctly marked "omit it".

**"Or a step they should not take?"** Three temptations, all from omissions rather than bad instructions:

- adding `raw` + `allow_raw` to the catch-all to make an upload work (M7);
- copying `[route.static]` unconditionally when the app has no built assets (m1);
- copying `/assets/*` for a Symfony/Encore app, which validates and silently caches nothing (m19).

The page correctly does **not** ask for `sudo`, `/etc`, a hand-written pool file, a `listen` socket, or a `php-fpm` process to start beside it. That part of the DX is right.

---

## 9. Smallest set of edits that would flip this to PASS

1. Fix B1: show `body = "…"` and the split rule.
2. Replace the `BLASTIT_PHP_FPM` assignment with a link (M1).
3. Add three sentences: what `blastit dev` does not do (M3), one bad-manifest error + `503`/`502` (M4), and one Doctrine `[proxy.*]` line (M6).
4. Add the multipart/`raw` warning (M7).
5. Reconcile `manifest.md:13` and add the `## FastCGI` section (M5, M8).

---

**Round-1 verdict (superseded): FAIL** — 1 BLOCKER, 8 MAJOR, 19 MINOR, 3 NIT. The BLOCKER is the missing ingress contract (B1): the canonical manifest 413s every POST, and the page never shows the key or the one-codec-per-route split. The MAJORs are the FPM-specific `BLASTIT_PHP_FPM` remedy (M1), the unverifiable PHP version (M2), the missing "what `blastit dev` does not do" (M3), the missing failure path (M4), the stale `app`/`[route.fastcgi]` reference (M5, M8), the missing Doctrine proxy (M6), and the `raw` temptation (M7).

---

VERDICT: FAIL — 0 blocker, 1 major, 13 minor, 3 nit
