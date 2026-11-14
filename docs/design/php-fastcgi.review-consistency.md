# Consistency review — Round 4: `php-fastcgi` + ADR-0008

- **Lens:** `consistency` (fresh context, Round 4)
- **Reviewer:** goldfish critic
- **Date:** 2026-11-14
- **Scope:** verify the four Round-3 MAJORs on disk, then re-run the lens across
  `docs/frameworks/php.md`, `docs/reference/manifest.md` (`[route.fastcgi]` region and the
  three planned edits), design §4.1 / §4.8.2 / §4.10.3 / §7.2 (`R11`) / §8 / §8.1 / §8.2 / §9,
  `ADR-0007`, `ADR-0008`, `ARCHITECTURE.md` §5.3/§8.1, and the `plan/nodes/MB/**` tree.

**Verdict: PASS. 0 BLOCKER, 0 MAJOR, 15 MINOR.** All four Round-3 MAJORs are fixed on disk;
the page, the planned manifest-reference edits, the design and ADR-0008 now agree on the
three keys, `HOME`, and `pm.max_requests`; one executor-kind source, one observation
vocabulary and one error mapping survive; §8.2 now carries the `exchange` row; §8.1 item 1
carries the file sets. The MINORs below are accepted follow-ups; none changes an implementer's
or `planner`'s target.

---

## Round-3 MAJOR verification (all fixed)

| Round-3 id | State | Evidence on disk |
|---|---|---|
| `C8-M1` `HOME` | **fixed** | §4.1:459 `env[HOME]=<runtime>/views/<uid>/tmp`; §4.8.2:1083 `env[HOME]=<runtime>/views/<uid>/tmp`; §7.2:1586 `HOME == TMPDIR == <runtime>/views/<uid>/tmp`; `ADR-0008:118-119` both `<runtime>/views/<uid>/tmp`; §9:2197. No stray `HOME=<runtime>/views/<uid>` remains. |
| `C8-M2` `pm.max_requests` | **fixed** | §4.8.2:1083 and :1084 both `max(BLASTIT_VIEW_MAX_REQUESTS, 500)`; `R11` §3:207, §7.2:1586, `ADR-0008:171`, §8.2:1831 identical. `DEFAULT_POOL_MAX_REQUESTS` is now only the `500` floor, not a competing formula. |
| `C8-M3` `exchange` sync row | **fixed** | §8.2:1818 adds `exchange(&self, plan: UpstreamPlan, body: Box<dyn BodySource + Send>, sink: Box<dyn ResponseSink + Send>) -> BoxFuture<'_, Result<ExchangeOutcome, UpstreamError>>`, `ExchangeOutcome::{Answered{status,bytes_in,bytes_out,protocol_status,dropped,observations}, Declined{next,plan}}`, `Observation { target, #[serde(flatten)] kind }`, and `Scheduler::session(..., permit: RoutePermit)` `pub(crate)`, matching §4.2:504-509, §4.2.1:537-543, §4.2.1:549. |
| `C8-M4` §8.1 file sets | **fixed** | §8.1:1792-1798 every split bullet now ends "each with **exactly the `Files` cell of §8's row for that id**"; spot-checked each child's Files cell against its Acceptance command (all acceptance files are in the leaf's `paths`). |

## Gate checks that hold

- **Three keys, one vocabulary.** `entry`/`listen`/`workers = 4`, no `env` key, in
  `php.md:16-19, 26-30`, §4.1:270-280, `ADR-0008:86-87`, and the planned `manifest.md`
  edits (§4.1:464-480, §8.2:1816).
- **One boot predicate.** `can_boot_without_app()` = `!routes.is_empty() && all(fastcgi ||
  static)` in §4.2:667-668, §8:1661-1662, `ADR-0008:253`; `is_all_static()` identical in
  §4.2:665-666, §8:1659-1660, `ADR-0008:254-255`; the `check` order is identical in
  §1.3:110-121, §4.10.3:1258-1266, §8:1666, `ADR-0008:255-262`.
- **One executor-kind source.** `blastit_manifest::Executor` + `ALL_KINDS` + `"static"` in
  §4.2:647-654, `ADR-0008:27-32`, §8's type matrix (`MB.3.5.1` owns, `MB.9.1.1` reads);
  "manifest backend name" survives only as quoted *stale* brief text in §8.2:1817/1824.
- **One observation vocabulary.** `Observation { target, #[serde(flatten)] kind }`,
  `ObservationKind` internally tagged, `wire_request`/`static_file` only in
  `http_obs.jsonl`, `executor_ran` semantics identical in §4.2.1:543, §4.10.1:1207-1216,
  §4.10.3:1274-1281.
- **One error mapping.** `Refusal::status()` sole authority; `Unavailable` → `503` +
  `Retry-After`, `Fault` → `502`, `Cancelled` → none (§4.2.1:539-540); `LockdownError`/
  `CgiError`/`RecordError` map only at `MB.3.6.2.1` (§4.5.1:838-841, `ADR-0008:139-142`).
- **ADR-0007 amendments.** Explicit `Amends` block (`ADR-0008:270-283`) covering decisions
  1 (socket/filter), 4 (precedence/`.php` `404`), 6 (`app` optional); the `ADR-0007` banner
  is scheduled in `MB.3.8` (§8:1692, §8.1:1810).
- **Log events.** `app.output` (design §4.2:568-571, §4.5.2:868, `ARCHITECTURE.md:1490`),
  `process.*` (design §4.8.2:1082, `ARCHITECTURE.md:1488`), and `wire_request` (design
  §4.2.1, §4.10.1) carry one name each; `Ctl::Pool*` are ctl frames, not log events.
- **Plan ids.** Every id in §8/§8.1 exists on disk (`MB.3.4.1/.2`, `MB.9.2.1/.2`,
  `MB.3.6.1..4`, `MB.9.3`, `MB.9.4`, `MB.3.7`, `MB.3.8`) or is created by §8.1 item 1
  (`MB.3.5.1/.2`, `MB.9.1.1/.2`, `MB.9.2.1.1/.2/.3`, `MB.3.6.1.1/.2`, `MB.3.6.2.1/.2`,
  `MB.3.6.3.1/.2/.3`, `MB.3.6.4.1/.2`); no dangling pre-split id.

---

## MINOR (accepted follow-ups)

### C9-m1. §8.2's `MB.9.1` row still uses non-mintable `.1a`/`.1b`, contradicting §8.1 item 3

`php-fastcgi.md:1824`: "fallback split `.1a`/`.1b` if over 550 lines", while §8.1:1811 says
"each is **two numeric `new` children**, e.g. `MB.9.2.1.2.1`/`MB.9.2.1.2.2` — `plan.py`'s
`ID_RE` rejects letter suffixes". §9:2206 claims `C8-m6` fixed, but this row was not updated.
`tools/plan.py` `ID_RE` (`^M(?:[A-Z]|\d+)(?:\.\d+)*$`) rejects `MB.9.1.1a`. Replace with
"`MB.9.1.1.1`/`MB.9.1.1.2`".

### C9-m2. Page says `pm.max_children` "must be at least this"; design/`R11` say "equal"

`php.md:30`: "`workers` | default `4`; requests at once; `pm.max_children` must be at least
this"; §4.8.2:1084: "`workers` vs `pm.max_children` | equal, written from one key"; §7.2:1586
asserts `pm.max_children == workers`. The page's own Deployments line (`php.md:148`) says
"Match `pm.max_children` to `workers`". Use "equal to `workers`" in the key table.

### C9-m3. Page omits the `workers` 1..=64 bound that §4.1 makes fatal

§4.1:280 makes `workers` `0`, `> 64` or non-integer fatal; `php.md:30` only says "default
`4`". A user setting `workers = 100` gets an undocumented boot error. Add "1–64" to the page
row (and to the planned `manifest.md` row, §4.1:475-477).

### C9-m4. `php.md:24` "no `app`" vs §4.1:473 "accepted and ignored"

`php.md:24`: "A FastCGI route is its own executor: no `app`"; §4.1:473-474: "on a manifest
with any `[route.fastcgi]`, `app` is accepted and ignored for those routes". A mixed manifest
(FastCGI + WSGI) needs `app` for the WSGI route; the page can be read as forbidding it. Say
"the FastCGI route ignores `app`; a mixed manifest still sets it for its WSGI routes".

### C9-m5. §8.1 item 2 re-adds files that item 1 already set on created nodes

Item 2 is titled "Exact `paths` adjustments on existing nodes" but includes `MB.3.5.2`,
`MB.9.2.1.1`, `MB.3.6.1.1`, `MB.3.6.3.1` and `MB.3.6.3.2`, which item 1 creates with
"exactly the `Files` cell". "Add `crates/blastit/src/supervise.rs`" reads as if it were
missing. Keep item 2 only for pre-existing nodes (`MB.3.4.2`, `MB.9.2.2`, `MB.9.3`, `MB.3.6`,
`MB.3.8`) and move the created-node notes into item 1's bullets.

### C9-m6. §8.1 item 3's `fastcgi_builder.rs` parenthetical contradicts "changes no path"

§8.1:1811: "`MB.3.6.2.1` → `.1` (`fastcgi.rs` pump) + `.2` (`fastcgi_builder.rs`: …; that
file is in `.1`'s paths, so this split adds no path)". If `.2` writes `fastcgi_builder.rs`,
it must own it; "in `.1`'s paths" says it does not. The closing rule "Each split is
mechanical and changes no path" means both children keep the parent's four paths. Delete the
parenthetical (or say "both children keep the parent's four paths").

### C9-m7. §8.1 item 3's `.1`/`.2` shorthand collides with existing sibling ids

"`MB.3.6.2.1` → `.1` + `.2`" reads as `MB.3.6.2.1`/`MB.3.6.2.2`, which already exist; the
same for `MB.3.6.3.2` → `.1`/`.2` and `MB.9.2.2` → `.1`/`.2`. Only the intro example
(`MB.9.2.1.2.1`/`.2.2`) disambiguates. Spell each child fully (`MB.3.6.2.1.1`/`.1.2`).

### C9-m8. ADR-0008's `Amends` block presents the new rule as ADR-0007's old text

`ADR-0008:278-279`: "**ADR-0007 decision 6:** "`app` is optional iff every route is
static-only or has its own executor" (`can_boot_without_app()`)". ADR-0007 decision 6
actually says "`app` is optional exactly when every route is static" (`ADR-0007:75`). Label
the quoted string as the replacement: "decision 6's `app` rule becomes …".

### C9-m9. ADR-0008 does not address ADR-0007 decision 1's "no new process"

ADR-0007 decision 1 (`ADR-0007:23-24`): "no new process, role, uid, socket, supervisor or
filter". ADR-0008 adds a `php-fpm` master per route; the `Amends` block supersedes only the
socket/filter clause (`ADR-0008:271-273`) and decision 6 says "No new role, uid or
supervisor" (`:158-159`) — it carefully omits "process" but never says why the pool's new
process is outside decision 1's scope (the static responder). Add one sentence: decision 1's
"no new process" is scoped to the static responder; the pool is the route's own executor.

### C9-m10. §8's `MB.3.6.3.2` Lands cell omits `DEFAULT_POOL_MAX_REQUESTS`

§4.8.2:1083 and §9:2198 say `supervise::DEFAULT_POOL_MAX_REQUESTS` is "defined in
`supervise.rs` and owned by `MB.3.6.3.2`", but §8:1687's Lands cell names only
`render_pool_conf`, `S0`–`S7`/`U1`, `pool_test`, the fake helper and the smoke test. Add the
constant to the Lands cell so the leaf that must define it is unambiguous.

### C9-m11. `ctl.rs`'s module doc still says `0x40`–`0x44`; §8 does not schedule the update

`crates/blastit-frame/src/ctl.rs:5-14` documents "own type-code range (`0x40`–`0x44`)" and
lists five rows. §8:1682's `MB.3.6.1.1` Lands cell updates `Ctl::Spawn`'s doc comment but not
the module range/table, which becomes `0x40`–`0x47` with `PoolSpawned`/`PoolReady`/
`PoolExited`. Add "update the module type-code table (`0x40`–`0x47`)" to the Lands cell.

### C9-m12. §8.2:1817 attributes a stale sentence to `MB.3.3` that the brief no longer has

§8.2:1817 lists "`RouteInfo` holds `upstream`" as stale text, but
`plan/nodes/MB/MB.3/MB.3.3/NODE.md:162-166` already says the handle lives beside the
descriptor and "adding `Arc<dyn Upstream>` to `RouteInfo` itself is not the shape". Drop that
clause from the stale-text column (keep `plan(&RequestHead)` and the `kind()` wording).

### C9-m13. §8.2:1818 names two nodes that do not exist yet as "stale text on disk"

The row's stale-text column names `MB.9.2.1.2` and `MB.3.6.2.1`, both created by §8.1 item 1
from §8's Files cell; there is no brief on disk to be stale. Say "the new briefs for
`MB.9.2.1.2`/`MB.3.6.2.1` must be written with this shape" (or drop them from the
stale-text column and keep them in the required-sync column).

### C9-m14. §8.2:1831 names the stale `zygote.rs` but silently drops `roles.rs` (and `MB.3.6.2` drops `lib.rs`)

`MB.3.6.3/NODE.md` also lists `crates/blastit/src/roles.rs`; `MB.3.6.2/NODE.md` lists
`crates/blastit-scheduler/src/lib.rs`. Item 1's "exactly the Files cell" drops both, and
neither §8.2 row says why (`roles.rs` is `MB.9.2.1.3`'s composition root; `lib.rs` is
`MB.9.2.1.2`'s `mod upstream;` site). Add both to the stale-paths sentences so the planner
does not re-add them.

### C9-m15. §8.2:1818 does not name `CancelToken` among the contract types to sync

The row names `BodySource`, `BodyStream` and `ResponseSink` but not `CancelToken`, which
§4.2.1:532 defines and §8's type matrix assigns to `MB.9.1.1`. A brief that adds
`BodySource` but keeps a scheduler-local cancel flag would compile but violate the seam. Add
"`CancelToken` (contract, `Arc<AtomicBool>`)" to the row.

---

VERDICT: PASS
