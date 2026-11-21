# Result MB.31.1

## Summary

`static … immutable` is now documented as the **same** grammar a `view` pattern has, in three
shapes, on every page this node owns:

| Written | Documented promise |
|---|---|
| `immutable` | every exact file the mount serves, **only** when each name changes with its bytes |
| `immutable "/assets/*", "/fonts/*"` | a trailing `/*` crosses `/` at any depth; `{id}`/`{id:int}`/`{id:uuid}` and literal dots as in a `view` |
| `immutable regex '/static/.+\.[0-9a-f]{12}\.[^/]+'` | the escape hatch, for hashed names in a mixed tree |

The six-glob staircase is gone from `docs/frameworks/python/django.md`,
`docs/reference/blastfile.md`, `docs/frameworks/static-and-spa.md`,
`docs/frameworks/php/laravel.md` and every design/ADR record; `grep` for it now exits `1`.

Wording the critics forced (and the grammar the parser must honour):

- **Match target is explicit**: "matched whole against the request path, **mount prefix
  included**: under `static "/static"`, write `immutable "/static/admin/*"`, not `"/admin/*"`"
  — an unmatched pattern is silent `no-cache`, so the reference now names the one mistake that
  fails without an error (`StaticFile::url_path`, design §9.4).
- **Bare `immutable` states its precondition** instead of only its mechanism: a tree with
  unhashed originals (`collectstatic`, a Vite `publicDir`) uses a pattern or `regex`.
- **Caps in one sentence**: at most 32 patterns per mount, each at most 4096 bytes, quoted or
  `regex '…'`; a bad pattern is refused with the `view` sentence. The limits table's `4096 list
  items` row no longer claims `immutable` (its cap is 32).
- **S3 in one place**: the `immutable` directive row keeps "an `index` or `fallback` answer is
  always `no-cache`"; the shapes table does not repeat it.
- `StaticPolicy.immutable` stops being `Vec<String>` and `ImmutableGlob`/the 256-byte ceiling die
  — recorded in `ADR-0012` (Amendment 2026-11-17) and design `blastfile-dsl.md` §9.4, with the
  grammar line `'immutable' (pattern (',' pattern)*)?` beside `pattern → Str | 'regex' RawStr`.
  `ADR-0007` keeps its history and gains the pointer; design `static-and-spa.md` §4.5 R5 and its
  `glob()` vector table carry dated supersession notes naming `pattern::compile` +
  `routing::compile_pattern`.

`crates/blastit-manifest/tests/docs_fences.rs` changed minimally: the Django pin is now
`assert_eq!(mount.policy.immutable, ["/static/*"])` (`MB.31.2` strengthens it to the typed form).

**Out-of-node-path change, disclosed:** the `dx` lens raised one BLOCKER on
`docs/frameworks/php/laravel.md:36` ("`*` stops at `/`"), which the node's own acceptance `grep`
scans but the node's `paths` omit. It was fixed in place with one clause ("a trailing `/*`
crosses `/`, so this one pattern covers the files under `assets/` at any depth"). The plan was
not mutated; `paths` should gain that file if the acceptance is kept as written.

## Evidence

### Fence, before → after (`docs/frameworks/python/django.md`)

```diff
 static "/static" {
     root "staticfiles"
-    immutable "/static/*", "/static/*/*", "/static/*/*/*",
-              "/static/*/*/*/*", "/static/*/*/*/*/*", "/static/*/*/*/*/*/*"
+    immutable "/static/*"
 }
```

### Commands

```console
$ uv run --no-project tools/plan.py validate
plan OK
$ cargo test -p blastit-manifest --test docs_fences
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ make docs-check
Build started
No issues found
Build finished in 0.63s
$ make docs-test
48 fences: 30 executed, 18 accounted, 0 failed · 24.0s
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 0.62s
$ grep -rn 'stops at `/`\|one glob per depth\|"/static/\*/\*"' \
    docs/frameworks docs/reference docs/decisions/ADR-0012-blastfile-dsl.md
$ echo $?
1
$ uv run --no-project tools/critical_guard.py --base df44f98 --node MB.31.1
critical-guard: df44f98..working tree, 13 changed file(s)
  no change to a protected critical test
$ make check
EXIT=0   (tail: `plan OK`, 0 lines matching ^FAIL)
```

No new fence was added, so the fence inventory (`PAGES`, `STATEMENT_LISTS`, `inventory.toml`)
needed no new entry: Django stays one document + two illustrations, the reference stays 14
statement lists.

### Critic lenses (`dx`, `ai-ergonomics`)

Both wrote `docs/design/immutable-pattern-grammar.review-<lens>.md`, first verdict
`changes required`, then a confirmation round after the fixes. **Final verdicts: `accept`,
`accept` — zero open BLOCKER/MAJOR.**

| Lens | Findings | Resolution |
|---|---|---|
| `dx` | 1 BLOCKER, 5 MAJOR, 4 MINOR | all accepted and fixed (B1 in `laravel.md`; request path + prefix example; `written alone` row; `/assets/chunks/x.js`; bare precondition; Django regex demoted to an option; 32/4096 caps; SPA row/sentence; the design note now names `StaticFile::url_path` and states that R5's "root-relative" parenthetical is the superseded wording) |
| `ai-ergonomics` | 3 MAJOR, 2 MINOR | MAJOR 1–3 accepted (prefix rule, bare precondition, 32-pattern cap); MINOR 5 accepted; MINOR 4 rejected — the SPA page keeps two forms because the reference is the one page owning the three-way grammar and a hashed Vite build never needs `regex` |

## Follow-ups

1. `crates/blastit-manifest/tests/static_mounts.rs:206,228` and its "one glob per depth" fixture
   belong to `MB.31.2`; `crates/blastit/src/lsp/hover.rs:123` and
   `docs/design/blastfile-lsp-zed.md:1068` (the hover row copied into the design record) belong
   to `MB.31.4`. Both already name the old grammar in their own `paths`.
2. Plan hygiene: add `docs/frameworks/php/laravel.md` to this node's `paths` (or widen the
   acceptance `grep`), so the BLOCKER fix above is inside the declared scope next time.
3. `CHANGELOG.md` still needs its `0.2.0` line — release prep (`MB.3.8`) owns it.
