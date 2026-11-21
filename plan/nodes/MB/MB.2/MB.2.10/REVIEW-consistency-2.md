# Consistency re-review 2 — static-and-spa ↔ ADR-0007 ↔ code

Lens: `consistency` (goldfish, MB.2.10 re-review). Read: `docs/design/static-and-spa.md`, `ADR-0007`,
`crates/blastit-scheduler/src/{static_asset,rewrite}.rs`, skimmed the normative page.

## Passing checks

- **Item 1, §4.2 pub surface.** Every `pub` item appears with the shipped shape: the consts,
  `StaticRefusal` (7), `StaticOutcome` (5), `AnswerKind` (3), `StaticFile`/`StaticMeta`,
  `StaticError`, `ImmutableGlob`, `Backend`, all four `StaticRoot` methods, `same_inode`,
  `segments`, `head_segments`, `is_dotfile`, `classify`, `STATIC_ALLOW`, `Rewrite`, `decide`,
  `declares_body`; `backend` is private as stated. No missing item.
- **Item 2, §4.3/§4.4/R8/§5.2.** Every outcome × GET/HEAD/other × body/no-body matches
  `decide`: `File`→405 (`Allow` only there)/413/`Serve`; `Missing`+app→`Forward`;
  `Missing`−app, `SpaLost`, `Hidden`→404; `Refused`→400. Rows 1–40 carry exactly those
  statuses (34 `Exact` immutable, 35 index no-cache).
- **Item 3, §7.2/fixture D.** Positives `14, 18–19, 27–35, 37–38, 40`; negatives `1–13, 15–17,
  20–26, 39`; D `/index.html` immutable, `/`, `/dashboard`, `/docs` no-cache — matches §4.5 and
  rows 34–35. The three pass-1 MAJORs are fixed.
- **Item 4, ADR-0007 §4–§5.** `spa`-unset app gating, servable-target 405, terminal refusals,
  exact-file-only year cache — all match design and code.

## Findings

### MAJOR

1. **`RouteInfo` construction count: §4.6 says four, §4.7/§8 say three.** `:660–662` "**All
   four** constructions … `routes_from_manifest` and the three direct `RouteInfo` builders in
   `stage1`"; `:756` "all three `RouteInfo` constructions chain `.with_static(…)`"; `:1063`
   "`.with_static` in all three constructions". Both cannot be right; MB.2.6 must patch every
   site or one dev/`--learn` branch loses `static_forward` and cannot spawn.

### MINOR

2. **Three `ADR-0007 §3` citations point at path safety.** `:86`, `:189` (per-route, no
   top-level block) and `:942` (one `.*` plus one `app`) are ADR **§2**; §3 is `openat2`.
3. **`:415` "The `405` head is built in `rewrite.rs`"** contradicts `rewrite.rs:25–26`
   ("Nothing here … builds a header") and §4.4's `canned(405)` writer (`:521–522`).
4. **§7.1/§7.2 disagree on rows 20–21.** `:973` gives the resolve suite "1–13, 15–17, 22–26,
   39"; `:1004` gives the same suite "1–13, 15–17, 20–26 and 39". Both are `static_asset.rs`.
5. **"Follow-up 1" (`:752`) is never defined** — no numbered list in §8/§9, so its exception to
   "No leaf needs an out-of-lane edit" (`:1054`) cannot be resolved.
6. **`:917` "`RESOLVE_BENEATH` for the walk"** — `walk` passes no `ResolveFlags`; it relies on
   per-segment `O_NOFOLLOW` and dot-segment-free input (§5.1 `:804–807`).
7. *(Out of lane, report only.)* `rewrite.rs:2` cites `MB.2.5a` and `static_asset.rs:2` cites
   `MB.2.4a`; §8 records those as epics with `.1`/`.2` children.

VERDICT: FAIL
