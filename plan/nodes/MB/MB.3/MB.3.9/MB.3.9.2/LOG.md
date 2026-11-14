# Log MB.3.9.2

## 2026-11-14T13:17 [lead]

created (task) Design + ADR amendment: dev lifecycle, provider reuse, discovery order, cleanup and error contract

## 2026-11-14T13:18 [unknown]

edited fields

## 2026-11-14T13:19 [lead]

status ready -> active

## 2026-11-14T13:19 [lead]

attempt 1/3 started

## 2026-11-14T13:19 [lead]

base 2ad094b; owner decision 2026-11-14: listen is removed from [route.fastcgi], workers optional and derived from blastit's view worker budget; MB.3.5.1 is implementing the new grammar in parallel, this node makes the documents match it

## 2026-11-14T13:50 [lead]

landed: grammar = entry + optional workers; listen gone (derived <run>/fastcgi/<route_index>/fpm.sock); workers from view_worker_cap(), 1..=64, over-budget/over-ceiling = boot error naming both numbers; make check EXIT=0, zensical strict clean, plan OK. Deviation from my brief (UnknownKey -> FastCgiListenDerived) is CORRECT: it matches the parser MB.3.5.1 actually wrote, and a value-free message is better than echoing the removed value. DEFERRED to MB.12: pool-file ownership, dev enforce_socket_inode, deleting PoolSpawn::External.

## 2026-11-14T13:54 [lead]

REVIEW FAIL (attempt 1). MAJOR: design:1445 'a leaf that cannot be freed never fails the boot' (B3 failure cell :1396 = warn + route Busy/503, never exit 1) has NO named test or owner leaf -- §7.2 and §8.1:2041 name only planted_entry_at_the_leaf_is_removed, whose inputs stop at 'non-empty directory'. Node acceptance demands 'every rule above is a named test'. Required: name eight_planted_stale_entries_leave_only_that_route_503 in §7.2 AND in MB.3.6.3.1's Lands cell (no root needed: temp parentfd + eight stale.* entries). MINORs to fix: (a) :2613 says 'three review passes' but :2625-2634 list only 1-2 -- add pass-3 rows or say 'two passes plus the round-3 re-verify'; (b) :1953 implies dev starts a pool, contradicting :1299-1301 (External until MB.3.9.4) and RESULT's claim that §7.3 defers it -- scope the sentence; (c) :426-430/§8.1:2041 route ALL four legs through FastCgiPoolSocketInvalid but production's goes through stage0::fail_pool_parent -- name the variant for the three dev legs only; (d) :1405 claims U1 succeeds at 0511 'because init is the owner and also holds CAP_DAC_OVERRIDE' -- at 0511 the owner has no w, only CAP_DAC_OVERRIDE unlinks, and :1440-1443 renames an entry to stale.<pid> so the 'no non-socket entry' clause is false; (e) model.rs:338-340 says the derivation is the scheduler's while design:1207-1217 says it is in blastit-manifest -- LEAD DECISION: do NOT put resolved_workers/MAX_POOL_WORKERS/FastCgiWorkersOverBudget into MB.3.5.1 (the pool path is FROZEN pending MB.12's embed-vs-pool decision, and workers may lose meaning under embed). Instead amend §8.1/§8.2 so those items belong to the pool leaf and are marked gated on MB.12, and make the model.rs comment and the design agree with that.

## 2026-11-14T13:54 [lead]

status active -> ready

## 2026-11-14T13:54 [lead]

attempt 2/3 started

## 2026-11-14T13:56 [lead]

status ready -> active

## 2026-11-14T14:02 [lead]

REVIEW FAIL (attempt 2). Five of six findings verified fixed on disk. Remaining: the SAME class of drift my finding (e) named, in three places the attempt-2 sweep missed -- (1) design:1946 'absent_workers_under_a_512_budget_is_boot_fatal' still says 'in MB.3.5.1's model tests'; (2) design:2252 §8.2's MB.3.5 row resolution cell still instructs 'add Manifest::check_fastcgi_workers(budget) + FastCgiWorkersOverBudget' right after saying they are not MB.3.5.1's; (3) design:2045 with :2253 and :1946 -- MB.3.5.2's Lands still requires workers_over_budget_is_boot_fatal and §7.2 lists workers_at_budget_is_accepted, both oracles of the deferred function, so that leaf's protected suite could not compile. Required: every oracle of check_fastcgi_workers/MAX_POOL_WORKERS/resolved()/FastCgiWorkersOverBudget/FastCgiWorkersCapExceeded moves to the pool leaf's Lands cell, gated on MB.12; MB.3.5.1 keeps grammar + FastCgiPoolSocketInvalid only. Not escalating to expert: this is a three-location consistency sweep, fully specified by the reviewer, not a hard problem; attempt 3 is cheaper and proportionate.

## 2026-11-14T14:02 [lead]

status active -> ready

## 2026-11-14T14:02 [lead]

attempt 3/3 started

## 2026-11-14T14:09 [lead]

status ready -> active

## 2026-11-14T14:12 [unknown]

edited fields

## 2026-11-14T14:12 [lead]

REVIEW FAIL (attempt 3). The three-line sweep landed (grep: 0 oracles attributed to MB.3.5.1/.2) but one REAL plan error surfaced: design:2046/:2191/:2256 attribute ManifestError::FastCgiPoolSocketInvalid to MB.3.5.1, which is done and lacks it (error.rs:497-537 has only Entry/ListenDerived/WorkersInvalid/DeadDeclaration; grep -rn FastCgiPoolSocketInvalid crates/ = empty), while :1957 needs the three dev legs to refuse through it inside MB.3.5.2's protected suite whose Files omit error.rs -> that suite cannot compile. LEAD ACTION taken: MB.3.5.2's paths now include crates/blastit-manifest/src/error.rs and its LOG says this leaf adds the variant. Attempt 4 required: move the variant to MB.3.5.2 in :2046/:2191/:2256, add error.rs to its Files cell, state MB.3.5.1 landed grammar only; fix the MINOR that says check_fastcgi_workers 'does not exist' (validate::check_fastcgi_workers(&location, policy, errors) DOES exist at validate.rs:597 -- reword to the budget-aware Manifest::check_fastcgi_workers(budget)); and record the critic cap in the §9 record so it does not read as 'fresh critics PASS'. Not escalating: each round is converging on a 2700-line doc and the remaining edits are line-level.

## 2026-11-14T14:12 [lead]

status active -> ready

## 2026-11-14T14:12 [lead]

attempt 4/4 started

## 2026-11-14T14:19 [lead]

status ready -> done
