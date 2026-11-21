# Result MB.2.1

## Summary

Wrote `docs/frameworks/static-and-spa.md` (147 lines, 10 documented commands, 8 console fences): the one page that defines `0.2.0`. It opens with a working 10-line `blastit.toml` (all-static route: `root`, `spa`, `immutable`, no `app`), then exactly three environments — Local (the Vite build target, `uv add blastit`, `uv run blastit dev`, two `curl -i` proofs for the immutable asset and the SPA fallback), CI/CD (all-static `check`/`--json` — audits every root, no pytest, exit `0`/`2`; the with-`app` case defers to `ci.md`) and Deployments (the same block under `compose-traefik`, executed by the harness's `presets` mode, linked to `deploy/`, not re-taught). The rewrite table is total (file → directory index → SPA → `app` → `404`), with one status per refusal class (`404` hides a symlink escape like a dotfile), `/.well-known/` exempt, and the front-controller case (`public/index.php`) stated with its `0.2.0` rewrite / `0.2.1` worker boundary. Security is a two-sentence promise that links out; no kernel mechanics.

Registered in the same change: a `[[page]]` entry plus one `[[page.fence]]` per fence, a nav entry, a link from `docs/frameworks/index.md`, and a complete static-site fixture. Two fresh-context critics (`dx`, `ai-ergonomics`) reviewed; every BLOCKER/MAJOR was resolved and both files end `VERDICT: PASS`.

## Evidence

```console
$ test "$(wc -l < docs/frameworks/static-and-spa.md)" -le 150 && echo "page is lean"
page is lean

$ test "$(grep -c '^\$ ' docs/frameworks/static-and-spa.md)" -ge 6 && echo "local + CI + deploy commands shown"
local + CI + deploy commands shown

$ grep -q 'public/index.php' docs/frameworks/static-and-spa.md && echo "public/index.php present"
public/index.php present

$ make docs-unit
63 passed, 57 subtests passed in 0.35s

$ make docs-check
Build started
No issues found
Build finished in 0.24s

$ make docs-test
PASS  docs/frameworks/static-and-spa.md  fence 1  example  0.0s  accounted: Vite writes `dist/` and this page serves `public/`; ...
PASS  docs/frameworks/static-and-spa.md  fence 2  install  0.2s  installed blastit-0.1.2-...whl and verified it against the built wheel
...
PASS  docs/frameworks/static-and-spa.md  fence 8  presets  0.0s  each preset exists and fails closed without capabilities
46 fences: 28 executed, 18 accounted, 0 failed · 23.2s

$ CARGO_NET_OFFLINE=true cargo test -p blastit-manifest --test docs_fences --locked
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

$ for f in docs/design/static-and-spa.review-dx.md docs/design/static-and-spa.review-ai-ergonomics.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "both lenses pass"
both lenses pass
```

Fence modes: 1 `example` (Vite builds outside this gate), 2 `install` (executes), 3/6/7 `static` (command + flags checked against the installed CLI), 4/5 `example` (accounted: the responder ships in MB.2.4/MB.2.5), 8 `presets` (executes `blastit --preset compose-traefik` and proves exit `1` plus the fail-closed message). Every accounted fence carries a written reason naming its replacement proof — `e2e/static_assets.py` replays this page's own commands in MB.2.7.

## Files

- `docs/frameworks/static-and-spa.md` (new)
- `docs/frameworks/index.md` (Next link)
- `zensical.toml` (Frameworks nav entry)
- `python/tests/runnable_docs/inventory.toml` (page entry + 8 fence entries)
- `python/tests/runnable_docs/fixtures/static-site/blastit.toml` (new)
- `python/tests/runnable_docs/fixtures/static-site/public/index.html` (new)
- `python/tests/runnable_docs/fixtures/static-site/public/assets/app.4f1c2a9b7d3e.js` (new)
- `python/tests/runnable_docs/fixtures/static-site/public/docs/index.html` (new)
- `docs/design/static-and-spa.review-dx.md` (new)
- `docs/design/static-and-spa.review-ai-ergonomics.md` (new)
- `plan/nodes/MB/MB.2/MB.2.1/RESULT.md` (this file)

## Follow-ups

1. **`docs/ci.md` and `docs/reference/cli.md` still describe the old `check`.** MB.2.6 now owns the all-static path (lead extended its `NODE.md`), but neither linked page is in its `paths`: add both so the exit table, the "Your suite" paragraph, the bootstrap paragraph and the `no_observations` row gain the all-static exception in the same release.
2. **The harness's `static` mode cannot parse `curl`.** The two served-output fences are therefore registered `example` (accounted, reason names the exact MB.2.7 proof) rather than `static`; if `MB.2.7` wants them statically checked, the harness needs `curl` in its command registry.
3. **`docs/security.md` and `docs/how-it-works.md` have no static section yet.** The page links both (NODE requires it) but their content lands with `MB.2.8`'s `ARCHITECTURE.md` sync; until then the pointers promise more than they deliver.
4. **The front-controller fence only proves the page's app-less block.** The PHP route's own replay needs the FastCGI worker (`0.2.1`, `MB.3`); the inventory reason records that split for `MB.2.7`/`0.2.1` rather than claiming it here.

## Review Resolution

`docs/design/static-and-spa.review-dx.md` (14 findings) and `...review-ai-ergonomics.md` (20 findings); both re-checked against the final page and both end `VERDICT: PASS`.

| Finding | Severity | Fix |
|---|---|---|
| B1 `check` can never reach `0` for a static site | BLOCKER | All-static path defined: no `app` → `check` audits every root (exists, directory, readable), no pytest, exit `0`; missing/non-directory root `2`, matching the extended MB.2.6. |
| A1/A8 manifest omits `app` without the rule; "route's app" is no key | BLOCKER/MAJOR | "An all-static manifest needs no `app`"; the fallback rows name "the manifest's `app`"; `spa` is "instead of the app". |
| A2 method/body rule kills the front controller | BLOCKER | Scoped: the responder answers `GET`/`HEAD` on paths it would answer; a path it does not answer reaches the app with the route's `methods`/`[route.ingress]`. |
| A3 `check` cannot pass on the page's own project | BLOCKER | Same all-static path as B1; fence 6's reason names MB.2.6's path. |
| B2 Vite writes `dist/`, page serves `public/` | MAJOR | `npx vite build --outDir public`, now its own `example`-registered fence with a written reason, one canonical target. |
| B3 opening flow out of order, no install branch | MAJOR | One ordered `## Run it`: build target, `uv init --app`/`uv add blastit` ("no `pyproject.toml` yet"), then the dev banner. |
| B4/A11 PHP rewrite stated as current, caveat last | MAJOR | Rewrite is the manifest's `app`; `public/index.php` and its worker are labelled `0.2.1`. |
| B7 `--json` shown for nobody | MINOR | One clause says what it is ("the same verdict as one document, for an agent"), and CI/CD owns the contract. |
| B9 deploy fence looks runnable locally | MINOR | "In the container image" before the fence, "Locally that exits `1` on purpose" after it. |
| B10 "no regex is involved" contradicts the manifest | MINOR | "the manifest keeps one `.*`". |
| B11 a front controller must omit `spa`, unsaid | MAJOR | Section opens: "A front-controller route omits `spa`: with `spa = true` the fallback answers first, so the app is never reached." |
| B13 all-static exemption contradicted by linked pages | MINOR | Stated in the page's own words and confined to `ci.md`'s "every manifest with an `app`"; the cross-page edit is Follow-up 1. |
| A4 static finding kind / JSON shape undefined | MAJOR | The unowned warning sentence was removed; only `unrouted_request` remains, the enum and exit codes stay `ci.md`'s, so no vocabulary is invented. |
| A5/A6/A7 no key table, glob semantics, incomplete precedence | MAJOR | Key table with defaults for `root`/`index`/`spa`/`immutable`; globs matched whole on the answering file's path, `*` stops at `/`; five ordered rows with statuses, directory fallthrough and missing fallback. |
| A9 entry-script exclusion has no declaring key | MAJOR | Moved into `0.2.1`: the worker "refuses to serve it as a file"; `0.2.0` promises nothing it cannot identify. |
| A10 refusal statuses a coin flip; `.well-known` | MAJOR | One status per class (`400` traversal/NUL/control/absolute/backslash; `404` a symlink escape or a dotfile, so neither reveals existence), `.well-known/` public, refusal final. |
| A14 all-static `check` contradicts `ci.md`/`cli.md` | MAJOR | Page states the exception in its own words and confines `ci.md` to "every manifest with an `app`"; MB.2.6 now owns the behaviour, the cross-page edit is Follow-up 1. |
| A15 immutable keyed on the request path caches fallback HTML | MAJOR | Keyed on the answering file's path; "every other answer, `index` included, is `no-cache`". |
| B5/A17 the second `check` fence proves nothing | MINOR | Kept (NODE requires a static-accounted fence there) and given its real reason: the block has no app, so `check` proves only its root. |
| B6/A12/B12/B14/A13/A18/A19/A20 polish | MINOR | Exit codes linked instead of re-taught; `405` carries `Allow: GET, HEAD`; `HEAD` is `GET` without a body; last row is "no file, no `spa`, no `app` → `404`"; fence reasons reworded; immutable's canonical form; the discriminator is the manifest (`app`), not the suite. |
| A16 security/how-it-works pointers have no static content | MINOR (open, accepted) | Follow-up 3: the pointers are required by NODE and land with MB.2.8's sync. |

### Gate re-review (attempt 2)

| Finding | Severity | Fix |
|---|---|---|
| Fence 8 was `static` with a false "no capabilities" reason, though `presets` runs it today | MAJOR | `mode = "presets"` and the reason deleted; `make docs-test` now executes it (`46 fences: 28 executed`). |
| All-static `check` attributed to MB.2.6 before that node owned it | MAJOR | MB.2.6's `NODE.md` gained "# All-static `check` path"; the page now matches it word for word: audit every root (exists, directory, readable), no pytest, exit `0`, missing/non-directory `2`. |
| Review Resolution omitted B7, B9, B10 and implied B13 | MINOR | Four explicit rows added above. |
| B1's resolution quoted text no longer on disk | MINOR | `review-dx.md` re-quotes the final CI paragraph and records the re-confirmation against the final revision. |
| Symlink escape said `400`, MB.2.7 asserts `404` | MINOR | Symlink escape moved to the `404` group with dotfiles; `review-ai-ergonomics.md` A10 updated to match. |
| `npx vite build` sat in prose, unaccounted | MINOR | Its own `console` fence, `example`-registered with a reason naming MB.2.7; page is 147 lines, 10 commands, 8 fences. |
