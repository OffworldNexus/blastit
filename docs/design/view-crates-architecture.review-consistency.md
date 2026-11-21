---
lens: consistency
target: docs/design/view-crates-architecture.md, docs/decisions/ADR-0016-view-crates-over-wire.md
round: sign-off (after round 6)
verdict: PASS
findings: 0 BLOCKER, 0 MAJOR, 8 MINOR
history: round 2 — 1 BLOCKER, 6 MAJOR, 10 MINOR; round 5 — 4 MAJOR, 8 MINOR; round 6 (sign-off) — 0 MAJOR
---

# Consistency review — view crates over the wire (sign-off)

Checked against `docs/VISION.md`, `ARCHITECTURE.md`, ADR-0009/0010/0012/0014/0016,
`docs/decisions/README.md`, `docs/design/backend-runtimes.md`, the `MB.36.*` nodes and the live
tree (`HEAD` `54ad5d9`, 2026-11-17). Every claim below was re-measured; the subject files were
not edited by this review.

## The six questions (confirm / deny)

| # | Claim | Verdict |
|---|---|---|
| 1 | ADR-0009/0010/0014 amended, not contradicted; ADR-0009 diff is one hunk | **CONFIRMED.** `git diff HEAD` on ADR-0009 is 1 insertion / 1 deletion, one hunk, the `Status:` line only. It names Decision 1's `WorkerRuntime`/`Upstream` trait requirement **and** the `setpgid(0,0)` sentence, Decision 2's `Upstream::capacity()` parenthetical, and Decision 5's "distribution command spelling **and wheel feature list**" — identical in scope to ADR-0016's header. ADR-0010 is untouched by design (amendment assigned to `MB.36.6`, whose `paths` do not include it until §8.0 P1 adds `docs/decisions/ADR-0010-…md` + `docs/decisions/README.md`); ADR-0014 is not rewritten, and its carrier change is recorded by ADR-0016's Consequences bullet ("*One carrier moves, the mechanism does not*"), which correctly cites Decision 2. `docs/decisions/README.md:15` already reads "Accepted (superseded in part by `ADR-0016`)". |
| 2 | Current DSL vocabulary only | **CONFIRMED** for both subject files. `grep 'blastit\.toml\|\[route\.'` is empty in both; they say `Blastfile`. §1's fixtures are current grammar: `static "/" { root "public" }`, `entrypoint php "public/index.php"` and `entrypoint python "shop.wsgi:application"` match `docs/design/blastfile-dsl.md:539,791` and `crates/blastit-manifest/src/blastfile/{syntax,parser}.rs`. The cited pages exist (`docs/local.md` `## Start`, `docs/reference/cli.md` `## blastit dev` / `## blastit check`, `docs/ci.md`, the three framework pages) and `crates/blastit-py/tests/critical_check_schema.rs` exists, so §1's row 5 oracle is real. |
| 3 | Every changed `ARCHITECTURE.md` section is owned by `MB.36.6` | **CONFIRMED.** The only live crate-naming text is `ARCHITECTURE.md:476-477` (§2.6: `WorkerRuntime` + `blastit-view`'s `PhpEmbedRuntime`) and `:1899` (§9.3: `blastit_view::serve`). §8.5 names §2, §2.4, §2.5, §2.6, §3.1, §4 and §9.3 — a superset — and `ARCHITECTURE.md` is in `MB.36.6`'s `paths`. Caveat: §7.3 (MINOR-4). |
| 4 | Crate/module names consistent with the design's own P5 | **CONFIRMED for the design and the ADR** — `-static` → `MB.36.2`, `-python` → `.3`, `-php` → `.5`; §4.1, §4.2.3, §4.4 D1/D2/D5, ADR-0016 decisions 1/4/5 and P5(h) all say `worker`/`discovery`/`pinning`/`probe`/`spawn` (no `fork.rs`), and every `paths` gap is listed in §8.0 P1 (`.2`: path/lib/Cargo/error/two sources/fixture/`critical_rewrite`; `.3`: `zygote.rs`, `critical_loaded_files.rs`; `.4`: `blastit-sys/**`, `child.rs`, `forking.rs`, `spawn.rs`; `.5`: `config.rs`, `supervise.rs`, `check.rs`, `tests/common/**`, `critical_parallel_views.rs`; `.6`: ADR-0010, README, `backend-runtimes.md`). Two node texts still differ (MINOR-1, MINOR-2). |
| 5 | ADR-0016 and its README row are consistent | **CONFIRMED.** README: "View Crates Over the Wire: `blastit-view` as Transport and One Crate per View Kind (`0.2.0`)" / `Proposed` / `MB.36.1`; ADR: same title, `Status: Proposed`, `Node: MB.36.1`, and its supersession list matches ADR-0009's amended status in scope. The five `--no-default-features --features php` sites in `scripts/build-standalone.sh` are exactly as the ADR counts them (header comment `:8`, echo `:98`, builds `:112`/`:115`, `cargo tree` `:225`). |
| 6 | §1 matches `VISION.md` | **CONFIRMED.** VISION §"The Guiding Principle" makes zero-`sudo` local DX primary and keeps kernel trivia out of user pages; §1 answers "**This epic changes nothing a developer types, reads or writes.** No new flag, no `Blastfile` keyword, no environment variable, no log event, no status code", backs it with seven executed captures across local, CI (`check --json`) and both boot refusals, and explicitly writes no new page ("the bloat `dx-first` forbids"). VISION's vocabulary (`Blastfile`, `entrypoint php`, `blastit dev`, `blastit check`, `--learn`) is used unchanged. |

## Carried MINOR findings (no BLOCKER/MAJOR)

1. **`MB.36.3`'s node body still contradicts the move schedule.** `NODE.md:46-49`: "What **stays**
   in `blastit-view` for now: … and — until `MB.36.5` deletes the trait — `runtime.rs` including
   … the Python `LoadedTracker`/`LoadedReporter`", against §4.2.2/§4.5/§8.2 (they move in `.3`
   with `critical_loaded_files.rs`). This was round 5's MAJOR-1; it is now a recorded deviation
   (§8.0 P5(f): "this design moves it … the node text must follow"), the node names the design of
   record in its own header, and P1/P5(b) already give the leaf the design's `paths` and
   acceptance line, so an implementer reading the assigned node plus its design cannot silently
   keep the tracker. The lead must apply the P5(f) edit **before** dispatch.
2. **The rest of `MB.36.5`'s prose is narrower than P5(h).** The title still says
   "(worker, **SAPI**, discovery, pinning, probe)" although §4.1 keeps `blastit-php-sapi` its own
   crate and the leaf only drops `blastit`'s direct dep; `:63-64` still routes `OutputStream` /
   `OUTPUT_LINE_CAP` to "`flow.rs`/`wire.rs`" (R3: `stream.rs`); `:56` says "roughly 1500" lines
   leave `stage0.rs` (§4.2.3/§4.2.5/§9 T11: ≈1 800). P5(c)/(h) record only the `RequestIo`/
   `RequestEnd`/`PinnedEntryRuntime` and `{fork,pinning,probe}` halves.
3. **§5's live counts lag §4.5.** §5 says "`critical_php_embed` (24 cases …)" and the totals line
   says "**58** protected CLI cases", while §4.5 re-measures `24 → 25` (the in-file pin leg). The
   post-`MB.36.5` protected CLI tier is 59. One sentence in §5, one number in the totals.
4. **`ARCHITECTURE.md` §7.3 keeps the sentence D1 declares inaccurate and is outside §8.5's
   sweep.** `ARCHITECTURE.md:1437-1439`: "A view's own `setpgid(0, 0)` runs in child setup *before*
   the filter is installed"; D1 measured that only the PHP probe is a group leader today. §8.5's
   list (§2, §2.4, §2.5, §2.6, §3.1, §4, §9.3) does not include §7.3, though `MB.36.6` owns
   `ARCHITECTURE.md` and can fix it in the same pass.
5. **ADR-0010's index entry still says `Accepted`.** `docs/decisions/README.md:16` and ADR-0010's
   `Status:` are unchanged while ADR-0016 (`Proposed`) already claims "(b) Decision 5's
   distribution *command spelling* … and the same command in ADR-0010 §4". They must move together
   in `MB.36.6` (P1 grants both files); until then the index describes an ADR the new record
   partially supersedes. Also pre-existing and outside this design: ADR-0015 has no README row.
6. **ADR-0016's `PhpProbeBoot` shorthand re-introduces the shape D2 rejects.** Decision 4:
   "`PhpProbeBoot` (slots, bindings, **seccomp, timeout, drop privileges**)" vs D2's typed
   `{ slots, bindings, privileges: Privileges, filter: Filter, timeout }` ("no `Config`, no
   `RouteInfo`, no bools"). `PhpSlot` is now correct (no `uid`), but `SpawnContext`'s field list in
   Decision 4 still omits `uid`, which D2 carries.
7. **§9's review inventory will not match this file.** §9 opens "`review-consistency.md` (round 2:
   1 BLOCKER, 6 MAJOR, 10 MINOR). … the files on disk are the current verdicts"; this sign-off
   file is 0/0/8. The lead should append the round-6 row (or delete the counters and let each
   review file's frontmatter be the only copy).
8. **P2 corrects one of five `backend-runtimes.md` occurrences.** `--no-default-features --features
   php` appears at `:55`, `:251`, `:252`, `:316` as well as §4.7; P2 names only "§4.7's command
   spelling". Either add the four sites or state that §5/§7 tables are historical and keep the old
   spelling deliberately.

## Verdict

All six checklist items confirm. The round-5 MAJORs are gone: ADR-0016's stage-0 end state now
matches §4.2.3/§4.2.5/§4.5 (≈1 800 code lines out, the 1 709-line module stays, ≈4 520), the
unsafe policy reads `forbid` for `-view`/`-static`/`-php` and `deny` + one `immortal` exception for
`-python` in §5, §9's S3 row and the ADR, `PhpSlot` carries no `uid`, and §4.1's DAG edges
(`view --> frame`, `phv --> frame`) now run dependent → dependency like every other edge. The eight
remaining items are prose/number/plan-text polish with named owners in §8.0 P1/P2 and `MB.36.6`;
none changes a seam, a security invariant, a DX surface or a build command. Under `design-docs` §3
a scoped crate design is accepted once zero BLOCKER/MAJOR remain.

VERDICT: PASS
