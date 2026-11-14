# `php-fastcgi` — consistency review, round 3 (focused re-check of the 2026-11-14 `MB.3.9.2` amendment)

- **Lens:** `consistency`
- **Scope:** the four MAJORs of round 2 (`F1`–`F4`) plus the repo spot-check list (item 5). Read as a
  whole: `docs/design/php-fastcgi.md`, `docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md`,
  and the shipped files the amendment cites.
- **Revision reviewed:** design **2627 lines**, sha256 `83a9ad77…9874f0e`, mtime **2026-11-14 13:41:30**;
  ADR **402 lines**, sha256 `f79d0e7e…bf0affd`, mtime 13:40:01. The design was rewritten at 13:41
  **during** this pass: body lines `< 2586` were byte-identical across the 13:40 → 13:41 snapshots
  (only the tail gained the new §9 "Amendment round" section), and every anchor below is
  re-verified against the 13:41:30 file. Re-hash before quoting if it moves again.
- **Verdict inputs:** `php-fastcgi.review-consistency-amendment-r2.md` (1 BLOCKER class carried over
  as 4 MAJOR), the shipped `blastit-manifest` / `blastit` tree, `docs/frameworks/php.md`.

**VERDICT: PASS**

| Grade | Count |
|---|---|
| BLOCKER | 0 |
| MAJOR | 0 |
| MINOR | 3 |

## Answers to the five checks

1. **`listen` (F1) — closed, with one ambiguous instruction left.** R1 (`:204-208`), §4.1's key table
   (`:381-385`), the variant table (`:405-410`), the removal note (`:410`), §7.2 (`:1911`,
   `listen_is_refused_by_name`), §8's `MB.3.5.1` row (`:2006`, "refusing `listen` with the landed
   `FastCgiListenDerived`"), §8.1 (`:2155-2156`) and the ADR `Amends` (`ADR-0008:298-305`) all name
   `FastCgiListenDerived` with the shipped message; `UnknownKey` appears **nowhere** in either
   document outside §9's historical review tables (`:2604`, `:2608`). The two residuals are
   §8.2's "drop … the arm" (`:2202`) and §4.1:652's generic "unknown-key errors" ⇒ **M2**.
2. **Directory modes (F2) — one story in the design; the ADR's amendment matches, its decision body
   does not.** Design: `<runtime>` `0755`, container `0711`, `<i>` `0700 init` → `0755 route_uid` in the
   window → `0511` steady, `dev`/`check` `0700` (R3 `:234-239`; §4.8.3 `B1`/`B2`/`B3`/`S2`/`S6`/`U1`
   `:1388-1399`; mode table `:1408-1419`; §5.1 `:1754-1759`); `U1` now chmods `0o511` (`:1399`). No
   `0555` survives in the design outside historical §9 rows (`:2253`, `:2529`). ADR `Amends` states the
   same three modes (`:285-294`), **but** decisions 1 and 6 still say `0555` six times (`:192`, `:196`,
   `:220`, `:225-228`) while decision 6's own `U1` sentence is inline-marked `0511` (`:223`) ⇒ **M1**.
3. **`<runtime>` ≠ `0700` (F3) — closed.** The three round-2 sentences now read "non-route-writable
   directory" (`:1047`, `:1381`), `B1` says "root-owned (`0`) `0755`, **not** `0700`" (`:1388`), and
   §5.1 reads "container `0711` … per-route directory `init_uid` `0700`→`0511`" (`:1754-1755`). Every
   surviving `0700` is the `dev`/`check` run root (`:221`, `:1171`, `:1178`), the `views/<uid>/tmp`
   dir, or the *creation* mode of `<i>` — all correct.
4. **`dev` pool (F4) — closed in both documents.** §7.2 says `dev_starts_one_pool_per_route`
   "replaces the retired `external_pool_never_spawns_php_fpm`, which asserted the opposite of the
   amendment" (`:1919`), and both §9 rows now carry the superseded marker (`:2551`,
   "*(superseded for the `dev` half by `MB.3.9.2`)*"; `:2563`, "*(superseded by `MB.3.9.2`; replaced
   by `dev_starts_one_pool_per_route` in `MB.3.9.5`)*"). The ADR retracts all three sentences and the
   oracle **by name** (`ADR-0008:354-361`) and pins `Managed` as the only shipped mode after
   `MB.3.9.4`. The surviving "never spawns" text (`ADR-0008:49-50`, `:112-116`, `:239-244`) is original
   decision text under the dated `Amends` heading — the allowed form.
5. **Repo spot-check — every numbered citation still matches.** `config.rs:674,677,841-842,1014-1023,
   1301-1321,1407-1411`; `zygote.rs:52,115-121`; `sock.rs:452-454,497-519`; `stage0.rs:667-668` (the
   `dev_fail` `eprintln!("blastit dev: …")` channel §1.2 relies on); `dev_learn.rs:70-85` (3-line
   banner, `1 route`) and `Dev::adopt` at `:180`; manifest in-flight grammar exactly as the design's
   premise (below). `docs/frameworks/php.md` now says Blastit runs PHP for the route with nothing left
   after Ctrl-C, and its table has the two keys with no `listen`. Two citation drifts, both in the
   verified table (not graded).

## Findings

| ID | Grade | Where (file:line) | What | Fix |
|---|---|---|---|---|
| M1 | MINOR | `ADR-0008:196,220,225-228` vs `ADR-0008:285-294`; `docs/design/php-fastcgi.md:1397,1399,1409-1410` | The ADR's normative Decision section says the pool parent is frozen "`init_uid:init_gid 0555`" (`:196`) and "**restore owner and mode** `init_uid:init_gid 0555`" (`:220`) — six occurrences, none inline-marked. The dated `Amends` says `0711`/`0511`, and §4.8.3 says `<i>` is `0511` with "no `r` (no `readdir`: route count/layout is not public)". An implementer who reads only the Decision section writes `0555`, i.e. a **listable** parent: the §7.2 leg `readdir(<runtime>/fastcgi)` is `EACCES` fails, and `group_or_world_writable_segment_is_refused`'s exact-mode predicate is contradicted. This is round-2 F2's failure mode surviving at a different address. | Add "(superseded by `MB.3.9.2` → `0511`; see *Amends*)" at `:196`/`:220`/`:225`, or make them "`0511` (amended)"; keep the history, but stop the Decision section contradicting the design's mode table. |
| M2 | MINOR | `docs/design/php-fastcgi.md:2202` and `:652` vs `:2006,2155-2156,1911`; `crates/blastit-manifest/src/parse.rs:958` | §8.2's fix column says "drop the field, **the arm**, the two variants and `DEFAULT_WORKERS`". The stale text it quotes is the value-parsing arm, and §8.1/§8 say "the parser **keeps** the landed `FastCgiListenDerived` arm" (`:2155-2156`) — but read alone, "drop the arm" instructs deleting `"listen" => FastCgiListenDerived`, after which `listen` falls to `_ => ManifestError::UnknownKey`: precisely the F1 regression, and `listen_is_refused_by_name` (`:1911`) then fails on the message. §4.1:652 compounds it: "`env`, `listen` and any other key are **unknown-key errors**" (true only of the *message*, not the variant). | §8.2: "drop the stale **value-parsing** `listen` arm; keep the landed `FastCgiListenDerived` refusal arm". §4.1:652: "`env` is an `UnknownKey`; `listen` is `FastCgiListenDerived`; any other key is `UnknownKey`." |
| M3 | MINOR | `docs/design/php-fastcgi.md:2020,2201,2269,2323`; `crates/blastit/src/dev_learn.rs:99,167,180` | Four plan/history cells name `DevRoot::adopt`. The repo type is `Dev` (`:167`) with `pub fn adopt` at `:180`; `DevRoot` (`:99`) only has `create`. §4.8.1/R3 (`:1170-1171`) spell `Dev::adopt` correctly, and so does the ADR (`:281`), so the design is split against itself on the one symbol item 5 asks to spot-check. | One spelling: `Dev::adopt` (the cited `dev_learn.rs:180` then resolves). |

## Verified (items 1–5)

| # | Claim | Evidence |
|---|---|---|
| 1 | One `listen` refusal, `FastCgiListenDerived`, no `UnknownKey` claim | `php-fastcgi.md:204-208` (R1, exact message), `:381-385` (key table), `:409-410` (variant + removal), `:1911` (`listen_is_refused_by_name`), `:2006` (§8 `MB.3.5.1` "landed `FastCgiListenDerived` (no value; exact message as §4.1)"), `:2155-2156` (§8.1 "keeps the landed … arm"), `:2202` (formulation ⇒ M2); `ADR-0008:298-305` (variant, exact message, "carries **no** value", rationale = drafts, not "never shipped"); repo `parse.rs:958`, `error.rs:512-514`, message asserted verbatim at `validate.rs:992-996`. `grep UnknownKey` in both docs: only §9 history `:2604`, `:2608`. |
| 2 | One mode story (`0755`/`0711`/`0511`, `dev`/`check` `0700`) | `php-fastcgi.md:234-239` (R3), `:1390` (`B3` `0o511`), `:1397` (`S6` `0o511`), `:1399` (`U1` `0o511`, no `0o555`), `:1408-1410` (mode table), `:1412-1419` (single-uid variant, `0700`, no `S2`/`S5`/`S6`), `:1754-1759` (§5.1), `:1916` (mode constants `0600`/`0511`/`0711`); `ADR-0008:285-294` (`0755`/`0711`/`0511`, `dev`/`check` `0700`). Design `0555`: only §9 `:2253`, `:2529` (historical). ADR decision bodies `0555` ⇒ M1. |
| 3 | No prose says `<runtime>` is `0700` / `fastcgi` is `0700` | `php-fastcgi.md:1047` ("B1 proves `<runtime>` … non-route-writable"), `:1381` ("non-route-writable directory"), `:1388` (`B1`: "root-owned (`0`) `0755`, **not** `0700`"), `:1408` ("the invariant, not `0700`"), `:1754-1755` (`0711` / `0700`→`0511`). Remaining `0700`: run root `:221,1171,1178`; `<i>` creation mode `:237,:1410`; `views/<uid>/tmp`; `<CheckRoot>/dev`. Repo agrees: `sock.rs:452-454` doc, `:497-519` `create_parents` `mkdir(0o755)`. |
| 4 | No surviving "`dev` never spawns a pool"; retired oracle marked superseded in both docs | `php-fastcgi.md:1919` (`dev_starts_one_pool_per_route` "replaces the retired `external_pool_never_spawns_php_fpm`"), `:2551` ("*(superseded for the `dev` half by `MB.3.9.2` …)*"), `:2563` ("*(superseded by `MB.3.9.2`; replaced by `dev_starts_one_pool_per_route` in `MB.3.9.5`)*"), `:1296-1318` (`Managed` after `MB.3.9.4`, `external_executor` always `false`), `:2205` (`dev` starts/stops the pool it owns); `ADR-0008:354-361` (retracts decision 1's, 2's "The External promise" + oracle, and 6's "never spawn pools", names the replacement). |
| 5 | Cited numbers/predicates still match the repo | `config.rs:674,677` (`max_workers`, `view_max_workers: Option<u32>`), `:841-842` (`16`/`None`), `:1014-1023` (env parse), `:1301-1321` (`view_worker_cap().checked_add(16)`), `:1407-1411` (`view_worker_cap()`); `zygote.rs:52` (`VIEW_PROCESS_MARGIN = 16`), `:115-121` (`nproc = view_worker_cap() + margin`); `sock.rs:452-454,497-519`; `stage0.rs:667-668` (`dev_fail`'s `eprintln!("blastit dev: …")` — §1.2's "existing channel"); `dev_learn.rs:70-85` (exact 3-line banner ending `1 route`) and `Dev::adopt` `:180`; `model.rs:328-338` (`FastCgiPolicy { entry, workers: Option<u32> }`), `parse.rs:958` (`FastCgiListenDerived`), `error.rs:512-514` (message); no `MAX_POOL_WORKERS`/`resolved_workers`/`check_fastcgi_workers` in `blastit-manifest` yet (exactly the `MB.3.5.1` target, per `:2006`); `php.md:33` (two-key table), `:63-65` ("Blastit runs PHP for the route itself … nothing left running after Ctrl-C"), no `listen`. Drift: design `:1228` cites `stage0.rs:1697-1705` for the second `set_cloexec` loop; the actual loop is `:1700-1707` (semantics unchanged). |

## Notes on evidence

- The mid-review rewrite (13:40:01 → 13:41:30) added only §9's `## Amendment round (MB.3.9.2 …)`
  section (`:2588-2627`); it did not touch F1–F4's addresses. Its "all fixed" rows (`:2605-2609`)
  are consistent with what this pass verified, except that `:2606`'s "residual `0700`/`0555` in
  the ADR … all fixed" oversells M1: the `0555` text is retained, only the `Amends` paragraph is new.
- `:2612-2614` says every historical §9 row is annotated "superseded by `MB.3.9.2`" "where a reader
  would otherwise follow it". True for the two oracle rows (`:2551`, `:2563`), **not** for the two
  `0555` rows (`:2253`, `:2529`). Historical rows are permitted either way; it is the meta-claim that
  is a sentence too strong.
- **Observed, not graded (outside this pass's F1–F4/item-5 scope, unchanged from round 2):**
  `:716` still justifies `enforce_socket_inode = false` with "allowing the developer to restart
  `php-fpm` in place"; `:1828` still says "3-key DX bar" (the grammar is two keys); `:1919` says
  "two legs … and a **third** leg" while `:2204` says three; `:2596-2598` (new) says who spawns the
  pool in `dev`/`check` is "not decided here / deferred to `MB.12`" while `:1296-1318` and `:2205`
  decide `Managed` + "`dev` starts/stops the pool it owns" — worth one clarifying clause before
  `MB.3.9.3` is dispatched. All are polish-level; none changes the grammar, the derived path, the
  socket modes, or the two oracles.
