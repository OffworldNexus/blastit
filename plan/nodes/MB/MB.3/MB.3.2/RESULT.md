# Result MB.3.2

## Summary

Wrote `docs/frameworks/php.md` (149 lines, 10 console fences, 12 commands): the page
that defines `0.2.1`. It opens with a working 10-line `blastit.toml` (`[route.static]`
on `public/` + `[route.fastcgi]` on the same route), then exactly three environments:
Local (zero `sudo`: `uv init --bare`/`uv add blastit`, a non-root `php-fpm -F -y`
pool with an absolute `listen` while the manifest's `listen` stays relative to
`blastit.toml`, `blastit dev` with the full banner, three `curl -i` proofs:
`/users/42` from `public/index.php`, an immutable `/assets/` file from the static
responder, and `/?-d+allow_url_include=1` still reporting `allow_url_include` `0`;
`dev --learn` stops the first `dev`, starts the learning one, answers the asset with
the front controller's `404`, records the path and writes the missing
`[route.static] root`), CI/CD (`check -- vendor/bin/phpunit`, `BLASTIT_URL`, the
`--json` unexercised-route finding) and Deployments (`compose-traefik`, fails closed,
one pool per route as the route uid). Registered in the nav, the framework index and
`inventory.toml` (every fence accounted; MB.3.7's `e2e/php2shell.py` is the
replacement proof). Both fresh-context lens reviews end in `VERDICT: PASS`.

## Review Resolution

dx (second-round file, resolution pass):
- **B1** (pool ownership) → line 146: one pool per route runs as that route's uid, so
  `0600` stays private; Local runs it from the project root.
- **B2** (CLI/CI branch) → line 3 marks `check -- <suite>` 0.2.1; lines 120–122 state
  the FastCGI branch and `BLASTIT_URL`; reference updates are Follow-up 1.
- **B3** (clean checkout) → line 112 names PHP, Composer and the Local pool step.
- **B4** (in-process PHPUnit) → lines 122–124 give the working
  `file_get_contents(getenv('BLASTIT_URL').'/users/42')` test.
- **B5** (`--learn`) → lines 103–104 name the concrete asset, Ctrl-C and the exact
  learned `[route.static] root`; `immutable` is the reader's.
- **B6** (mechanism links) → Follow-up 2 (mechanism pages are outside this node);
  MB.3.7 proves the promise.
- **B7** (proxy/env) → Follow-up 3 (MB.3.3); the page scopes to the HTTP route and the
  no-DB `statements: 0` fixture.
- **B8** (entry/app) → line 29 `entry` is manifest-relative (under `root` when both are
  set); line 25 states the no-app executor rule.
- **B9** (`spa`) → accepted MINOR: the block omits it; `static-and-spa.md` owns the
  trap.
- **B10** (cache) → lines 81–82: point `immutable` at `public/assets/` only, the
  directory the bundler fills with hashed names, never a path that can answer HTML or
  an unhashed original.
- **B11** (0.2.0 `unknown key`) → accepted MINOR: line 3 states 0.2.1.
- **B12** (second terminal) → accepted MINOR: `-F` and the `--learn` Ctrl-C imply it.

ai-ergonomics (second-round file, resolution pass):
- **A1** (entry invariant) → line 29 is conditional, so the fastcgi-only `--learn`
  state validates.
- **A2** (no-app reference) → line 25 states the FastCGI executor/`[route.ingress]`
  rule; reference entry is Follow-up 1 (MB.3.5).
- **A3** (`check` branch) → lines 3 and 120–122; reference branch is Follow-up 1.
- **A4** (`BLASTIT_URL` reader) → line 122: the suite reads it, never the app.
- **A5** (`--learn` stop/scope) → Ctrl-C and the static write are explicit; route/query
  learning is `local.md`'s contract.
- **A6** (ingress) → lines 33–34 cover POST/PUT/PATCH and `raw` + `allow_raw`.
- **A7** (proxy outputs) → Follow-up 3 (MB.3.3); page scope is the HTTP route.
- **A8** (index says pytest) → `docs/frameworks/index.md` now says "runs your suite".
- **A9** (`workers` link) → accepted MINOR: the table is the contract until MB.3.5.
- **A10** (mechanism links) → Follow-up 2.
- **A11** (24 vs 3) → the failing `--json` run now says `"requests": 3`.
- **A12** (0.2.1 scope) → line 3 names the block, the PHP `--learn` write and
  `check -- <suite>`.
- **A13** (repetition) → deploy says "Match `pm.max_children` to `workers`".
- **A14** (stop the first dev) → accepted MINOR: `-F` is foreground by definition.

## Evidence

### Acceptance: page is lean and shows the commands

```console
$ test "$(wc -l < docs/frameworks/php.md)" -le 150 && echo "page is lean"
page is lean (149 lines)
$ test "$(grep -c '^\$ ' docs/frameworks/php.md)" -ge 6 && echo "local + CI + deploy commands shown"
local + CI + deploy commands shown (12)
$ for k in 'uv add blastit' 'blastit dev' '--learn' 'blastit check --json' 'public/index.php' 'php-fpm' 'route.fastcgi' 'route.static' 'compose-traefik'; do grep -q -- "$k" docs/frameworks/php.md || echo "missing: $k"; done
(no output)
```

### Acceptance: registration

```console
$ grep -q 'php.md' zensical.toml && grep -q '(php.md)' docs/frameworks/index.md
(exit 0)
$ grep -q 'docs/frameworks/php.md' python/tests/runnable_docs/inventory.toml
(exit 0)
```

### Acceptance: docs-unit, docs-check, docs-test

```console
$ make docs-unit
63 passed, 57 subtests passed in 0.36s

$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.27s

$ make docs-test
PASS  docs/frameworks/php.md  fence 1  install               0.2s  installed blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl and verified it against the built wheel
PASS  docs/frameworks/php.md  fence 2  example               0.0s  accounted: php-fpm is not a dependency ...
PASS  docs/frameworks/php.md  fence 3  static                0.0s  statically checked every command, flag and snippet ...
PASS  docs/frameworks/php.md  fence 4  example               0.0s  accounted: The worker ... public/index.php ... {"route":"user.show","id":"42"}
PASS  docs/frameworks/php.md  fence 5  example               0.0s  accounted: ... text/css; charset=utf-8 ... immutable ...
PASS  docs/frameworks/php.md  fence 6  example               0.0s  accounted: ... allow_url_include ... 0 ... query string ...
PASS  docs/frameworks/php.md  fence 7  static                0.0s  statically checked ... MB.3.7 ... [route.static] root = "public" ...
PASS  docs/frameworks/php.md  fence 8  static                0.0s  statically checked ... BLASTIT_URL ... asset and the worker ...
PASS  docs/frameworks/php.md  fence 9  static                0.0s  statically checked ... --json ... unexercised route ...
PASS  docs/frameworks/php.md  fence 10  presets               0.0s  each preset exists and fails closed without capabilities

57 fences: 30 executed, 27 accounted, 0 failed · 21.1s
```

### Acceptance: both lenses pass

```console
$ for f in docs/design/php.review-dx.md docs/design/php.review-ai-ergonomics.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "both lenses pass"
both lenses pass
```

### Acceptance: make check

```console
$ make check
...
57 fences: 30 executed, 27 accounted, 0 failed · 25.5s
uv run --no-project tools/plan.py validate
plan OK
(exit 0)
```

## Files
- `docs/frameworks/php.md` (new, 149 lines)
- `docs/frameworks/index.md` (PHP link; "runs your suite")
- `zensical.toml` (nav: PHP-FPM, Symfony and Laravel)
- `python/tests/runnable_docs/inventory.toml` (one `[[page]]`, ten `[[page.fence]]`)
- `docs/design/php.review-dx.md` (new)
- `docs/design/php.review-ai-ergonomics.md` (new)
- `plan/nodes/MB/MB.3/MB.3.2/RESULT.md` (this file)

## Follow-ups
1. `docs/ci.md` and `docs/reference/cli.md` still say `--` goes to pytest and `check`
   runs pytest; add the FastCGI branch (`-- <suite>`, `BLASTIT_URL`, exit codes) and
   the no-app `[route.fastcgi]` exemption. `docs/reference/manifest.md`'s
   `[route.fastcgi]` entry is MB.3.5's; `ci.md`/`cli.md` need a docs leaf (MB.6 owns
   the reference page).
2. `docs/security.md` and `docs/how-it-works.md` have no FastCGI mechanism section;
   the page points there per the node law. Needs an owner (MB.3.8 syncs
   `ARCHITECTURE.md` only).
3. MB.3.3 must pin how a PHP worker receives the proxy's rewritten `DATABASE_URL`
   without the real secret (a FastCGI param or a supervisor-fed pool `env`); the page
   deliberately scopes to the HTTP route and the no-DB `statements: 0` fixture.
4. MB.3.3 must honor the page's `workers` default `4` and the one-pool-per-route
   ownership model; MB.3.7 replays every fence's command and asserts the documented
   results.
5. `docs/frameworks/static-and-spa.md` correctly says the FastCGI worker is 0.2.1;
   unchanged.
