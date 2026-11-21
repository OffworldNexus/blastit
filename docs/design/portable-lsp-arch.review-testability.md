VERDICT: FAIL

Lens: testability (verification & oracles)

# `MB.35.3` — portable `blastit-lsp`: testability review

Scope: the uncommitted diff of `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (the
`MB.35.3` amendment), `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/design/blastfile-lsp-zed.md` §1, §1.3, §8.3a, §11, §14 and the requirement index, and
`ARCHITECTURE.md` §2.7. Read against `docs/research/2026-11-17-cross-platform-blastit-lsp.md`,
`plan/nodes/MB/MB.35/{MB.35.3,MB.35.4,MB.35.5,MB.35.6}/NODE.md`, `plan/nodes/MB/MB.33/NODE.md`,
`docs/reference/cli.md` `## blastit-lsp`, `docs/local.md` `## Editors`, the Makefile,
`scripts/platform_matrix.py`, `scripts/check_zed_extension.py`, `scripts/check_manifest_purity.py`,
`tools/critical_guard.py` and `crates/blastit/tests/critical_lsp.rs` (protected, 2477 lines).

Command run here (unprivileged, offline, green — so the findings below are content, not a red
tree):

```console
$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — the declared counts and the proved subset are printed (eight wheels,
four archives, twelve shims; four of eight wheels, two of four archives, six of twelve shims
built and exercised), and the line names no `lsp` family
```

Not one line in that output names the `lsp` family. It is green because the amendment's claim is
invisible to it (M4).

---

## BLOCKER

### B1 — R70's "byte-identical `critical_lsp.rs`" cannot compile after the move, and is protected out of reach

The amendment's central equivalence obligation is:

> "`crates/blastit/tests/critical_lsp.rs` stays **byte-identical** and keeps spawning
> `env!("CARGO_BIN_EXE_blastit") lsp`: the delegation is proved from the side that must not
> change." — design §1.3, obligation 1 (lines 270–272)

> "**R70** … the protected `critical_lsp.rs` runs **byte-identical** against `blastit lsp` |
> `cargo test -p blastit --test critical_lsp`; `git diff --exit-code --
> crates/blastit/tests/critical_lsp.rs`; `critical_lsp_stdio.rs`" — design §14.1 (line 2868)

That file does not spawn only a binary: one of its tests reads the server's **source** through
`include_str!` (and another reads `cli.rs`, which stays):

```rust
// crates/blastit/tests/critical_lsp.rs:2048-2054
fn the_lsp_module_is_a_text_tool() {
    let sources = [
        include_str!("../src/lsp/mod.rs"),
        include_str!("../src/lsp/hover.rs"),
    ];
```

`MB.35.4` `git mv`s exactly those two files to `crates/blastit-lsp/src/{lib,hover}.rs` and its
acceptance pastes `git status --short` "showing `crates/blastit/src/lsp/` is gone". `include_str!`
is resolved at compile time, so on the first `cargo test -p blastit --test critical_lsp` after the
move the test crate fails to build — R70's own first acceptance command cannot pass, and "any
behavioural change during the move" (§14.2) is never even measured.

There is no escape inside the current plan:

- Leaving `crates/blastit/src/lsp/mod.rs` as a one-line `pub use blastit_lsp::*;` makes the file
  resolve but **vacuously** greps a re-export for `std::fs`/`std::env`/`blastfile::check`/
  `set_hook`/`catch_unwind`; the positive half of the test (`:2081-2085`) fails on the shim.
- Editing the two literals is a *breaking* change to a protected file, and `MB.35.4`'s `paths`
  list does not include `crates/blastit/tests/critical_lsp.rs`; `tools/critical_guard.py` (module
  docstring; `BREAKING_STATUSES = {"M","D","T"}`) reports a violation unless a glob in that
  node's `NODE.md` covers it.
- `git diff --exit-code -- <file>` is also the wrong oracle even if the file were untouched: it
  compares the working tree to the **index**, so a committed edit passes. It cannot police the
  property it is quoted for.

The document also cites a path that does not exist today, which is the tell that this line was
never executed:

> "one assertion in `critical_lsp.rs` over `include_str!("../src/lsp.rs")`" — design §1.2
> (line 188); the real literal is `../src/lsp/mod.rs`, and there is no `lsp.rs`.

**Fix (exact).**
1. Add `crates/blastit/tests/critical_lsp.rs` to `MB.35.4.paths` and say why in the design
   ("the move edits two `include_str!` literals and nothing else"), so `critical_guard.py`'s
   covering-glob rule is satisfied.
2. Restate R70 as **"assertions byte-identical; only the two `include_str!` path literals change
   (`../../blastit-lsp/src/lib.rs`, `../../blastit-lsp/src/hover.rs`)"**, and move the two grep
   tests' *new* home to `crates/blastit-lsp/tests/critical_lsp_source.rs` (new, protected) so the
   standalone crate keeps its own source assertions after the extraction.
3. Replace the proof with a diff whose only changed lines are those two literals plus a real
   baseline guard: `uv run --no-project tools/critical_guard.py --base <pre-node-commit> --node
   MB.35.4`, and `git diff <pre-node-commit> -- crates/blastit/tests/critical_lsp.rs | grep -c
   '^[+-][^+-]'` = 4. `docs/design/blastfile-lsp-zed.md` §1.2's `../src/lsp.rs` is corrected to
   the actual literal at the same time.

### B2 — R69's build proof is red before the first line: the `parameterize` re-export is not in `MB.35.4`'s paths

Design §1.3 makes the two-dependency rule true with one line in another crate:

> "the Quick Fix's rewrite … is **re-exported** by `blastit-manifest` … (`crates/blastit-manifest/src/lib.rs:108`) … **That is one added line in `crates/blastit-manifest/src/lib.rs`, part of `MB.35.4`'s paths**" — design §1.3 (lines 235–240, 256–262)

`MB.35.4.paths` is:

```yaml
paths:
  - "crates/blastit-lsp/**"
  - "crates/blastit/src/lsp/**"
  - crates/blastit/src/lib.rs
  - crates/blastit/Cargo.toml
  - Cargo.toml
  - Cargo.lock
```

`crates/blastit-manifest/src/lib.rs` is not there. The current source imports the function
directly — `crates/blastit/src/lsp/mod.rs:55 use blastit_sqlpolicy::parameterize;` — and the
manifest crate re-exports only `{Columns, TableGrant, TableGrants}` today
(`crates/blastit-manifest/src/lib.rs:107`). So `MB.35.4` has exactly two outcomes, both red:

- keep the import → the only way to compile is a third dependency, `blastit-sqlpolicy`, so
  `make lsp-portable` (R71) and R69's "`[dependencies]` **exactly** `{ blastit-manifest,
  serde_json }`" go red; or
- edit `crates/blastit-manifest/src/lib.rs` → outside the node's paths (rule 5), and the change is
  invisible to review because the design says it is in scope.

**Fix.** Add `crates/blastit-manifest/src/lib.rs` to `MB.35.4.paths` (one line), and in the design
quote the exact added line (`pub use blastit_sqlpolicy::parameterize;` beside the existing
re-export at `:107`) so the guard's "exactly two" and the leaf's paths cannot disagree. Do not
weaken R69 to "two or three".

---

## MAJOR

### M1 — S12's "opens no path, no socket" has no assertion in the new artifact's guard

The amendment's strongest security sentence is:

> "A `setcap` on the portable binary would grant a process that opens no path, no socket and no
> child" — ADR-0013 `MB.35.3` amendment (line 272)

S12's proof is `make lsp-portable`, whose source rule is (§1.3 line 253; R71 line 2869):

> "no line … contains `std::os::unix`, `std::os::linux`, `std::os::windows`, `libc::`, `rustix`,
> `nix::`, `pyo3`, `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os`, `std::process::Command`,
> or `std::env`"

`std::fs`, `std::path`, `std::net`, `TcpListener`/`TcpStream` and `UnixStream` are **absent**. The
only assertion that a `Blastfile` path or a socket is never opened lives in `critical_lsp.rs`'s
list (`std::fs`, `std::path`, `std::os::unix::net` — `:2056-2073`), i.e. in the very file B1
says must be dismembered, in another crate, and covering only the Unix socket module. After the
extraction, S12's "no path, no socket" half is asserted by nothing that the portable crate owns,
and `std::net` was never asserted anywhere.

**Fix.** Add to `check_lsp_portable.py`'s token table `std::fs`, `std::path`, `std::net`,
`TcpListener`, `TcpStream`, `UnixStream`, `File::open`, `OpenOptions` — one property per token,
each with its negative twin — and restate S12 as "`make lsp-portable`" with that list, not as a
sentence a reader has to reconstruct from §1.2.

### M2 — "`[dependencies]` is exactly …" does not cover the manifest shapes that would break portability

The guard's first rule is a `tomllib` read of one table:

> "`[dependencies]` is exactly `{ blastit-manifest, serde_json }` … `scripts/check_lsp_portable.py`
> parses the manifest with `tomllib`" — design §1.3 (line 251)

A `[target.'cfg(target_os = "windows")'.dependencies] libc = "0.2"`, a
`[build-dependencies]` (already needed by `pg_query`, so the table will exist), or an optional
dependency switched on by a `default` feature all leave `[dependencies] == 2` true. The design then
offers two closure checks of different strength — "`cargo metadata --offline`'s normal closure"
(line 252, unfiltered, which *would* see cross-target edges) and "`cargo tree -p blastit-lsp
--edges normal`" (line 254, host-filtered, which would not) — and never says which is
authoritative. On the five-target claim that gap is the whole property: the crate is portable
because no *platform-specific* edge exists, and the check as written is proven on Linux only.

**Fix.** In §1.3 and R71 state: (a) the guard asserts every dependency table in
`crates/blastit-lsp/Cargo.toml` (`dependencies`, `build-dependencies`, `target.*`,
`[features]`'s optional entries) contains only the two names or is empty, and (b) the closure walk
is `cargo metadata --offline --locked` **without** a platform filter, with `--filter-platform`
for each of the five targets where the host can resolve them. One negative twin: adding a
`[target.'cfg(windows)'.dependencies]` row goes red.

### M3 — R71 promises "each with a negative twin"; `MB.35.6` implements two of twelve

> "**R71** … the source greps (`std::os::unix/linux/windows`, `libc::`, `rustix`, `nix::`,
> `pyo3`, `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os`, `std::process::Command`,
> `std::env`) — **each with a negative twin**" — design §14.1 (line 2869)

`MB.35.6` item 2 enumerates five mutations: "add `rustix` → red; add `use std::os::unix::fs` →
red; add a `Command::new` → red; delete a dependency the code needs → the compile is red, not this
guard; remove the target from `make check` → the wiring test is red." Two tokens of twelve have a
twin, and two of the five cases prove a different checker (rustc, the Makefile). The repo's own
precedent is the opposite: `test_manifest_purity.py`, `test_packaging_gates.py` and
`test_check_zed_extension.py` each mutate *every* listed property.

**Fix.** Either drive the twins from the same table the guard iterates (`for token in TOKENS:
mutate(token) -> red`), so a new token cannot land without a twin, or weaken R71 to "one negative
twin per rule class" and say so. The first is cheaper than the drift.

### M4 — R74's `make platform-matrix-check` proof is vacuous for the `lsp` family, and "archives" collides with the serving family

> "**R74** … the counts live in one place: `ADR-0010` §4 carries the `lsp` family row,
> `scripts/platform_matrix.py`'s `lsp` family is the target list and the runner per leg … |
> `make platform-matrix-check`" — design §14.1 (line 2872)

> "the packaging matrix … Fails when: a stated count above the declared family, or a dropped leg
> without `dropped_because`" — design §14.2 (line 2882)

The module today knows only the serving families: `declared_of`/`proven_of` map
`shim|wheel|archive|tarball|image` (`scripts/platform_matrix.py:521-549`), the dropped-leg check
loops over `LEGS` only (`:551-568`), and the `PROVEN`-key validation builds `declared_keys` from
`shim_legs()|wheel_legs()|binary_legs()` (`:651-661`). The page-count regex `CLAIM` matches only
`shims?|wheels?|archives?|tarballs?` (`:466-480`). Consequently:

- nothing in the amendment's four documents is counted by it — `--check` prints no `lsp` line
  (output above), so "`5` targets", "five assets" and "`one archive per target`" are invisible;
- a dropped `lsp` leg gets no `dropped_because` enforcement unless `MB.35.5` re-implements the
  `enabled`/`dropped_because` pair for a second list, which no document asks for;
- the obvious way to make them visible is to call them "archives", which resolves to
  `declared_of("archives") == len(binary_legs()) == 4` — so a true page sentence about five `lsp`
  archives becomes a **false** red against the standalone family, while "5 portable binaries" or
  "5 assets" stays unread. The module's own header calls this failure out ("the gate went vacuous
  the first time", `:421-436`).

**Fix.** In §8.3a add a named family and its guard: `lsp` legs in `platform_matrix.py`
(`--matrix lsp`, `--summary` row, `enabled`/`dropped_because`), a distinct count noun
(`lsp archives` / `lsp assets`) added to `CLAIM` and `PROVEN_CLAIM`, and a `PINNED` line for the
one page a user reads. Then R74's proof is real and `MB.35.5`'s twin "rename an asset in one place →
red" covers the doc, not only the workflow's download pairing. Until then, state R74's proof as
`MB.35.5`'s obligation, not as a green target.

### M5 — "One `run()`" is asserted, not proved: nothing fails if the server is copied back

> "There is **one** `run()` behind both binaries … the protected
> `crates/blastit/tests/critical_lsp.rs` runs **unchanged** … and the new … `critical_lsp_stdio.rs`
> replays the minimum session" — design §1.3 (lines 244–247)

> "One `run()`, one protocol, one set of sentences: `blastit lsp` and `blastit-lsp` cannot drift,
> and the protected suite proves it from the delegation side." — ADR amendment (lines 298–299)

Consider the exact regression this obligation exists for: someone re-adds
`crates/blastit/src/lsp/mod.rs` as a copy and reverts `lib.rs` to `pub mod lsp;`. Every named
proof stays green — `critical_lsp.rs` runs against `blastit lsp` (now the copy, byte-identical
behaviour), `critical_lsp_stdio.rs` runs against `blastit-lsp` (still the crate). A *minimum*
session cannot distinguish one implementation from two identical ones, and the transcript `diff`
in `MB.35.6` compares two runs of a single session, not two code paths. The claim is a structural
fact (`pub use blastit_lsp as lsp;`) with no structural check.

**Fix.** One static assertion, in the new crate's protected test (or as a fourth rule in
`check_lsp_portable.py`): `include_str!("../../blastit/src/lib.rs")` contains
`pub use blastit_lsp as lsp;`, `crates/blastit/src/lsp/` does not exist, and
`crates/blastit/Cargo.toml` names `blastit-lsp` exactly once. Say in §14.2 that this is the proof
of identity and the stdio suite is the proof of behaviour.

### M6 — `VERDICT: PASS` is satisfiable with `NOT RUN` platforms, and no command reads the evidence file

> "`docs/testing/E2E-MB.35.md` (`MB.35.6`) | … per-platform smoke, or `NOT RUN` with the exact
> blocker" — design §14.2 (line 2883)

`MB.35.6` says both things:

> "`VERDICT: PASS` only when every documented platform has evidence" (§4 goal)

> "`docs/testing/E2E-MB.35.md` ends with `VERDICT: PASS`, and every platform in `MB.35.5`'s matrix
> has either a CI URL or `NOT RUN` with the reason." (# Acceptance)

The second sentence is the one a cheap implementer will satisfy: three `NOT RUN` rows with
reasons plus `VERDICT: PASS`. That is the "honest ending" the amendment praises in one place and
permits in another — and for the Apple legs it is the *expected* state (ADR: "buildable, unproven
for the Rust wrapper"), so this is the likeliest outcome, not a hypothetical. Nothing in `make
check` parses the file, so the release gate sees only a human-written word, contradicting
`VISION.md`'s "one fast, unprivileged command … proves nothing was forgotten".

**Fix.** Three states, defined once: `PASS` requires zero `NOT RUN` and five CI URLs;
`INCOMPLETE` requires at least one `NOT RUN` with a reason; `FAIL` is a failing leg. Then a
checkable rule — either a `--check` mode in `scripts/check_lsp_portable.py` or a five-line
`grep`-based test in `MB.35.6` — asserts `PASS ⇒ NOT RUN count == 0` and that every target in
`platform_matrix.py`'s `lsp` family appears exactly once. `MB.35.5`'s release job should depend on
`PASS`, not on the file's existence.

---

## MINOR

### m1 — The drop procedure names two of the four published places that state the five targets

> "it is dropped from the `lsp` family in `scripts/platform_matrix.py` with `dropped_because`, and
> from design §8.3a's asset table, in the same commit" — ADR amendment (lines 284–286)

`docs/reference/cli.md:120-130` has a five-row target table, `docs/troubleshooting.md` says "no
asset for that pair ([the five])", `editors/zed/README.md` says "If none can run", and
`ADR-0010` §4's row says five targets. `--check` reads none of them (M4). Fix: list every site in
the ADR paragraph, and make the count check of M4 the thing that fails when one is missed.

### m2 — Two drifting counts in the doc under review

`MB.35.4` calls `critical_lsp.rs` "the 1880-line" suite; it is 2477 lines. Design §1's row 1 still
says "`crates/blastit/src/lsp/{mod,hover}.rs` (**new**, ≤ 750 lines)" and ADR decision 1 says "~700
lines in `crates/blastit/src/lsp.rs`", while the shipped pair is 1591 + 337. The numbers are
record-text, not contracts, but a cheap implementer reads "≤ 750" as a bound. Fix: drop the line
counts or state them as "≈1.9k today" in the amendment.

### m3 — `make lsp-portable` does not exist yet, so R69–R73 have no runnable proof before `MB.35.6`

`grep -n "lsp-portable" Makefile` returns nothing; `make check:` (Makefile:823) lists six guards
and not this one. §14.2 labels it "(`MB.35.6`)" and `MB.35.4`/`MB.35.5` carry manual `cargo tree`
acceptance, so this is honest — but the requirement index's "Proving tier" for R69–R74 reads as if
the target is current. Fix: mark those rows "pending `MB.35.6`; interim: `cargo tree` clause in
`MB.35.4`'s acceptance", and have `MB.35.5` depend on the guard, not only on `MB.35.4`.

### m4 — `blastit-lsp <arg>` is documented and untested

`docs/reference/cli.md:93-94`: "there is no `--help` and no `--version`, and an argument is
ignored while it waits on stdin, so started by hand it exits `1`". `blastit lsp --bogus` → `2` is
asserted (`lsp_subcommand_is_reachable`); the new binary's "argument is ignored, then `1`" has no
test, and it is a real divergence between the two binaries on the same `run()`. Fix: one case in
`critical_lsp_stdio.rs` (`--bogus` → reads stdin, `1` on EOF) or delete the sentence.

### m5 — The archive the extension joins is never asserted by a test

`§8.3a`'s assertion column checks the *extension's literal string* (`lib: executable` asserts
`format!("blastit-lsp-{version}/blastit-lsp-{target}/{exe}")`), and `lsp_smoke.py` spawns
`target/release/blastit-lsp`, not an extracted archive. The only proof that the producer's archive
matches the consumer's join is `MB.35.5`'s manual `tar -tzf … # one dir, one executable`. A zip
whose root is bare or whose top directory is `blastit-lsp-<target>/` but nested one level deeper
fails at the user's first start, on five platforms, with no red test. Fix: one assertion in
`build-lsp.sh`'s acceptance or `lsp_smoke.py --archive`: extract to a temp dir, assert
`blastit-lsp-<target>/blastit-lsp` exists and is executable, then run the smoke against *that*
file.

---

## Verified true (so not findings)

- **The new protected suite runs in `make check` without a Makefile edit.** `rs-test-critical`
  globs `crates/*/tests/critical_*.rs` (Makefile:296-304), and `critical-skip-check` greps the
  same glob (Makefile:314-323), so `crates/blastit-lsp/tests/critical_lsp_stdio.rs` is discovered,
  run with `--locked`, and refused if it ever contains `#[ignore]`/`needs_root`/a skip.
  `tools/critical_guard.py` protects it from its first commit by `PROTECTED_RUST_PREFIX`; no
  Makefile or guard registration is missing.
- **`make check` already carries the three guards the amendment leans on** —
  `platform-matrix-check`, `zed-check`, `test-critical` (Makefile:823) — so wiring
  `lsp-portable` in is one prerequisite token, and `MB.35.6`'s "remove it from `check` → red" twin
  has the `grep -Eq '^check:.*…'` precedent from §12.8.
- **`make platform-matrix-check` is genuinely fast, offline and unprivileged** and its
  `PINNED`/`CLAIM` machinery fails closed when a pinned page or a count disappears
  (`scripts/platform_matrix.py:672-741`). The mechanism is right; M4 is about extending it, not
  replacing it.
- **The dependency checks are achievable unprivileged:** `cargo metadata --offline --locked` and
  `cargo tree -p blastit-lsp --edges normal` both run without a network or a privileged account,
  and the research (`§4.2`) already proves `grep -c -E 'libc|rustix|nix|pyo3'` is 0 over
  `blastit-manifest`'s closure today — so the guard has a true baseline to pin.
- **The ADR's per-target evidence column honestly distinguishes declared from proved:**
  `aarch64-unknown-linux-gnu` is "**declared, unbuilt** — accepts the target, never calls it
  proved" (ADR:280), both Apple targets are "buildable, unproven" (`:281-282`), Windows is
  "proven upstream" and Linux x86_64 "proven by the matrix" (`:278-279`). §14.3 repeats the
  caveat verbatim for aarch64 ("the archive the release publishes is the claim under test"), so a
  reader cannot mistake a declaration for a proof. Nothing here over-claims.
- **The five asset names, formats and extraction paths agree across the three documents that state
  them** — design §8.3a's table (1637-1643), ADR-0010 §4's row (line 42) and
  `docs/reference/cli.md:120-130`: `blastit-lsp-<target>.tar.gz` on Linux/macOS,
  `.zip` on Windows, one top-level `blastit-lsp-<target>/`, executable at
  `blastit-lsp-<version>/blastit-lsp-<target>/blastit-lsp[.exe]`, and the extension's
  `download_file(url, "blastit-lsp-<version>", kind)` destination is the prefix of that same
  path. `MB.35.5`'s acceptance lists exactly those five names.
- **`docs/testing/E2E-MB.35.md`'s missing status is not hidden:** §14.2 and `MB.35.6` say the file
  does not exist yet and name the node that writes it. The finding is the verdict rule (M6), not
  a missing promise.
