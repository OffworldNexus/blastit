# REVIEW — consistency, round 3 (MB.2.10)

Code is truth: `stage1.rs` (3 `RouteInfo::new`), `route.rs::routes_from_manifest` (1), `scheduler.rs`, `rewrite.rs`, `static_response.rs`.

## Confirmed fixed (round 2 flags)
(a) 1 + 3 = four sites, counted the same in §4.6 / §4.7 / §8. (b) ADR cited by decision number only. (c) §4.2:408-417 says `rewrite.rs` builds no header. (d) rows 20–21 are refusals in §7.1/§7.2. (e) the walk is per-segment `O_NOFOLLOW`; `RESOLVE_BENEATH` appears only for `openat2`. (f) Follow-up 1 is anchored to `MB.2.2/RESULT.md` (exists).
Statuses agree with `canned` (400/404/413 bodies), ADR decisions 4–6 agree with `R7`/`R8`/`R13`.

## MAJOR
**M1 §4.7:766-767 — the "all four" call expression cannot compile at one site.**
> **all four** `RouteInfo` construction sites chain `.with_static(route.static_policy().cloned(), manifest.app().is_some())` (`routes_from_manifest` plus the three direct `RouteInfo` builders in this file)

`stage1.rs:275` builds `RouteInfo::new(LEARN_FALLBACK_ROUTE, false, dev_uid)` — no `route` is in scope there, so `route.static_policy()` does not exist. Only `:281` and `:289` are manifest-route builders. Newly introduced by the count fix. Suggest: "the three manifest-route sites chain …; the learn fallback takes `with_static(None, has_app)` (or is left as-is)".

**M2 §4.2:252 — stale landing status.**
> `static_response.rs` joins when `MB.2.4.2` lands.

`crates/blastit-scheduler/src/static_response.rs` (317 lines) and `tests/critical_static_assets.rs` are staged in this tree, and its `respond` matches §4.2. §8:1079 still records landed line counts only for `static_asset.rs` (400) and `rewrite.rs` (166), so the sync says a shipped module is unshipped in the one paragraph MB.2.5.2 reads for its API. Say all three diff cleanly today and add `static_response.rs`'s landed count.

## MINOR
**m1 §4.2 — the "only items below are `pub`" ledger is not literal.** `pub fn http_date` (`static_response.rs:274`) is absent from §4.2, though §9:1119 claims the ledger is "literally diffable against the crate"; `status: u16 // 200 or 304` also omits the `404` of the `refusal_not_found()` defence arm the code documents.

**m2 §9:1127 — "the `405` writer is the shared canned one"** contradicts §4.4 and `canned` (`scheduler.rs:3116`), which has no `405` arm and only supplies reason/body. Say "the shared rejection helper".

**m3 §4.6:651-653 — "both `Reject` and the stored-`body_plan_error` answer release it"** reads as if every `Reject` holds a slot, but §4.3:454-456 answers a head refusal (steps 3–4) before `admit_ready`. Qualify the bullet with "in `dispatch_static`" to prevent a double-release misread.

VERDICT: FAIL
