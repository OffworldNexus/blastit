# Consistency review — static-and-spa ↔ ADR-0007 ↔ shipped code

Lens: `consistency` (goldfish, MB.2.10 doc-sync). Read: `docs/design/static-and-spa.md`, `ADR-0007`, `static_asset.rs`, `rewrite.rs`, skimmed `critical_rewrite.rs` (672 lines) and the normative page.

## Checks that pass

- **§4.2 pub surface.** Every `pub` item in `static_asset.rs` (the four size consts, `STATIC_RESOLVE_FLAGS`, `StaticRefusal`/`StaticOutcome`/`AnswerKind`/`StaticFile`/`StaticMeta`/`StaticError`/`ImmutableGlob`/`Backend`/`StaticRoot`, `open`/`open_with`/`resolve`/`resolve_with`, `ImmutableGlob::{new,matches}`, `same_inode`, `segments`, `head_segments`, `is_dotfile`, `classify`) and all four in `rewrite.rs` (`STATIC_ALLOW`, `Rewrite`, `decide`, `declares_body`) appear with the shipped signatures, variants and `Reject` field types. No missing item.
- **§4.4 second table + R8.** Matches `rewrite::decide` for all five `StaticOutcome`s × GET/HEAD/other × body/no body: `File` → 405 (non-GET/HEAD, body or not), 413 (GET/HEAD + body), else Serve; `Missing`+app → Forward; `Missing`−app / `SpaLost` / `Hidden` → 404; `Refused` → 400; only the 405 carries `Allow`. No cell differs.
- **§4.3 vs §5.2.** All 40 matrix rows' statuses match the outcome `decide` yields (rows 20/21 are mis-described only in the summaries below, not in the matrix).
- **§8 ceiling.** 1,600 lines for `tests/critical_rewrite.rs`, shared by MB.2.5.1/MB.2.5.2 per §7.2; `wc -l` = 672, matching "MB.2.5.1 landed 672".

## Findings

### MAJOR

1. **§5.2/§7.2 call two refusal rows "positive twins".** `:879` "Rows 14 and 18–21, 27–35, 37, 40 are the positive twins"; `:978` and `:979` repeat "18–21 … positive". But §5.2 row 20 (`/.well-known/../.env`) is `400` and row 21 (`/.well-known/.env`) is `404`: `head_segments` returns `Refused(Traversal)` for 20 and `Hidden` for 21 (`static_asset.rs:621–624`), and `decide` maps them to 400/404. The negative lists (`:978` "15–17, 22–26"; `:979` "1–13, 15–17, 22–26") omit 20–21, so a test author following §7.2 must assert a `200`+sha256 for two refusals. Positives are 14, 18–19, 27–35, 37, 40; 20–21 belong in the negative lists.
2. **§7.2's fixture-D cache assertion is inverted.** `:978` "`/assets/*` exact → immutable, `/index.html`/`/docs` → `no-cache` (fixture D)". Fixture D is `immutable = ["/index.html"]` (`:831`), §5.2 row 34 says `/index.html` (fixture D) is "`200` immutable", and §4.5's vector says the `/index.html` pattern matches "only as `AnswerKind::Exact`; `/` and `/dashboard` … stay `no-cache`". The no-cache answers are `/`, `/dashboard`, `/docs` (DirectoryIndex/SpaFallback), not `/index.html`; a test written literally here contradicts the matrix.
3. **ADR-0007 §4 lets a `spa = true` miss fall through to the app.** Lines 51–53: "Exact file → directory `index` → SPA `index` (only when `spa = true`) → the manifest's `app` → `404`." `StaticRoot::missed` (`static_asset.rs:376–392`) returns `Missing` only when `!self.spa`; with `spa` it answers `SpaFallback` or `SpaLost`, and `decide` (`rewrite.rs:100–115`) never forwards a `SpaLost`. §4.3 steps 8–9 and the page ("the app is never reached", `docs/frameworks/static-and-spa.md:118–119`) say so. The `app` arrow needs "when `spa` is unset".

### MINOR

4. ADR §4 "Refused and hidden paths are terminal for every method and body" and §5's 404 enumeration name 2 of the 4 terminal outcomes: `SpaLost` (step 8) and the no-app miss (step 10) are also 404 for every method/body in §4.4/R8 and `decide`. Statuses agree; the ADR's list is narrower than the design's.
5. §4.4's second table labels `Hidden` "(step 11)", but dotfiles are `Hidden` from step 4 (`head_segments` → `Err(StaticOutcome::Hidden)`, `static_asset.rs:622–624`) — the table's own `POST /.env` case. Cite "steps 4, 11"; the 404/no-`Allow` cells are right.
6. §4.2 (`:249`) says "`StaticRoot`'s fields are `pub(crate)`"; `backend` (`static_asset.rs:255`) is private and `root` is `#[allow(dead_code)]` (`:237–238`). The "only the items below are `pub`" claim holds; the field sentence overstates.
7. §4.2's `url_path` doc (`:346`) says "every alias of one file shares one key: … the same `url_path`, `ETag` and cache header". Under fixture D, `/index.html` (Exact → immutable) and `/` (DirectoryIndex → no-cache) answer the same file but have different `(AnswerKind, url_path)` cache keys, per §4.5. True only for syntactic aliases of one kind (`//`); qualify it.
8. §4.4's first table (`:481`) says a `GET`/`HEAD` with a declared body on a `Forward` is "the route's `[route.ingress]` decides, exactly as today". `plan_body` short-circuits bodyless-safe methods before the codec (`blastit-ingress/src/lib.rs:295–299`), so the answer is `BodyNotAllowed`'s stored `413`, not an ingress decision. Status right, mechanism misattributed.
9. §8 (`:1051`) says `tests/critical_static_resolve.rs` and `tests/common/static_fixture.rs` are "file ceilings shared by `MB.2.4.1` and `MB.2.4.2`", while §7.2 (`:979`) and the leaf table give the resolve suite only to MB.2.4.1 (MB.2.4.2 gets `critical_static_assets.rs`). If MB.2.4.2 appends resolve cases, say so; otherwise only the fixture is shared.

VERDICT: FAIL
