VERDICT: FAIL

# Implementability review, final pass — `MB.35.3` (portable `blastit-lsp`)

**Lens:** `implementability` (cheap-model buildability).
**Prior rounds:** `docs/design/portable-lsp-arch.review-implementability.md` (round 1, two BLOCKERs),
`docs/design/portable-lsp-arch.review-implementability.round2.md` (0 BLOCKER, 2 MAJOR).
**Scope:** `docs/design/blastfile-lsp-zed.md` §8.3a and §14 (including §14.4),
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (the `MB.35.3` amendment),
checked against `plan/nodes/MB/MB.35/{MB.35.4,MB.35.5}/NODE.md`, `scripts/platform_matrix.py`,
`Makefile`, and `crates/blastit/src/{cli.rs,lsp/}`, `crates/blastit-manifest/src/lib.rs`.

Read in `.worktrees/release-0.2.0` at base `143afaf` plus the uncommitted `MB.35.3` working tree.

**Counts: 0 BLOCKER, 2 MAJOR, 5 MINOR.**

The round-2 numeric claim is now in `ADR-0010` §4, so that MAJOR is closed. The drop-a-leg
finding is **not** closed: the design disowns the doc edits but the ADR still orders them "in the
same commit", and — now that the numeric claim exists — the missing edit is what turns
`make platform-matrix-check` red on exactly the failure path. A second MAJOR appears in the
same claim: R74's prescribed count noun cannot match the sentence `MB.35.3` wrote, so the gate
would be green about a claim it never reads. `MB.35.4` is buildable from the design plus §14.4;
`MB.35.5` is buildable on the happy path only.

---

## Round-2 disposition

| Round-2 | State | Evidence |
|---|---|---|
| **MAJOR-1** — the numeric `lsp` claim is missing from `ADR-0010` §4 | **resolved** | `ADR-0010` line 47: "**The `lsp` family is declared where every leg is, and its one numeric claim is `5` `blastit-lsp` assets** — the count `--check` reads once `MB.35.5` adds the noun to the gate." The file is in `MB.35.3`'s `paths`, so the node can own it. Residual: the noun R74 names cannot read this sentence — see MAJOR-2. |
| **MAJOR-2** — the drop-a-leg doc edits are owned by no leaf | **not resolved** (and now worse) | §14.4's new row (line 3007) says the edits are "the lead's amendment or a dispatched doc leaf, not that leaf's commit". `ADR-0013` lines 316–325 were not amended and still say "in the same commit … from every place that states the targets". Because the claim of round-2 MAJOR-1 now exists, the un-owned edit is a red gate rather than stale prose — see MAJOR-1 below. |

Round-2 MINOR-2 (the "four points" paragraph now lists the `binary.env` fourth point) is closed.
MINOR-1, MINOR-3, MINOR-4, MINOR-5, MINOR-6 and MINOR-7 are still open or partly open (below).

---

## MAJOR-1 — the drop path cannot pass `make check`: the one doc the leaf may not edit is the one that goes red

**Quote (`ADR-0013`, lines 316–325):**

> "**If a leg fails at `MB.35.5`**, it is dropped from the `lsp` family in
> `scripts/platform_matrix.py` with `dropped_because`, and — in the same commit — from every
> place that states the targets: this amendment's platform table, design §8.3a's asset table and
> §14.3's five bullets, `docs/reference/cli.md` `## blastit-lsp`'s table,
> `docs/troubleshooting.md`'s "no asset for that pair" row, and `ADR-0010` §4's row."

**Quote (design §14.4, line 3007):**

> "`MB.35.5` owns the matrix family and the workflow; its `paths` forbid `docs/**`, so the page
> and ADR edits are the lead's amendment or a dispatched doc leaf, not that leaf's commit — the
> design does not pretend one node can do both"

**Why this now blocks rather than merely drifts.** `declared_of()` counts only enabled legs
(`scripts/platform_matrix.py:282–310`), and `check()` refuses any document whose stated count
exceeds that ceiling (`:707–717`: `count > ceiling` → `problems.append(...)`). Once the `lsp`
family exists, a dropped Apple leg makes the ceiling one lower than `ADR-0010` §4's "`5`
`blastit-lsp` assets", so `make platform-matrix-check` is **red**. That target is:
- in `make check` (`Makefile:823`), and
- named in `MB.35.5`'s own acceptance (`plan/nodes/MB/MB.35/MB.35.5/NODE.md:92`,
  `make workflow-check platform-matrix-check packaging-gates-test`), and
- listed in §14.2 (line 2967) as one of R74's proofs.

The only green route is editing `ADR-0010` §4 (and the pages) in that commit — precisely what
`MB.35.5`'s `paths` (lines 9–16) and its "**Do not touch** … `docs/**`" line (74–76) forbid.
§14.4 assigns the edit to "the lead", but the lead is not a step inside the node: nothing in the
leaf can make the gate green while it runs, and `attempts: 3` will be spent before any dispatch
happens. The two Apple legs are explicitly "buildable, unproven" (`ADR-0013` platform table), so
this is the likely path, not an edge case.

**Failure mode for a cheap implementer.** A red Apple leg is dropped in the matrix with
`dropped_because` as instructed; `make check` goes red on `ADR-0010` line 47; the implementer
reads `ADR-0013` ("same commit"), edits five docs, and the lane rule (`AGENTS.md` 5,
`plan.py validate`/`guard.py`) refuses the commit. Reading §14.4 instead leaves the gate red.
Either way the node fails, and the failure is in the design, not the code.

**Exact fix (choose one, both in `MB.35.3`'s `paths`).**
1. Add the five documents to `MB.35.5`'s `paths` and narrow its blanket line to "except the
   drop-a-leg sentences `ADR-0013` enumerates"; or
2. Keep the doc edits off the leaf, and make the gate survivable on the drop path: amend
   `ADR-0010` §4 so the claim is not a fixed number a dropped leg can refute (e.g. state the
   family and let `--summary` own the count), and replace `ADR-0013`'s "in the same commit" with
   "`MB.35.5` drops the leg and records `dropped_because`; the page and ADR sentences are a
   doc-sync leaf the lead dispatches, and `platform-matrix-check` stays red until it lands" —
   plus the same sentence in §14.4. Option 2 still needs a plan entry (`MB.35.5a`) or an explicit
   waiver, because `MB.35.5`'s acceptance cannot include a target that is known-red at exit.

Also note a record-level point: a design row cannot supersede an **Accepted** ADR. If the
design's position is the one that stands, `ADR-0013` must say so; a future reader (and the next
cheap implementer) will trust the ADR.

---

## MAJOR-2 — R74's count noun cannot match the claim `ADR-0010` now carries, so `--check` reads nothing

**Quote (design R74, line 2954):**

> "… the page noun `blastit-lsp assets?` in `CLAIM`/`PROVEN_CLAIM` (not bare `assets`, which
> would collide with the standalone family), `lsp` branches in `declared_of`/`proven_of` …"

**Quote (`ADR-0010`, line 47):**

> "… its one numeric claim is `5` `blastit-lsp` assets** — the count `--check` reads …"

`CLAIM` (`scripts/platform_matrix.py:455–467`) is `MARK count MARK \s+ QUALIFIER family`, and
`QUALIFIER` includes the alternative `` `[^`\n]*`\s+ `` (`:458–459`) — one backticked span.
In the ADR sentence the qualifier therefore consumes "`` `blastit-lsp` `` " *including the name*,
so a family alternative spelled `blastit-lsp assets?` can never match: the text left for the
family is bare `assets`. Verified against the exact regex and the exact sentence: adding the
noun as R74 writes it yields **no match**; only an alternative containing bare `assets?` matches
(`('5', 'assets')`). The `--check` output today confirms the sentence is invisible: no `lsp`
note is printed for `ADR-0010:47`.

There is a second, independent mismatch. `declared_of`/`proven_of` index a dict by the captured
family (`:525–549`), and `check()`'s `declared_keys` uses the plural family word as its first
element (`:658–665`). R74 asks for "`lsp` branches", but neither candidate capture (`assets` or
`blastit-lsp assets`) equals `lsp`; the first raises `KeyError` in `declared_of`, the second
raises it too, and R74 gives no key spelling. `PROVEN` entries would then be refused unless the
same key is invented in `declared_keys`.

**Failure mode.** The implementer follows R74 literally, `CLAIM` gains an alternative that
matches nothing, `make platform-matrix-check` stays green, §14.2's "a stated `lsp` count above
the declared family" can never fire, and the round-2 MAJOR is re-created as a vacuous gate —
exactly the failure the testability review named ("the gate went vacuous the first time",
`scripts/platform_matrix.py:421–436`). If they instead add bare `assets?`, `declared_of` raises
`KeyError` until they guess the key, which R74 does not give.

**Exact fix.** In R74 state the whole contract in one line, e.g.: "`assets?` is added to the
`CLAIM`/`PROVEN_CLAIM` family list (no serving page states a numeric `assets` count — verified),
`declared_of`/`proven_of` gain an `asset` key mapping to the enabled `lsp` legs, `declared_keys`
unions `("assets", target)`, and `PINNED` gains `ADR-0010` §4's "`5` `blastit-lsp` assets"
line with the twin 'delete the sentence → red'." Drop the parenthetical's reason: `assets` does
not collide with the serving families (their nouns are `archives`/`tarballs`); what collides is
renaming the LSP assets "archives", which `ADR-0010` already forbids.

---

## Leftover MINORs (brief)

1. **§8.3a line 1699's "explicit" archive glob still matches the editor family.** Verified in a
   scratch directory: `blastit-*-unknown-linux-*.tar.gz` matched
   `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz`. So the row's claim "so neither count swallows
   the other" is false, and `--count binaries` would over-count once both families are in
   `dist/`. Spell the two names: `blastit-x86_64-unknown-linux-*.tar.gz` and
   `blastit-aarch64-unknown-linux-*.tar.gz`, and include the Windows `.zip` in the final sums
   explicitly. (It fails loudly in the leaf's own gate, hence MINOR.)
2. **`MB.35.5`'s acceptance still runs bare `python3`.** `NODE.md:85` pipes `--matrix lsp`
   through `python3 -c`, and `NODE.md:86`/`:94` invoke `scripts/platform_matrix.py` directly;
   both are AGENTS rule 9 violations and §14.4's `MB.35.5` row does not correct them (the
   `MB.35.6` row does say `uv run --no-project python`). Add it to §14.4's `MB.35.5` row.
3. **`--max-glibc` is still assumed on `build-lsp.sh`.** `ADR-0010` §4, §8.3a line 1695 and
   §14.4 read as if the new script already parses it; the flag lives in
   `scripts/build-standalone.sh:49`. Say "factor `build-standalone.sh`'s readelf floor block
   into a helper and give `build-lsp.sh` the same check", as round 2 asked.
4. **§14.3 line 2979 still says "declared, unbuilt"** for the arm64 Linux leg; the matrix's word
   is *declared, unproven* (`PROVEN` is built-and-exercised). Same as round-2 MINOR-5.
5. **`MB.35.4`'s brief still cites `cli.rs:572`;** the call is at `:586` (verified), and §14.4
   does not correct it. Also carries: `MB.35.6`'s `paths` still cannot make the docs fix its own
   brief invites (round-2 MINOR-7) — not this node's, but still unowned.

---

## Verified true (no change needed)

- **`MB.35.4` is buildable from design + §14.4.** Both missing `paths` are handed off
  (`crates/blastit-manifest/src/lib.rs`, `crates/blastit/tests/critical_lsp.rs`), the four
  `include_str!` literals and "assertion bodies unchanged" are stated, `#![forbid(unsafe_code)]`
  and the identity rule are stated, and the import rewrite
  (`use blastit_manifest::parameterize;`) is named. The cited anchors are real:
  `crates/blastit-manifest/src/lib.rs:108` re-exports beside `TableGrant`/`Columns`;
  `crates/blastit/src/cli.rs:586` is the `crate::lsp::run()` call; the files are 1591/337/2477
  lines, matching §14.4's corrected counts.
- **`MB.35.5`'s happy path is buildable.** The five asset names agree across `ADR-0013`,
  §8.3a, `MB.35.5`'s list and the extension's join; `declared_of`/`proven_of`/
  `declared_keys`/`--summary`/`--matrix`/`--leg`/`--count` exist at the seams R74 names
  (`scripts/platform_matrix.py:282–371, 525–549, 658–665, 745+`); `OUTPUT_SPECS` and the
  per-leg `dist/SHA256SUMS.<target>` pattern have current models to copy
  (`publish-pypi.yml:160–187, 299–301`); `manylinux_2_28` plus `build-standalone.sh`'s floor
  block are the right precedent for the Linux legs.
- **The new claim cannot break the gate before `MB.35.5` runs.** `assets` is not a family
  today, so `make platform-matrix-check` passes now with no `lsp` note (run at base `143afaf` +
  this tree); the pinned serving counts still match.
- **`ADR-0010` §4, `ARCHITECTURE.md` §2/§2.7 and §1.3 agree** on the crate's dependency set,
  the five-target family, the editor-only scope and `pub use blastit_lsp as lsp;`.
