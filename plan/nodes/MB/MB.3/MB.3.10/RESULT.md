# Result MB.3.10

## Summary

PHP's user documentation now describes the one supported backend: `[route.php]` with
`entry` and an optional `workers` value, executed by the host's NTS `libphp`
inside the route's own worker (`ADR-0010` §1–§2). `php-fpm`, FastCGI, `[route.fastcgi]`,
`listen` and any fallback are gone from every page that describes what Blastit does;
the H1 and both nav labels read "PHP (Symfony, Laravel)". A host without a usable NTS
`libphp` is documented as a boot refusal naming the distro package and
`BLASTIT_LIBPHP`, not a degraded mode.

The page is 150 lines (was 149 before the retarget; 163, 153 and 152 in review).
Local stays two steps — write `blastit.toml`, run `blastit dev` — and says plainly that
installing the distro `libphp` package is the one privileged step, that `composer install`
precedes the first run, and that `blastit dev --learn`/`check --learn` record routes and
exact queries into `blastit.toml`.

Two claims from the first pass were **wrong against the code and are removed**, not
reworded:

* `workers` above the derived concurrency is **not** a boot error today: nothing reads
  `PhpPolicy.workers`; only `1..=64` is parsed (`validate.rs::check_php_workers`). The
  page's row reads "optional, `1`–`64`; omit it — Blastit derives the per-route budget"
  and the reference's "an optional value from 1 to 64; omit it and Blastit uses the
  per-route budget": neither promises a bound that does not exist, and neither calls the
  value downward. The ADR's bound is `MB.3.13.2`'s; the sentence returns then.
* "`entry` is never served as a file" is **false**: the static responder serves any
  regular file under `root`. The `.php`/`.phar` refusal is `MB.9.2.2` (ready, not
  landed). The page, the manifest reference and `static-and-spa.md` now say only that
  Blastit executes `entry`, that `[route.static]` serves regular files as bytes, and
  that application sources therefore stay out of a served root.

`crates/blastit-manifest/tests/docs_fences.rs` lost its `STALE` accounting with the
entry: with the page booting, the self-clearing test had no subject, so the const, the
lookup and the test are gone and every documented fence must boot. `settings.md` is
untouched: the `BLASTIT_LIBPHP` row is `MB.3.13.2`'s, so `php.md` links to
`../reference/settings.md` without the (empty) `#backends` anchor.

## Evidence

```console
$ grep -rniE 'php[-_ ]?fpm|fastcgi|route\.fastcgi' docs/frameworks/php.md \
    docs/frameworks/index.md docs/frameworks/static-and-spa.md zensical.toml
$ echo $?
1
$ grep -rn '0\.2\.1' docs/frameworks/php.md
$ echo $?
1
$ grep -rn 'above the budget\|budget is a boot error\|refuses to serve it' \
    docs/frameworks/php.md docs/reference/manifest.md docs/frameworks/static-and-spa.md
$ echo $?
1
$ grep -c '^```console' docs/frameworks/php.md
8
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.32s
```

```console
$ make docs-unit
...............................................................  [100%]
63 passed, 65 subtests passed in 0.43s
```

```console
$ make docs-test
...
PASS  docs/frameworks/php.md  fence 8  presets   0.0s  each preset exists and fails closed without capabilities
57 fences: 28 executed, 29 accounted, 0 failed · 21.6s
exit=0
```

The PHP page's fences are 1–2 and 5–7 statically checked against the installed CLI, 3–4
accounted (no PHP app fixture in the gate; `make php-sapi-e2e` proves the worker's body
and head), 8 preset. The third curl (the hostile query string) was folded into one
sentence and the reference's rule in `security.md`; the page now carries eight fences.

```console
$ cargo test --locked -p blastit-manifest --test docs_fences
running 5 tests
test every_documented_manifest_parses_and_validates ... ok
test result: ok. 5 passed; 0 failed
```

```console
$ make check
guard: OK (11 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 17 changed file(s)
  no change to a protected critical test
63 passed, 65 subtests passed in 0.40s
No issues found
57 fences: 28 executed, 29 accounted, 0 failed · 19.5s
plan OK
exit=0
```

Critics reviewed the **final** text (round 6, `docs/design/php-dev-local.review-*.md`;
round 4 had 2 MAJOR on the sudo claim and `composer install` order, and round 5 read a
superseded 163-line draft and is kept as history only):

* `dx` **PASS** — 0 BLOCKER, 0 MAJOR, 4 MINOR, 2 NIT against `php.md` at 150 lines.
* `ai-ergonomics` **PASS** — 0 BLOCKER, 0 MAJOR, 3 MINOR, 2 NIT, with the page's
  `[route.php]` fence re-read against `validate.rs::check_php*` and first-try valid.

Resolved from round 6: `name` restored in the second-route sentence (a copy would
otherwise duplicate `name = "app"` and abort on `DuplicateRouteName`), a lead-in before
the `blastit dev` fence and "from that directory" for its cwd, `composer install` still
named before the first run (prose, not a fence: the harness would have to know
`composer`), and `php8X-embed` shown with `php84-embed` as its example. Left as their
NITs, both owned elsewhere: the `BLASTIT_LIBPHP` link lands on an empty
`settings.md` §Backends (`MB.3.13.2`), and `manifest.md:106` still names 0.1.1.

## Files

* `docs/frameworks/php.md` — retargeted onto `[route.php]`; 150 lines.
* `docs/frameworks/index.md`, `zensical.toml` — nav label "PHP (Symfony, Laravel)".
* `docs/frameworks/static-and-spa.md` — "the embedded PHP worker"; a PHP app is that row
  with a `[route.php]` block; no refusal claim.
* `docs/reference/manifest.md` — `[route.php]` row and §PHP; `app` row no longer says
  "any route that is not static"; a static miss reaches the app **or** the front
  controller.
* `crates/blastit-manifest/tests/docs_fences.rs` — `STALE` removed.
* `python/tests/runnable_docs/inventory.toml` — eight fences re-registered with
  truthful modes/reasons; `MB.3.18` citation resolves (`plan.py show MB.3.18`).
* `docs/design/php-dev-local.review-{dx,ai-ergonomics}.md` — critic rounds 4 and 5.

## Follow-ups

1. **`[route.php]` cannot boot on this branch.** `crates/blastit/src/stage0.rs` and
   `check.rs` call `php_backend_unavailable`, which refuses every PHP route with "the
   per-route worker wiring lands in MB.3.13.3"; `no_python_build.rs::a_php_only_manifest_is_refused_by_the_interim_php_gate`
   pins it. The page documents the 0.2.0 target, so it is false until `MB.3.13.3` lands
   (and it must then produce the loader's package/`BLASTIT_LIBPHP` refusal the page
   quotes).
2. **`check -- <suite>` and `BLASTIT_URL` do not exist yet.** `check` runs `pytest -q`
   and `docs/ci.md:9,98` says so; PHPUnit after `--` and the `getenv('BLASTIT_URL')`
   recipes need `MB.3.6.4` plus CI's PHP form. The page's CI section is the target.
3. **`workers` above the budget.** `MB.3.13.2` implements `ADR-0010` §2's downward
   bound; until then no code reads `PhpPolicy.workers`, which is why neither row claims
   a boot error.
4. **`.php` source disclosure.** `MB.9.2.2` owns the `.php`/`.phar` refusal in the
   static stage; the pages state only what is true today.
5. **`settings.md ## Backends` is empty**; `MB.3.13.2` adds the `BLASTIT_LIBPHP` row.
6. **`docs/reference/manifest.md:106`** still says "0.1.1 has no multipart codec"
   (release sweep, not this node).
7. Not taken from the reviews: a `chown` line in Deployments (the deploy page's), an
   `--env-file` remedy, `uv` as a stated prerequisite, and `--port` (all MINOR/NIT;
   every other framework page makes the same assumptions). The deploy page's recipe
   still lacks `libphp`, and `docs/ci.md` still describes pytest only.
