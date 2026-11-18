VERDICT: FAIL

Lens: testability (verification and oracles). Final verification pass on `MB.35.3`, read
against `docs/design/portable-lsp-arch.review-testability.round2.md` (1 BLOCKER, 5 MAJOR),
`docs/design/blastfile-lsp-zed.md` §1.3, §8.3a, §11, §14 (including §14.4) and the requirement
index, `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (the `MB.35.3` amendment), and the
repository facts re-checked in the `release/0.2.0` worktree:
`crates/blastit/tests/critical_lsp.rs` (2477 lines),
`crates/blastit/src/lsp/{mod,hover}.rs` (1591/337), `tools/critical_guard.py`, `Makefile`,
`Cargo.lock`, `crates/blastit-sqlpolicy/Cargo.toml`, and
`plan/nodes/MB/MB.35/{MB.35.4,MB.35.5,MB.35.6}/NODE.md`.

**Counts: 0 BLOCKER, 6 MAJOR, 5 MINOR.** No round-2 BLOCKER remains a BLOCKER: its plan-file
half is genuinely handed off in §14.4. The verdict is still `FAIL` because several requirements
in R69–R76 have a "Proved by" cell that does not fail when the property is deleted, and two of
the round-2 residuals survive in the design's own oracle text.

---

## Round-2 disposition

| Round-2 finding | Status |
|---|---|
| **B1** — protected-file edit asserted by the design, forbidden by `MB.35.4` | **Plan half RESOLVED as a stated hand-off.** §14.4's `MB.35.4` row names both paths and replaces "byte-identical" with "assertion bodies unchanged, four `include_str!` literals moved"; ADR decision 6 says the lead applies it. **Oracle half NOT resolved** — see MAJOR-1. |
| **M1** — `parameterize` re-export owned by nobody | **RESOLVED.** §14.4 names `crates/blastit-manifest/src/lib.rs` and the one-line re-export; ADR decision 2's "the plan's list does not yet" is now followed by decision 6's "the lead applies it". `blastit-manifest` already depends on `blastit-sqlpolicy` (`Cargo.toml:15`). |
| **M2** — five guard fixes only in §14.2; the brief still specifies the weaker oracle | **RESOLVED as a hand-off** by §14.4's `MB.35.6` row (every dependency table, §1.3 row 3's tokens, one twin per token, the identity rule). Two clauses are still missing — MINOR-2 and MINOR-5. |
| **M3** — `unsafe` token collides with `#![forbid(unsafe_code)]` | **RESOLVED in the requirement.** §1.3 row 3 now bans unsafe *items and blocks* (`unsafe {`, `unsafe fn`, `unsafe impl`, `unsafe trait`), excludes the attribute from the ban, and adds "the guard also asserts that `#![forbid(unsafe_code)]` **is present** … (a positive check, so deleting the attribute is a red)" (line 255). §11 S12 agrees. The proof table and hand-off do not repeat it — MINOR-4. |
| **M4** — R76 has no failing oracle | **NOT RESOLVED.** The over-claim moved rather than disappeared: R76 now says "no advisory gate claimed" (line 2956), while the ADR still says "`MB.35.6` records … an advisory check for it" (lines 350–351). The pin is still a record. See MAJOR-2. |
| **M5** — three-state rule only in §14.2; the shipped precedent is `PASS` beside `NOT RUN` | **PARTLY RESOLVED.** The three states are defined (§14.2, line 2970) and handed off (§14.4's `MB.35.6` row). The defined states do not cover a sanctioned dropped leg, no checker is named, and the release job does not depend on the verdict. See MAJOR-4. |
| **m1** — stale line counts | **RESOLVED as a hand-off.** §14.4 records 1174/254/1880 against the measured 1591/337/2477 and tells the leaf to correct or drop them. |
| **m2** — R71's token table has no pin | **PARTLY.** §14.2 now pins the table in the test file ("a token deleted from the guard is a red", line 2965), but R71's own text still only says "a new token cannot land without one" and §14.4 does not say "pinned". See MINOR-5. |
| **m3** — the twins are not run by any gate | **RESOLVED.** §1.3 (lines 246–249): "like `manifest-purity`, that target runs the guard **and** `scripts/test_check_lsp_portable.py`, so the negative twins are gated rather than merely written." |
| **m4** — obligation 3 forbids the exit-code divergence obligation 2 requires | **RESOLVED.** §1.3 obligation 3 is now scoped: "the **only** exit-code difference is the documented one" (lines 294–297). |
| **m5** — archive-extraction test has no acceptance command | **PARTLY.** §14.2 and §14.4 now name a command, but it proves one entry of the three and cannot run the Windows asset. See MAJOR-3. |

---

## MAJOR

### MAJOR-1 (round-2 **B1**, residual) — R70's "four changed lines and nothing else" oracle is unsatisfiable, and the command it cites cannot fail

The design states, twice, a proof that is not the proof the repository permits:

> "**Four** `include_str!` path literals must follow the move … `critical_lsp.rs` must join
> them, `tools/critical_guard.py` sees the change as declared, and the oracle is a diff whose
> every changed line is one of those literals (`git diff <pre-node> --
> crates/blastit/tests/critical_lsp.rs` has four `+`/`-` lines and nothing else)" — §1.3,
> obligation 1 (lines 276–287)

> "a diff against the pre-node commit whose only changed lines are those four literals
> (`tools/critical_guard.py --base <pre-node-commit>`)" — R70 (line 2950)

> "add both paths (one re-export line; four `include_str!` literals); 'assertion bodies
> unchanged, four literals moved'" — §14.4, `MB.35.4` row (line 3004)

Two independent facts break it:

1. **The diff cannot be four lines.** The comment immediately above the literals names the
   old locations:

   ```rust
   // §1's entry is `lsp/mod.rs` and its hover data lives beside it as
   // `lsp/hover.rs`: both are this module, so both carry the same list.
   ```

   `crates/blastit/tests/critical_lsp.rs:2048-2050`. After the move there is no
   `crates/blastit/src/lsp/mod.rs`, and the two sources are in a different crate, so the
   comment is false unless it is reworded. A correct edit is **eight** changed lines (two
   literal lines and two comment lines, each a `-`/`+` pair), not four. The design's oracle
   therefore either ships a false comment in a protected file or fails its own acceptance.
   §14.4 does not hand off the comment reword, and `MB.35.4/NODE.md`'s acceptance is only
   `git diff --find-renames --stat` (line 83), which shows renames, not changed lines.

2. **`tools/critical_guard.py --base` is report-only without `--node` and never reads diff
   bodies.** `_run` returns `EXIT_OK` when `not nodes or args.report`
   (`tools/critical_guard.py:235`); `--node` adds covering-glob authorization
   (`:258-277`), and `evaluate`/`authorizing_pattern` inspect Git statuses and paths, not
   `git diff` output. The sentence "the oracle is a diff whose every changed line is …" is
   attributed to a command that cannot police it.

**Failure mode.** A cheap implementer follows §14.4, replaces the four literals, leaves the
stale comment, runs `git diff --find-renames --stat` plus `critical_guard.py --base`, sees
green, and ships a protected file whose comment describes files that no longer exist. The
"only changed lines" property is still unproven, which is exactly what round 2 asked to make
falsifiable.

**Fix.** Reword the two comment lines in the same edit and state the exact expected count
(eight changed lines, or six if the comment collapses to one line). In R70 and §14.1 quote
both commands, and add both to §14.4's `MB.35.4` row:

```console
$ uv run --no-project tools/critical_guard.py --base <pre-node> --node MB.35.4   # authorization
$ test "$(git diff <pre-node> -- crates/blastit/tests/critical_lsp.rs | grep -c '^[+-][^+-]')" = 8
```

If the intent is to keep the protected file at four changed lines, move the source-grep
assertions into the new crate and say so; do not leave the false comment.

---

### MAJOR-2 (round-2 **M4**, residual) — R76's pin is a record, and the ADR still claims an advisory check that no command performs

> "**R76** … the tree is pinned by `Cargo.lock` (`pg_query` resolves to one version, and
> `cargo update` is what moves it), and the closure is recorded with **no advisory gate
> claimed** — 0.2.0 has no `cargo audit`/`cargo deny` target, which is stated rather than
> implied" — §14.1 (line 2956)

> "**Proved by:** §11's first paragraph; `cargo tree -p blastit-lsp -i pg_query` (one version)
> and the recorded closure in `docs/testing/E2E-MB.35.md` (`MB.35.6`)" — same line

> "`pg_query` stays pinned at `6.2.1`, and `MB.35.6` records the normal closure and an
> advisory check for it." — ADR-0013, Consequences (lines 350–351)

Three problems:

1. **No failing oracle.** `cargo tree -i pg_query` prints two entries if a second version
   arrives; nothing fails. `crates/blastit-sqlpolicy/Cargo.toml:19` is the caret requirement
   `pg_query = "6.2.1"`, and `Cargo.lock` is a committed file, so a deliberate or accidental
   bump is green everywhere. "Pinned by `Cargo.lock`" is true of every dependency and is not a
   property that can be deleted. This is the same defect round 2 reported; the requirement was
   reworded around it rather than given a check.
2. **The ADR contradicts the requirement.** R76 says "no advisory gate claimed"; the ADR says
   `MB.35.6` "records … an advisory check for it". There is no `deny.toml`, no `cargo audit`
   or `cargo deny` target in `Makefile`, `scripts/` or `.github/`, `MB.35.6`'s Goal never
   mentions advisories, and its `paths` reach only `docs/testing/E2E-MB.35.md`.
3. **"The only parsers" is false.** `crates/blastit/src/lsp/mod.rs:310` is
   `serde_json::from_slice::<Value>(&body)`, so the process parses the framed envelope with
   `serde_json` before a `Blastfile` reaches `blastit-manifest` and `libpg_query`. §11's first
   paragraph (lines 2420–2422) omits it too. Scope the claim to the document:
   "the `Blastfile`'s parsers are `blastit-manifest`'s safe Rust and `libpg_query`'s
   parse-only C; the envelope is parsed by `serde_json`."

**Fix.** Either (a) add a `PINNED`-style row to `check_lsp_portable.py` — `cargo metadata
--offline --locked` resolves exactly one `pg_query` and it is `6.2.1`, with a negative twin
that adds a second version and expects red — and delete "advisory check" from the ADR; or (b)
drop "the tree is pinned" and "only parsers" from R76, say the closure is a record, and keep
the proven claims (S2's no-execution, R69's two direct dependencies, R71's closure). Say which
half is a gate and which is a record; do not let the ADR keep the stronger claim.

---

### MAJOR-3 (round-2 **m5**, escalated) — the archive-extraction acceptance checks one of the three entries it claims, omits the consumer's `<version>` join, and cannot run the Windows asset

> "extract to a temp dir, assert the three entries, then smoke **the extracted file**" —
> §14.2 (line 2968)

> "the acceptance is `tar -xzf <asset> -C "$tmp" && test -x
> "$tmp/blastit-lsp-<target>/blastit-lsp" && uv run --no-project scripts/lsp_smoke.py
> --binary "$tmp/blastit-lsp-<target>/blastit-lsp"`" — §14.4, `MB.35.5` row (line 3005)

The command checks only the executable. It never asserts `LICENSE`, never asserts
`SHA256SUMS`, and never asserts "exactly those entries"; the three-entry rule is in §8.3a
(line 1697) and §14.2, but the command that is supposed to prove it does not read them. It
checks `$tmp/blastit-lsp-<target>/<exe>`, while the consumer's join is
`format!("blastit-lsp-{version}/blastit-lsp-{target}/{exe}")` (§8.3a, lines 1689–1693), so the
`<version>` component is untested. On Windows the asset is a `.zip` and `test -x` is Unix-only;
`tar -xzf` does not extract a zip. The design says "on all five targets", but the named command
is Unix-only — and the Windows leg is the one whose root and executable name differ. A zip
nested one level deeper, or missing `LICENSE`, still fails at the user's first start with all
tests green, which is the exact failure round 2 named.

**Fix.** Add one cross-platform `scripts/lsp_smoke.py --archive <asset>` (stdlib `zipfile` /
`tarfile`) that: extracts into a directory named for the asset; asserts the entry set is
exactly `{blastit-lsp[.exe], LICENSE, SHA256SUMS}`; verifies the in-archive `SHA256SUMS`;
constructs the consumer's `blastit-lsp-<version>/blastit-lsp-<target>/<exe>` path; and then
runs the existing smoke. Use it on all five legs and quote it in §14.4, replacing the
`tar -xzf` / `test -x` fragment.

---

### MAJOR-4 (round-2 **M5**, residual) — the evidence-state rule is undefined for a sanctioned drop, names no checker, and gates no release

> "**three** states, defined once: `PASS` = five CI URLs and zero `NOT RUN`; `INCOMPLETE` =
> at least one `NOT RUN` with a reason; `FAIL` = a failing leg. Every target in the `lsp`
> family appears exactly once" — §14.2 (line 2970)

> "If a leg fails at `MB.35.5`, it is dropped from the `lsp` family in
> `scripts/platform_matrix.py` with `dropped_because`" — ADR-0013 (lines 316–319)

A dropped target is neither a CI URL nor a `NOT RUN`: with four URLs and one
`dropped_because`, `PASS` is false (not five URLs), `INCOMPLETE` is false (no `NOT RUN`), and
`FAIL` is false (nothing failed). The state is undefined on the exact path the ADR sanctions,
and dropping a leg is the expected outcome for at least one Apple target.

The rule also has no home that can fail. §14.2 says "a checkable rule in `MB.35.6`" but names
no script, mode, or command; §14.4's `MB.35.6` row says only "the three evidence states"; and
neither the design nor the ADR makes the release job depend on `PASS`. The shipped precedent is
the failure mode: `docs/testing/E2E-MB.34.md:5-6` reads "**VERDICT: PASS** — one item is
`NOT RUN` (§5, no Zed here)", and nothing in `make check` parses the file.

**Fix.** Define the states over the family's *enabled* legs plus dropped legs: `PASS` = one CI
URL per enabled leg **and** every dropped leg has `dropped_because`, with zero `NOT RUN`;
`INCOMPLETE` = an enabled leg has `NOT RUN` with a reason, or a dropped leg lacks one; `FAIL` =
a failing leg. Name the checker (`scripts/check_e2e_states.py` or
`lsp_smoke.py --check-e2e docs/testing/E2E-MB.35.md`), add it to `MB.35.6`'s acceptance, and
make the release job `needs:` it. Say "five until a sanctioned drop, then the family's enabled
count", not "five CI URLs", in §14.2 and the `MB.35.6` hand-off.

---

### MAJOR-5 — R70's portable-binary suite runs on three of five targets; the Windows and Linux-aarch64 legs never run the only oracle for the argv and environment contract

> "`x86_64-pc-windows-msvc` … `windows-latest`, build + smoke transcript"
> "`aarch64-unknown-linux-gnu` … same floor and smoke"
> "`aarch64-apple-darwin` … `cargo test -p blastit-lsp` + smoke"
> — ADR-0013 platform table (lines 310–314)

> "`crates/blastit-lsp/tests/critical_lsp_stdio.rs` … one `--bogus` case that proves the
> portable binary ignores an argument … rather than exiting `2`" — §1.3, obligation 2 (lines
> 288–293)

`cargo test -p blastit-lsp` appears only on the two Apple legs. `lsp_smoke.py` exercises
neither the `--bogus` case nor the cleared environment. The stdio suite is the only oracle for
both, so R70's "`cargo test -p blastit-lsp`" is host-only on two targets — including Windows,
where the argv and environment contract is most likely to differ. A smoke transcript cannot
stand in for the protected test.

**Fix.** Add `cargo test -p blastit-lsp` to the Windows and Linux-aarch64 legs (`pg_query.rs`'s
own CI already ran `cargo test` on `windows-latest` at the pinned version). If a leg cannot run
it, state `skipped_suites_because` and add the `--bogus` and `.env_clear()` cases to
`lsp_smoke.py` so the smoke is at least equivalent.

---

### MAJOR-6 (not a round-2 residual; a gap in R72's proof) — the three-step resolution order has no assertion; a download-first extension is green

> "**R72** The editor's order is settings → `which("blastit-lsp")` / `which("blastit") lsp` →
> one Release download … | `make zed-check` after `MB.33`, whose guard asserts §8.3a's
> properties" — §14.1 (line 2952)

§8.3a's guard table asserts the five asset names and types, the executable join, the download
destination, the unconditional `make_file_executable`, the absence of a `[[capabilities]]`
block, the single URL, and "the two `which` names in order" (lines 1695–1707). It does not
assert that the settings lookup precedes both `which` calls or that the download is last. §14.2
(line 2966) lists the same properties and no ordering check.

**Failure mode.** An implementation that resolves the download first — or falls through to the
download before consulting `LspSettings` — passes every named guard property. A developer with
`binary.path` set gets a network call (and an offline failure) on every cold start, which is
the DX the order exists to prevent, with a green `make zed-check`.

**Fix.** Add `lib: order` to §8.3a: assert the `LspSettings::for_worktree` call precedes both
`which` calls and the `latest_github_release`/`download_file` block, with a negative twin in
`scripts/test_check_zed_extension.py` that swaps steps 1 and 3. Name it in R72 and §14.2.

---

## MINOR

- **MINOR-1 — `main.rs`'s "one shape" has no named assertion.** §1.3 row 3 says it is "pinned to
  one shape — the attribute, one `fn main()`, one `std::process::exit(...)`", but the token
  rule's file set is `lib.rs`/`hover.rs`. A `main.rs` that adds `std::net::TcpListener` or a
  second call passes. State the exact allowed-content check and a negative twin.
- **MINOR-2 — the §14.4 `MB.35.6` row omits the unfiltered closure and `--filter-platform`.**
  R71 requires the named forbidden set over the **unfiltered** closure "plus `--filter-platform`
  per target where resolvable" (line 2951), but the hand-off says only "every dependency
  table" (line 3006). The leaf's brief already has a `cargo metadata` closure bullet; the gap
  is the host-vs-target nuance that catches a target-scoped dependency.
- **MINOR-3 — §14.4's `MB.35.6` row misdescribes its own brief.** It says "two negative twins"
  and "acceptance piped to bare `python3`"; `MB.35.6/NODE.md` already uses `UV_OFFLINE=1 uv run
  --no-project --with pytest==9.2.0 python -m pytest` (line 84) and lists five mutations (lines
  40–43). The row would make a no-op correction. Separately, `MB.35.5`'s acceptance *does* use
  bare `python3` (`MB.35.5/NODE.md:85`), and §14.4 does not correct that leaf.
- **MINOR-4 — the positive `#![forbid(unsafe_code)]` check is in §1.3 row 3 but not in the proof
  table or the hand-off.** §11 S12's assertion column (line 2441) and §14.4's `MB.35.6` row do
  not repeat it, so a cheap implementer reading only the proof/hand-off can still delete the
  attribute green — the property round 2 asked to make falsifiable since the rule now states it.
- **MINOR-5 — R71's token pin lives only in §14.2.** §14.2 says "the token table is **pinned**
  in the test file (a token deleted from the guard is a red)" (line 2965), but R71's own text
  only promises "a new token cannot land without one" and §14.4 says "one twin per token from
  the guard's own table". A guard built from the hand-off alone leaves deletions green.
- **MINOR-6 — the identity rule is narrower than R70's claim.** §1.3 row 5 says "no
  `crates/blastit/src/lsp/` directory"; Rust's other spelling is `crates/blastit/src/lsp.rs`. A
  dead `src/lsp.rs` copy would pass. Assert neither spelling, and assert `src/lib.rs` contains
  the exact `pub use blastit_lsp as lsp;` and no `mod lsp;`.
- **MINOR-7 — `MB.35.6`'s `paths` exclude `docs/**`.** Its Goal says "If a documented command
  is wrong, the fix is on the page (list `docs/**` here)", but `paths` are the four files in
  `scripts/`, `Makefile` and `docs/testing/`. The runnable-docs half is blocked by the lane;
  add the page or a follow-up node to §14.4.

---

## Verified true (so not findings)

- **Four `include_str!` literals, exactly.** `critical_lsp.rs:2052-2053` and
  `hover.rs:294,316` move; `:1419`, `:2036` and `:2093` do not. The path arithmetic is
  correct: `../../blastit-lsp/src/{lib,hover}.rs` from `crates/blastit/tests/`, and
  `../../../docs/reference/blastfile.md` from `crates/blastit-lsp/src/`.
- **The re-export is one line and its dependency already exists.**
  `crates/blastit-manifest/Cargo.toml:15` already has `blastit-sqlpolicy`;
  `blastit_sqlpolicy::parameterize` is public (`template.rs:333`); `lib.rs:108` already
  re-exports from that crate.
- **The line counts are right.** 1591 / 337 / 2477; §14.4 records the brief's 1174/254/1880
  as stale rather than repeating them.
- **`make lsp-portable` has a real precedent and a slot.** `manifest-purity` runs both the
  guard and its self-test (`Makefile:166-168`), and `make check` has the prerequisite list
  (`Makefile:823`); `lsp-portable` is a genuine hand-off, not a green line about another
  family.
- **`critical_guard.py --base/--node` is real** (`:258-277`) and its covering-glob rule is
  what makes `MB.35.4`'s first red outcome real. The missing `--node` in R70 is a citation
  bug, not a missing tool.
- **`platform_matrix.py` already enforces `dropped_because`** and its `--check` scans every
  `docs/**/*.md`, so R74's distinct count noun is a real constraint once the family exists.
- **`ADR-0010` §4, `ARCHITECTURE.md` §2.7 and the amendment agree** on the crate row
  ("**exactly** `blastit-manifest` and `serde_json`"), the five targets, the unprivileged-only
  claim and the "never serves" sentence. No requirement in R69–R76 lacks a document of record.

## Closing

The round-2 plan contradictions are genuinely handed off, and the extraction arithmetic,
crate boundary, token collision and re-export ownership are now right. What remains is proof
quality: R70's diff is not the diff the design says it is and its cited command cannot fail;
R76's pin is a record while the ADR still claims an advisory check; the extraction command
proves one entry of three and not the Windows layout; the evidence-state rule has a hole where
a sanctioned drop lives and no gate; two of five targets never run the portable binary's
protected suite; and R72's resolution order is asserted only in prose. These are local fixes,
not a redesign — but until they are in, `MB.35.3`'s own acceptance sentence ("R69–R76 each have
a proof that fails when the property is deleted") is not met.
