# DX review — `docs/frameworks/php.md` (mechanism-neutral rewrite)

## Round 3 — verification of D18–D28 (page still 149 lines)

Checked against the text, not the fix list: `docs/frameworks/php.md` (149),
`docs/reference/settings.md:22-26,51-56`, `docs/reference/manifest.md:68-106,205-231,308`,
`docs/getting-started.md:34-36`, `docs/frameworks/index.md:54-61`,
`docs/design/php-fastcgi.md:62,146,154,157,1964`, `docs/ci.md` headings.

| Id | Status | Covered by |
|---|---|---|
| D18 | resolved | `:50` "Blastit runs PHP 8.2 or newer for the route"; `:69` "A boot that cannot use PHP stops before serving and says why"; `:146` "The image needs the PHP Blastit runs"; `settings.md:26` "the PHP Blastit runs for a `[route.fastcgi]` route". Name and discovery default unchanged, as mandated. |
| D19 | resolved | `:42-44`: PostgreSQL only, top-level `[proxy.db]` with `protocol`/`input`, own `[route.db]` grant; matches `manifest.md:153-156,217,308`. |
| D20 | resolved | `:146-147`: uid `21000` (`settings.md:55` = `20000+1000+0`), Symfony `var/`, Laravel `storage/` + `bootstrap/cache/`; the `chown` itself is the deploy page's. |
| D21 | partially | `:37-38` now has `name`, `path`, `[route.fastcgi] entry`, `body = "json"` — but the ordering hint is gone (N1) and `body` is bare, not `[route.ingress] body`, so a copy writes an unknown route key (loud boot error; the old MINOR). |
| D22/D25 | resolved | `:121` "`check` exits `2` when no command follows it"; "not pytest" gone; matches design `:157,1964`. |
| D23 | resolved | `:124` Laravel `Http::get(getenv('BLASTIT_URL').'/users/42')`. |
| D24 | resolved | `:93` "the front controller's `/` route". |
| D26 | resolved | `:67` "run `composer install` yourself". |
| D27 | resolved | `:33` "caps this route at — never above"; design `:1875` accepts `workers = 8` at budget `8`. |
| D28 | resolved | `:44-45` splits `dev --learn` (proposes `root`) from `check --learn` (writes queries); `:114` "Bootstrap once, never as the gate" labels the block. |

Carried, unchanged: D4 (`:4`), D9 (`:87`), D12 (`:93-94`), D17 (`:135`).

### N1. The second route has no ordering instruction, and the page's own route is a catch-all [MAJOR]

Where: `docs/frameworks/php.md:36-38`; `docs/reference/manifest.md:308`;
`docs/getting-started.md:34-36`; `docs/frameworks/index.md:56-58`

> A route takes one codec, so a JSON API beside the form app is a second `[[route]]`
> with its own `name`, `path`, `[route.fastcgi] entry` and `body = "json"` — first
> match wins.

What a reader does wrong: routes are tried in manifest order and the first match wins
(`manifest.md:308`), and the page's only route is `path = ['.*']` (`:13`). The sentence
says "second", so the reader appends the API route after the catch-all — and the
catch-all always matches first. `POST /api/...` then reaches the form route, whose
codec answers `400` (`:39`), or the front controller answers it. No boot error, no
finding, and `check` stays green if the suite never calls `/api`. The rule the rest of
the docs teach is "the first match wins, so a catch-all goes last"
(`getting-started.md:35-36`, `frameworks/index.md:57-58`); this page shows the
catch-all first and never says the new route goes above it. Round 2's quoted text had
"a second route above this one"; the fix dropped the only ordering hint.

Fix (paid for by the existing clause): "…and `[route.ingress] body = "json"`, above the
catch-all — first match wins." This also removes the bare `body` of D21's remainder.

VERDICT: FAIL — 0 blocker, 1 major.

---

## Round 2 — current page (149 lines) and the `BLASTIT_PHP_FPM` row

Reviewed: `docs/frameworks/php.md` as it stands (149 lines),
`docs/reference/settings.md:22-26` (§Backends),
`docs/design/php-fastcgi.md` §1.1–§1.4 and the `MB.3.9.2` banner,
`docs/reference/manifest.md` §Proxies/§Static, `docs/ci.md`, `docs/local.md`,
`docs/frameworks/static-and-spa.md`, `docs/VISION.md`. Mechanism test: every
sentence must be true under both `php-fpm` spawn and embedded libphp; a sentence
false under libphp is a defect.

### The owner's test, counted

| Environment | Steps | Reader writes | Blastit derives |
|---|---|---|---|
| Local | 3 — `uv tool install blastit`; `blastit.toml`; `blastit dev` | `entry`, `root`, `immutable`; optional `workers` | PHP lifecycle, concurrency, socket, `SCRIPT_FILENAME`, `.php` refusal, cache headers |
| CI/CD | 1 — `blastit check -- vendor/bin/phpunit` | nothing new (the suite must speak HTTP) | loopback, `BLASTIT_URL`, observation, findings, exit code |
| Deploy | 2 — `blastit --preset compose-traefik`; `chown -R 21000 var/` | the uid (`21000`) and the chown | per-route uid, fail-closed |

Three local steps, zero `sudo`, no `/etc`, one CI command. The page no longer owns
a pool knob; `workers` is "optional; omit it". The deploy count is now two because
`:146` requires a `chown` the page does not show. The remaining defects are the PHP
prerequisite (mechanism-specific), the DB paragraph (wrong table, false protocols),
and the deploy `chown` (no uid).

### Round-1 status

| Id | Status | Note |
|---|---|---|
| D1 | resolved | `:123-124` names `KernelBrowser`, `$this->get()`, `HttpBrowser`, `file_get_contents`. |
| D2 | carried → D18 | `:49` now says "on the machine (or in the container)", but the requirement and `:68-69` are spawn-specific. |
| D3 | carried — external | `docs/ci.md:9-18,98-111` still documents pytest; see external list. |
| D4 | carried | `:4` still a version note before the example; now one clause. |
| D5 | carried — external | H1/nav still "PHP-FPM"; see external list. |
| D6 | resolved | "its own executor"/"supervises" gone; `:28`, `:64`. |
| D7 | resolved | "nothing left running after Ctrl-C" (`:65`). |
| D8 | resolved | the Local learn-static-root paragraph is gone; the remaining `--learn` sentence is D28. |
| D9 | carried | `:86` still curls a fixture hash; Encore's `public/build/` is named (`:81-83`). |
| D10 | resolved | `:32` says "relative to `blastit.toml` ... `public/index.php`, never `index.php`". |
| D11 | resolved | "Nothing else to declare" is gone; `:43-45` is the DB paragraph (D19). |
| D12 | carried | `:102` "refused, not forwarded" is still a wire-level claim; see D12 below. |
| D13 | resolved | `:71` "With a front controller that answers `/users/42`". |
| D14 | resolved (NIT) | `:33` "can only lower" should be "at or below"; see D27. |
| D15 | resolved | `:50-51` "it reads `./blastit.toml` from the directory you run it in". |
| D16 | carried — external | `docs/reference/manifest.md` has no `[route.fastcgi]`; see external list. |
| D17 | carried | `:135` still one ~330-character line; see D17 below. |

### New findings

#### D18. The PHP prerequisite and the boot error are spawn-specific [MAJOR]

Where: `docs/frameworks/php.md:49-50`, `:68-69`, `:146`; `docs/reference/settings.md:26`

> PHP 8.2 or newer on the machine (or in the container) that runs Blastit, and Blastit
> installed once.

> PHP it cannot find stops the boot, naming what it looked for
> ([settings](../reference/settings.md#backends)).

> Your app image must contain PHP, and each route's uid needs write access to its `var/`.

> `BLASTIT_PHP_FPM` | `php-fpm`, then versioned names on `PATH` | the PHP binary Blastit
> starts for a `[route.fastcgi]` route; environment-only, never a manifest key

What a reader does wrong: under embedded libphp there is no PHP on the machine to
find and no binary to start; `:49` sends them to install PHP for nothing, `:68-69`
promises a boot error that cannot occur, and the settings row is false. Under spawn,
the common Docker Compose Symfony setup (PHP in the app container, none on the host)
has no one-step path: the host has no PHP, and the app container has no Blastit. A
machine with `php` 8.3 and `php-fpm` 8.1 passes `:49` and fails at the first request,
because the page never says which binary it means. `:146`'s "must contain PHP" is not
the binary the row names (`php-fpm`), so an image with the `php` CLI and no `php-fpm`
boots and fails.

Fix: make the page's prerequisite neutral and let the reference row own the
mechanism: "Blastit runs PHP 8.2 or newer for the route; the [backends
reference](../reference/settings.md#backends) names the PHP it uses." Replace
`:68-69` with "A boot that cannot use PHP stops before serving and says why." Replace
`:146` with "The image needs the PHP Blastit runs (see
[settings](../reference/settings.md#backends))." In the row, replace "the PHP binary
Blastit starts" with "the PHP runtime Blastit uses for a `[route.fastcgi]` route" and
move the `php-fpm`/versioned-name discovery order to the mechanism-specific design.
If libphp wins, the variable and the row go away; the page must not depend on them.

#### D19. The DB paragraph names the wrong table and promises MySQL/Redis [MAJOR]

Where: `docs/frameworks/php.md:43-44`

> A route that talks to PostgreSQL, MySQL or Redis declares a `[proxy.<name>]`; with
> none it gets no connection.

What a reader does wrong: (a) `[proxy.<name>]` is top-level, not a route table; a
route declares `[route.<name>]` (`[route.db]`), so a reader who writes `[proxy.db]`
inside `[[route]]` gets an unknown-key boot error. (b) MySQL and Redis proxies do not
exist in 0.2.1 (`crates/blastit-manifest/src/model.rs:69-87` has `postgresql` only;
`docs/reference/manifest.md` documents only `postgresql`), so a MySQL/Redis reader
gets an unknown-protocol boot error. (c) A top-level `[proxy.db]` alone is not
enough: a route with no `[route.db]` gets no listener (`docs/local.md:75`), and
`--learn` writes the queries but not the proxy's `input` names (`docs/ci.md:117`). A
Doctrine reader follows the sentence, runs `--learn`, and the app's first connection
still fails.

Fix: "A route that talks to PostgreSQL needs the top-level `[proxy.db]`
(`protocol = "postgresql"`, `input = { url = "DATABASE_URL" }`) and its own
`[route.db]` grant; with no `[route.db]` it gets no listener. `--learn` writes the
queries you exercise; the proxy's `input` names are yours." Drop MySQL/Redis until
they ship.

#### D20. The deploy `chown` has no uid and no command [MAJOR]

Where: `docs/frameworks/php.md:146`

> Your app image must contain PHP, and each route's uid needs write access to its `var/`.

What a reader does wrong: the reader knows *that* the route uid needs `var/`, not
*which* uid or *how*. The uid is `BLASTIT_UID_BASE + 1000 + i` (`21000` for the first
route, `docs/reference/settings.md:55`), and the page never gives it or a `chown`. A
Laravel reader has no `var/` at all: Laravel's writable directories are `storage/`
and `bootstrap/cache/`. The first deploy is a 500 (Symfony cannot write
`var/cache`), and the page's only deploy sentence does not fix it.

Fix: "The image needs the PHP Blastit runs (see
[settings](../reference/settings.md#backends)); each route's uid (`21000` for the
first) needs write access to the app's writable directories — Symfony's `var/`,
Laravel's `storage/` and `bootstrap/cache/`. `chown -R 21000 var/` in the image
build."

#### D21. The second-route asides are not valid TOML [MINOR]

Where: `docs/frameworks/php.md:37-41`

> A route takes one codec, so a JSON API beside the form app is a second route above
> this one (`path = ['/api/.*']`, `body = "json"`, the same `entry`) — the first match
> wins.

> A multipart upload has no codec yet: `raw` with `allow_raw = true` on that path only,
> never the catch-all — the loud opt-out.

What a reader does wrong: the parenthetical omits `name` (required) and the route's
own `[route.fastcgi]`; "the same `entry`" is prose, not TOML. A reader who copies it
writes a route with no name or no executor and gets a boot error. The upload aside
has the same gap: "on that path only" needs a second `[[route]]` with `name`, `path`,
`[route.fastcgi]`, and `[route.ingress]`, which the page never shows.

Fix: show the 6-line second route once (same block with `name`, `path`,
`[route.fastcgi] entry`, `[route.ingress] body`), or say "a second `[[route]]` with
its own `name`, `[route.fastcgi] entry`, and `[route.ingress]`". The upload aside can
point at that block with `body = "raw"`, `allow_raw = true`.

#### D22. "`check` exits `2` without it" is ambiguous [MINOR]

Where: `docs/frameworks/php.md:122-123`

> ... for a FastCGI manifest that is your suite, not pytest, and `check` exits `2`
> without it.

What a reader does wrong: the nearest noun is "pytest", so "without it" reads as
"without pytest"; the page's command has no pytest, so the reader may install pytest
or think `check` will fail. The intended meaning is "without a command after `--`"
(`docs/design/php-fastcgi.md:1826`: `check` with no `--` exits `2`).

Fix: "`check` exits `2` when no command follows `--`." (And delete "not pytest"; see
D25.)

#### D23. The HTTP-client remedy is Symfony-only [MINOR]

Where: `docs/frameworks/php.md:123-125`

> ... so use `HttpBrowser` or
> `file_get_contents(getenv('BLASTIT_URL').'/users/42')`.

What a reader does wrong: `HttpBrowser` is Symfony's BrowserKit class; a Laravel
reader looks for it in Laravel and does not find it. Laravel's HTTP-speaking
equivalent is `Http::get(getenv('BLASTIT_URL').'/users/42')` (or
`file_get_contents`). The page names both frameworks in the same sentence but gives
only one framework's client.

Fix: "use Symfony's `HttpBrowser`, Laravel's `Http::get()`, or
`file_get_contents(...)`".

#### D24. "The home route" reads as a Blastit route [MINOR]

Where: `docs/frameworks/php.md:92`

> A query string is data, never PHP options. The home route echoes what PHP saw:

What a reader does wrong: "the home route" is the app's `/` route, but the response
`{"route":"home",...}` looks like a manifest route name, and the page's manifest has
one route named `app`. A reader may add `[[route]] name = "home"` — a knob they
should not own — or wonder why their `app` route does not echo this. The same shape
appears at `:78` (`{"route":"user.show",...}`), but there the page says "With a front
controller that answers `/users/42`", which disambiguates; `:92` does not.

Fix: "The front controller's `/` route echoes what PHP saw:".

#### D25. "not pytest" is noise for a PHP reader [NIT]

Where: `docs/frameworks/php.md:122`

> ... for a FastCGI manifest that is your suite, not pytest ...

What a reader does wrong: nothing breaks, but the clause invites a question the page
never answers ("do I need pytest?"), and the page's audience is PHP. The contrast
belongs in `docs/ci.md`, not here.

Fix: delete "not pytest".

#### D26. "your app sets itself up" is muddled [NIT]

Where: `docs/frameworks/php.md:65-66`

> It does not install dependencies, migrate, warm caches, read `.env` or check your PHP
> version: your app sets itself up.

What a reader does wrong: "your app sets itself up" is meant to cover
dependencies/migrations/caches, but it sits after "does not check your PHP version",
so it reads as "your app checks the PHP version" (Symfony's Composer platform check
does, but that is not the point). The reader cannot tell whether they must run
`composer install` or not.

Fix: "It does not install dependencies, migrate, warm caches or read `.env`; run
`composer install` yourself. It does not check your PHP version either."

#### D27. `workers` "can only lower" is imprecise [NIT]

Where: `docs/frameworks/php.md:33`

> optional; omit it. It can only lower this route's concurrency, and a value above the
> budget Blastit derives is a boot error

What a reader does wrong: an explicit value equal to the derived budget is legal
(`docs/design/php-fastcgi.md:31-33`), but "can only lower" implies strict inequality;
a reader who wants the default number may think `workers = 16` is refused. The
boot-error clause is correct; the first clause is not.

Fix: "optional; omit it. It caps this route at — never above — the concurrency Blastit
derives; a value above the budget is a boot error."

#### D28. The `--learn` paragraph conflates `dev` and `check`, and its command sits under CI/CD [MINOR]

Where: `docs/frameworks/php.md:44-45`, `:115-119`

> `--learn` edits `blastit.toml` in place: it writes the grants you exercised and
> proposes a missing `root`. Review the diff, then run without it.

> Bootstrapping an existing app, `--learn` records instead of denying:
>
> ```console
> $ blastit check --learn -- vendor/bin/phpunit
> ```

What a reader does wrong: (a) the `root` proposal is a `dev --learn` feature
(interactive click-through); `check --learn` runs the suite and writes queries, and
the design does not pass `--learn` to the child `dev`
(`docs/design/php-fastcgi.md:121-122`), so a reader who runs `check --learn` on an app
with no `[route.static]` gets no `root` proposal. (b) The `check --learn` command sits
under `## CI/CD`; `:45` only says "Review the diff, then run without it", which does
not tell a CI author that the block is one-time. A reader may add it to the workflow,
and every CI run rewrites `blastit.toml` and records instead of denying, so the gate
passes with missing grants. The block is a one-time bootstrap, not a CI step.

Fix: "`blastit dev --learn` proposes a missing `root`; `check --learn` writes the
queries you exercised. Review the diff, then run `check` without `--learn`; never use
`--learn` as the CI gate." Move the `check --learn` block to a "Bootstrap" subsection
under Local (or label it "one-time, not CI").

### Carried round-1 findings that still stand

- **D4** (`:4`): "`0.2.1` adds it, with `check -- <suite>`." The outcome now leads,
  but this is still a version note before the example. Delete it or move it after the
  example. [MINOR]
- **D9** (`:86`): the curl still uses `app.4f1c2a9b7d3e.css`, a hash the reader does
  not have; Encore's `public/build/` is now named, but the "it works" curl is still
  unreproducible. Use "any file `ls public/assets` prints". [MINOR]
- **D12** (`:102`): "A hostile request parameter is refused, not forwarded" is still
  a wire-level claim; under libphp there is no forwarding. Replace with "A query
  string cannot become a PHP option, and only `entry` runs." [MINOR]
- **D17** (`:135`): the `--json` document is still one ~330-character line;
  pretty-print it or show only `routes` and `findings`. [NIT]

### External (recorded, not counted)

- `docs/ci.md:9-18,98-111` still says `check` runs `pytest` and `--` feeds pytest;
  the page's `check -- <suite>` is correct. (D3)
- `docs/reference/manifest.md` has no `[route.fastcgi]` section or row; the page's
  `entry`/`workers` table and footer link have no reference home. (D16)
- `docs/frameworks/index.md:8,71` and the page H1 say "PHP-FPM"; if the mechanism
  flips to libphp, the title is false. (D5)
- `docs/frameworks/static-and-spa.md:121-122` says "A PHP app is this row with
  `public/index.php` as its entry script" — `[route.static]` has no `entry` key; the
  PHP page's `[route.fastcgi]` is the real block. A reader following the footer link
  hits a contradiction. (not counted)

VERDICT: FAIL — 0 BLOCKER, 3 MAJOR, 8 MINOR, 4 NIT

---

## Round 1 (kept for history)

Lens: `dx`. Target: the rewritten page (`MB.3.9.1`), judged against the fixed
constraints: one-line `[route.fastcgi]` (`entry`, optional `workers`), Local =
write the manifest + `blastit dev`, `BLASTIT_PHP_FPM` env-only, <150 lines,
mechanism-neutral. A mechanism-neutrality defect is a DX defect here: every
sentence that commits to a pool is a sentence the reader must unlearn within days.

Read for this review: the page, `docs/reference/settings.md` §Backends,
`docs/design/php-fastcgi.md` §1.1–§1.4 and §4.1 (banner: `listen` gone, `workers`
optional, dev-pool lifecycle re-decided), `docs/frameworks/django.md`,
`docs/frameworks/static-and-spa.md`, and the pages the target links
(`docs/ci.md`, `docs/reference/manifest.md`, `docs/local.md`).

## The owner's test, counted

| Environment | Steps | Reader writes | Blastit derives |
|---|---|---|---|
| Local | 3 — `uv tool install blastit`; `blastit.toml`; `blastit dev` | `entry`, `root`, `immutable`; optional `workers`, optional `BLASTIT_PHP_FPM` | PHP start/stop, concurrency, socket, `SCRIPT_FILENAME`, `.php` refusal, cache headers |
| CI/CD | 1 — `blastit check -- vendor/bin/phpunit` | nothing new (the suite must speak HTTP) | loopback, `BLASTIT_URL`, observation, findings, exit code |
| Deploy | 1 — `blastit --preset compose-traefik` | nothing | per-route uid, fail-closed |

The count passes: three local steps, zero `sudo`, no `/etc`, no pool file, one
command in CI and one in deploy. The one knob a reader might wrongly own
(`workers`) is labelled "usually omit". The findings below are the places where
the reader still cannot tell what to write, or what just happened.

## Findings

### D1. The named suite is in-process by default in both frameworks, and the page does not say so [MAJOR]

Where: `docs/frameworks/php.md:108–113`

> `check` puts the front controller on loopback as `BLASTIT_URL=http://127.0.0.1:8080`
> and runs the command after `--`: for a FastCGI manifest that is the suite, not pytest.
> The suite reads `BLASTIT_URL`, never the app; a test that does
> `file_get_contents(getenv('BLASTIT_URL').'/users/42')` speaks HTTP; an in-process test
> sends nothing.

What a reader does wrong: a Symfony reader's first suite is `WebTestCase` /
`KernelBrowser`; a Laravel reader's is `$this->get()`. Both call the kernel
in-process, so `public/index.php` never runs and the page's own rule — "A route is
exercised when its front controller ran" — reports `0 requests, 0/1 routes`, exit
`1`. They have a green local app and a red CI gate. The one clause that explains it
("an in-process test sends nothing") does not name their test client, so it reads
as a note about someone else's suite.

Fix: name the default and the switch in two lines: "Symfony's `KernelBrowser` and
Laravel's `$this->get()` run in-process and send nothing; use `HttpBrowser` (or
`file_get_contents`) against `BLASTIT_URL`." No new mechanism, no new config.

### D2. PHP must be on the machine that runs `blastit dev`, and the failure is never shown [MAJOR]

Where: `docs/frameworks/php.md:35`, `:49–52`

> PHP 8.2 or newer; install Blastit once.

> Blastit starts and supervises PHP for the route itself: one command, no server to
> start beside it, nothing left running afterwards. PHP it cannot find on `PATH` is
> one variable — `BLASTIT_PHP_FPM=/opt/php/bin/php-fpm`
> ([settings](../reference/settings.md#backends)).

What a reader does wrong: the common Symfony setup is Docker Compose — PHP lives
in the app container, the host has none. "PHP 8.2 or newer" reads as satisfied
("my app runs PHP 8.3"), yet `blastit dev` on the host cannot start it. The only
remedy shown is a path the reader does not have, and the page never shows the boot
line that would connect "I have PHP" to "not found".

Fix: "Blastit starts PHP on the machine (or container) that runs `blastit dev`; if
boot says `php: not found`, point `BLASTIT_PHP_FPM` at it." Show the one-line
error. This stays neutral — no pool words, no package names.

### D3. The linked CI page still says `check` runs pytest [MAJOR]

Where: `docs/frameworks/php.md:108–109`, `:124`; `docs/ci.md:100–111`, `:98`

> for a FastCGI manifest that is the suite, not pytest

`docs/ci.md` (linked at line 124):

> With an `app`, `blastit check` runs `pytest -q`; without `pytest` it exits `2`
> (`uv add --dev pytest`). … Arguments after `--` go to pytest:

> `2` could not start (manifest, config, a static root, no `pytest`)

What a reader does wrong: the page's command is right, but the page it sends the
reader to says the opposite — that `--` feeds pytest and that `2` means "no
pytest". A reader who reads CI first writes `blastit check -- -k …` or installs
pytest for a manifest that has no `app`. `docs/reference/cli.md` is silent on
`-- <suite>` too.

Fix: release gate — `docs/ci.md` §Your suite gains the FastCGI branch (no `app` →
the command after `--` is the suite; `2` = manifest/entry/workers, not pytest) and
`cli.md`'s `check` section names `-- <suite>`, before 0.2.1. Until then the page is
the only correct statement in the docs set, which is a fragile place for it.

### D4. The page opens with a release note, not an outcome [MINOR]

Where: `docs/frameworks/php.md:3`

> `0.2.1` adds `[route.fastcgi]`, the PHP `--learn` write and `check -- <suite>`.

What a reader does wrong: the first line is a changelog entry with two internal
nouns ("the PHP `--learn` write", "`check -- <suite>`"). It answers "what changed?"
instead of "what do I get?", and a 0.2.0 reader learns nothing they can act on.

Fix: open with the outcome — "Serve a Symfony or Laravel app from
`public/index.php` with one manifest block. No nginx, no rewrite rules." Move the
version note to one line after the example, or drop it.

### D5. The H1 commits the page to PHP-FPM [MINOR]

Where: `docs/frameworks/php.md:1` (and the link text at `docs/frameworks/index.md:8,71`)

> # PHP-FPM, Symfony and Laravel

What a reader does wrong: nothing yet — but if the mechanism flips to embedded
libphp, the title is false, and it is the most visible sentence on the page. (It is
a SAPI name, not a pool, and `docs/VISION.md` also says "PHP-FPM / FastCGI"; that
is why this is MINOR, not MAJOR.)

Fix: "PHP, Symfony and Laravel" in the H1 and the two index links. One word, and
the page survives either decision.

### D6. "its own executor" and "supervises" are pool vocabulary [MINOR]

Where: `docs/frameworks/php.md:22`, `:49`

> A FastCGI route is its own executor: no `app`, and `[route.ingress]` applies to it.

> Blastit starts and supervises PHP for the route itself

What a reader does wrong: "executor" is an `ARCHITECTURE.md` noun; the reader
cannot tell whether "no `app`" is an instruction (delete `app`) or a statement
(you need none). "Supervises" implies a master/worker pool — the exact mechanism
under re-decision.

Fix: "A `[route.fastcgi]` route needs no `app`: the front controller is the app,
and `[route.ingress]` applies to it." / "Blastit runs PHP for the route itself:
one command, no server beside it, nothing left running afterwards."

### D7. "stops it with `blastit dev`" [MINOR]

Where: `docs/frameworks/php.md:30`

> Blastit starts PHP for the route from `entry` and stops it with `blastit dev`.

What a reader does wrong: reads it as "running `blastit dev` stops PHP", then
looks for a stop command that does not exist.

Fix: "… and stops it when `blastit dev` exits (Ctrl-C)."

### D8. The learn loop edits the manifest but never says to restart [MINOR]

Where: `docs/frameworks/php.md:88–96`; `docs/local.md:23`

> Ctrl-C writes `[route.static] root = "public"`. Add `immutable`.

`docs/local.md:23`: "The manifest is read once. Restart after editing a route or a
grant."

What a reader does wrong: adds `immutable`, curls the asset again in the same
session, gets `no-cache`, and concludes the glob is wrong.

Fix: one clause — "restart `blastit dev` (the manifest is read once) and curl
again."

### D9. "Find the asset paths" finds `root`, not the glob — and uses a hash the reader does not have [MINOR]

Where: `docs/frameworks/php.md:62–64`, `:86–96`

> Request `/assets/app.4f1c2a9b7d3e.css`; Ctrl-C writes
> `[route.static] root = "public"`. Add `immutable`.

What a reader does wrong: the section title promises the asset paths; learn writes
only `root`, and `immutable` — the one value that differs per framework
(`public/assets/` with AssetMapper, `public/build/` with Encore) — stays manual.
The example hash is a fixture; the reader's file has another name, so the same
curl is a front-controller 404 in their tree (and the Local demo at line 67 is
unreproducible for the same reason).

Fix: "request any file `ls public/assets` prints", plus one clause: "Encore writes
`public/build/` — use `immutable = ["/build/*"]`."

### D10. The `entry` row reads as "relative to `root`" [MINOR]

Where: `docs/frameworks/php.md:26`

> required; the front controller, relative to `blastit.toml` (under `root` when both are set)

What a reader does wrong: with `root = "public"`, writes `entry = "index.php"`
because the parenthetical reads as a base, not a constraint. Boot refuses with
`fastcgi entry "index.php": must be under static root "public"` (design §4.1)
while the file exists at `public/index.php`.

Fix: "relative to `blastit.toml`; with `[route.static]` the path includes `root` —
`entry = "public/index.php"`, never `"index.php"`."

### D11. "Nothing else to declare" reads as "the manifest is complete" [MINOR]

Where: `docs/frameworks/php.md:29`

> Nothing else to declare: Blastit starts PHP for the route from `entry` and stops
> it with `blastit dev`.

What a reader does wrong: the sentence is scoped to the route's PHP lifecycle in
the author's head, and read as the whole manifest by a Doctrine/Laravel reader,
whose first query is then denied. (B7's scope decision — DB grants belong to the
manifest page — stands; only the sentence over-claims.)

Fix: "Nothing else to declare **for this route**: …".

### D12. "A hostile FastCGI param" is a wire-level promise [MINOR]

Where: `docs/frameworks/php.md:83`

> A hostile FastCGI param is refused, not forwarded; `SCRIPT_FILENAME` is pinned to `entry`.

What a reader does wrong: it promises a defense inside the FastCGI session. If the
mechanism flips to embedded libphp there is no param stream and the sentence must
be deleted; the reader's durable takeaway is the demo above it — a query string
cannot become a PHP option.

Fix: "A query string cannot become a PHP option, and only `entry` runs. Mechanisms
in [security](../security.md) and [how it works](../how-it-works.md)."

### D13. The curl responses come from an app the page never shows [MINOR]

Where: `docs/frameworks/php.md:55–60`, `:76–81`

> `{"route":"user.show","id":"42"}`

> `{"route":"home","allow_url_include":"0","query_string":"-d+allow_url_include=1"}`

What a reader does wrong: curls `/users/42` and gets Symfony's own 404 HTML;
nothing says the JSON is the example front controller's. The page's first "it
works" moment is unreproducible.

Fix: one clause before the first curl — "with a front controller that answers
`/users/42`" — or show the three-line controller.

### D14. `workers` says "below" and never says when to touch it [MINOR]

Where: `docs/frameworks/php.md:27`

> optional, usually omit; caps this route below the concurrency Blastit derives

What a reader does wrong: "below" implies strictly less; an explicit value equal
to the derived cap is legal, and one above it is a boot error (design R4). The
reader who does touch it has no criterion for choosing a number.

Fix: "optional; omit it. Caps this route at — never above — the concurrency
Blastit derives; set it only when one route's PHP is memory-heavy."

### D15. Nothing says where to run `blastit dev` [MINOR]

Where: `docs/frameworks/php.md:43`; `docs/local.md:18`

> $ blastit dev

`docs/local.md:18`: "It reads `./blastit.toml` from the current directory (no
parent search)."

What a reader does wrong: runs it from `public/` and gets a "no manifest" error
with no hint.

Fix: "Run it from the directory that holds `blastit.toml` (no parent search)."

### D16. The footer's reference links have no FastCGI content yet [MINOR]

Where: `docs/frameworks/php.md:84`, `:135`; `docs/reference/manifest.md`,
`docs/security.md`, `docs/how-it-works.md`

> See [Deployments](../deploy/index.md) · [the manifest](../reference/manifest.md) ·
> [static assets](static-and-spa.md).

What a reader does wrong: clicks the manifest reference for the block and finds no
`[route.fastcgi]` row or section (grep: zero hits); `security.md` and
`how-it-works.md` have no PHP text either. The "read more" is a dead end at 0.2.1.

Fix: release gate — land design §4.1's `manifest.md` reconciliation (route row,
`## FastCGI` section, the `app` footnote) and the two mechanism paragraphs before
the page ships; `cli.md`'s `check` section should name `-- <suite>` too.

### D17. The `--json` document is one ~330-character line [NIT]

Where: `docs/frameworks/php.md:121`

> `{"schema_version":1,"ok":false,"requests":3,"statements":0,"routes":{"total":1,"exercised":0},"proxies":{},"findings":[{"kind":"unexercised_route","severity":"error","route":"app","fix":{"action":"add_test"}}]}`

What a reader does wrong: it wraps on every screen, and the one field they need
(`"exercised":0`) is hard to find.

Fix: pretty-print as `docs/ci.md:61–77` does, or show only the `routes` and
`findings` keys.

## Mechanism-neutrality audit

| Line | Sentence | Verdict |
|---|---|---|
| 1 | "PHP-FPM, Symfony and Laravel" | rewrite-if-flip (D5) |
| 22 | "its own executor" | survives both mechanisms (a route has its own process) but is internal jargon (D6) |
| 29 | "starts PHP … stops it with `blastit dev`" | safe; wording bug only (D7) |
| 49 | "starts and supervises PHP" | "supervises" is the pool word (D6) |
| 51 | "`BLASTIT_PHP_FPM=/opt/php/bin/php-fpm`" | fixed constraint; the prose around it is neutral (D2 is about the missing error, not the name) |
| 83 | "A hostile FastCGI param is refused" | rewrite-if-flip (D12) |
| 108 | "puts the front controller on loopback" | safe (both mechanisms serve HTTP) |
| 132 | "each route's PHP runs as that route's uid" | safe (per-route process either way) |

No `pm.max_children`, socket path, pool file, `listen`, or per-route pool appears
on the page. In substance the rewrite is neutral; only the H1 and line 83 carry
the old mechanism.

## Verdict

Three MAJORs: the CI paragraph does not name the framework's in-process test
client (D1), the PHP prerequisite never says "on the `blastit dev` machine" and
never shows the boot error (D2), and the linked CI page still documents pytest for
`--` (D3). Everything else is wording, one-clause fixes, or a release gate for
reference pages owned elsewhere. The manifest, the two-step local flow, the knob
discipline (`workers` "usually omit") and the deploy story are right.

VERDICT: FAIL — 0 BLOCKER, 3 MAJOR, 13 MINOR, 1 NIT.
