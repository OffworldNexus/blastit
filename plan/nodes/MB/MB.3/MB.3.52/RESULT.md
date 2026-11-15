# RESULT: MB.3.52 — canonical fence registered; Django static depth pinned by a test

STATUS: done (attempt 2)

## Attempt 2 — what changed

| File | Change |
|---|---|
| `docs/frameworks/django.md` | sixth `immutable` glob; claim now six segments with the select2/i18n example |
| `crates/blastit-manifest/tests/docs_fences.rs` | plain-`toml` fragment support; new test parses the page's `[route.static]` block under a minimal header and asserts the exact `immutable` list |

Item 2 is untouched: the canonical fence in `index.md` is still
` ```toml title="blastit.toml" `, still in `FILES`, and its red/green pair from
attempt 1 still holds (reproduced below). The only shared change is that
`blastit_fences` now delegates to a `collect_fences(document, opening)` helper so the
fragment case can reuse the parser; its behaviour is identical (all five original
tests stay green).

No `inventory.toml` change: it indexes only `console` fences, and none moved.

## MAJOR — the depth was still one short, and now it is verified

Checked against the real Django 6.1.3 tree in the uv cache
(`~/.cache/uv/archive-v0/QI200WCqFyz9utn2`):

```console
$ grep -m1 -A3 VERSION django/__init__.py
VERSION = (6, 1, 3, "final", 0)
$ find django/contrib/admin/static/admin -type f | sed 's|^django/contrib/admin/static/||' \
    | awk -F/ '{print NF, $0}' | sort -rn | head -3
6 admin/js/vendor/select2/i18n/zh-TW.js
6 admin/js/vendor/select2/i18n/zh-CN.js
6 admin/js/vendor/select2/i18n/vi.js
$ sed -n '605,613p' django/contrib/admin/widgets.py
        i18n_file = (
            ("admin/js/vendor/select2/i18n/%s.js" % self.i18n_name,)
            if self.i18n_name
            else ()
        )
```

Deepest admin static path is **six** segments after the static root; the previous
"five segments (`/static/admin/js/vendor/jquery/…`)" claim is deleted. The page now
reads:

```toml
immutable = ["/static/*", "/static/*/*", "/static/*/*/*",
             "/static/*/*/*/*", "/static/*/*/*/*/*", "/static/*/*/*/*/*/*"]
```

> depth, and as deep as the deepest fingerprinted file: Django's admin ships
> `admin/js/vendor/select2/i18n/<lang>.js`, six segments under `/static/`, so a depth you
> leave out revalidates those hashed files on every request.

## MINOR — the fragment is now parsed, and its depth pinned

**Chosen:** a fragment-with-prefix case in `docs_fences`, not a second titled fence on the
page — a titled fence would make `every_documented_manifest_parses_and_validates` demand
exactly one fence and see two, and the `[route.static]` block is presented as an addition to
a manifest that already has a header, which is how it is now parsed:

```rust
let document = format!("version = 1\napp = \"app:app\"\n\n{fragment}");
```

The test asserts the parsed route (`name = "static"`, `path = ['/static/.*']`), the
policy `root = "staticfiles"`, no warnings, and `policy.immutable` equals the six documented
patterns exactly. It also asserts the page carries exactly one plain `toml` fragment, so a
second unidentified fragment fails too.

**Red first** — test written before the sixth glob was added to the page:

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
test the_django_static_fragment_pins_its_documented_depth ... FAILED
---- the_django_static_fragment_pins_its_documented_depth stdout ----
thread '…' panicked at crates/blastit-manifest/tests/docs_fences.rs:236:5:
assertion `left == right` failed: the page lists one immutable glob per depth, as deep as the deepest fingerprinted file Django's admin ships
  left: ["/static/*", "/static/*/*", "/static/*/*/*", "/static/*/*/*/*", "/static/*/*/*/*/*"]
 right: ["/static/*", "/static/*/*", "/static/*/*/*", "/static/*/*/*/*", "/static/*/*/*/*/*", "/static/*/*/*/*/*/*"]
test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

**Green after the page edit:**

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
running 6 tests
test only_a_complete_blastit_fence_is_collected ... ok
test an_invalid_regex_is_refused ... ok
test the_django_static_fragment_pins_its_documented_depth ... ok
test the_static_page_manifest_has_its_documented_shape ... ok
test a_drifted_static_block_does_not_satisfy_the_documented_shape ... ok
test every_documented_manifest_parses_and_validates ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

The failure is on the `immutable` value, not on a missing fence: the page is parsed, and a
wrong depth reddens the suite.

## Item 2 (accepted in attempt 1) — evidence unchanged

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
test every_documented_manifest_parses_and_validates ... ok
```

Registration red (before the fence was titled): `docs/frameworks/index.md carries exactly
one complete blastit.toml fence — left: 0, right: 1`. Load-bearing proof (attempt 1, page
restored byte-identically): a `/tmp` copy with `body = "json"` → `body = "text"` fails with

```console
docs/frameworks/index.md does not boot:
manifest has 1 error(s)
  - route[0].ingress.body: expected "json", "form", "none" or "raw", found "text"
```

## Items 1 and 4 (unchanged from attempt 1)

- `flask.md`'s `products` route carries `[route.ingress] body = "form"`, and slot 4 points
  at it.
- `index.md` §6: "Point the platform's check at it — a route for it never runs — and keep
  database queries out of any health route: the platform check then learns blastit is up,
  not the pool's health."

## Acceptance (attempt 2)

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.31s

$ make docs-unit
83 passed, 65 subtests passed in 0.64s

$ make docs-test
49 fences: 29 executed, 20 accounted, 0 failed · 18.7s
(whole target green, including scripts/build-wheel.sh and the wheel link check)

$ uv run --no-project tools/guard.py
guard: OK (13 staged/untracked file(s) checked)

$ uv run --no-project tools/plan.py validate
plan OK

$ wc -l docs/frameworks/*.md
  118 docs/frameworks/django.md
  115 docs/frameworks/flask.md
  115 docs/frameworks/index.md
  103 docs/frameworks/php.md
  118 docs/frameworks/static-and-spa.md
  569 total
```

Budgets: intro 115 ≤ 130; django 118, flask 115 ≤ 120. No page over budget.

Also clean on the touched crate: `cargo fmt --all -- --check` and
`cargo clippy -p blastit-manifest --tests --locked -- -D warnings`.

`make check` was not run from this shared worktree: `docs/design/README.md`,
`plan/BUDGET.md` and `plan/JOURNAL.md` carry another live node's in-flight edits
(MB.3.51), so a whole-workspace gate would report state this node did not produce.

## Follow-up

- `python/tests/test_runnable_docs.py`'s module docstring still says "executes 38
  snippets" while `make docs-test` reports 49 fences (pre-existing; outside this node's
  paths).
