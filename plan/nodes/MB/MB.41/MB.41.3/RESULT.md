# MB.41.3 — Result

Teach both docs harnesses to read fences inside `=== "…"` content tabs, fix the
Docker outer seccomp for `rs-test-php-image`, and pass the full gate.

## 1. `docs_fences` (Rust)

`crates/blastit-manifest/tests/docs_fences.rs`

- **Indented fences.** `collect_fences` recognises a fence at any indentation
  (the opening is already read from its trimmed line) and now de-indents every
  body line by the *opening's own* indentation (`leading_width` + `deindent`), so
  a 4- or 8-space tab body reaches `Manifest::parse_blastfile` as the text a
  reader copies. `scan_page`/`class_of` still classify the de-indented body the
  same way (document / statement list / illustration).
- **One Shape per document, not per page.** The single `match file { … }` pin is
  replaced by `SHAPES: [(&str, &[Shape]); 13]` — a per-page, per-document pin in
  page order — plus `pinned_shapes(file)`, which panics for a page with an
  executed document and no row. The Python tab is `Application`, a PHP tab that
  mounts `public/` is `ViewsAndMount`, the static tab is `StaticSite`; the
  framework pages keep the shapes they were pinned with.
- **Re-pinned from measurement** (`python/tests/runnable_docs/fences.py`, one
  pass over every published page):

  | | before (pinned) | after (measured) |
  |---|---|---|
  | documents over `PAGES` | 12 | **17** |
  | illustrations over `PAGES` | 46 | **62** |
  | `TOTAL` | `Counts::new(12, 46)` | **`Counts::new(17, 62)`** |
  | `STATEMENT_LISTS` | `ci.md 1`, `local.md 1`, `reference/blastfile.md 14` | **`reference/blastfile.md 14`** |

  Per-page rows now: `docs/index.md (3, 5)`, `docs/getting-started.md (3, 10)`,
  `docs/blastfile.md (3, 8)`, `docs/local.md (0, 12)`; every other row is
  unchanged. The exhaustive scan is untouched, so an unpinned page that gains a
  document still fails.
- **The §20 corpus reader mirrors it.** `crates/blastit-manifest/tests/common/mod.rs`
  keeps the "copy of the scanner" contract: `fences_on` now de-indents the same
  way, so the tabbed documents enter the protected round-trip/fixed-point corpus
  as the text a reader copies, and `DOCUMENTED_PAGES` drops `docs/local.md`
  (12 → 11 pages) because that page's document became per-language `console`
  commands in `MB.41.1`. No protected `critical_*.rs` file changed; the corpus
  still fails by name if a listed page stops yielding a document, and
  `ROUND_TRIP_FLOOR` (20) still clears with 15 fixtures + 17 documents.
- **New unit tests** (both directions, plus the de-indent edge):
  - `a_broken_document_inside_a_php_tab_fails_the_suite` — a `text title="Blastfile"`
    fence indented under `=== "PHP"` with `not_a_directive` is collected, its body
    is de-indented, `parse_document` refuses it and `describe_failure` names the
    page; the same tab passes once the directive is fixed.
  - `an_eight_space_indented_document_is_de_indented_once` — an 8-space
    (admonition) fence loses exactly the opening's indent and keeps the block's
    own 4-space body.

```
$ cargo test -p blastit-manifest --test docs_fences
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

## 2. Runnable docs (`harness.py`, `inventory.toml`, `test_docs_assets.py`)

`fences.py` already found and de-indented indented `console` blocks; the harness
now *runs* them like the top-level ones, with four small changes:

- `mode_check_roots` (`check-roots`, in `EXECUTING`): a store-less static audit —
  every command must be `blastit check`, exit `0`, and a `--json` document must
  say `ok: true` with `routes.total == 0` (a mount is not a route, so the human
  form prints no audit line and `assert_clean` would demand a request count that
  can never exist).
- `prepare` no longer copies the harness's shared `pyproject.toml`/`uv.lock`
  when the fence runs `uv init`, so the documented `uv init --app && uv add
  blastit` tour is executed instead of refused with *pyproject.toml already
  exists*.
- `_serve_argv` excludes `fmt`, and `mode_summary` only calls `assert_clean` for a
  real `blastit check` command (not for `blastit fmt --check`).
- `CliHelp` gained `fmt`, probed from the installed `blastit fmt --help`, and
  `_static_blastit` accepts `blastit fmt [--check]`.

**Inventory deltas** (page → console fences, before → after):

| page | fences | notes |
|---|---|---|
| `README.md` | 3 → 3 | fence 1 is now one mixed tour (wheel install + dev/check + archive) → `example` with the harnesses that run each half named; its banner line is back to `view  hello  GET, HEAD  /.*` (the binary prints the regex, MB.41.1 had made it `/*`). |
| `docs/index.md` | 1 → 4 | fence 1–2 `summary` (shop), fence 3 `example` (PHP `dev` + preset, no libphp), fence 4 `example` (static `dev` + `check`; a mount prints no audit line, `e2e/static_assets.py` owns it). |
| `docs/getting-started.md` | 3 → 6 | fences 2–3 `example` (release archive, not fetched offline), 4 `dev` (hello), 6 `dev` (static-site), 5 `example` (PHP `dev`, no libphp). The banner line is `/*` → `/.*` again. |
| `docs/blastfile.md` | 3 → 7 | 1 `summary` shop (`dev` + `check` + `fmt --check`), 2 `summary` hello (`uv add blastit` + `dev`), 4 `dev` static-site, 5 `check` shop, 7 `check-roots` static-site; 3 and 6 are the PHP tabs, accounted with the libphp reason. |
| `docs/local.md` | 6 → 10 | 1 `dev-flags` hello (with `port_fallback`), 2/5 `dev` hello, 4 `dev` static-site, 7/10 `check-roots` static-site, 8 `learn-then-check` learn; 3/6/9 are the PHP tabs. |
| `docs/ci.md` | 5 → 1 | the page keeps one three-command `check-flags` fence (shop). |
| `docs/frameworks/static-and-spa.md` | 6 → 6 | fences 1 and 5 now **execute** (`dev` / `check-roots` on `static-site`) instead of carrying the stale "no static fixture" excuses; the page's own banner/`check` lines are bare `blastit`. |
| `docs/frameworks/php/index.md` | 2 → 2 | indices unchanged; the stale "no PHP, so `fmt --check` cannot run" reason now says the flag is checked against the CLI and `docs/blastfile.md` runs `fmt --check` for real. |

`test_docs_assets.py` was re-measured for the same rewrite: `EXPECTED_FENCES` is
now **40** `text` blocks / **32** highlighted / **8** plain (was 34 / 29 / 7);
`docs/ci.md` and `docs/local.md` rows are gone (their `text` fences are now
`console`/`yaml`), and the new spine documents are pinned.

```
$ make docs-unit
117 passed, 65 subtests passed in 1.54s
$ uv run --no-project python python/tests/runnable_docs/run.py
59 fences: 32 executed, 27 accounted, 0 failed · 25.9s
```

## 3. `rs-test-php-image` outer seccomp (`Makefile`)

Both `docker run` invocations of the target carry
`--security-opt seccomp=unconfined`, with a Makefile comment (column 0, never
inside a `\`-continued recipe line) naming Docker's default outer profile:

```make
# Docker's default outer seccomp profile (`moby/profiles/seccomp/default.json`) answers `fanotify_init`
# with EPERM, and `blastit dev`'s reload needs it; `seccomp=unconfined` on the outer container leaves
# blastit's own inner `seccomp(SECCOMP_SET_MODE_FILTER)` enforcing every view/probe sandbox inside.
```

Blastit's inner filter is untouched: the probes that assert it
(`a_probe_that_hangs_is_killed`, the seccomp witness lines) still run inside.

## Fix round (review-ux + review-arch)

Seven targeted fixes; no protected `critical_*.rs` file was touched.

1. **Six false `example` reasons are now real checks.** `docs/blastfile.md` f3,
   `docs/getting-started.md` f5, `docs/index.md` f3 and `docs/local.md` f3 moved
   to `mode = "static"` (which validates every executable, subcommand, flag and
   preset through `_static_blastit`/`_static_presets`) and their reasons now say
   what actually runs. `docs/blastfile.md` f6 and `docs/local.md` f9 moved to the
   new `fmt-check` mode, so their `fmt` command is executed (see 2).
2. **`fmt-check` (`docs/frameworks/php/index.md` f2, `docs/blastfile.md` f6,
   `docs/local.md` f9).** New `mode_fmt_check` prepares **the page's own**
   `text title="Blastfile"` document (`_page_blastfile`, tab-aware: the PHP CI tab
   has no document of its own, so the last `=== "PHP"` tab's file is used), runs
   every `blastit fmt` command with `expect(..., "exact")` and requires exit `0`;
   a non-`fmt` command in the same fence is accounted only through the
   inventory's written `reason`. `docs/local.md` f9 has no page document, so it
   formats the canonical `shop` fixture. The page's trailing newline is
   normalised with a comment explaining why (`fmt --check` judges the file).
3. **`dev-roots` for the landing page's static tab (`docs/index.md` f4).** New
   `mode_dev_roots` starts the documented `blastit dev` on `fixtures/static-site`,
   `expect`s the page's banner, then runs the fence's `blastit check` through
   `assert_root_audit` (exit `0`, stderr empty, documented output compared,
   `--json` `ok: true` with `routes.total == 0`).
4. **The static banner's mount row.** `docs/frameworks/static-and-spa.md` now
   shows `  static  /  GET, HEAD  public`; `assert_banner_rows` (called from
   `mode_dev` and `mode_dev_roots`) compares the page's documented `view`/`static`
   rows with the live ones **as a list** when the fence documents the
   `http · scheduler · dbproxy · N routes` summary, so a dropped row fails (proven
   by hand: the three-row banner is refused with the documented/live lists).
   `e2e/static_assets.py` still does what it claims (it slides a window for *all*
   documented banner lines, now four, contiguous in the live banner); its
   docstring's "first three lines" wording is stale — out of this node's `paths`.
5. **The `denial` fence is back.** `docs/local.md` §"A denial" carries the
   `sql.denied … products db parse SELECT …` line again, wired to
   `mode = "denial"` + `fixtures/denial` (fence 11), so `docs/security.md`'s
   citation is true and the `500` / `42501` promise is asserted again.
6. **`mode_check_roots` compares output.** The per-command body was extracted to
   `assert_root_audit`, which now calls `self.expect(command, code, stdout,
   "exact")` and refuses any stderr on a clean audit; `mode_check_roots` and
   `mode_dev_roots` share it.
7. **The broken-PHP-tab test is no longer tautological.** It now asserts the
   panic quotes *that* parse error (`errors.errors().first()`), that the error
   names `not_a_directive`, and only then the outer page/fence contract; the fixed
   copy of the same tab still parses and validates.

Re-measured after fix 5 (a `console` fence on `docs/local.md`):

- `docs_fences`: `docs/local.md (0, 12)` → `(0, 13)`, `TOTAL` `(17, 62)` → `(17, 63)`.
- `docs-test`: 59 → **60** fences, **36 executed, 24 accounted, 0 failed**;
  `docs-unit` 117 passed; `cargo test -p blastit-manifest --test docs_fences`
  13 passed; `make py-lint` clean.

## Acceptance

- `make check` — run once at the end; tail:

```
ok   docs: settings fence
zed-check: OK (59 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
............................................
----------------------------------------------------------------------
Ran 44 tests in 8.215s

OK
uv run --no-project tools/plan.py validate
plan OK
```

- `make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_KEEP=1 && make rs-test-php-image` — tail:

```
  ok  php 8.4 abi 1: status 200, cpu_ns 10284593 <= wall_ns 12117479, exit()+fatal contained, next request served
exit(): status 500  outcome=bailed  cpu_ns=1253068  wall_ns=18157429  cost_ns=18157429  retired pid 802  respawned pid 803 (ppid 780) uid 0  live=[803]
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.85s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
docker run --user 1000:1000 blastit-php-sapi-e2e:local  # the PR_SET_DUMPABLE(0) witness
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 28 filtered out; finished in 0.20s
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
```

## Follow-ups

- The dev banner prints the compiled regex (`/.*`) for a `"/*"` pattern. The
  harness now pins the real output, so the page and banner cannot drift again;
  showing the author's spelling would be a UX change in the product, not a doc
  fix.
