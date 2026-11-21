# MB.20 — RESULT

## Summary

`docs/frameworks/` is rebuilt: one chooser, a connector page per runtime, one page per framework,
and the static page. The five flat pages and the nine-slot contract are gone. Every page is
Blastfile-only, within its budget, and every fence is registered in
`python/tests/runnable_docs/inventory.toml` with the mode and reason its predecessor had.

| Page | Lines |
|---|---|
| `docs/frameworks/index.md` | 35 |
| `docs/frameworks/python/index.md` | 66 |
| `docs/frameworks/python/django.md` | 109 |
| `docs/frameworks/python/flask.md` | 85 |
| `docs/frameworks/php/index.md` | 100 |
| `docs/frameworks/php/symfony.md` | 55 |
| `docs/frameworks/php/laravel.md` | 64 |
| `docs/frameworks/static-and-spa.md` | 84 |

The outline and its two PASS verdicts were committed as `48d322c`, before any page existed
(acceptance 10). Nothing in this phase reopens that outline.

## Scope supersession

`MB.19` acceptance 4 (one capability matrix, framework × executor × static × database) and its
nine-heading-per-page contract are **retired on purpose**. The matrix *is* the flat organization
this node replaces; each group's differences now live on its own page, and the root chooser carries
a table instead of a matrix. This is not a regression.

No prose was lost with the slots: each removed passage's mechanics live in
`docs/reference/blastfile.md` (or the deploy/settings page named below).

| Removed passage | New home |
|---|---|
| Executor slots (`index.md:37-41`, each page's §1) | `reference/blastfile.md#sections` (`entrypoint python`/`php`, `preload`, one entrypoint per file) and `#static-mounts` |
| Static-asset slots (`index.md:43-48`, `django.md:38-54`, `php.md:55-59`) | `#static-mounts` (defaults, `*` stops at `/`, one glob per depth, the `.well-known` exception) and `#request-precedence` |
| Database slots (`index.md:50-57`, `django.md:55-69`, `flask.md:37-78`, `php.md:61-67`) | `#stores`, `#grants`, `#traits`, `#connection-setup`; the binding rule once on `python/index.md#bind-parameters-server-side` |
| Ingress slots (`index.md:59-66`, `django.md:74-79`, `php.md:69-73`) | `#view-directives` (codec table, unacknowledged `raw`) and `#request-precedence` for a mount's `GET`/`HEAD`/`405` |
| Secrets slots (`index.md:68-74`, `php.md:75-79`) | `#stores` and `settings#the-app-and-the-file` (`BLASTIT_ENV_STRIP`); `execve` stripping stays in `ARCHITECTURE.md`/`security.md` |
| Health slots (`index.md:76-81`, all pages) | `settings#port-and-health` (`BLASTIT_HEALTH_PATH`) |
| Migration slots (`index.md:83-87`, all pages) | `deploy/anything-else.md#migrations` (own role, direct URL, outside Blastit) |
| Learn/check slots (`index.md:89-97`, `django.md:96-105`) | `#what-learn-writes`, `#blastit-check`, `ci.md#what-it-fails-on`, `ci.md#for-agents-json` |
| Deployment slots (`index.md:99-106`, `django.md:107-110`, `php.md:102-110`) | `deploy/index.md`, `settings#admission-and-threads`, `settings#privileges-and-the-uid-layout` |

The PHP framework pages carry **no** deliberate one-line `libphp` pointer: their intros link the
connector and the connector owns every `libphp` fact, so `grep -rn 'libphp'` on `php/symfony.md` and
`php/laravel.md` prints nothing.

## Acceptance evidence

### 1. Tree exactly as tabled

```console
$ find docs/frameworks -name '*.md' | sort
docs/frameworks/index.md
docs/frameworks/php/index.md
docs/frameworks/php/laravel.md
docs/frameworks/php/symfony.md
docs/frameworks/python/django.md
docs/frameworks/python/flask.md
docs/frameworks/python/index.md
docs/frameworks/static-and-spa.md
```

`git status --short docs/frameworks` shows the three deletions (`django.md`, `flask.md`, `php.md`),
the two modified pages and the two new directories — no orphan. Every group is reachable from
`docs/frameworks/index.md` in one click, and `zensical.toml`'s `Frameworks` entry now nests
`Python (WSGI)`, `PHP (embedded libphp)` and `Static sites and SPAs` with their children.

### 2. Nine-slot boilerplate gone

```console
$ grep -rn '^## [1-9]\. ' docs/frameworks/
(no output)
$ for f in docs/frameworks/index.md docs/frameworks/python/*.md docs/frameworks/php/*.md docs/frameworks/static-and-spa.md; do
    grep -E '^[A-Za-z`].{80,}' "$f"; done | sort | uniq -d
(no output)
```

No page carries a nine-item legend; the root index's nine questions are questions with one link
each (two where filling and proving are different jobs), no matrix and no default.

### 3. Root chooser

`index.md` is 35 lines: the runtime table records what each runtime writes and links the connector
and both framework pages (one click each), question 8 says a PHP app has no request audit in 0.2.0,
and Node/Tomcat appear only as the roadmap line.

### 4. Python connector

66 lines; `entrypoint python "module:attribute"` + `{ preload "<module>" }` in a ten-line stub,
`uv add blastit`, one view/one worker, the local Unix-socket `DATABASE_URL`, the binding rule under
`{ #bind-parameters-server-side }`, and the learn/check pair. No framework setting appears on the
page.

### 5. Python framework pages

Django (109) carries the working file with `## The Blastfile { #manifest }`, the six-pattern static
mount, `dj_database_url`/`server_side_binding`, `transactions`, the admin's real grant directives,
the TLS settings and the `check` fence. Flask (85) carries `uv add` fences, the `connection` trait
with `transactions` and the lowercase probe, `NullPool`, and the migrations command.

```console
$ grep -n '{ #manifest }' docs/frameworks/python/django.md
6:## The Blastfile { #manifest }
```

### 6. PHP connector

100 lines; `entrypoint php` once at top level, the shim-carrying release archive on `PATH`, the
distro packages for 8.3–8.5 and the one `sudo` step, the uid rule, `public/` as the static root, the
database wiring, the learn sweep and the `fmt --check` gate.

```console
$ grep -rn 'We support supported PHP versions: 8.3, 8.4 and 8.5.' docs/frameworks/php/
docs/frameworks/php/index.md:57:We support supported PHP versions: 8.3, 8.4 and 8.5.
$ grep -rn '8\.3 and 8\.4' docs/frameworks/
(no output)
```

### 7. PHP framework pages

Symfony (55) names `cache:warmup` and `doctrine:migrations:migrate --no-interaction`; Laravel (64)
names `PDO::ATTR_EMULATE_PREPARES => false`, `route:cache`/`view:cache`, the Vite build and
`php artisan migrate --force`. Each has its own working Blastfile with a store; neither restates
`libphp`.

### 8. Static and SPA page

84 lines; the working file, the four directives in page voice, `vite build --outDir public`, the two
`curl` header checks, the deep-URL story, the roots audit and the preset. The six console fences are
unchanged, and the page names no database.

```console
$ grep -n 'postgres\|body json\|DATABASE_URL' docs/frameworks/static-and-spa.md
(no output)
```

### 9. Navigation, references, rendering

`make docs-check` is green (`No issues found`), so every anchor resolves, including `#manifest`,
`#bind-parameters-server-side` and the three question links. The four inbound references are
repointed: `getting-started.md:66`, `troubleshooting.md:70`, `deploy/compose-traefik.md:169`,
`deploy/digitalocean-paas.md:64`. `visual-checker` PASSed: 8 pages in light, dark and 390×844, the
drawer lists all 8, all four anchor jumps land, no clipping and no findings.

### 10. DX-first order

```console
$ git show --stat --oneline 48d322c
48d322c [MB.20] Outline the /frameworks rework, both goldfish verdicts PASS
 docs/design/frameworks-restructure.md              | 490 +++++++++++++++++++++
 docs/design/frameworks-restructure.review-ai-ergonomics.md |  36 ++
 docs/design/frameworks-restructure.review-dx.md    |  19 ++
```

Both review files end `VERDICT: PASS` in that commit, and no page existed before it.

### 11. Gates and language

```console
$ make docs-check        → No issues found
$ make docs-unit         → 107 passed, 65 subtests passed
$ make docs-test         → 48 fences: 30 executed, 18 accounted, 0 failed
$ make doc-tone          → doc-tone: ok — 30 user-facing page(s), no banned phrase
$ uv run --no-project tools/plan.py validate → plan OK
$ cargo test -p blastit-manifest --test docs_fences → 11 passed, 0 failed
$ cargo test -p blastit --lib → 214 passed, 1 pre-existing `#[ignore]`
$ cargo fmt --all -- --check → clean
$ cargo clippy -p blastit-manifest -p blastit --all-targets -- -D warnings → clean
$ make rs-test → exit 0 across 122 targets
```

No page exceeds its budget; the support sentence appears once; the apology grep is empty; the
retired-manifest guard is green (inside `docs-unit`); and every Blastfile fence parses — each was
extracted and run through `blastit fmt`, which reports only layout, never a syntax error, and the
static page's file is already canonical.

**Strict `docs_fences` gate closed.** `crates/blastit-manifest/tests/docs_fences.rs` is re-pinned to
the measured tree: `PAGES` 10→13 (`frameworks/index` 0 documents/0 illustrations — the chooser
carries no fence; `python/index` 1/0, `python/django` 1/2, `python/flask` 1/2, `php/index` 1/3,
`php/symfony` 1/0, `php/laravel` 1/0, `static-and-spa` 1/6), `TOTAL` 10/32→12/31;
`STATEMENT_LISTS` 4→3 (dropped `frameworks/django.md`); `Shape::ViewsAndMount` for `python/django`
and the three PHP pages, `Shape::Application` for `python/index` and `python/flask`;
`the_django_page_static_fragment_still_parses` now reads `python/django.md` and still asserts prefix
`/static`, root `staticfiles`, six `immutable` patterns. No rule widened, no allowance added. Red
first, then green: `cargo test -p blastit-manifest --test docs_fences` → 11 passed, 0 failed (2
failed before the edit). `crates/blastit/src/check.rs:458` now prints
`(see docs/frameworks/python/django.md)`; no test asserted the old text.

## Files

- `docs/frameworks/index.md`, `python/{index,django,flask}.md`, `php/{index,symfony,laravel}.md`,
  `static-and-spa.md` (new/rewritten)
- `docs/frameworks/{django,flask,php}.md` (deleted)
- `zensical.toml` (nested groups)
- `docs/getting-started.md`, `docs/troubleshooting.md`, `docs/deploy/{compose-traefik,digitalocean-paas}.md`
  (one link each)
- `docs/reference/blastfile.md` (the `.well-known` exception in `#static-mounts`)
- `python/tests/runnable_docs/inventory.toml` (re-indexed per page/fence)
- `crates/blastit-manifest/tests/docs_fences.rs` (re-pinned to the new tree)
- `crates/blastit/src/check.rs` (the `docs/frameworks/python/django.md` hint)
- `scripts/test_doc_tone.py` (one-line retarget, out of `MB.20.paths`, required for `docs-unit`)
- `plan/nodes/MB/MB.20/RESULT.md`

## Follow-ups

1. **Landed in MB.20 (closed).** `crates/blastit-manifest/tests/docs_fences.rs` is re-pinned to the
   measured tree (`PAGES` 13, `TOTAL` 12/31, `STATEMENT_LISTS` 3, the Django mount read from
   `python/django.md`, `ViewsAndMount` for Django and the three PHP pages, `Application` for
   `python/index` and `python/flask`), red then green: 2 failures before the edit, 11 passed after.
   No rule widened, no allowance added; the numbers are in acceptance 11.
2. **Comment citations still name the old paths:** `src/dev_learn.rs:952`, `src/stage0.rs:585` and
   the `critical_*` doc comments. The `check.rs:458` hint itself is repointed. The `critical_*`
   files are protected — leave them.
3. **`docs/ci.md`** needs the PHP paragraph (`fmt --check`, `check` exit `2`); the connector states
   the gate, but the CI page is still Python-only.
4. **`e2e/php2shell.py` + `e2e/compose.yaml:133`**: repoint the mount and `PAGE`, drop the install
   and preset kinds, add `fmt --check` as an explained kind, and rewrite the self-test/docstring
   sentences.
5. **`e2e/static_assets.py`**: its docstring and `DENIALS` comment cite the page's refusal list,
   which now lives in the reference — reword.
6. **`docs/deploy/index.md:57`, `docs/deploy/anything-else.md:26,139`**: `egress = true` → `egress
   any`.
7. **`docs/troubleshooting.md:80` — landed in MB.20 (closed).** The `is_secure()` row now names
   `BLASTIT_TRUSTED_PROXIES` and drops the setting.
8. **`docs/reference/settings.md`**: document `BLASTIT_PHP_SHIM_DIR` (the Stage-0 refusal names it
   for a `dev`/`check` boot).
9. **`visual-checker` — closed.** PASS: 8 pages in light, dark and 390×844, the drawer lists all 8,
   all four anchor jumps land, no clipping and no findings.
10. **`docs/frameworks/php/index.md`'s `fmt --check` fence is `example`**, not `static` as the
    outline pins: it is accounted but never executed. Teaching the runnable-docs static checker
    `fmt` and flipping the mode is the harness change that closes the gap.
