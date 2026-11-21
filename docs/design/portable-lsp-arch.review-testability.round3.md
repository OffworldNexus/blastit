VERDICT: FAIL

Lens: testability (verification & oracles). Round 3 of `MB.35.3`, read against
`docs/design/portable-lsp-arch.review-testability.round2.md`,
`docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (the `MB.35.3` amendment),
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/design/blastfile-lsp-zed.md` §1.3, §8.3a, §11, §14 and the requirement index, and
`ARCHITECTURE.md` §2.7, with the repository facts re-checked in this tree:
`crates/blastit/tests/critical_lsp.rs` (2477 lines), `crates/blastit/src/lsp/{mod,hover}.rs`
(1591/337), `tools/critical_guard.py`, `Makefile`, `scripts/platform_matrix.py`,
`Cargo.lock`, `plan/nodes/MB/MB.35/{MB.35.3,MB.35.4,MB.35.5,MB.35.6}/NODE.md`.

**Counts: 0 BLOCKER, 6 MAJOR, 4 MINOR.** The round-2 plan contradictions are now
hand-offs in §14.4 and are not re-graded; the findings below are the design's own text,
its proofs, or its hand-off table being weaker than the requirement it cites. The design's
central extraction claims are factually right (see "Verified true"): four `include_str!`
literals move, the path arithmetic is correct, the re-export is one line, and the line
counts are 1591/337/2477.

---

## Round-2 status

| Round-2 finding | Status |
|---|---|
| **B1** — protected-file edit asserted by the design, forbidden by `MB.35.4` | **Plan half resolved** by §14.4/ADR decision 6 ("`crates/blastit/tests/critical_lsp.rs` must join them"). **The design's own R70 oracle is still wrong**: see **M1**. |
| **M1** — `parameterize` re-export owned by nobody | **Resolved.** §14.4 names both paths and the one-line re-export; `blastit-manifest` already depends on `blastit-sqlpolicy` (`crates/blastit-manifest/Cargo.toml:15`) and `parameterize` is public (`crates/blastit-sqlpolicy/src/template.rs:333`). |
| **M2** — five guard fixes only in §14.2, the brief still weaker | **Mostly resolved** (§14.4 row + R71). The hand-off still omits the unfiltered closure/`--filter-platform` — **m2**. |
| **M3** — `unsafe` token collides with `#![forbid(unsafe_code)]` | **Collision resolved** (patterns are `unsafe {`/`unsafe fn`/`unsafe impl`/`unsafe trait`; the attribute is explicitly not a hit). **The required attribute's presence has no failing oracle** — **M2**. |
| **M4** — R76 has no failing oracle | **Over-claim resolved** (`no advisory gate claimed`). **Still no failing oracle** for the version pin/"only parsers" — **M5**. |
| **M5** — three-state rule only in §14.2; repo precedent is `PASS` beside `NOT RUN` | **Moved into the right place** (§14.2 + §14.4). **Not exhaustive for a sanctioned drop, and not a release gate** — **M4**. |
| **m1–m5** | **Resolved.** §14.4 records the stale counts; the token table is pinned; the twins are gated by `make lsp-portable`; obligation 3 is scoped to "the only exit-code difference"; the extraction command now exists — but see **M3** for what it actually proves. |

---

## MAJOR

### M1 — R70's diff oracle is still unsatisfiable and still cites a command that cannot fail

The design states, as the proof of "one `run()`":

> "the oracle is a diff whose every changed line is one of those literals
> (`git diff <pre-node> -- crates/blastit/tests/critical_lsp.rs` has four `+`/`-` lines and
> nothing else)" — design §1.3, obligation 1 (lines 283–285)

> "a diff against the pre-node commit whose only changed lines are those four literals
> (`tools/critical_guard.py --base <pre-node-commit>`)" — design R70 (line 2942)

> "crates/blastit/tests/critical_lsp.rs (protected; two path literals change, nothing else)"
> — design §14.2 (line 2955)

Three facts in this tree break those sentences:

1. **The diff cannot be four lines.** The comment immediately above the two literals is:

   ```rust
   // §1's entry is `lsp/mod.rs` and its hover data lives beside it as
   // `lsp/hover.rs`: both are this module, so both carry the same list.
   ```

   `crates/blastit/tests/critical_lsp.rs:2048-2050`. After the move there is no
   `crates/blastit/src/lsp/mod.rs`, and the two sources are in another crate, so the
   comment is false unless it is reworded. A correct edit is therefore **eight**
   `+`/`-` lines (two literal lines + two comment lines, each a `-`/`+` pair), not four.
   The design's "four lines and nothing else" either ships a false comment in a protected
   file or fails its own acceptance.
2. **`tools/critical_guard.py --base <commit>` is report-only without `--node`.** The
   guard's `_run` returns `EXIT_OK` when `not nodes` (lines 229–234), and `--node` only
   checks *covering globs* (`evaluate`/`authorizing_pattern`); it never reads diff bodies.
   The design needs `--node MB.35.4` to make the protected-file change authorized — and a
   separate line-count command to make the body claim falsifiable.
3. **The oracle is not in the leaf's acceptance.** `MB.35.4/NODE.md` runs
   `git diff --find-renames --stat` (line 83), which shows renames, not changed lines. A
   cheap implementer can reword the comment (or not) and still pass every listed command.

**Fix.** Reword the two comment lines in the same edit and state the exact expected count
(eight changed lines for a two-line reword, or the chosen count if the comment is replaced
with one line). In R70 and §14.1 quote both commands:

```console
$ uv run --no-project tools/critical_guard.py --base <pre-node-commit> --node MB.35.4   # authorization
$ test "$(git diff <pre-node-commit> -- crates/blastit/tests/critical_lsp.rs | grep -c '^[+-][^+-]')" = 8
```

and add both to `MB.35.4`'s acceptance via §14.4. If the intent is to keep the protected
file at exactly four changed lines, move the source-grep assertions into the new crate
instead and say so; do not leave the false comment.

---

### M2 — `#![forbid(unsafe_code)]` is required on both files and its deletion is green

The design repeats the attribute as the compiler half of the "no privilege crate" argument:

> "plus the compiler half: `#![forbid(unsafe_code)]` at the top of `src/lib.rs` **and**
> `src/main.rs`, so an `extern "C"` declaration (which needs `unsafe`) cannot be added
> even without a crate." — design §1.3, row 3 (line 255)

> "`#![forbid(unsafe_code)]` in `lib.rs` and `main.rs`" — design R73 (line 2945)

But the guard's rule is a **token grep**: "an unsafe item or block (`unsafe {`, `unsafe fn`,
`unsafe impl`, `unsafe trait`), or `extern "C"`" (row 3), and "one mutated source per token"
(§14.2, line 2957). The attribute is explicitly "not a hit". Nothing asserts that the
attribute is **present**. Delete it from `lib.rs` or `main.rs` and every listed command
stays green; add `unsafe extern "Rust"` or an unsafe static (not one of the four patterns)
and the grep misses it. That is exactly the property round 2 asked to make falsifiable
("delete the attribute -> red"), and it is still not there.

**Fix.** Add an assertion in `scripts/check_lsp_portable.py` that each of
`src/lib.rs` and `src/main.rs` begins with `#![forbid(unsafe_code)]`, with a negative twin
that deletes it and expects red. Write it into §1.3 row 3, §11 S12's assertion column, and
the §14.4 `MB.35.6` row. Keep the token patterns as the second half.

---

### M3 — The extraction acceptance checks one file, not the three it claims, and cannot run on the Windows asset

The design states the property and then the command that is supposed to prove it:

> "extract to a temp dir, assert the three entries, then smoke **the extracted file**"
> — design §14.2 (line 2960)

> "the archive's one top-level directory is `blastit-lsp-<target>/` holding exactly the
> executable, `LICENSE` and a `SHA256SUMS` over those files" — design §8.3a (line 1696)

> "the acceptance is `tar -xzf <asset> -C "$tmp" && test -x
> "$tmp/blastit-lsp-<target>/blastit-lsp" && uv run --no-project scripts/lsp_smoke.py
> --binary "$tmp/blastit-lsp-<target>/blastit-lsp"`" — design §14.4 (line 2997)

The command checks **only the executable**. It never asserts `LICENSE`, never asserts
`SHA256SUMS`, and never asserts "exactly those entries". It also checks
`$tmp/blastit-lsp-<target>/<exe>`, while the consumer's join is
`blastit-lsp-<version>/blastit-lsp-<target>/<exe>` (the extension's
`download_file(..., "blastit-lsp-<version>", kind)`); the `<version>` component is not in
the test. On Windows the asset is a `.zip` and the runner has no `test -x`; `tar -xzf` on a
`.zip` does not extract it. The design says "on all five targets" but the named acceptance
is Unix/`.tar.gz` only — the one leg whose archive root and executable name differ is the
one leg not exercised.

**Fix.** One cross-platform `scripts/lsp_smoke.py --archive <asset>` (stdlib `zipfile` /
`tarfile`) that: extracts into a directory named for the asset; asserts the entry set is
exactly `{blastit-lsp[-.exe], LICENSE, SHA256SUMS}`; verifies the in-archive `SHA256SUMS`;
constructs the consumer's `blastit-lsp-<version>/blastit-lsp-<target>/<exe>` path; and then
runs the existing smoke. Use it on all five legs and quote it in §14.4, replacing the
`tar -xzf`/`test -x` fragment.

---

### M4 — The evidence-state rule is not exhaustive after a sanctioned drop, has no named checker, and is not tied to the release job

> "**three** states, defined once: `PASS` = five CI URLs and zero `NOT RUN`;
> `INCOMPLETE` = at least one `NOT RUN` with a reason; `FAIL` = a failing leg. Every
> target in the `lsp` family appears exactly once" — design §14.2 (line 2962)

> "If a leg fails at `MB.35.5`, it is dropped from the `lsp` family in
> `scripts/platform_matrix.py` with `dropped_because`" — ADR-0013 (lines 316–319)

> "If `MB.35.1` dropped a target, the four remaining names are the contract and the
> `dropped_because` is quoted." — `MB.35.5/NODE.md:109`

A dropped target is neither a CI URL nor a `NOT RUN`: with four URLs and one
`dropped_because`, the rule's `PASS` is false (not five URLs), `INCOMPLETE` is false (no
`NOT RUN`), and `FAIL` is false (nothing failed). The state is undefined, on the exact
path the ADR sanctions. Separately, the "checkable rule in `MB.35.6`" names no script,
mode or command, and neither the design nor the ADR makes the release job depend on
`PASS`; a `FAIL`/`INCOMPLETE` row can still ship unless something else fails.

**Fix.** Define the states over the family's *enabled* legs plus dropped legs, e.g.
`PASS` = one CI URL per enabled leg **and** every dropped leg has `dropped_because` and
zero `NOT RUN`; `INCOMPLETE` = an enabled leg has `NOT RUN` with a reason, or a dropped
leg lacks one; `FAIL` = a failing leg. Name the checker (`lsp_smoke.py --check-e2e
docs/testing/E2E-MB.35.md`, or a ten-line `scripts/check_e2e_states.py`) and add
`--check-e2e` as a `needs:` gate on the release job. Say "five until a sanctioned drop,
then the family's enabled count", not "five CI URLs", and add the same sentence to the
`MB.35.6` hand-off.

---

### M5 — R76 still has no proof that fails when the property is deleted, and its "only parsers" sentence is false

> "**R76** ... the tree is pinned by `Cargo.lock` (`pg_query` resolves to one version, and
> `cargo update` is what moves it), and the closure is recorded with **no advisory gate
> claimed** ... a future advisory gate is a follow-up, not a promise" — design §14.1
> (line 2948)

> "**Proved by:** §11's first paragraph; `cargo tree -p blastit-lsp -i pg_query` (one
> version) and the recorded closure in `docs/testing/E2E-MB.35.md`" — R76, same line

`cargo tree -i pg_query` prints two lines if a second version arrives; nothing fails. The
caret requirement is still `pg_query = "6.2.1"` (`crates/blastit-sqlpolicy/Cargo.toml:19`),
and `Cargo.lock` is at `6.2.1`, but a committed lockfile bump is green. "Pinned by
`Cargo.lock`" is true of every dependency and is not a requirement. The advisory half is
now honestly a record — good — but the version half has no `PINNED` assertion like the
one `scripts/platform_matrix.py` already uses for its own lines.

The first sentence is also inaccurate: `crates/blastit/src/lsp/mod.rs:310` is
`serde_json::from_slice::<Value>(&body)`, so the process parses the framed envelope with
`serde_json` before the Blastfile reaches `blastit-manifest` and `libpg_query`. "The only
parsers it reaches are ..." is not true of the process, and is not proven even for the
document.

**Fix.** Either (a) add a `PINNED`-style row in `check_lsp_portable.py` — `cargo metadata
--offline --locked` resolves exactly one `pg_query` and it is `6.2.1` — with a negative
twin that adds a second version (fixture) and expects red, and say the advisory row is a
record; or (b) drop "the tree is pinned" and "only parsers" from R76 and leave the proven
claims: S2's no-execution, R69's exact two direct dependencies, and R71's closure. Scope
the sentence to "the Blastfile's parsers", and name `serde_json` as the envelope parser.

---

### M6 — The Windows and Linux-aarch64 legs never run the protected stdio suite, so R70's proof is host-only on two of the five targets

> "`x86_64-pc-windows-msvc` ... `windows-latest`, build + smoke transcript"
> "`aarch64-unknown-linux-gnu` ... `quay.io/pypa/manylinux_2_28_aarch64` (native arm
> runner), same floor and smoke"
> "`aarch64-apple-darwin` ... `macos-latest`, build + `cargo test -p blastit-lsp` + smoke"
> — ADR-0013 platform table (lines 310–314)

The two Apple legs run `cargo test -p blastit-lsp`; the Windows and Linux-aarch64 legs only
run `lsp_smoke.py`. The stdio suite is the **only** oracle for the portable binary's argv
contract (`--bogus` reads stdin and exits `1`) and its `.env_clear()` behaviour
(design §1.3 obligation 2; §14.2's `the_standalone_binary_speaks_the_same_session`). The
Windows leg is precisely where the argv/environment contract is most likely to differ, and
the smoke script does not exercise either case. The design says "the released archive runs
from a clean directory with no `PATH` entry; ... per-platform smoke" — a smoke is not the
protected test.

**Fix.** Add `cargo test -p blastit-lsp` to the Windows and Linux-aarch64 legs (the crate's
tests are host-independent; `pg_query.rs`'s own CI already ran `cargo test` on
`windows-latest` at the pinned version). If a leg cannot run it, say why in
`dropped_because`/`skipped_suites_because` and add the `--bogus` and `.env_clear()` cases
to `lsp_smoke.py` so the smoke is at least equivalent. Do not let "smoke transcript" stand
in for R70's proof.

---

## MINOR

### m1 — `main.rs`'s "one shape" has no named assertion; the token grep excludes the file

> "`src/main.rs` is otherwise pinned to one shape — the attribute, one `fn main()`, one
> `std::process::exit(blastit_lsp::run() as i32)` — so the legitimate `std::process::exit`
> cannot weaken the grep" — design §1.3 row 3 (line 255)

The token rule's file set is `lib.rs`/`hover.rs`; `main.rs` is "pinned" but no check is
named. A `main.rs` that adds `std::net::TcpListener` or a second call would pass the
lib/hover grep. **Fix.** State the exact allowed-content check (a per-file regex or an
exact set of lines) in §1.3 row 3 and §14.2's `make lsp-portable` row, with a negative twin.

### m2 — The §14.4 `MB.35.6` hand-off omits the unfiltered closure/`--filter-platform`

R71 requires "the **named** forbidden set over the **unfiltered** normal closure (plus
`--filter-platform` per target where resolvable)" (line 2943), but the §14.4 row says only
"every dependency table" (line 2998). The leaf's brief already has a `cargo metadata`
closure bullet, so the gap is the *unfiltered* host-vs-target nuance, not the closure
itself. **Fix.** Add "unfiltered `cargo metadata --offline --locked`; `--filter-platform`
per target where resolvable" to the `MB.35.6` row.

### m3 — The §14.4 `MB.35.6` row misdescribes its own brief

> "`MB.35.6` | ... two negative twins ... acceptance piped to bare `python3`" — §14.4
> (line 2998)

`MB.35.6/NODE.md:83` already uses `UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0
python -m pytest`; there is no bare `python3` in its acceptance. Its test item lists five
mutations, not two (lines 40–43). The row's "requires" column would make a no-op correction.
**Fix.** Correct the row to the real gaps (token list, identity, evidence states) and drop
the false `python3`/two-twin claims; if the test count is meant, say "at least one twin per
token, plus the wiring twin".

### m4 — The identity rule says "no `src/lsp/` directory" while R70 says "no second `lsp` module"

> "no `crates/blastit/src/lsp/` directory, `pub use blastit_lsp as lsp;` present, one
> `blastit-lsp` edge" — §1.3 row 5 / §14.2

Rust's other module spelling is `crates/blastit/src/lsp.rs`. The exact `pub use blastit_lsp
as lsp;` anchor plus `critical_lsp.rs`'s `lsp::run()` grep catch a live copy, but the stated
check is narrower than the claim, and a dead `src/lsp.rs` copy would pass. **Fix.** Assert
neither `src/lsp.rs` nor `src/lsp/` exists, and assert `src/lib.rs` has the exact `pub use`
and no `mod lsp;`. One line in `check_lsp_portable.py` and one in §14.4.

---

## Verified true (so not findings)

- **Four `include_str!` literals, exactly.** `critical_lsp.rs:2052-2053` and
  `hover.rs:294,316` are the four that move; `critical_lsp.rs:1419` (`docs/reference/
  blastfile.md` from the test), `:2036` (`../src/cli.rs`) and `:2093`
  (`../../blastit-manifest/src/error.rs`) do not. The design's path arithmetic is correct:
  `../../blastit-lsp/src/{lib,hover}.rs` from `crates/blastit/tests/`, and
  `../../../docs/reference/blastfile.md` from `crates/blastit-lsp/src/`.
- **The re-export is one line and the dependency already exists.**
  `crates/blastit-manifest/Cargo.toml:15` already has `blastit-sqlpolicy`;
  `blastit_sqlpolicy::parameterize` is public (`template.rs:333`); `lib.rs:108` already
  re-exports from that crate. `MB.35.4`'s `paths` widening is the only plan edit.
- **The line counts are right.** `mod.rs` 1591, `hover.rs` 337, `critical_lsp.rs` 2477;
  §14.4 records the brief's stale 1174/254/1880 rather than repeating them.
- **`make lsp-portable` has a real precedent.** `manifest-purity` runs both the guard and
  its self-test (`Makefile:166-168`); `make check` has the slot (`Makefile:823`). The
  `lsp-portable` target is a genuine hand-off, not a green line about another family.
- **`critical_guard.py --base/--node` is real** (`:258-277`) and its covering-glob rule is
  what makes `MB.35.4`'s first red outcome real. The missing `--node` in R70 is a citation
  bug, not a missing tool.
- **`platform_matrix.py` already enforces `dropped_because`** for its own legs
  (`:559-565`) and `--check` scans every `docs/**/*.md` (`:429-441`), so R74's "distinct
  count noun" is a real constraint, not prose.
- **`cargo metadata`/`cargo tree` are runnable and offline-safe**; `Cargo.lock` resolves
  `pg_query` 6.2.1 (`:1099-1100`), and `crates/blastit-sqlpolicy/Cargo.toml:19` is the
  caret requirement (which is why M5 asks for a `PINNED` assertion, not a trust fall).
- **`ADR-0010` §4, `ARCHITECTURE.md` §2.7 and the amendment agree** on the crate row
  ("exactly `blastit-manifest` and `serde_json`"), the five targets, the unprivileged-only
  claim, and the "never serves" sentence. No requirement in R69–R76 lacks a document of
  record.
- **The three-state rule is an improvement on the shipped precedent**
  (`docs/testing/E2E-MB.34.md`'s "`VERDICT: PASS` — one item is `NOT RUN`"). M4 is about
  the drop case and the missing gate, not the rule's intent.

## Hand-offs still open (not graded as document defects)

These are `NODE.md` vs design differences for the lead to apply; §14.4 does not name them:

- **`MB.35.4`** — the brief's "`main.rs` ... (keep the panic hook and the deny lints from
  `mod.rs`'s header)" (lines 33–34) reads as putting the hook in `main.rs`; the design puts
  the header, hook and `catch_unwind` in `lib.rs` and pins `main.rs` to the attribute plus
  one call. Add the placement to the §14.4 row, and add the `--node MB.35.4` guard and
  line-count diff to its acceptance (M1).
- **`MB.35.5`** — the `--matrix lsp` acceptance uses bare `python3`
  (`MB.35.5/NODE.md:85`), against AGENTS rule 9; §14.4 corrects `MB.35.6`'s similarly but
  not this. Say `uv run --no-project python -c`.
- **`MB.35.6`** — its `paths` (`scripts/check_lsp_portable.py`, the test, `Makefile`,
  `docs/testing/E2E-MB.35.md`) exclude the pages it is told to execute ("If a documented
  command is wrong, the fix is on the page"), so the runnable-docs half is blocked by the
  lane. Add the page or a follow-up node to §14.4.

## Closing

The design's extraction arithmetic and its crate boundary are now right, and the round-2
plan contradictions are genuinely handed off. What remains is proof quality: R70's diff is
not the diff the design says it is, the `forbid(unsafe_code)` attribute can be deleted
green, the extraction command proves one of three entries and not the Windows layout, the
evidence state has a hole where a sanctioned drop lives, R76's pin is a record rather than
a gate, and two of the five targets never run the suite that proves the argv contract.
Those are six local fixes, not a redesign; until they are in, `MB.35.3`'s own acceptance
sentence ("`MB.35.4` ... can be written without re-deciding ... an invariant") is not met.
