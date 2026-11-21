# MB.19 — Restructure `/frameworks`: one "adapting your app" page + nine capability slots

## Summary

`docs/frameworks/index.md` is **"Adapting your application"**: the nine capability slots,
each with what blastit needs and what breaks without it, a *"Where the pages differ"*
matrix (executor × static × database), and one canonical manifest shape that every page
copies. The four framework pages carry **the same nine headings, in the same order**
(md5-identical after stripping the two bridge anchors), saying only what is
framework-specific.

Attempt 2 fixes the reviewer's two MAJORs and three MINORs: the false "a static route
needs no process" claim is gone (a forwarding static route does get a process), the three
framework-independent passages are each single-homed in the intro, and the `allow_raw`,
PHP-privilege and trailing-newline defects are corrected.

Per-page line counts (`wc -l`; every file now ends with a newline, so editor lines and
`wc -l` agree). Budgets: intro ≤ 130, each page ≤ 120:

| Page | Lines | Console fences |
|---|---|---|
| `docs/frameworks/index.md` | 114 | 0 |
| `docs/frameworks/django.md` | 115 | 1 |
| `docs/frameworks/flask.md` | 112 | 1 |
| `docs/frameworks/php.md` | 103 | 3 |
| `docs/frameworks/static-and-spa.md` | 118 | 6 |

## Attempt 2 — reviewer findings and where each was fixed

| # | Finding | Fix |
|---|---|---|
| MAJOR 1 | `index.md` slot 9: "a static route needs no process" was false | Rewritten: *"One route is one uid: a route whose static responder forwards — to `app` or its own `[route.php]` — gets a process, while `spa = true` answers alone and needs none."* Verified against `route.rs:78-84` (`static_forward = policy && !spa && forwards`), `routes_from_manifest` (`forwards = has_app \|\| route.php().is_some()`) and `scheduler.rs:886-892` (`spawnable = static_policy.is_none() \|\| static_forward`). The same belief also hid in `django.md` slot 9 ("every route forks"), now "each view process starts with the URLconf already in memory". |
| MAJOR 2a | `check --learn` rule repeated on index + django + flask | Kept only in `index.md` slot 8. Django keeps the Django-specific half (`pytest-django` + `manage.py` → `test_<database>`, and pasting a reported statement into `queries`); flask keeps the framework-specific half (`app.py` importable from the project root). |
| MAJOR 2b | `BLASTIT_MAX_WORKERS` sizing repeated on index + django + flask | Kept only in `index.md` slot 9. Flask slot 9 is now only the catch-all/route-splitting fact; django slot 9 is only `preload`. |
| MAJOR 2c | migration rationale repeated on django + flask + php | Kept only in `index.md` slot 7 (own role, direct URL, `ALTER TABLE`). Each page keeps its own command plus "outside blastit". |
| MINOR | `allow_raw` implied to silence the boot warning | Slot 4 now: *"`raw` forwards the bytes unchanged and the boot warns either way; `allow_raw = true` is what stops `blastit check` reporting `raw_ingress`."* Matches `validate.rs:806-814` (unconditional `ManifestWarning::RawIngress`) and `audit_v1.rs:1057-1072` (finding gated on `!allow_raw`). |
| MINOR | `php.md` "the one privileged step on the host" | Now "the one privileged step **for PHP itself**": production `blastit` also starts as root or with capabilities. |
| MINOR | `index.md` "the same keys" untrue (django adds `preload`, php/static omit `app`) | Now "the same blocks in the same order". |
| MINOR | `django.md`/`static-and-spa.md` had no trailing newline (121/118 editor lines) | Newlines added; django also cut to 115 lines so every page is under its budget. |

## Evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.28s
```

```console
$ make docs-unit
............................................................................... [ 95%]
....                                                                     [100%]
83 passed, 65 subtests passed in 0.57s
```

```console
$ make docs-test            # builds the wheel, then runs every public fence
PASS  docs/frameworks/django.md  fence 1  check                 0.9s  check reported a clean suite
PASS  docs/frameworks/flask.md  fence 1  install-external      0.0s  accounted: Installs flask, psycopg and pytest …
PASS  docs/frameworks/php.md  fence 1  static                0.0s  statically checked every command, flag and snippet (…)
PASS  docs/frameworks/php.md  fence 2  static                0.0s  statically checked every command, flag and snippet (…)
PASS  docs/frameworks/php.md  fence 3  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/frameworks/static-and-spa.md  fence 3  dev-curl   2.1s  served and answered
PASS  docs/frameworks/static-and-spa.md  fence 4  dev-curl   2.1s  served and answered
49 fences: 29 executed, 20 accounted, 0 failed · 21.6s
```

The two `dev-curl` fences are executed coverage: the static page's `curl -i` transcripts
are replayed against a real `blastit dev` on the `static-site` fixture, so the status line
and both headers are asserted, not eyeballed.

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
test every_documented_manifest_parses_and_validates ... ok
test the_static_page_manifest_has_its_documented_shape ... ok
test a_drifted_static_block_does_not_satisfy_the_documented_shape ... ok
test result: ok. 5 passed; 0 failed
```

```console
$ uv run --no-project tools/guard.py
guard: OK (8 staged/untracked file(s) checked)
```

```console
$ wc -l docs/frameworks/*.md
  115 docs/frameworks/django.md
  112 docs/frameworks/flask.md
  114 docs/frameworks/index.md
  103 docs/frameworks/php.md
  118 docs/frameworks/static-and-spa.md
```

Extra checks by hand (no Docker, so `e2e/` itself was not run):

- `e2e/static_assets.py --self-test` → 17/17 PASS; parsing the rewritten page yields
  exactly the commands that driver replays or skips (`dev`, `skip npx vite build`,
  2 × `curl`, `check`, `check-json`, `skip --preset`).
- Each page's complete `blastit.toml` fence was copied out and fed to `blastit check`
  (parse + validation + env rewrite run before pytest): django, flask and php reach the
  pytest precondition with no manifest error.

## Files

- `docs/frameworks/index.md`, `django.md`, `flask.md`, `php.md`, `static-and-spa.md`.
- `python/tests/runnable_docs/inventory.toml` — fences registered:

| Page | Fence | Mode | Fixture / reason |
|---|---|---|---|
| django | 1 | `check` | `shop` |
| flask | 1 | `install-external` | package index; only blastit is this workspace's wheel |
| php | 1 | `static` | `uv tool install` checked against the installed `uv` |
| php | 2 | `static` | `dev --learn` flags checked; no PHP fixture on this host |
| php | 3 | `presets` | preset exists and fails closed |
| static-and-spa | 1 | `example` | `uvx` needs the package index; e2e compares the banner |
| static-and-spa | 2 | `example` | Node build; e2e names it as its one skip |
| static-and-spa | 3, 4 | `dev-curl` | `static-site` fixture (executed) |
| static-and-spa | 5 | `example` | `uvx`; e2e replays `check`/`--json` |
| static-and-spa | 6 | `presets` | preset exists and fails closed |

Stale inventory reasons were replaced: the static page's three "ships in MB.2.4/MB.2.5,
so this fence cannot answer yet" entries now cite `e2e/static_assets.py`, and two of them
execute.

## Page/code mismatches found

1. **`php.md` documented a CI flow that does not exist.** The old page taught
   `blastit check -- vendor/bin/phpunit`, `check --learn -- <suite>`, `--json` and
   `BLASTIT_URL=http://127.0.0.1:8080`, and claimed `0.2.0` ships them. On this branch
   `grep -r BLASTIT_URL crates python` is empty and `check` only runs `pytest -q`
   (`crates/blastit/src/check.rs`); a PHP-only manifest exits `2` asking for pytest
   (verified by hand). The page now says so; `MB.3.6.4` owns the suite runner.
2. **`check --learn` does not fill a missing grant on a route that already matches.**
   Verified: route `path = ['.*']` with `queries = []`, suite hits `/products/` →
   `learn: 0 routes, 0 queries added` while `blastit check` prints
   `1 statement not granted`. Removing `[route.db]` changes nothing; only an **unrouted**
   request gets a proposal (`learn: 1 routes, 1 queries added`). `blastit dev --learn`
   *does* add the missing query (`learn: 0 routes, 1 queries added`). The intro teaches
   that split. **`docs/ci.md#bootstrap-an-existing-project` and
   `docs/local.md#fill-the-manifest-in-one-pass` still promise single-pass `check --learn`**
   — out of this node's paths (follow-up 1).
3. **A static route can hold a view process.** The attempt-1 text claimed the opposite
   (attempt-2 MAJOR 1). Code: `route.rs:78-84`, `scheduler.rs:886-892`.
4. **`docs/reference/manifest.md:20-22` says an `app` on an all-static manifest is a boot
   error.** The code boots it: `app` + one `spa = true` static route starts `blastit dev`
   with `1 route` (verified). The static page no longer repeats the claim; the reference
   is out of scope (follow-up 2).
5. **`allow_raw` is not a runtime gate.** `body = "raw"` parses and boots without it
   (verified); it is an unconditional boot warning plus a `check` finding gated on
   `!allow_raw`.
6. **Every symlink under a static root is `404`**, not only one that leaves the root
   (`STATIC_RESOLVE_FLAGS` includes `NO_SYMLINKS`). The page says so; the
   `e2e/static_assets.py` promise comment is narrower than the code.
7. **`immutable` cannot express a recursive tree.** `*` never crosses `/`
   (`ImmutableGlob::matches`), so a collected Django tree needs one pattern per depth; the
   page shows three and says why.
8. **`[route.php] workers` may only lower** the concurrency blastit derives
   (`crates/blastit-manifest/src/error.rs:508-514`); phrased as a cap.
9. **Two published anchors vanished with the old headings.** `deploy/compose-traefik.md`
   and `deploy/digitalocean-paas.md` link `../frameworks/django.md#manifest`, and
   `troubleshooting.md` links `frameworks/index.md#bind-parameters-server-side`. Rather
   than edit three out-of-scope pages, the targets carry explicit ids
   (`## 1. Executor { #manifest }`, `## 3. Database { #bind-parameters-server-side }`);
   heading text, order and count are otherwise identical.

## Critic review resolution

Two `critic` subagents ran in parallel in attempt 1 (`dx`, `ai-ergonomics`) with the brief
"can I adapt my framework from this page without opening another one, and does any page
repeat the intro?". `dx` wrote `/tmp/opencode/mb19-review-dx.md` (1 blocker, 8 major, 14
minor); `ai-ergonomics` reported in its reply (its review file was not written before it
hit its step limit). Attempt 1 closed every blocker/major except the repetition of
framework-independent sentences, which the `dx` critic had flagged (`M9`) and the
reviewer re-raised as MAJOR 2; attempt 2 finishes it.

| Finding | Resolution |
|---|---|
| BLOCKER (dx): django imported `dj_database_url` with no install step and never ran the app | accepted → slot 1 names `uv add blastit 'psycopg[binary]' dj-database-url` and `uv run blastit dev` |
| MAJOR (dx): "no code" overpromised | accepted → "a manifest block, a few settings, and one rule about secrets — no plugin and no monkeypatching" |
| MAJOR (dx): `raw` "only with `allow_raw`" | accepted in attempt 1, sharpened in attempt 2 (mismatch 5) |
| MAJOR (dx): "a route with none of the three stops the boot" | accepted → "Without `app`, every route must be one of the last two; with `app`, a route that declares neither is a WSGI view" (verified) |
| MAJOR (dx): django "five settings", `STATIC_ROOT` missing | accepted → count dropped, `STATIC_ROOT` named in slot 2 |
| MAJOR (dx): PHP `libphp` is a silent root step | accepted → package per distro plus the `apt` line; attempt 2 limits the claim to "for PHP itself" |
| MAJOR (dx): PHP `check` outcome | accepted → exits `2` without pytest, nothing to observe with it |
| MAJOR (dx): flask `[group.<name>]` placeholder, `--learn` does not write groups | accepted → `[group.connection.db]` + `groups = ["connection"]` |
| MAJOR (dx): symlink wording | accepted → "any symlink under `root` (none are followed)" |
| MAJOR (dx) `M9`: nine sentences restate the intro | accepted in attempt 1 for health/secret rationale; **completed in attempt 2** for the `--learn` rule, `BLASTIT_MAX_WORKERS` and the migration rationale (attempt-2 MAJOR 2) |
| MAJOR (ai-ergonomics): admin endorsed as `unrestricted` | accepted → `[route.db.tables]` first, `unrestricted` as the last resort with its cost |
| MAJOR (ai-ergonomics): reader never told to export the real `DATABASE_URL` | accepted → intro slot 5 |
| MAJOR (ai-ergonomics): `--learn` scope | accepted → intro slot 8 and both WSGI pages (mismatch 2) |
| MAJOR (ai-ergonomics): `--json` / finding kinds missing | accepted → slot 8 names `--json` and four kinds, links the CI table |
| MAJOR (ai-ergonomics): outbound calls unmentioned | accepted → slot 9 names `egress = true` |
| MINOR (dx): `myshop/settings.py`, `transaction_control`, shallow globs, `uv init`, npm promise, invented 0.2.2/0.2.3, health `GET`/`HEAD`, dead-grant wording, invocation styles, matrix title, "proposes" vs writes | accepted, applied |
| MINOR (dx): trimmed `curl` transcripts | accepted as prose ("headers trimmed"); a `…` line would break `static_assets.py`'s header parser |
| MINOR (reviewer): `allow_raw`, PHP privilege, "same keys", trailing newlines | accepted in attempt 2 (table above) |

No `BLOCKER` or `MAJOR` remains open.

## Follow-ups (out of scope)

1. **`docs/ci.md` / `docs/local.md` overstate `check --learn`** (mismatch 2). Either the
   code should propose grants for routed requests (`MB.3.6.4`) or those pages should teach
   the `dev --learn` split the framework pages now use.
2. **`docs/reference/manifest.md:20-22`** repeats the all-static `app` boot-error claim
   the code does not enforce (mismatch 4).
3. **`immutable` has no recursive glob.** A collected Django/Laravel tree needs one
   pattern per depth; a `**` segment would remove three lines of caveat from two pages.
4. **`docs/deploy/compose-traefik.md:77`, `docs/deploy/digitalocean-paas.md:17`** and
   **`docs/troubleshooting.md:69`** should retarget their links to the new headings
   (`#1-executor`, `#3-database`) when MB.18 rewrites the deploy pages; the explicit ids
   are a bridge, not the final wording.
5. **`e2e/static_assets.py`'s refusal comment** says "a symlink that leaves `root` is
   `404`"; the code refuses every symlink (mismatch 6).
6. **`docs/index.md` "Is it for you?"** still says "Yes for WSGI … No for non-CPython
   interpreters", which the PHP backend in 0.2.0 contradicts.
