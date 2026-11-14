# `php-fastcgi` — consistency review, amendment round 3b (single re-verify of a passed lens)

- **Lens:** `consistency`. Scope limited to the round-3 MINORs M1–M3 and item 4 of the re-check
  brief (no post-pass contradiction in the four named places).
- **Revision reviewed:** design **2651 lines**, sha256 `7c15b2cf…ce4265`, mtime **2026-11-14
  13:44:40**; ADR **409 lines**, sha256 `d0f7bbfd…ba868`, mtime 13:43:45. That is +24 / +7
  lines over the round-3 snapshot (`2627` / `402`). Every line number below is from these
  bytes; the M1–M3 addresses were re-resolved, not copied.
- **Verdict inputs:** `php-fastcgi.review-consistency-amendment-r3.md`, `docs/VISION.md`,
  `ADR-0008`, and the shipped files the amendment cites.

**VERDICT: FAIL**

| Grade | Count |
|---|---|
| BLOCKER | 0 |
| MAJOR | 1 |
| MINOR | 3 |

## Findings

| ID | Grade | Where (file:line) | What | Fix |
|---|---|---|---|---|
| C-R3b-1 | MAJOR | `docs/design/php-fastcgi.md:1396` (`B3` syscall cell and failure column) vs `:1445-1455` (new leaf rule), `:1434-1443` (`remove_entry`), `:1405` (`U1`) | `B3` still says "removal failing with anything but `ENOENT` is a boot error" and its failure column says "Boot error naming capability or foreign entry at `sock_name`, `exit 1`". The paragraph that closes security finding S-3/SEC-R2-1 says the opposite for the bounded give-up: "If all eight rename slots are occupied the leaf name is still taken … **A leaf that cannot be freed never fails the boot** … a boot **warning** … `B3` continues with the other routes and the process serves … no ceiling that can turn into an `exit 1`". `remove_entry`'s return contract is unstated (`Removed`/`Absent`/`Unfreed`?), so the only outcome statement an implementer has is `B3`'s — implement it literally and one route's hostile non-empty leftover wedges every route's boot on a preserved `<runtime>`, i.e. the exact defect the rename-aside was added to remove. `B3` is also stale in the same cell: its removable list omits the non-empty directory that `remove_entry` now handles. | Give `remove_entry` an explicit outcome and make `B3` say: `Unfreed` → boot warning naming the route and the entry, this route's pool stays `Busy` (`503`), **no** `exit 1`; only `Err` (`EPERM`/`EACCES`/`EIO`) is `exit 1`. Amend the failure column and the removable list to match `:1434-1451`. |
| m1 | MINOR | `:1426` vs `:1330`, `:1956-1957` | The `dev`/`check` single-uid row attributes the socket's `0600` to "php-fpm's umask", but §4.8.2 always renders `listen.mode=0600` and §7.3 asserts that directive in **`dev`'s** generated pool file. Same value, wrong named mechanism in the one table item 4(a) turns on. | "the socket is `listen.mode=0600` that uid (§4.8.2)". |
| m2 | MINOR | `:2196-2197` vs `:339-342` (R20) and `:2193` | "`MB.3.9.3` runs phase 2 `check_fastcgi_leaf` **after the child `dev` reports its pool ready**" reads as the `check` parent probing after readiness; R20's check outcome is "an early child exit → `exit 2`", and `check_child_leaf_failure_exits_2_naming_the_derived_socket` is unreachable if the child has already reported ready (a parent-side phase-2 failure's exit code is stated nowhere). The consistent reading — the child runs phase 2 after **its pool's** `PoolReady` and exits `1`, `check` maps the early exit to `2` — is one possessive away. | "after the child `dev`'s pool reports ready", and name the child `1` → `check` `2` mapping. |
| m3 | MINOR | `:1335` vs `:220` (R3), `:1174-1176` (§4.8.1), `ADR-0008:285-286` | §4.8.2's `dev` row is the one surviving site that calls the run root `<DevRoot>`; every other site and the ADR say `<run_root>`. `BLASTIT_CHECK_DEV_ROOT` exists, so `<DevRoot>` also reads as that variable. (Item 3's literal target — `DevRoot::adopt` — is gone.) | `<run_root>/fastcgi/<route_index>/fpm.sock`. |

## Verified

| # | Claim | Evidence |
|---|---|---|
| M1 | Decision 6 opens with the `MB.3.9.2` note; every `0555` is after it; `Amends` agrees | `ADR-0008:161-165` declares "every `0555` in this decision is the pre-amendment value" and names the amended modes (`<runtime>` `0755`, container `0711`, `<i>` `0700`→`0755`→`0511`, `dev`/`check` `0700`, single-uid); every value occurrence follows it: `:197`, `:201`, `:225`, `:228`, `:231`, `:233`; no `0555` in decisions 1–5. `Amends` at `:290-299` (three modes) and `:359-360` ("the freeze/bind window keeps its **shape** and changes its **modes** (the note on decision 6 is authoritative)"). |
| M2 | No site deletes the `FastCgiListenDerived` arm; only `!has_listen`/`MissingKey` + `DEFAULT_WORKERS` go | `:385` (key row, `FastCgiListenDerived`, no value), `:409-410` ("the one `listen` refusal is `FastCgiListenDerived`"), `:414`, `:655-656` ("`env` and any other key are `UnknownKey` errors; `listen` has its own refusal"); `:2027` ("refusing `listen` with the landed `FastCgiListenDerived` (no value; exact message as §4.1)"); `:2176-2178` (keeps the arm, loses `!has_listen` `MissingKey` and `DEFAULT_WORKERS`); `:2226` ("**keep** the landed … arm … lose only `!has_listen` `MissingKey`"). `DEFAULT_WORKERS` appears only in drop lists; "drop the arm" appears nowhere. `ADR-0008:303-310` names the surviving variant and "carries **no** value" (the ADR never discusses the parser arm, and never says to remove it). |
| M3 | `Dev::adopt` only, no `DevRoot::adopt` | `Dev::adopt` at `:136`, `:221`, `:1175`, `:1185`, `:1394`, `:2041`, `:2225`, `:2229`, `:2293`, `:2347`; `ADR-0008:286`. `grep DevRoot` yields only the `<DevRoot>` placeholder at `:1335` ⇒ m3. |
| 4(a) | Single-uid table agrees with R20, §4.8.2 and the ADR | `:1423-1428`: `S0`/`S3` skip `drop_privileges` (keep harden/filter/dumpable), `S2`/`S5`/`S6` skipped, `S1`/`S4`/`S7` as written, `U1` kill + **no freeze**; `:1430-1432` (`0700` + invoking-uid predicate, `0511`/`0711`/`S2`/`S5`/`S6`/`drop_privileges` production-only); `ADR-0008:359-360` "skips `drop_privileges`, `S2`, `S5`, `S6` and `U1`'s freeze"; `R20:339-349` only requires phase 2's process and the leaf/parent outcomes, which the variant preserves; §4.8.2's `dev` row `:1335` says "no uid change, one identity"; `§9:2631` lists the same set (`S0`,`S1`,`S3`,`S4`,`S7`,`U1`). Only the mechanism wording differs ⇒ m1. |
| 4(b) | Leaf rule agrees with R20/§7.2 | R20 (`:339-349`) constrains the leaf's post-start state (`dev` `exit 1`, `check` early child exit `2`) and the **parent** (boot error every mode); it says nothing about a boot-time cleanup give-up. §7.2's leaf leg is `:1941` ("`dev` with a missing/foreign-uid/`0755` socket exits `1`", check twin named), not a boot-wedge claim. The rule's production scope (B3, preserved `<runtime>`, other routes serve) is compatible with both. The contradiction is with `B3` itself ⇒ C-R3b-1. |
| 4(c) | `pool_parent_modes_are_0600_0511_0711` matches the mode table | Oracle `:1937` and `:2043` ("the parent directory, the socket and the `fastcgi` container: exact masks after `S6`"); table `:1412-1416` (`<i>` `0511`, container `0711`, socket `0600`) and `R3:234-241` (`0700`→`0755` route in the window→`0511` steady; socket `0600`); `B2` `:1395` (`0711`, `0700` normalise) and `S6` `:1403` (`0511`, socket `0600`). The oracle's three values are the table's three non-`0755` levels, in the same paths; `<runtime>` `0755` is deliberately outside the oracle's name and scope. |
| 4(d) | `check_leaf`'s named exit-code tests match R20 | `:2201` `leaf_missing_after_spawn_exits_1_in_dev` (`MB.3.9.5`) = R20's `dev` `exit 1`; `:2193` (and `:1941`) `check_child_leaf_failure_exits_2_naming_the_derived_socket` (`MB.3.6.4.2`) = R20's check `exit 2`; `§1.3:147` repeats "an early child exit or busy port exits `2`". `:1203` leaves the phase-2 call site to `MB.3.9.3`, which R20's outcome contract does not contradict. Only the ordering phrasing at `:2196-2197` is loose ⇒ m2. |

## Notes

- **Observed, not graded** (carried from round 3, unchanged): `:2619-2622` still lists "who
  spawns it in `dev`/`check`" as deferred to `MB.12`, while `:1306-1307`, `:2229` and
  `ADR-0008:366-368` decide it (`Managed`, `dev` starts/stops the pool it owns). Round 3
  graded this polish-level before `MB.3.9.3`; it is still worth the one clarifying clause.
- **Observed, not graded (testability, outside this lens):** `:1937`'s `getdents`-from-a-foreign-uid
  leg of `pool_parent_modes_are_0600_0511_0711` cannot run in the same-uid `PoolHarness`, and
  no `e2e/rootless` leg lists it; the `0511`/`0600` mode assertions are marked `requires-root`
  only on the `R2`/`R3`/`R20` row, not on the `R11` row that names this oracle.
- The M1–M3 fixes are structural, not cosmetic: the ADR now carries the note *before* the
  stale values instead of rewriting them, the parser instruction is "keep + lose", and the
  symbol is one spelling. No new `0555`, `DevRoot::adopt`, or "drop the arm" text was
  introduced anywhere in either document.
