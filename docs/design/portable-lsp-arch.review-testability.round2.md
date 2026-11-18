VERDICT: FAIL

Lens: testability (verification & oracles). Round 2 of the `MB.35.3` review, read blind
against `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (`MB.35.3` amendment),
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4, `docs/design/blastfile-lsp-zed.md`
§1, §1.3, §8.3a, §11, §14 and the requirement index, and `ARCHITECTURE.md` §2.7, checked against
`docs/design/portable-lsp-arch.review-testability.md` (round 1), `plan/nodes/MB/MB.35/{MB.35.4,
MB.35.5,MB.35.6}/NODE.md`, `Makefile`, `tools/critical_guard.py`, `scripts/platform_matrix.py`,
`scripts/check_workflows.py`, `crates/blastit/tests/critical_lsp.rs` (protected, 2477 lines) and
`docs/testing/E2E-MB.34.md`.

**Counts: 1 BLOCKER, 5 MAJOR, 5 MINOR.** The design text answers every round-1 finding, but nine
of those answers stop at the prose: the node that must execute them still says the old thing
(B1, B2→M1, M1/M2/M3/M5→M2, M6→M5, m2→m1, m5). Verified in this tree: `mod.rs` 1591 lines,
`hover.rs` 337, `critical_lsp.rs` 2477, `pg_query` 6.2.1 in `Cargo.lock`.

---

## BLOCKER

### B1 (round-1 **B1**, residual) — R70's `critical_lsp.rs` edit is asserted by the design and forbidden by the node that runs it

The design now states, as fact:

> "`critical_lsp.rs` joins `MB.35.4`'s `paths`, `tools/critical_guard.py` sees the change as
> declared, and the oracle is a diff whose every changed line is one of those literals
> (`git diff <pre-node> -- crates/blastit/tests/critical_lsp.rs` has four `+`/`-` lines and
> nothing else)" — design §1.3, obligation 1 (lines 279–283)

`plan/nodes/MB/MB.35/MB.35.4/NODE.md` says the opposite, twice:

> "It is the standalone binary's own proof; the 1880-line `crates/blastit/tests/critical_lsp.rs`
> stays **byte-identical** and keeps proving the delegation." (§Goal, item 3)

> "`crates/blastit/tests/critical_lsp.rs` is protected and **must not be edited** — it is not in
> `paths`, deliberately." (§Context, "Protected files")

Its `paths` are still `crates/blastit-lsp/**`, `crates/blastit/src/lsp/**`,
`crates/blastit/src/lib.rs`, `crates/blastit/Cargo.toml`, `Cargo.toml`, `Cargo.lock` — no
`crates/blastit/tests/critical_lsp.rs`. `tools/critical_guard.py` enforces exactly this
(`BREAKING_STATUSES = {"M","D","T"}`; a breaking change without a covering glob is a violation),
so the node as written has two red outcomes: keep the literals and the test crate fails to
compile at `cargo test -p blastit --test critical_lsp` (the file's fourth acceptance command), or
edit them and the protected-file guard fires.

The factual half of round 1 is now right and I confirm it: `critical_lsp.rs` has five
`include_str!` calls — `:1419` (`docs/reference/blastfile.md`), `:2036` (`../src/cli.rs`),
`:2052-2053` (`../src/lsp/{mod,hover}.rs`), `:2093` (`blastit-manifest/src/error.rs`) — and only
the two at `:2052-2053` follow the move to `../../blastit-lsp/src/{lib,hover}.rs`. "Two
literals" is exact.

Two things the round-1 fix added are still wrong:

1. **The oracle is a trap.** The comment immediately above those literals names the old paths
   (`:2048-2050`):

   ```rust
   // §1's entry is `lsp/mod.rs` and its hover data lives beside it as
   // `lsp/hover.rs`: both are this module, so both carry the same list.
   ```

   A "four `+`/`-` lines and nothing else" diff therefore *requires* leaving a false comment in a
   protected file. State the oracle as "two literals replaced **and** the two comment lines
   reworded — six changed lines, no assertion touched", or move the source-grep test as round 1
   proposed.
2. **The diff is not machine-checked.** §1.3 gives the property; §14.1 R70 attributes it to
   `tools/critical_guard.py --base <pre-node-commit>`, which only checks covering globs (verified:
   `--base`/`--node` exist at `tools/critical_guard.py:259-277`; nothing in the file compares diff
   bodies). The `grep -c '^[+-][^+-]'` command round 1 asked for is still absent.

**Fix.** Apply the two plan edits the design already describes — `crates/blastit/tests/
critical_lsp.rs` in `MB.35.4.paths`, and delete "must not be edited"/"byte-identical" from its
Goal and Context — in the same change as the amendment, and quote the exact runnable diff command
in §14.1. Until the node changes, `MB.35.3` fails its own acceptance sentence ("The design is
accepted when `MB.35.4` (extraction) and `MB.35.5` (CI) can be written without re-deciding a
name, a format, a target or an invariant"): `MB.35.4` must re-decide whether it may touch a file
its own brief forbids.

---

## MAJOR

### M1 (round-1 **B2**, residual) — the `parameterize` re-export correction is recorded but owned by nobody

> "**`MB.35.4` owns it and its `paths` must be widened to name that file** (today's list does
> not, so the leaf would otherwise have to choose between a third dependency and a lane
> violation)" — design §1.3 (lines 262–264)

> "`MB.35.4` owns that one line, and its `paths` must name
> `crates/blastit-manifest/src/lib.rs` — the plan's list does not yet" — ADR-0013 `MB.35.3`
> amendment, decision 2

The ADR is honest, and the premise is real: `crates/blastit/src/lsp/mod.rs:55` is
`use blastit_sqlpolicy::parameterize;`, and `blastit-manifest`'s `lib.rs` is still not in
`MB.35.4.paths`. The amendment records a correction that no node will apply — `MB.35.3.paths`
are the four documents, and there is no "hand-offs" section like §12.8 saying who edits the leaf.
So R69's two red outcomes survive verbatim; only the prose that predicts them changed.

**Fix.** Add the paths line to `MB.35.4/NODE.md` now, or add one §14.4 "hand-offs" table (the
§12.8 precedent: "*Rows marked hand-off belong to a sibling node's acceptance block*") naming the
exact edit per node. Do not let "the plan's list does not yet" be the last word.

### M2 — Five round-1 fixes exist only in §14.2; the guard's own brief still specifies the old, weaker oracle

§14.2 promises the guard covers "every dependency table", "one mutated source per token", the
identity rule, and the widened token list. `plan/nodes/MB/MB.35/MB.35.6/NODE.md` still says:

> "`crates/blastit-lsp/Cargo.toml`'s `[dependencies]` is exactly `{ blastit-manifest, serde_json }`"

> "no `crates/blastit-lsp/src/**` line contains `std::os::unix`, `std::os::linux`,
> `std::os::windows`, `libc::`, `rustix`, `nix::`, `pyo3`, `#[cfg(unix)]`, `#[cfg(target_os` or
> `std::process::Command`" (§Goal, item 1)

> "one mutation per property on a throwaway copy (add `rustix` → red; add
> `use std::os::unix::fs` → red; add a `Command::new` → red; …)" (§Goal, item 2)

So, on disk: `[dependencies]` only (no `[build-dependencies]`, no `[target.*]`, no optional
features — round-1 **M2**); ten tokens plus a separate `std::env` bullet against §1.3 row 3's
twenty-four, with `std::fs`, `std::path`, `std::net`, `unsafe`, `extern "C"` and `#[cfg(windows)]`
absent (round-1 **M1**); two tokens with a negative twin, and two of the five mutations proving
rustc or the Makefile rather than this guard (round-1 **M3**); no unfiltered-closure-per-target
walk; no structural identity rule (round-1 **M5**); no `--filter-platform`. A cheap implementer
follows the node; the properties stay unproven and `make lsp-portable` goes green over a crate
that could `TcpListener::bind` or `fs::read`.
`MB.35.4` likewise has no identity assertion, although §1.3 row 5 says "the new crate's protected
suite" carries it.

**Fix.** One §14.4 hand-off table (M1's), with the token table copied verbatim from §1.3 row 3
into `MB.35.6`'s Goal and "every table" / "`--filter-platform`" spelled out, plus "`critical_lsp_
stdio.rs` gains rule 5's structural assertion" in `MB.35.4`.

### M3 — The guard cannot be green: the token `unsafe` and the required `#![forbid(unsafe_code)]` are the same string

> "no line in `crates/blastit-lsp/src/lib.rs` or `src/hover.rs` contains any token of §1.2's
> list — … nor `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os`, `unsafe`, or `extern "C"`"
> — design §1.3, row 3

> "`src/lib.rs`/`src/hover.rs` hold no `std::fs`, … `unsafe` or `extern "C"` (the full token list
> is §1.3's third row, and `#![forbid(unsafe_code)]` is the compiler half)" — §11 S12

> "plus `#![forbid(unsafe_code)]` on both the library and the binary" — ADR-0013 decision 1

The repo's grep convention is substring `contains` (`crates/blastit/tests/critical_lsp.rs:2070`:
`assert!(!source.contains(forbidden))`). `#![forbid(unsafe_code)]` contains `unsafe`, so the
required header is a false red on the very rule that exists to keep an `extern "C"` declaration
out — and the cheapest way for a cheap implementer to get R71/R73/S12 green is to delete the
attribute, i.e. the guard's fix would weaken the property. The `unsafe` twin round-1 **M1** asked
for makes the collision deterministic.

The same ambiguity breaks the `main.rs` pin: "`src/main.rs` is **outside** this rule and pinned
instead to its three-line shape — one `std::process::exit(blastit_lsp::run() as i32)` and nothing
else" (§1.3 row 3) contradicts ADR decision 1's `#![forbid(unsafe_code)]` "on both the library
and the binary": a file cannot both carry an inner attribute and be "nothing else" in three
lines.

**Fix.** Name the patterns, not the words: forbid `unsafe` followed by `{`/`fn`/`impl`/`extern`,
and add a per-token assertion that `#![forbid(unsafe_code)]` **is present** in `lib.rs` (its own
twin: delete the attribute → red). Replace the `main.rs` "three lines" pin with an exact allowed
content set (`forbid(unsafe_code)` + one `fn main` calling `blastit_lsp::run()` through
`process::exit`).

### M4 — R76 has no failing oracle at all

> "**R76** … `pg_query` pinned at `6.2.1` and its closure's advisories recorded | §11's first
> paragraph; `MB.35.6`'s recorded closure and advisory row in `docs/testing/E2E-MB.35.md`"
> — design §14.1

Nothing in this repository reads that row: there is no `deny.toml`, no `cargo audit`, no
`cargo deny` reference in `Makefile`, `scripts/` or `.github/` (checked). `MB.35.6`'s Goal never
mentions advisories and its paths only reach `docs/testing/E2E-MB.35.md`. And "pinned at 6.2.1" is
a lockfile fact, not a requirement: `crates/blastit-sqlpolicy/Cargo.toml:19` is the caret
requirement `pg_query = "6.2.1"`, so any `cargo update` moves it, and nothing fails. Delete the
property (bump the crate, drop the row) and every check stays green — which is what round 2 was
asked to test.

**Fix.** Give R76 the cheap machine half the repo already uses for pinned facts: assert
`Cargo.lock`'s `pg_query` version equals the recorded one (a `PINNED`-style row in
`check_lsp_portable.py`), so a bump is a deliberate red, and keep the advisory row as the review
evidence it actually is. Say which half is a gate and which is a record.

### M5 (round-1 **M6**, residual) — the three-state rule exists in §14.2 only, and the repo already has the bad precedent

> "**three** states, defined once: `PASS` = five CI URLs and zero `NOT RUN`; `INCOMPLETE` = at
> least one `NOT RUN` with a reason; `FAIL` = a failing leg" — design §14.2, "the evidence states"

`plan/nodes/MB/MB.35/MB.35.6/NODE.md` still says both old things:

> "`VERDICT: PASS` only when every documented platform has evidence." (§Goal, item 4)

> "`docs/testing/E2E-MB.35.md` ends with `VERDICT: PASS`, and every platform in `MB.35.5`'s
> matrix has either a CI URL or `NOT RUN` with the reason." (# Acceptance)

The second is the box a cheap implementer ticks, and this repository already ticks it:
`docs/testing/E2E-MB.34.md:5-6` reads "**VERDICT: PASS** — one item is `NOT RUN` (§5, no Zed
here)". So the failure mode is not hypothetical; it is the shipped convention, and nothing in
`make check` parses the file (no `E2E-` reference in `Makefile`, `scripts/` or `python/tests/`).
For the Apple legs "`NOT RUN`" is the *expected* state, so `PASS`-beside-`NOT RUN` is the likely
ending, not the unlikely one.

**Fix.** Put the rule where it can fail: `MB.35.6` gains a `--check-e2e docs/testing/E2E-MB.35.md`
mode (or a ten-line test) asserting `PASS ⇒ NOT RUN count == 0`, and `INCOMPLETE` otherwise, and
`MB.35.5`'s release job depends on `PASS`. State it in the node, not only in §14.2.

---

## MINOR

### m1 (round-1 **m2**, residual) — `MB.35.4` still carries three wrong line counts
Design §1 row 1 is fixed ("~1.9k lines as shipped: 1591 + 337"). `MB.35.4/NODE.md` still says
"the 1880-line `critical_lsp.rs`" and "`mod.rs` (1174 lines) + `hover.rs` (254)".
Verified: 2477 / 1591 / 337. Record-text, but a cheap implementer reads "1174" as the file it is
moving and reports a surprise. Fix the node, or drop numbers there.

### m2 — R71's token table has no pin, so a token can be *deleted* without a red
R71 now reads "each token with a negative twin **driven by the guard's own token table**, so a
new token cannot land without one". Additions are covered; deletions are not — remove a token
from the table and its twin disappears with it, and `make lsp-portable` stays green while S12
loses a claim. The repo's precedent is `scripts/platform_matrix.py`'s `PINNED` list, which fails
when "a count gate that pins a missing document is not a gate". Add one assertion naming the
exact token set (as `critical_lsp.rs:2056-2073` already does for its smaller list).

### m3 — the twins are not run by any gate
`scripts/test_check_lsp_portable.py` appears only in `MB.35.6`'s acceptance block; `make check`
runs `packaging-gates-test` for its peers' self-tests but nothing runs this one
(`grep -rn test_check_lsp_portable Makefile scripts/` → nothing). A negative twin that never runs
is a twin that rots. Add it to `lsp-portable` or to `packaging-gates-test`.

### m4 — §1.3 obligation 3 forbids the exit-code divergence obligation 2 requires
> "No message, sentence, bound, exit code or capability changes" (obligation 3)

vs. the new, correct case: "one `--bogus` case that proves the portable binary ignores an
argument (reads stdin, exits `1` at EOF) rather than exiting `2` like `blastit lsp`"
(obligation 2). The divergence is intentional, documented in `docs/reference/cli.md`, and now
tested — but obligation 3's absolute wording would make a reviewer file it as a bug. Scope it:
"no *session* exit code, message, sentence, bound or capability changes; argv handling differs as
obligation 2 states". (Round-1 **m4** is otherwise resolved: the case is in §1.3 and §14.2, and
`critical_lsp.rs:2022-2026` still asserts `2` for `blastit lsp --bogus`.)

### m5 (round-1 **m5**, residual) — the archive-extraction test has no acceptance command
§14.2 promotes it to a real test — "extract to a temp dir, assert the three entries, then smoke
**the extracted file**" — but `MB.35.5`'s acceptance still has only
`tar -tzf …/blastit-lsp-*.tar.gz        # one dir, one executable`, i.e. the eyeball round 1
flagged. The producer/consumer join (the extension's
`format!("blastit-lsp-{version}/blastit-lsp-{target}/{exe}")`) is still proven by nobody: a zip
nested one level deeper fails at the user's first start on Windows with all tests green. Make the
extraction a line in `MB.35.5`'s acceptance (and `lsp_smoke.py --archive`) or delete the claim
from §14.2.

---

## Verified true (so not findings)

- **Round-1 B1's premise is exact.** Five `include_str!` calls in the protected file, two of them
  the relocated server sources; the other three literals (`cli.rs`, `error.rs`,
  `docs/reference/blastfile.md`) are untouched by the move, so "two literals" is the whole
  mechanical edit — the remaining gaps are the node contradiction (B1) and the stale comment /
  non-runnable oracle detail.
- **`tools/critical_guard.py --base/--node` is a real, runnable reviewer command** (module
  docstring line 15; `argparse` at `:259-277`), and its covering-glob rule is what makes B1 a
  build-order defect rather than a theoretical one.
- **The new protected suite needs no Makefile edit.** `rs-test-critical` globs
  `crates/*/tests/critical_*.rs` and passes `--locked` for non-`blastit` crates
  (`Makefile:296-304`), and `critical-skip-check` refuses `#[ignore]`/`needs_root` in the same
  glob (`:314-323`), so `crates/blastit-lsp/tests/critical_lsp_stdio.rs` is discovered and
  skip-checked from its first commit.
- **`make check` has a home for every new proof except `lsp-portable`** (`Makefile:823`: 20
  prerequisites including `platform-matrix-check`, `workflow-check`, `packaging-gates-test`,
  `zed-check`, `test-critical`); `grep -rn lsp-portable` over `Makefile`, `scripts/`, `.github/`
  returns nothing, which is why §14.2 and the requirement index saying "pending `MB.35.6`" is
  honest rather than a claim.
- **Round-1 M4 is real and now honestly scoped.** `scripts/platform_matrix.py` knows only
  `shims|wheels|archives|tarballs` (`CLAIM` at `:461-477`, `declared_of`/`proven_of` at
  `:525-549`; `--matrix`/`--count` choices are `wheels|binaries|shims|images` at `:868-876`), so
  §8.3a's distinct-noun rule and ADR-0010 §4's "never by renaming its assets 'archives'" are the
  right constraint; `_pages()` scans every `docs/**/*.md` (`:429-441`), so the new noun really
  will govern the four documents under review.
- **R74's proofs are no longer over-claimed:** the requirement index ("pending `MB.35.6`;
  interim proof is `MB.35.4`'s `cargo tree` line") and §14.2 ("once `MB.35.5` adds the family")
  both state the current absence, and `MB.35.5`'s receipt is a runnable
  `--matrix lsp | len == 5` assertion.
- **The drop procedure now names all four sites** (design §8.3a's table,
  `docs/reference/cli.md`'s five-row table, `docs/troubleshooting.md`'s "no asset for that pair"
  row, `ADR-0010` §4) — round-1 **m1** resolved, and the five asset names agree across all four.
- **"One `run()`" now has a structural oracle** (§1.3 row 5: no `crates/blastit/src/lsp/`,
  `pub use blastit_lsp as lsp;` present, one `blastit-lsp` edge), which is the only kind of check
  that can fail when the server is copied back — a minimum session cannot, as round 1 said. It
  just needs its home in `MB.35.4`/`MB.35.6` (M2).
- **`--bogus` is the right case and it is testable**: the portable `main` ignores argv by
  construction, the current suite asserts `2` for `blastit lsp --bogus`
  (`critical_lsp.rs:2022-2026`), and the new suite's `1` is the only oracle that would catch a
  second argv parser being added.
- **S12's "no path, no socket" claim now has a token list that can express it** (`std::fs`,
  `std::path`, `std::net`, and `TcpListener`/`TcpStream` through `std::net`), replacing the
  Unix-socket-only list round 1 found — the remaining defect is the `unsafe` collision (M3), not
  the coverage.
- **`ARCHITECTURE.md` §2.7 and `ADR-0010` §4 agree with the amendment** on the crate row
  ("**exactly** `blastit-manifest` and `serde_json`"), the five-target build surface, the
  delegation sentence and the unprivileged-only claim, so no requirement in §14 lacks a document
  of record.
