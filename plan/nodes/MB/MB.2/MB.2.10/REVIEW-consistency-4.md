# Consistency review 4 — `docs/design/static-and-spa.md` (final verification)

Scope: the five fixes of the previous pass only. Code is truth: `tests/common/static_fixture.rs`, `src/static_response.rs`, `src/rewrite.rs`, `src/stage1.rs`.

1. **FIXED** — §5.2 rows 32/33/40 (lines 923/924/931) read `no-cache` in F/N, matching `Fixture::F | Fixture::N => (false, &[])`; the real-fixture oracles survive: S's `/assets/*` asset stays `immutable`, fixture D's `/index.html` (34) is `immutable` while `/` and `/dashboard` (35) are `no-cache`. Explainer at 937–942 says the same.
2. **MINOR** — §7.1 (1013), §7.2 (1042) and the `MB.2.4.2` row (1100) correctly give 24 response cases through `decide` and leave the refusal rows to `critical_static_resolve.rs`, but §8's chain sentence (1078) still says "its 40-row matrix drives the whole table **through `decide`**", re-owning all 40 rows to `MB.2.4.2` and contradicting §9:1178 ("that file owns 24 response cases").
3. **FIXED** — §4.2 (360–381) lists exactly `StaticResponse`, `StaticBody`, `respond`, `http_date`; all four are the module's only `pub` items, and "landed with `MB.2.4.2`" (252) replaces the old "joins when" wording.
4. **FIXED** — §4.6 (678–687), §4.7 (788–791) and §8 (1102) name the same three declared-route sites (`routes_from_manifest` plus the two `stage1` dev branches) and keep `LEARN_FALLBACK_ROUTE` on `RouteInfo::new` defaults; `stage1.rs:275/281/289/293` matches. `slot_releasable` is `pub(crate)` with the `#[cfg(test)]` table in `scheduler.rs` (§4.6, §9); `MB.2.5.2` is unlanded, so only the doc can be checked.
5. **MINOR** — line 486 cites `docs/reference/manifest.md:260` as already stating "first matching route wins"; that line is the SQL-grant text ending "an entry." The rule is at `manifest.md:307` (MB.2.3's `[route.static]` section shifted it). Drop the line number or point at 307.
   No other dangling reference found: page headings (`Front controller`, five-row `What is served`), `ADR-0005`, `ADR-0007` decisions 1–6 and every `§`/`R1..R15` target exist.

BLOCKER 0 · MAJOR 0 · MINOR 2 — item 5's sweep is not clean (one-line doc fix); no behaviour, status, key or Rust change implied.
VERDICT: FAIL
