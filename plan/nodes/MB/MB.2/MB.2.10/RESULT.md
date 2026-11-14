# RESULT — MB.2.10: sync the static design with the shipped API surface and status rules

STATUS: done
Base: `d37f765` on `release/0.2.0`. The doc also covers `b8e8806` (`MB.2.4.2`) and the
`MB.2.5.2` flow half now on disk (`scheduler.rs`, `critical_rewrite.rs`, uncommitted),
which were read as the source of truth for the oracle map.

## Summary

Six doc-sync findings are in the design, and the design matches the code on disk:

1. §4.2 lists `static_asset::head_segments` — the `#[doc(hidden)] pub` head-step entry
   `MB.2.5.2` must call — with its justification (`path::percent_decode` is
   `pub(crate)`; the protected suites compile the crate as a dependency).
2. §4.2 lists `rewrite::{STATIC_ALLOW, declares_body}` and the landed
   `static_response::{StaticResponse, StaticBody, respond, http_date}` surface (plus the
   unreachable over-ceiling `404` arm), so "only the items below are `pub`" is literally
   diffable: 19 `pub fn`/`pub const` in `static_asset.rs` + `rewrite.rs`, all in §4.2.
3. §4.4 states that a refusal is terminal for **every** method and body: `Refused` →
   `400`, `Hidden`/`SpaLost` → `404` even on `POST`, and only a **servable** target earns
   `405` with exactly `Allow: GET, HEAD`. `R8` and the ADR agree; a `POST` to a `spa`
   route is `405` when the fallback resolves and `404` when it does not.
4. §8 records `tests/critical_rewrite.rs` as one file owned by **two** leaves
   (`MB.2.5.1` landed 672; `MB.2.5.2`'s flow half brings it to 1,365) under a
   **1,600-line file ceiling**, and a leaf's changed-line budget never justifies trimming
   a §7.2 case.
5. §5.2 rows 32/33/40 read `no-cache` in the F/N columns, because fixtures F and N
   declare `immutable = []`; the year-long rule keeps real-file oracles in the S column
   and in fixture D (`/index.html` `immutable` vs `/` `no-cache`).
6. §7.1/§7.2/§8 now credit **each of the three suites with the rows it actually drives**
   (table below) — no suite is credited with another's matrix.

The code was treated as the source of truth: no behaviour, status, key or Rust change is
implied, and no discrepancy turned out to be a code defect.

## Oracle map (re-read from the files, attempt 2)

Counts: `grep -c '^#\[test\]'` → **11** (`critical_static_assets.rs`), **14**
(`critical_static_resolve.rs`), **13** (`critical_rewrite.rs`); the assets file's
`cases()` has **24** entries (`:144–306`).

| File | Rows it drives | What it owns |
|---|---|---|
| `critical_static_resolve.rs` (14 tests) | the full **40-row** `matrix()` (`:151–364`), over `Backend::Openat2` **and** `Backend::Walk` | resolution and `classify`: refusals, hidden existence, bytes/`ETag` of the answering file, `segments`, `same_inode`, `STATIC_RESOLVE_FLAGS` |
| `critical_static_assets.rs` (11 tests) | 24 `200` cases — numbered rows **18, 28, 30, 32–35, 37, 40**, plus the page's unnumbered plain-asset, `/index.html`, `/`, `/dashboard/settings` and `/assets/missing.js` rows and the F/N twins; its one `decide` status test adds `400` for rows **1/2/6/7** and `404` for **15/16/22/25/26**, plus `405`/`Allow`, `413`, `Serve`, `Forward`/`404`/`index`, `SpaLost` | the head rules: `Content-Type`, `Content-Length`, `ETag`, `Last-Modified`, `nosniff`, the `(kind, url_path)` cache rule, `304`, `HEAD`, `http_date` |
| `critical_rewrite.rs` (13 tests) | rows **1–13** → `400` for GET/HEAD/POST; **15–17, 21–26, 39** → `404` for GET/POST; row **20** → `400`; miss rows **14, 19, 27, 29, 31, 38**; row **37** as the exact NFC twin | the total order at `decide`, refusal terminality, denial-not-masked, `405`/`Allow`, `413`, forward/re-enter, no view spawn, pump/timeout/slot, short read → `RespAbort { reason: "static: read failed" }` |

Rows 14, 17, 19, 20, 21, 23, 24, 27, 31 and 38 are **not** in
`critical_static_assets.rs`; the same §7.2 table now says so, and `RespAbort` is credited
to `critical_rewrite.rs`, not to the response suite.

## Evidence

### 1. API surface (NODE Acceptance #1)

```console
$ grep -n 'head_segments\|declares_body' docs/design/static-and-spa.md
285:#[doc(hidden)] pub fn head_segments(target: &str) -> Result<Vec<String>, StaticOutcome>;
416:pub fn declares_body(headers: &[(Vec<u8>, Vec<u8>)]) -> bool;
423:`has_body` is `rewrite::declares_body`'s result and nothing else: `MB.2.5.2` computes it
435:| 3 | `head_segments(target)` refuses the path (§5.2 rows 1–13) | `400`, final |
447:malformed or hidden path is answered immediately, even under load) by `head_segments()`
459:`rewrite::declares_body` takes from `head.headers()` and stores only the resulting
```

```console
$ grep -rn 'pub fn \|pub const ' crates/blastit-scheduler/src/{static_asset.rs,rewrite.rs} | grep -v 'pub(crate)' | wc -l
19
```

### 2. Refusal terminality (NODE Acceptance #2)

```console
$ grep -n 'terminal\|405' docs/design/static-and-spa.md | head -6
34:| `GET`/`HEAD` only, `405` + `Allow: GET, HEAD`, body → `413` | §4.4 |
123:  `GET`/`HEAD` is `405` + exactly `Allow: GET, HEAD`; a `GET`/`HEAD` that declares a
126:  `405`/`413`. A path with no static target and an `app` forwards to it with the route's
392:/// The exact `Allow` value a static `405` carries: `b"GET, HEAD"` (`R8`, §4.4).
400:    Reject { status: u16, allow: Option<&'static [u8]> },  // 400/404/405/413
425:builds no header: the `405`'s `Allow` value is chosen there (`allow = Some(STATIC_ALLOW)`)
```

The rule is §4.4's second table (`§4.3 outcome, any method and any body`): `Refused`,
`Hidden`, `SpaLost` and the no-app miss carry `400`/`404` and no `Allow`, under the bold
"A refusal is terminal for every method, and only a servable target is ever `405`."

### 3. Oracle map (attempt-2 re-check; NODE Acceptance #3)

```console
$ grep -c '^#\[test\]' crates/blastit-scheduler/tests/{critical_static_assets,critical_static_resolve,critical_rewrite}.rs
crates/blastit-scheduler/tests/critical_static_assets.rs:11
crates/blastit-scheduler/tests/critical_static_resolve.rs:14
crates/blastit-scheduler/tests/critical_rewrite.rs:13

$ sed -n '144,306p' crates/blastit-scheduler/tests/critical_static_assets.rs | grep -c '^\s*case('
24

$ for t in '%252e' '/DOCS' 'cafe%CC' '.well-known/missing' '/assets"'; do printf '%-20s assets=%s\n' "$t" "$(grep -c -- "$t" crates/blastit-scheduler/tests/critical_static_assets.rs)"; done
%252e                 assets=0
/DOCS                 assets=0
cafe%CC               assets=0
.well-known/missing   assets=0
/assets"              assets=0

$ grep -n 'RespAbort' crates/blastit-scheduler/tests/critical_rewrite.rs | tail -1
1196:        Some(Frame::RespAbort { reason }) => assert_eq!(reason, "static: read failed"),

$ wc -l crates/blastit-scheduler/tests/critical_rewrite.rs
1365 crates/blastit-scheduler/tests/critical_rewrite.rs
```

Rows 14, 19, 27, 31 and 38 (and 17/20/21/23/24) are absent from the response suite, so
§7.2's `R5`/`R9`/`R10` cell no longer claims them; the split is the table above and is
copied verbatim into the design. Row 20 is `400` (`critical_rewrite.rs:402–408`) and the
`R7` cell claims only the `StaticOutcome` (`critical_static_resolve.rs` never decides a
status), while the fixture-D cache clause names only `/` and `/dashboard` — `/docs` is an
S case.

### 4. Docs gates (NODE Acceptance #4)

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.33s
```

```console
$ make docs-unit
...............................................................         [100%]
63 passed, 57 subtests passed in 0.39s
```

### Critics

| Round | Lens | Findings | Verdict |
|---|---|---|---|
| 1 | consistency | 0 / 3 / 6 | FAIL |
| 1 | implementability | 0 / 5 / 3 | FAIL |
| 2 | consistency | 0 / 1 / 6 | FAIL |
| 2 | implementability | 0 / 3 / 4 | FAIL |
| 3 | consistency | 0 / 2 / 3 | FAIL |
| 3 | implementability | 1 / 1 / 3 | FAIL |
| 4 | consistency | 0 / 0 / 2 | FAIL (minors only) |
| 4 | implementability | 0 / 0 / 3 | PASS |

All BLOCKER/MAJOR findings are resolved; the round-4 minors are fixed too. Reports:
`REVIEW-{consistency,implementability}{,-2,-3,-4}.md` in this directory.

## Files

| File | Change |
|---|---|
| `docs/design/static-and-spa.md` | §3 `R8`; §4.2 (surface + reasons); §4.3 (head vs slot work, register-then-refuse, one header copy); §4.4 (second table, `has_app`, one rejection helper, no pinned `405` body); §5.2 (F/N `no-cache`, positive-row lists); §5.3 (walk wording); §7.1–§7.2 (per-file oracle map, `400` for rows 9/11/12, `no-store`, `RespAbort` owner); §8 (budgets, shared suite, un-re-synced brief); §9 (pass 3) |
| `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md` | decisions 4–5: `spa`-unset-`app` order, servable-target `405`, terminal refusal for every method, decision-number citations |

No Rust, test, manifest, plan-status or dependency change. No pre-existing critical file
touched.

## Deviations (named)

1. **Six items, not four.** Items 1–4 came from the dispatch; items 5–6 from
   `MB.2.4.2`'s review while this node ran; both are folded into the same two files.
2. **§4.2 covers `MB.2.4.2`'s surface** because that leaf landed (`b8e8806`) mid-node.
3. **Latent contradictions fixed beyond the six**: §7.2 called rows 20–21 positive twins
   (they are `400`/`404`), fixture D's `/index.html` `no-cache` (it is `immutable`), rows
   9/11/12 `404` (they are `400`), "`RESOLVE_BENEATH` for the walk" (the walk is
   per-segment `O_NOFOLLOW`), and `docs/reference/manifest.md:260` (the rule is at `:307`).
4. **`slot_releasable` stays `pub(crate)`**; a protected-suite oracle would need a
   `lib.rs` export outside `MB.2.5.2`'s paths, so a `#[cfg(test)]` table in
   `scheduler.rs` is specified instead.
5. **Attempt 1's oracle map was wrong and is replaced.** It credited
   `critical_static_assets.rs` with rows 14, 19, 27–35, 37–38, 40 and rows 15–17/20–26
   "through `decide`", and with the short-read `RespAbort`; the file drives only the 24
   `200` cases above plus a status test covering `400` rows 1/2/6/7 and `404` rows
   15/16/22/25/26. Attempt 2 read all three files and §7.1/§7.2/§8 now state the real
   split. The `canned(405)` wart is no longer pinned as the contract either.
6. **Attempt 3 (review) fixed four mechanical claims in the same map**: row 20
   (`/.well-known/../.env`) is `400`, not `404`, in §7.2 `R5`/`R6` and §9; the `R7`
   negative clause says `outcome + "no byte of the target"`, not `status + …`
   (`critical_static_resolve.rs` never decides a status); the fixture-D cache clause
   names only `/` and `/dashboard` (`/docs` is an S case); and the shared file is
   **1,365** lines, not 1,353. No table was restructured.

## Follow-ups

1. **Resync `MB.2.5.2`'s `NODE.md` before it starts** (`status: ready`): it still names
   the removed `static_reads` counter, says `decide` runs before `plan_request_body`, and
   its ≤ 600 changed-lines budget does not mention the shared `critical_rewrite.rs`
   ceiling. Its flow half is already on disk at 1,365 lines, inside that ceiling. Node
   file is outside this node's paths.
2. **`canned(405)`**: the lead routes the proper
   `405 => ("Method Not Allowed", "method not allowed\n")` arm to `MB.2.5.2`; the design
   now fixes only the status and `Allow` and does not pin the generic pair.
3. **`scheduler.rs`'s `begin_raw` comment** promises "no header is copied into an owned
   `Vec`", which `declares_body`'s `&[(Vec<u8>, Vec<u8>)]` signature contradicts on a
   static route; the design records the one-copy exception, and `MB.2.5.2` owns the file
   and should amend the comment (or a later leaf gives `declares_body` a borrowed form).
4. **Retired node ids in code comments** (`MB.2.4a`, `MB.2.5a`, `MB.2.5b`) no longer
   exist in `plan/`; comments only, no behaviour. Left for a rustdoc-cleanup leaf.
5. **`MB.2.4.2`'s fixture follow-up is closed**: it asked either the frozen F/N fixture
   or the design to change; this node changed the design's §5.2 cells and documented why,
   so no fixture change is needed.
