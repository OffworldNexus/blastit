# MB.2.2 — Design static serving, rewrite precedence and path safety (ADR-0007)

## Summary

`docs/design/static-and-spa.md` turns the accepted `docs/frameworks/static-and-spa.md`
into the 0.2.0 technical design, and `ADR-0007` records the lasting decisions. The
design settles, for `MB.2.3`–`MB.2.7` (and the proposed docs leaf `MB.2.9`):

- the exact `[route.static]` grammar (`root`, `index`, `spa`, `immutable`), its
  defaults, the five new `ManifestError` variants with fixed message text, why the
  block is per-route rather than `[static.<name>]`, and that root existence is checked
  by a separate `Manifest::check_static_roots` call so `docs_fences.rs` stays green;
- one **total** rewrite order (11 steps: health → non-static route → `400` refusal →
  dotfile `404` → exact file → directory index → SPA index → SPA `404` → app → `404` →
  hidden-entry `404`) with "a refusal is final" and "a denied path is never masked by
  `index.html`";
- the path boundary (`openat2` with
  `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS`, an `O_PATH` probe plus a
  `(st_dev, st_ino)`-checked read open, a per-segment `O_NOFOLLOW` fallback, one decode,
  no normalization, a total errno table) and a 36-row attack matrix with per-fixture
  statuses that `MB.2.4`/`MB.2.5` table-drive;
- response semantics (closed `Content-Type` table, `ETag`, `Last-Modified`, `304`,
  `HEAD`, `nosniff`, and a cache key of `(answer kind, answering path)` so only an
  exact-file answer can be `immutable`), method/body gating (`405` + `Allow`, `413`),
  a bounded `spawn_blocking` body pump inside the existing admission/credit machinery
  (slot held until the read result, so a stalled mount parks at most `max_workers`
  readers), and `404`-not-`403` for hidden existence;
- the app-less `dev` boot order (app optional iff all routes static, root audit and a
  scheduler-uid readability probe before any listener is bound) and the all-static `check` contract
  (no pytest, root audit, exit `0`/`2`, unchanged `--json` shape, empty stdout),
  mirroring MB.2.6's extended `NODE.md`;
- the `R1`–`R15` → module → protected-suite oracle map, naming the four new
  `critical_*.rs` files (MB.2.3–MB.2.6), the shared static fixture, and the `MB.2.7`
  fence-by-fence replay map.

No key, flag, env var or log field is added beyond the page; `**` globs are not
invented either (one metacharacter, `*`, per page).

## Evidence

```console
$ test -f docs/design/static-and-spa.md && test -f docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md && echo "design + ADR exist"
design + ADR exist
$ grep -qi 'openat2' docs/design/static-and-spa.md && grep -q 'RESOLVE_BENEATH' docs/design/static-and-spa.md && echo "mechanism"
mechanism
$ grep -q 'public/index.php' docs/design/static-and-spa.md && echo "front controller"
front controller
$ grep -c '^|' docs/design/static-and-spa.md
186
```

```console
$ for f in docs/design/static-and-spa.review-security.md docs/design/static-and-spa.review-implementability.md docs/design/static-and-spa.review-testability.md; do tail -1 "$f" | grep -q 'VERDICT: PASS'; done && echo "three lenses pass"
three lenses pass
$ tail -1 docs/design/static-and-spa.review-security.md
VERDICT: PASS
$ tail -1 docs/design/static-and-spa.review-implementability.md
VERDICT: PASS
$ tail -1 docs/design/static-and-spa.review-testability.md
VERDICT: PASS
```

```console
$ grep -n 'static-and-spa' docs/design/README.md
8:| [static-and-spa.md](static-and-spa.md) | Blastit 0.2.0 Technical Design: Static Asset Serving, Rewrite Precedence and Path Safety | In review | MB.2.2 |
$ grep -n 'ADR-0007' docs/decisions/README.md
13:| [ADR-0007](ADR-0007-static-asset-serving-and-rewriting.md) | Static Asset Serving and Rewrite Precedence Inside the Scheduler | Proposed | MB.2.2 |
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
```

```console
$ make docs-unit
...............................................................         [100%]
63 passed, 57 subtests passed in 0.35s
```

Sizes: `docs/design/static-and-spa.md` 1015 lines / 186 table rows;
`ADR-0007` 86 lines. `make check` and `make e2e` are the lead's gates (not run here).

## Files

- `docs/design/static-and-spa.md` (new)
- `docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md` (new)
- `docs/design/README.md` (one row)
- `docs/decisions/README.md` (one row)
- `docs/design/static-and-spa.review-security.md` (new, critic)
- `docs/design/static-and-spa.review-implementability.md` (new, critic)
- `docs/design/static-and-spa.review-testability.md` (new, critic)
- `plan/nodes/MB/MB.2/MB.2.2/RESULT.md` (this file)

## Plan state (verified after the planner's 20:52 split)

The two path extensions this design used to request are now in the plan, so **no leaf
needs an out-of-lane edit**: `MB.2.6` owns `crates/blastit/src/stage1.rs`; the `MB.2.4`
epic and its children own `static_asset.rs`, `static_response.rs`,
`tests/critical_static_resolve.rs`, `tests/critical_static_assets.rs` and
`tests/common/static_fixture.rs`; `MB.2.5`'s children own `rewrite.rs`, `route.rs`,
`scheduler.rs`, `tests/critical_rewrite.rs`; `MB.2.9` owns the five public docs
(`docs/security.md`, `docs/how-it-works.md`, `docs/ci.md`,
`docs/reference/{cli,manifest}.md`) and `inventory.toml`.

Two things still have to happen before the leaves start, both owned by the lead and
neither a design change:

1. **Re-sync the child briefs.** `MB.2.4`'s epic still names a `(dev, ino, size, mtime)`
   `ETag` and one unprivileged device-node case; `MB.2.5`'s epic still implies the
   pass-1 `decide` signature; `MB.2.6` still promises a static-root banner line and
   "manifest line" messages; `MB.2.7` still says `400/404` and `E2E_STATIC_PORT` for the
   banner. Design §8 names the split and the depth-overlap override so the briefs can be
   brought into line in one pass.
2. **The children exist** as `MB.2.4.1`, `MB.2.5.1`, `MB.2.4.2`, `MB.2.5.2` (nested under
   the epics `MB.2.4`/`MB.2.5`, created 20:52; `plan.py validate` OK), with the chain
   `MB.2.3 → MB.2.4.1 → MB.2.5.1 → MB.2.4.2 → MB.2.5.2 → MB.2.6`; only their dispatch
   briefs need re-syncing (item 1).

## Follow-ups

1. **`MB.2.6`'s "banner reports static routes and their roots" conflicts with the
   page.** `docs/frameworks/static-and-spa.md:44-48` shows the unchanged three-line
   banner for the app-less block and `MB.2.7` replays it verbatim; the design keeps
   `dev_learn::banner` unchanged (design §1.2). Reword the `MB.2.6` note (Plan state 1).
2. **"manifest line" is not in the manifest error model.** `MB.2.6`'s note asks a
   missing root to name "the manifest line"; `ManifestError`/`Location` carry no spans
   (`Location::Route { index, name }` only). The design pins
   `"<manifest path>: <errors>"` with route name and index; adding spans to the parser
   is a separate change this design does not smuggle in.
3. **Symlinks inside a root are refused, not only escaping ones.** The page promises
   `404` only for a symlink that leaves `root`; `RESOLVE_NO_SYMLINKS` (mandated by this
   node's `# Context`) refuses every symlink, so Capistrano-style layouts must name the
   real path. Tightening, not a contradiction; the page could gain one sentence in
   `MB.2.9`.
4. **Nested immutable output needs one pattern per depth** (`/assets/*/*`), because
   0.2.0 ships the page's single metacharacter. A later docs node can add `**` with the
   page change it implies.
5. **`MB.2.7` must split fence 8 into two contexts** (unprivileged host → exit `1`;
   capable container → preset boots) and run fence 3 on port `8080`; both are in design
   §7.3 and need the node's assertion list updated (Plan state 1).

6. **A mechanical inventory reason→assertion check has no owner.** `MB.2.9` rewrites the
   six `inventory.toml` reasons from design §7.3, but the check that a reason naming a
   node names an assertion in it would live in
   `python/tests/runnable_docs/test_harness.py`, which is in no leaf's `paths`. Add it to
   `MB.2.9` (or a tooling leaf) if the gate should enforce it.

7. **`MB.2.9` depends only on `MB.2.2` yet edits `docs/reference/manifest.md`,** which
   `MB.2.3` also owns. Add `MB.2.3`/`MB.2.6` to `MB.2.9`'s `depends_on` (the epic
   `MB.2.5 → MB.2.4` ordering was fixed by the lead).

## Review Resolution

Pass 1: `security` 1 BLOCKER / 7 MAJOR / 2 MINOR, `implementability` 5 BLOCKER / 6
MAJOR / 1 MINOR, `testability` 3 BLOCKER / 8 MAJOR / 4 MINOR. All were resolved in the
design (which reshaped §1.4, §2, R2–R5, R7, R10, R11, R13, R15, §4.2–§4.7, §5, §7 and
the ADR), and **all three lenses were re-run**; the pass-2 findings and their
resolutions are in the table below under their `S2-`/`I2-`/`T2-` ids.

Pass 2 (new: `S11`–`S15`, `I13`–`I15`; open: `S8`, `S11`, `I11`, `I13`, `I14`, `T10`,
`T2`, `T6`, `T7`, `T13`, `T15`) was resolved by the bounded reader pool, the `MB.2.9`
docs leaf, the by-value ownership chain, the per-mode probe, the `#[doc(hidden)] pub`
backend seam, the total `classify` arm, the two fence contexts and the `MB.2.4`/`MB.2.5`
split. **Pass 3 closed every open item in all three lenses: 0 BLOCKER / 0 MAJOR, and
each artefact's last line is `VERDICT: PASS`.** Pass-3's five cosmetic residuals
(`respond(&self, …)`, `open_with → Result`, `StaticMeta` field types, the `BTreeSet`
spelling and the §4.6 slot-release sentence) were folded in afterwards and do not
change any contract.

| Finding | Severity | Resolution |
|---|---|---|
| `S1` responder's uid is not init | BLOCKER | `accepted -> §1.4, §4.7, §5.1, R13, ADR §1/Consequences`: the responder is `UID_BASE+2` (`ARCHITECTURE.md` §2.4); the root must be `0755`/`0644`-readable by it; boot step 5 probes it with the existing `visibility::unreadable` in the scheduler's dropped identity, before any fork; `StaticRoot::open` stays the last authority |
| `S2` `index` may name a dotfile | MAJOR | `accepted -> R3, §4.1, §4.2`: a dotfile segment in `index` is fatal (`must not contain a dotfile segment`); negative twin added to §7.2 |
| `S3` `immutable` may match the index / SPA answer | MAJOR | `accepted -> R5, §4.5, §5.2`: the cache key is `(answer kind, answering path)`; only `AnswerKind::Exact` is glob-matched, `DirectoryIndex`/`SpaFallback` are always `no-cache`; fixture D + rows 34/35 assert both |
| `S4` errno map incomplete and an oracle | MAJOR | `accepted -> §5.1`: one total errno table (`ENOENT`/`ENOTDIR` → `Missing`; everything else → `Hidden`), `MAX_STATIC_PATH_BYTES`/`MAX_SEGMENT_BYTES` make `ENAMETOOLONG` unreachable, and the 404-vs-fallthrough oracle is stated as the page's own hidden-existence policy, limited to the status |
| `S5` blocking reads stop the loop | MAJOR | `accepted -> R10, §4.6`: one `spawn_blocking` pread per response (≤ 64 KiB), results over a new `static_in` select arm, one read in flight, local-FS assumption stated, `request_timeout` still aborts |
| `S6` `root = "/"` both fatal and legal | MAJOR | `accepted -> R2, §4.1, §5.1`: an absolute `root` is a boot error; the "legal but publishes the container" sentence is deleted; negative twin added |
| `S7` mounts inside `root` | MAJOR | `accepted -> §5.1`: mounts are followed (no `RESOLVE_NO_XDEV`; a bind-mounted asset dir is legitimate), the boundary claimed is the path namespace, `EXDEV` stays as defence and has no tested row |
| `S8` the page's security-model link is unowned | MAJOR | `accepted -> §8 (MB.2.9)`: the docs leaf `MB.2.9` owns `docs/security.md`, `docs/how-it-works.md`, `docs/ci.md`, `docs/reference/{cli,manifest}.md` and `inventory.toml`; `MB.2.8` explicitly may not touch public docs, so it was never the owner; design §5 is the source |
| `S9` special files opened before classification | MINOR | `accepted -> §4.2, §5.1`: `O_PATH` probe + `fstat` classification, read-open (`O_RDONLY|O_NOCTTY|O_NONBLOCK`) only for a regular file; ADR §3 updated |
| `S10` nosniff / inode ETag / `.well-known` / `has_body` wording | MINOR | `accepted -> §4.5`: `X-Content-Type-Options: nosniff`; `ETag` is `"<size>-<mtime_ns>"` (no `st_dev`/`st_ino`); `has_body` documented as "declares a body". `rejected -> §5.2 row 19`: skipping the fallback for a `.well-known` miss contradicts the page ("a path it does not answer reaches the app"), so a challenge file must exist under `root` — no design change |
| `I1` no view-less admission path | BLOCKER | `accepted -> §4.6`: `can_dispatch_static`, `dispatch_static`, `admit_ready`'s static branch, both queue walks and `start` never `try_spawn` a static route, slot released at `RESP_END` |
| `I2` `decide` cannot express the table | BLOCKER | `accepted -> §4.2, §4.3`: `StaticOutcome::SpaLost`, `decide(outcome, method, has_body, has_app)`, `Reject { status, allow }`, `AnswerKind` on `StaticFile` |
| `I3` dev `RouteInfo` never gets the policy | BLOCKER | `accepted -> §4.7, Follow-up 1`: `stage1.rs` chains `.with_static` in all three constructions; path extension requested |
| `I4` `manifest_dir` has no owner | BLOCKER | `accepted -> §4.7`: `Config.manifest_dir` absolutised once at load, carried in the stage-1 document, copied by `stage1::scheduler_config` |
| `I5` all-static boot dies in stage 1 | BLOCKER | `accepted -> §4.7`: `load` returns `py.None()` and skips import/preload/warmup when there is no app; the app rule already runs in `stage0` before the forks; `Parts.app` unchanged (no `supervise.rs` change) |
| `I6` inline reads | MAJOR | `accepted -> §4.6`: same fix as `S5` |
| `I7` ingress planning runs first | MAJOR | `accepted -> §4.4`: resolve/`decide` precedes `plan_request_body`; a selected target never buffers or transcodes |
| `I8` `SchedulerError::StaticRoot` has no owner | MAJOR | `accepted -> §4.7`: reuse `SchedulerError::Protocol("static root …")` (`error.rs` untouched, no path change); `StaticRefusal`/`StaticMeta` spelled in §4.2 |
| `I9` over-long limit and "no normalization" | MAJOR | `accepted -> R11, §4.2`: `MAX_STATIC_PATH_BYTES`/`MAX_SEGMENT_BYTES` constants, `segments()` owns decode-to-segments (in `MB.2.4`), R11 says "no dot-segment resolution and no Unicode normalization; empty and `.` segments are dropped" |
| `I10` `groups` ambiguity | MAJOR | `accepted -> R4`: the rule checks only what is **declared on the route** (inline `groups`, route grants, `[route.ingress]`, `prefork`); manifest-wide groups are not re-checked |
| `I11` line budget | MAJOR | `accepted -> §8`: split `MB.2.5` at pump vs rewrite/dispatch if it passes 600 lines; never truncate the matrix |
| `I12` errno/FIFO wording | MINOR | `accepted -> §5.1`: `ENXIO`/`EISDIR` in the table; "classified, never read" |
| `T1` undecidable cache rule | BLOCKER | `accepted -> R5, §4.5`: same fix as `S3`; the glob table's `/index.html` row now says "Exact only" |
| `T2` device row has no unprivileged oracle | BLOCKER | `accepted -> §4.2, §5.2 row 36, §7.2, Follow-up 4`: `classify(FileType, len, max)` is the pure table; row 36 is e2e-only |
| `T3` permission branches untested | BLOCKER | `accepted -> §7.1, §7.2`: fixtures assert `geteuid() != 0` (hard failure), `chmod 000` file and `0o400` directory with `Drop` restore; `permission denied` asserted in both the manifest and CLI suites |
| `T4` status-only positive rows | MAJOR | `accepted -> §5.2, §7.2`: every positive row also asserts the body sha256 and the fixture `ETag`; alias-equality cases added |
| `T5` `Content-Type` table untested | MAJOR | `accepted -> §7.2`: one table-driven row per extension, plus none/uppercase/unknown |
| `T6` fallback unreachable | MAJOR | `accepted -> §4.2, §7.2`: `Backend::{Openat2, Walk}` + `open_with`; the whole matrix runs over both |
| `T7` replay has no allowlist | MAJOR | `accepted -> §7.3, Follow-up 3`: a fence-by-fence replay/exempt map with the missing assertions named |
| `T8` `400/404` is not binary | MAJOR | `accepted -> §7.3`: the driver uses the matrix's exact status (`400` where the matrix says `400`) |
| `T9` `docs_fences` proves only syntax | MAJOR | `accepted -> §7.2`: the static fence also pins `is_all_static()`, `root`, `spa`, `immutable` |
| `T10` shared fixture | MAJOR | `accepted -> §7.1, §8, Follow-up 2`: `tests/common/static_fixture.rs` owned by `MB.2.4`; `World` sets `cfg.manifest_dir` |
| `T11` tempdir collisions | MAJOR | `accepted -> §7.1`: unique leaf (`module-pid-nanos`), ownership-exclusive create, `Drop` cleanup, short socket names |
| `T12` `**` invented | MINOR | `accepted -> §2, R5, §4.5, §6 item 7, Follow-up 4`: `*` is the only metacharacter; `/**` vectors removed; nested output uses one pattern per depth |
| `T13` `http_date`/ETag flake | MINOR | `accepted -> §7.2`: vectors assigned to `critical_static_assets.rs`; the rewrite case sets a known future mtime (`utimensat`), not elapsed time |
| `T14` `outcome`/`views` unobservable | MINOR | `accepted -> §7.2`: exact `outcome` strings asserted; `views` claim replaced by `log.spawns` + "the route never sees `REQ_HEAD`" |
| `T15` inventory reasons unbacked | MINOR | `accepted -> §7.3, MB.2.9`: the design mirrors the reason text one-for-one; reconciliation after `MB.2.7` |
| `S11` timed-out reads park threads/fds without bound | MAJOR (pass 2) | `accepted -> R10, §4.6`: the admission slot is held until the read result (never released by the timeout), and only a slot-holder may spawn a read, so `slots ≤ max_workers` *is* the reader bound — no separate counter; `§4.6` adds idempotent `release_static_slot`, and `§7.1`/`§7.2` assert the fd bound and a healthy stream alongside |
| `S12` `remaining == 0` pump arm | MINOR (pass 2) | `accepted -> §4.6 step 1, §7.2`: `RESP_END` before any read; zero-length `GET`/`HEAD` on both backends asserted |
| `S13` ETag overclaim | MINOR (pass 2) | `accepted -> §4.5, §7.2`: "changes when a rewrite changes the size or the mtime"; the suite asserts one exact `ETag` computed from `stat` and a change after `utimensat` |
| `S14` "before any fork" | MINOR (pass 2) | `accepted -> §4.7`: "before any listener is bound and before any role child exists; the only pre-boot fork is the read-only probe" |
| `S15` read open does not re-assert the type | MINOR (pass 2) | `accepted -> §5.1`: the read-open `fstat` must report `S_ISREG` and the same `(st_dev, st_ino)` |
| `I13` probe uid/`layout` absent in `dev`/`check`, error prose | MAJOR (pass 2) | `accepted -> §4.7 step 5, §4.2`: production probes with `layout.scheduler()`; `dev`/`check` probe with `geteuid()` (no fork, mode bits); `Manifest::static_roots` produces the paths; the message is built as `static root <root>: permission denied`, not `Blocker::message()` |
| `I14` `decide`/`respond`/`static_target` ownership | MAJOR (pass 2) | `accepted -> §4.2`: `decide(StaticOutcome)` and `respond(self, StaticFile, …)` take the file **by value**; `Request.static_target` is the single owner until `dispatch_static` moves it; `release` drops `static_body` then `static_target`; no `dup` |
| `I15` pump/timeout residuals | MINOR (pass 2) | `accepted -> §4.2, §4.6, §4.7`: `dispatch_static` sets `dispatched_at`/`deadline`; `expire_requests`/`client_cancel` mark a view-less request complete and the idempotent `release_static_slot` frees it after its read result; `Scheduler` owns `static_roots` and the `static_in` sender; `StaticResponse.reason`; `static_max_bytes` comes only from the constant |
| `I11` budgets and stale briefs | MAJOR (pass 2) | `accepted -> §8`: `MB.2.4.1`/`MB.2.4.2` and `MB.2.5.1`/`MB.2.5.2`, production ≤ 500 lines, matrix never truncated, response construction in `static_response.rs`; child briefs re-synced first (Plan state 1) |
| `T10` design-named files outside `paths` | BLOCKER (pass 2) | `accepted -> §8, Plan state`: the planner's split puts `tests/common/static_fixture.rs` in `MB.2.4`'s epic, `stage1.rs` in `MB.2.6`, and `MB.2.9` in the tree; `MB.2.4.1`/`MB.2.5.1`/`MB.2.4.2`/`MB.2.5.2` are the real leaves |
| `T6` `#[cfg(test)]` seam invisible to integration tests | MAJOR (pass 2) | `accepted -> §4.2, §7.2`: `#[doc(hidden)] pub fn open_with` (no `cfg(test)`), so `critical_static_assets.rs` compiles against `Backend::Walk` |
| `T7` fence 3 port / fence 8 context | MAJOR (pass 2) | `accepted -> §7.3`: fence 3 replays on `127.0.0.1:8080` in its own container; fence 8 has two contexts (host, unprivileged, exit `1`; capable container, preset boots and answers `/`) |
| `T2` `classify` block-device row | MAJOR (pass 2) | `accepted -> §4.2, §7.2`: the table covers `RegularFile`/`Directory`/`Fifo`/`Socket`/`CharacterDevice`/`Symlink`, and `_ => Hidden` is the total case; `MB.2.4.1`'s device row moves to `MB.2.7` (Plan state 1) |
| `T13` ETag shape/nosniff | MINOR (pass 2) | `accepted -> §7.2`: exact `ETag` and `nosniff` assertions on `200` and `304`; rewrite claim tested with `utimensat` |
| `T15` inventory reasons drift | MINOR (pass 2) | `accepted -> §7.3, MB.2.9`: that leaf rewrites the six reasons from §7.3; a mechanical reason→assertion check needs `python/tests/runnable_docs/test_harness.py`, which no leaf owns (Follow-up 6) |

### Attempt 2 (review FAIL) — 8 findings

| Finding | Severity | Fix |
|---|---|---|
| 1 leading-`//` hole (`segments`) | MAJOR | `accepted -> R11, §4.2, §4.3, §5.2 row 6, §7.2`: the decoded path must start with exactly one `/`; a second leading `/` (`//x`, `/%2Fx`) is `StaticRefusal::Absolute`; interior `//` still collapses; the `segments` table asserts both |
| 2 global `static_reads` never re-driven | MAJOR | `accepted -> §4.6`: the separate `static_reads` gate and re-drive queue are **removed** — only a slot-holder spawns a read and a slot is held to its read result, so the bound is structural and no request can be stranded; §7.2 keeps the >`max_workers`-streams wave-completion test |
| 3 ADR contradicted §5.1 on `403`/oracle | MAJOR | `accepted -> ADR §5`: the "never `403`: no existence oracle" clause is replaced by §5.1's admitted sentence (hidden existence is the page's own `404` policy, status-only, canned body, no content) |
| 4 `(dev,ino)` check has no assertion | MAJOR | `accepted -> R11, §4.2, §7.2, §8`: `#[doc(hidden)] pub fn same_inode((u64,u64),(u64,u64)) -> bool` is tabled in `critical_static_resolve.rs`, and `#[doc(hidden)] pub resolve_with(segments, Some(hook))` swaps the leaf between probe and read open so the TOCTOU arm answers `Hidden` — a real oracle, not a mutation-audit claim |
| 5 ADR "before any fork" | MINOR | `accepted -> ADR §1/§6`: replaced with the design's sentence (before any listener is bound and any role child exists; the only pre-boot fork is the read-only probe), including the `dev`/`check` euid case |
| 6 NFC/NFD and magic-link rows missing | MINOR | `accepted -> §5.2 rows 37–39` and the fixture (`café.js` NFC, `proclink -> /proc`): NFC hits, NFD misses, a magic link behind a root symlink is `ELOOP`/`404`; `RESOLVE_NO_MAGICLINKS` is the second wall |
| 7 dead Follow-up refs, MB.2.9 row | MINOR | `accepted -> RESULT`: `T12` now cites Follow-up 4, `T15`/`S8` cite `MB.2.9`, and §8's `MB.2.9` row lists `docs/reference/manifest.md` |
| 8 old `2.4a/b` ids | MINOR | `accepted -> §8`: the real ids `MB.2.4.1`, `MB.2.5.1`, `MB.2.4.2`, `MB.2.5.2` with the chain `MB.2.3 → 2.4.1 → 2.5.1 → 2.4.2 → 2.5.2 → 2.6`, the depth-overlap override (response in `static_response.rs`, `StaticRoot` fields `pub(crate)`), and the epic parents |

Because the fixes were MAJOR and split the modules, all three lenses were re-run eight
times (passes 4–11) against the evolving text; each artefact's anchors are re-quoted and
every one ends `VERDICT: PASS` (0 BLOCKER / 0 MAJOR; the only open notes are prose). The
acceptance commands were re-run (below).

### Passes 4–11 (critic re-runs after the attempt-2 fixes)

| Finding | Severity | Fix |
|---|---|---|
| `S16`/`I16`/`T19` `(dev,ino)` call site had no writable oracle | MAJOR | `accepted -> §4.2, §7.2`: `same_inode` takes `(u64,u64)` tuples and is tabled in the protected suite; `resolve_with(segments, Some(hook))` runs a swap between probe and read open and must answer `Hidden`; the mutation-audit claim is withdrawn |
| `S19` double release of a parked slot | MAJOR | `accepted -> §4.6`: one idempotent `release_static_slot` (decrement only while `slots` is true); timeout/cancel mark complete and never free a parked read's slot |
| `T16` unreachable reader bound / unprovable re-drive | MAJOR | `accepted -> §4.6`: the `static_reads` counter, `can_spawn_read` and the re-drive queue are removed; `slots ≤ max_workers` + slot-held-to-read-result *are* the bound, and the wave-completion test asserts the real invariant |
| `S17` `url_path`/FS assumptions | MINOR | `accepted -> §4.2, §5.2, §7.1`: `url_path` is built from the resolved segments (interior `//` aliases share one `ETag`); the fixture asserts a case-sensitive, normalization-preserving filesystem as a hard failure |
| `T17` rows 37/37b filesystem precondition | MINOR | `accepted -> §7.1`: the NFD/NFC and case-sensitivity preconditions are asserted, and all four fixtures share one tree |
| `T18` row 38 cannot isolate `NO_MAGICLINKS` | MINOR | `accepted -> §5.3`: row 38 proves the symlink refusal (`ELOOP`); `NO_MAGICLINKS` is stated as the second wall, asserted by the flag set, not by an unprivileged row |
| `T15` harness-check owner | MINOR | `accepted -> Follow-up 6`: the mechanical check needs `test_harness.py`, owned by no leaf; `MB.2.9` rewrites the reasons |
| `T10`/plan graph residual | MINOR | `accepted -> Follow-up 7`: `MB.2.9` deps and the epic `depends_on` ordering |
| `I11` brief residuals | MINOR | `accepted -> §8`: `MB.2.4`'s brief note now names 36→38 rows and `MB.2.5.1`'s row includes `lib.rs`; the rest is Plan state 1 |
| `S19` (pass 5) guard too weak | MAJOR | `accepted -> §4.6`: `slot_releasable(holds_slot, pumping) = holds_slot && !pumping`, unit-tabled; release only while it holds; `static_reads` and the re-drive queue removed |
| `I17` (pass 6–9) Forward slot | BLOCKER | `accepted -> §4.6`: `Forward` releases its static slot, clears `static_pending`/`dispatched_at`/`deadline`, re-enters `admit_ready`'s ordinary queue/spawn/`dispatch`; no `dispatch_reserved` |
| `I18`/`T22` plan vs `413` contradiction | MAJOR | `accepted -> §4.4, §7.2`: policy routes are admitted at the head with `body_ready = true`; `plan_body`'s `Err` is stored in `body_plan_error`; `Serve` answers `405`/`413` and drops it; `Forward` answers it or buffers |
| `I20` static admission gate missing | MAJOR | `accepted -> §4.6`: `admit_ready` is `if can_dispatch_static { dispatch_static } else if queue full { 503 } else { enqueue }`, and both `dispatch_queued` walks take a queued `static_pending` request through `dispatch_static` |
| `S22` stored-error slot leak | MAJOR | `accepted -> §4.6`: the stored-`body_plan_error` answer releases the slot exactly like `Reject` |
| `S23`/`I21` handback deadline | MINOR | `accepted -> §4.6`: handback clears `dispatched_at`/`deadline`, records `queued_since` for a `Buffer` plan, and `expire_queue` times out a body that never ends |
| `S24`/ADR slot sentence | MINOR | `accepted -> ADR §1`: "releases that slot and re-enters the ordinary view path" |
| `T15` (pass 4) harness-check owner | MINOR | `accepted -> Follow-up 6`: `MB.2.9` rewrites the reasons; the mechanical check needs `test_harness.py`, owned by no leaf |
| `T16` (pass 4) reader bound | MAJOR | `accepted -> §4.6`: the counter/re-drive queue were deleted; `slots ≤ max_workers` with the slot held to the read result *is* the bound |
| `S16`/`I16`/`T19` (pass 4) TOCTOU oracle | MAJOR | `accepted -> §4.2, §7.2`: tuple-based `same_inode` + `resolve_with(…, Some(hook))` substitution test |
| `S17`/`T17`/`T18` | MINOR | `accepted -> §4.2, §5.2 rows 37–40, §5.3, §7.1`: `url_path` from resolved segments, alias row 40, fixture FS preconditions, `ResolveFlags` assertion for `NO_MAGICLINKS` |

### Attempt 3 (review FAIL) — 2 MAJOR + 2 MINOR, plus one plan fix verified

| Finding | Severity | Fix |
|---|---|---|
| the `MB.2.4.2`-after-`MB.2.5.1` chain deadlocked against epic `MB.2.5 → MB.2.4` | MAJOR (plan) | **fixed by the lead**: `MB.2.5`'s `depends_on` is `[MB.1, MB.2.3]`; verified, §8's chain stands |
| RESULT still told the lead to create children that exist | MAJOR | `accepted -> Plan state 2`: the four children exist under the epics (created 20:52), the chain is `2.3 → 2.4.1 → 2.5.1 → 2.4.2 → 2.5.2 → 2.6`, only briefs need re-syncing |
| the `RESOLVE_*` flags had no readable surface for the suite | MAJOR | `accepted -> §4.2, §5.3, §7.2, §8`: `#[doc(hidden)] pub const STATIC_RESOLVE_FLAGS: ResolveFlags`, named as the assertion in `critical_static_resolve.rs` |
| R11 credited the mutation audit, §7.2 credited `resolve_with` | MINOR | `accepted -> R11`: one owner — the `resolve_with` substitution test exercises the identity-check call site |
| Follow-ups 6/7 sat under Review Resolution | MINOR | `accepted -> RESULT`: both moved under `## Follow-ups`; item 7 drops the plan ordering the lead fixed |

