VERDICT: FAIL

# Implementability close-out — `MB.35.3`

**Lens:** `implementability` (cheap-model buildability).
**Scope:** the two claimed resolutions and `MB.35.4`'s buildability. Read in
`.worktrees/release-0.2.0` at base `143afaf` plus the uncommitted `MB.35.3` tree.

**Counts: 0 BLOCKER, 2 MAJOR, 2 MINOR.**

---

## MAJOR-1 — the drop path still has no exit a leaf can take: the red-by-design sentence is in the resolution table, not §14.4

**Landed** (`ADR-0013`, line 318):

> "… in the same change set, which is a doc edit by whoever owns those files …"

**Not landed** (`design` §14.4, line 3009, the `*a dropped leg*` row — the whole row):

> "`MB.35.5` owns the matrix family and the workflow; its `paths` forbid `docs/**`, so the page
> and ADR edits are the lead's amendment or a dispatched doc leaf, not that leaf's commit — the
> design does not pretend one node can do both"

The row names an owner but never the gate state, and it is the only dropped-leg row a leaf reads.
The sentence the author cites appears **only** in the resolution table (`design`, line 3083):
"§14.4 says `make check` is red between the matrix edit and the doc edit **by design**". A
`grep -n "red between\|by design"` over `docs/` returns line 3083 alone; neither §14.4 nor
`ADR-0013` says it.

**Failure mode (unchanged).** A failing leg at `MB.35.5` is dropped with `dropped_because`;
`ADR-0010` line 47 still states "`5` lsp assets"; `declared_of()` counts enabled legs only
(`scripts/platform_matrix.py:525–549`), so `make platform-matrix-check` is red. `MB.35.5`'s
acceptance (`NODE.md:92`) includes that target, its `paths` (lines 9–16) contain no `docs/**`,
and `NODE.md:74–76`'s parenthetical ("list the page in `paths` if so") is not an action a leaf can
perform — `paths` is managed frontmatter. There is no `MB.35.5a` and no acceptance waiver, so
`attempts: 3` is spent with no green route.

**Exact fix (one of):**

1. Put the sentence in §14.4 and name the exit state — "`make workflow-check
   packaging-gates-test` green; `platform-matrix-check` red until the doc edit lands; the lead
   dispatches `MB.35.5a`" — and add that node, or an explicit acceptance waiver, to the plan; or
2. Add `ADR-0010` §4 and the page sentences `ADR-0013` enumerates to `MB.35.5`'s `paths` and
   narrow the blanket "Do not touch" line, so one leaf can close the gate.

---

## MAJOR-2 — the noun is fixed but the key is not: `lsp` cannot equal the captured family

**Landed.** `ADR-0010` line 47 now reads "its one numeric claim is `5` lsp assets", with no
backticked span. Applying the current `CLAIM` (`scripts/platform_matrix.py:455–467`) with
`lsp assets?` appended to that exact sentence yields `('5', 'lsp assets')`, so the `QUALIFIER`
backticked-span alternative no longer swallows the name. That half is genuinely fixed.

**Not landed** (`design`, line 2956, R74):

> "the page noun `lsp assets?` in `CLAIM`/`PROVEN_CLAIM` and a `declared_of`/`proven_of` key
> `lsp`"

`declared_of`/`proven_of` (`scripts/platform_matrix.py:525–549`) normalize the captured family
before the dict lookup:

```python
family = family.lower().rstrip("s")
```

`lsp assets` becomes `lsp asset`, never `lsp`. A literal `"lsp": …` entry is therefore unreachable;
`check()` calls `declared_of(match.group("family"))` and raises `KeyError`, crashing
`make platform-matrix-check`. R74 also says "`lsp` branches" two clauses later, so the implementer
is given both a dict key and a branch and must guess which. `declared_keys` (`:658–665`) unions raw
tuple labels, so `("lsp", target)` there is internally consistent with `PROVEN` but does not make
the `"lsp"` dict key reachable.

**Failure mode.** The implementer follows "key `lsp`", adds it, runs the gate and gets a traceback
on the one sentence this MAJOR is about; the repair (`lsp asset`, or `family.startswith("lsp")`, or
stripping an `lsp ` prefix before the lookup) is invented, not specified.

**Exact fix.** Pin one label in R74 and §14.4: "the captured family is `lsp assets`; normalize to
`lsp asset` and key both maps that way, with `declared_keys`/`PROVEN` using the same label", or
keep `lsp` and state the normalizer change. Then the claimed agreement is true.

---

## MINOR

1. R74's own example still reads "(`5` `blastit-lsp` assets)" (`design:2956`), which no longer
   matches `ADR-0010`'s sentence and matches nothing once `lsp assets?` is added (verified: zero
   matches) — stale half of the same claim, unpinned so not red.
2. §14.4's `MB.35.4` row leaves `cli.rs:572` uncorrected (the call is
   `crates/blastit/src/cli.rs:586`) and still shows the stale 1174/254/1880 counts; "correct or
   drop the counts" covers the counts, not the anchor.

---

## Verified true

- `ADR-0010` line 47 carries the count in the form the new family alternative reads; no
  backticked span remains.
- **`MB.35.4` is buildable from §14.4** (`design:3006`): both missing `paths` are handed off
  (`crates/blastit-manifest/src/lib.rs`, `crates/blastit/tests/critical_lsp.rs`), the four
  `include_str!` literals are named (two in `hover.rs` for the docs page, two in `critical_lsp.rs`
  for the moved sources), "assertion bodies unchanged" is stated, `#![forbid(unsafe_code)]` and the
  structural identity check are stated, and the import rewrite is named. `blastit-manifest` already
  depends on `blastit-sqlpolicy` (`Cargo.toml:15`), so the one re-export line is real; the anchors
  are real (`cli.rs:586` is the `crate::lsp::run()` call; the files are 1591/337/2477 lines).
