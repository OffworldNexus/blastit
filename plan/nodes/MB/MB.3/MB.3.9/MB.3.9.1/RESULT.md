# MB.3.9.1 — RESULT

DRAFT: PHP mechanism pending MB.12 embed-vs-pool; mechanism-neutral parts only.

## Delivered

- `docs/frameworks/php.md` (149 lines): working example first, `[route.fastcgi]` is one
  key — `entry` — plus `[route.ingress] body = "form"`; `workers` is one row marked
  "optional; omit it". Local is the install line (`uv tool install blastit`) and
  `blastit dev`. No `php-fpm.conf`, `mkdir`, `php-fpm -F`, `listen`, `workers = 4`,
  `uv init --bare` or `uv add` anywhere on the page.
- CI/CD: `blastit check -- <suite>`, the in-process test clients (Symfony
  `KernelBrowser`, Laravel `$this->get()`), the one-time `check --learn -- <suite>`
  bootstrap, and the `--json` document. Deployments: one preset command, the
  fail-closed note, uid `21000` and the writable dirs.
- `docs/reference/settings.md`: new `## Backends` section with the `BLASTIT_PHP_FPM`
  row (env-only, never a manifest key; the page links it once).
- `python/tests/runnable_docs/inventory.toml`: php.md re-accounted, 9 console fences
  (was 10; the by-hand pool fence is gone, the `check --learn` fence is new).
- `crates/blastit-manifest/tests/docs_fences.rs` unchanged: `FILES` is already 8 with
  php.md and the page carries exactly one complete manifest fence (the `MB.3.5.1`
  parser now refuses `listen`; the page no longer writes it).

## Mechanism-specific sentences (scope change, 2026-11-14)

- `docs/reference/settings.md:26` keeps `BLASTIT_PHP_FPM` — the name and its default
  search order (`php-fpm`, then versioned names on `PATH`) — as instructed; that row is
  the one mechanism-specific statement in this change.
- `docs/frameworks/php.md:1` keeps the H1 "PHP-FPM, Symfony and Laravel": the nav label
  lives in `zensical.toml:37` and `docs/frameworks/index.md:8,71` repeats it, both
  outside this node's paths (follow-up below).
- Nothing else on the page names a pool or `php-fpm`: `grep -c 'php-fpm -F'` is `0`,
  `grep -c listen` is `0`, and `pm.max_children`, socket paths and "one pool per route"
  do not appear.

## Length (ceiling ~150)

149 lines. The additions were paid for by deletions:

- round-2 MAJOR (`check --learn -- <suite>`): removed "Run it from the directory that
  holds `blastit.toml` (no parent search):" and folded the rule into the Local intro
  ("…reads `./blastit.toml` from the directory you run it in").
- round-3 MAJOR (the JSON route must precede the catch-all): removed "Review the diff,
  then drop `--learn` for the gate." — it repeated "Bootstrap once, never as the gate"
  in CI/CD.
- `[proxy.db]`/uid/chown and the `--learn` split were paid for by merging the
  hostile-parameter paragraph into the query-string lead and tightening the deploy
  paragraph.

## Critics

Reports on disk (rounds 1-3 each, round 1 kept for history):
`docs/design/php-dev-local.review-dx.md`,
`docs/design/php-dev-local.review-ai-ergonomics.md`.

Round 3 was the final round allowed by the dispatcher. It returned the same single
MAJOR from both lenses — N1, "the JSON route must be above the catch-all, routes are
first-match in order" — which is fixed in the page after the report:

> … a second `[[route]]` above this catch-all, with its own `name`,
> `path = ['/api/.*']`, `[route.fastcgi] entry` and `[route.ingress] body = "json"` —
> first match wins, in manifest order.

No BLOCKER and no unresolved MAJOR remains. Every earlier round-2 MAJOR is marked
resolved in those reports (D18-D28 except the partials below; ai M9, m21, m23).

## Accepted MINORs (with reason)

- dx D4: the "`0.2.1` adds it" note stays — it is a page promise in design §1.1.
- dx D9: the asset curl keeps its fixture hash — MB.3.7 replays that exact command.
- dx D12: the "hostile request parameter" wording is now in the query-string lead; no
  separate paragraph, which is what D12 asked for.
- dx D17: the `--json` document stays one line — MB.3.7 asserts that document and
  `docs/ci.md` shows the same shape.
- dx D20 (partial): the page names uid `21000` and the writable directories; the
  literal `chown` belongs to `docs/deploy/**` (not this node's paths).
- ai m2-m5, m10, m19: `uv`, `--port`, `root`'s path rules, the `name` grammar, the
  suite environment and glob nesting are reference material (`cli.md`, `manifest.md`);
  each would cost lines the ceiling does not have.
- ai m22 (partial): `check --learn` is named as writing `blastit.toml`; the page no
  longer claims `--learn` needs no review (the gate line is in CI/CD).
- ai m24 (partial): "The image needs the PHP Blastit runs" — the image build itself is
  the deploy page's.

## Evidence

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.28s

$ make docs-test
56 fences: 29 executed, 27 accounted, 0 failed · 20.1s

$ make check
guard: OK (34 staged/untracked file(s) checked)
test result: ok. … (workspace, all targets)
56 fences: 29 executed, 27 accounted, 0 failed · 20.1s
plan OK
exit=0
```

`make check` also reports (report-only) the two pre-existing
`crates/blastit-fastcgi/tests/critical_*.rs` modifications from `MB.3.5.1`'s working
tree; they are not this node's changes.

## Follow-ups

- `docs/ci.md` is still pytest-shaped: it has no FastCGI branch and never names
  `-- <suite>`. Owner: the leaf that owns `ci.md` (MB.3.6 / MB.3.5.2).
- `docs/reference/manifest.md`: no `[route.fastcgi]` section, and its `app` row still
  says "required when any route is not static" — MB.3.5.2's reconciliation.
- `docs/frameworks/static-and-spa.md:121-122` calls a PHP app "this row with
  `public/index.php` as its entry script"; `[route.static]` has no `entry` key.
- H1 + nav + `docs/frameworks/index.md` links should drop "PHP-FPM" once MB.12 decides
  (`zensical.toml:37`).
- design §1.1's page-promise table now differs from the page on learn (CI uses
  `check --learn -- <suite>`; `dev --learn` proposes `root`) — MB.3.9.2/3.9.3 owners.
- The install line is the generic `uv tool install blastit`; standalone and distro
  packages were deliberately not invented (MB.12 owns distribution).

## Reviewer follow-up (two MINOR factual defects, fixed)

- The boot-error quote is now the real two-line shape, verified by running the built
  binary against a manifest with `listen`: `blastit dev: manifest ./blastit.toml:
  manifest has 1 error(s)` then `  - route "app" (index 0): fastcgi entry "index.php":
  must be under static root "public"` (no `:12:`; `ManifestErrors` prints no line
  number, `error.rs:640-649`).
- PHP ownership is one clause: 8.2+ must be on `PATH` (or `BLASTIT_PHP_FPM`), Blastit
  runs it and does not install or upgrade it; the "does not check your PHP version"
  clause was removed rather than repeated.
- Still 149 lines; install line, `BLASTIT_PHP_FPM` link and the route-order paragraph
  unchanged.
