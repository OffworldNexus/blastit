# Result MB.36.11 — the wheel's `DT_NEEDED` set (R12) and the deleted trait names (R3)

STATUS: done (attempt 3; every review finding addressed)

One new file, `crates/blastit/tests/critical_artifacts.rs` (2 110 lines, 6 tests),
one corrected page, `docs/design/view-crates-architecture.md` (widened `paths`,
attempt 2), and this report. `crates/blastit/Cargo.toml`, `Cargo.lock` and every
other file are untouched: **no new dependency**, no `#[ignore]`, no `#[allow]`, no
`unsafe`, no `unwrap()`/`expect()`, and no `Command::new` — the wheel, the zip,
the deflate stream and the ELF are read with `std` alone, so a host without
`readelf`/`unzip` cannot make this check fail open *or* fail for the wrong reason.

The tests:

| Test | What it pins |
|---|---|
| `the_wheel_extension_links_no_interpreter_and_no_php` | R12 on the published file |
| `the_wheel_without_an_extension_is_refused` | 11 fixture groups (below) |
| `the_deleted_transport_trait_stays_deleted` | R3 over `crates/` + the transport layout |
| `the_trait_scan_catches_a_planted_name` | the scan's four-way negative twin |
| `inflate::tests::deflate_vectors_decode` / `…grows_past_its_declared_size…` | the RFC 1951 reader's own fixture (one zlib vector per block type) |

Fixture groups in the wheel test: (1) a wheel with no `blastit/_blastit*.so` is
refused by name; (2) a plain `libc` extension is accepted; (3) `$ORIGIN/../…` is
accepted, as the script's `--allow-origin-rpath` allows it; (4) `libpython` is
refused; (5) `libphp` is refused; (6) a build-host `RUNPATH` is refused;
(7) a truncated ELF and a non-ELF member are refused; **(8) an understated
`DT_STRSZ` is refused**; **(9) an ELF with no `DT_NEEDED` is refused**;
**(10) `ELFCLASS32` is accepted with `libc` and refused with `libpython`**;
**(11) a big-endian ELF is refused**; **(12) a `DT_NEEDED` whose only text is the
empty string is refused** (the section would resolve to `[""]` and certify clean
while proving nothing). Groups 8–11 are the attempt-2 additions, 12 the
attempt-3 one; 8, 9 and 12 are the fixtures proved red by mutation (below).

## Acceptance

```console
$ cargo test -p blastit --test critical_artifacts --locked
running 6 tests
test inflate::tests::a_stream_that_grows_past_its_declared_size_is_refused ... ok
test inflate::tests::deflate_vectors_decode ... ok
test the_wheel_without_an_extension_is_refused ... ok
test the_trait_scan_catches_a_planted_name ... ok
test the_deleted_transport_trait_stays_deleted ... ok
test the_wheel_extension_links_no_interpreter_and_no_php ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.60s
EXIT=0
```

```console
$ make check
…
zed-check: OK (59 properties checked)
doc-tone: ok — 30 user-facing page(s), no banned phrase
lsp-portable self-test: ok — 229 assertions over 59 fixtures (43 cargo-checked)
plan OK
EXIT=0
```

`make check` runs this suite twice, both green: `rs-test`
(`--workspace --all-targets --features python`, `check4.log:308`) and
`rs-test-critical` (`check4.log:4580`), each `6 passed; 0 failed; 0 ignored`;
`protected critical tests hold no skip construct` passed (`check4.log:3789`), i.e.
no `#[ignore]` shape in the new file.

```console
$ make critical-guard NODE=MB.36.11
critical-guard: HEAD..working tree, 7 changed file(s)
node MB.36.11: paths ['crates/blastit/tests/**', 'scripts/**',
'docs/design/view-crates-architecture.md']
  no change to a protected critical test
EXIT=0

$ cargo fmt --all -- --check
EXIT=0

$ uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

## The three required proofs (unchanged, re-run on attempt 2)

**(A) No wheel → one printed line, pass.** `dist/` moved aside entirely, and an
empty `dist/` (the two shapes a clean checkout can have):

```console
$ mv dist /tmp/opencode/mb3611-dist/dist && cargo test … -- --nocapture
no blastit-0.2.0 wheel in …/dist: the R12 check needs a `scripts/build-wheel.sh`
build (set BLASTIT_REQUIRE_WHEEL=1 to require the artifact)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
EXIT_no_dist=0

$ mkdir -p dist && cargo test … -- --nocapture
no blastit-0.2.0 wheel in …/dist: the R12 check needs a `scripts/build-wheel.sh` …
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
EXIT_empty_dist=0
```

**(B) `BLASTIT_REQUIRE_WHEEL=1` and no wheel → non-zero.**

```console
$ BLASTIT_REQUIRE_WHEEL=1 cargo test -p blastit --test critical_artifacts --locked
---- the_wheel_extension_links_no_interpreter_and_no_php stdout ----
no blastit-0.2.0 wheel in …/dist: … (set BLASTIT_REQUIRE_WHEEL=1 to require the artifact)
Error: "BLASTIT_REQUIRE_WHEEL=1 is set but …/dist holds no blastit-0.2.0 wheel: build it with
`scripts/build-wheel.sh` (`make docs-test` and the CI wheel legs do), then re-run
`cargo test -p blastit --test critical_artifacts --locked`"
test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
EXIT_require=101
```

**(C) A planted name goes red — including from an *untracked* file.** Planted
`crates/blastit-view/src/zz_planted.rs` (git shows `??`, so `git ls-files` would
miss it), ran the R3 test, removed it:

```console
$ git status --short crates/blastit-view/src/zz_planted.rs
?? crates/blastit-view/src/zz_planted.rs
$ cargo test -p blastit --test critical_artifacts --locked the_deleted_transport_trait
the deleted transport seam is back under …/crates:
  …/crates/blastit-view/src/zz_planted.rs:2: `WorkerRuntime` (the deleted worker trait
  (`ADR-0016` decision 2)): pub trait WorkerRuntime {
  fix: `ADR-0016` decision 2 deletes `WorkerRuntime`/`RequestIo`/`RequestEnd`/`flush_response` …
test result: FAILED. 0 passed; 1 failed; 0 filtered out
EXIT_planted=101
$ rm crates/blastit-view/src/zz_planted.rs && cargo test … --locked
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
EXIT_restored=0
```

**(D) Two competing wheels are a failure** (`harness.py`'s rule — the version
decides, never mtime or the alphabet):

```console
$ cp dist/blastit-0.2.0-cp314-….whl dist/blastit-0.2.0-cp313-….whl && cargo test … the_wheel_extension
Error: "…/dist holds 2 wheels for version 0.2.0: blastit-0.2.0-cp313-…, blastit-0.2.0-cp314-….
The check cannot tell which one is the artifact — delete the ones this run did not build
(`dist/` is generated) and re-run `scripts/build-wheel.sh`"
EXIT_two=101
```

## Attempt-2 fixes

### The two new refusal paths, red-then-green

Each mutation was applied to the working file, the fixture re-run, the file
restored from `cp`, and `cmp` required byte equality before the next step.

```console
# 1. the DT_STRSZ end bound (string_at)
$ mutate `if stops >= limit {` -> `if false && stops >= limit {`
Error: "an understated DT_STRSZ was accepted: the R12 rule set is not enforced"
test result: FAILED. 0 passed; 1 failed; 5 filtered out          EXIT_mutA=101
$ cp ca.orig critical_artifacts.rs && cmp … && echo "restore A: cmp identical"
restore A: cmp identical
test result: ok. 1 passed; 0 failed                              EXIT_restoredA=0

# 2. the zero-NEEDED guard (assert_portable)
$ mutate `if facts.needed.is_empty() {` -> `if false && facts.needed.is_empty() {`
Error: "an ELF with no DT_NEEDED was accepted: the R12 rule set is not enforced"
test result: FAILED. 0 passed; 1 failed; 5 filtered out          EXIT_mutB=101
$ cp ca.orig critical_artifacts.rs && cmp … && echo "restore B: cmp identical"
restore B: cmp identical
test result: ok. 6 passed; 0 failed                              EXIT=0
```

### `DT_STRSZ` now bounds the string's **end**, not just its offset

`string_at` used to accept an offset inside the table and then read the name to
the next NUL, wherever that was — so `DT_STRSZ = 1` certified a file whose names
live past byte 1. It now computes `offset + end` and refuses a string that stops
at or past `DT_STRSZ`, naming both numbers. Fixture (8) is that file:
`needed = ["libc.so.6"]`, `DT_STRSZ = 5`, which the pre-fix reader accepted.

### Three refusal branches that had no fixture now have one

`zero-DT_NEEDED` (anti-vacuity: a reader that found nothing must not certify
every file), `ELFCLASS32` (accepted with `libc`, refused with `libpython`, so the
32-bit path is parsed rather than skipped) and big-endian (refused by name).
`synthetic_elf` grew a `Synthetic { class, data, needed, search, strsz }` shape so
one builder writes all of them; the little-endian ELF64 default is unchanged.

## Attempt-3 fixes

### 1. An empty `DT_NEEDED` name is refused (fixture 12)

`string_at` now refuses a dynamic string whose text is empty, before it is ever
handed to the rule set: a `NEEDED` that names no library resolves nothing (and an
empty search-path entry names the current directory), so an artifact could
otherwise certify clean while the section proved nothing. The refusal says
exactly that. Fixture (12) is `needed = [""]` — the pre-fix reader resolved it to
`[""]` and passed.

```console
$ mutate `if end == 0 {` -> `if false && end == 0 {`
Error: "a DT_NEEDED with an empty name was accepted: the R12 rule set is not enforced"
test result: FAILED. 0 passed; 1 failed; 5 filtered out          EXIT_mut=101
$ cp ca2.orig critical_artifacts.rs && cmp … && echo "restore: cmp identical"
restore: cmp identical
test result: ok. 1 passed; 0 failed                              EXIT_restored=0
```

### 2. `OUTPUT_LINE_CAP` is counted, not `contains`-checked

`assert_transport_layout` now requires `pub enum OutputStream` **and**
`pub const OUTPUT_LINE_CAP` to be declared exactly once each, in
`blastit-view/src/stream.rs`, so §7's "declared once" is literally what the suite
asserts (the re-export in `blastit-view/src/lib.rs` names the item, never
`pub const`, so it is not counted). Proved by planting a second copy in an
untracked `crates/blastit-view/src/zz_double.rs`:

```console
$ printf 'pub const OUTPUT_LINE_CAP: usize = 1;\n' > crates/blastit-view/src/zz_double.rs
Error: "`pub const OUTPUT_LINE_CAP` must be declared once, in `crates/blastit-view/src/stream.rs`
… but it is declared in: blastit-view/src/stream.rs, blastit-view/src/zz_double.rs"
test result: FAILED. 0 passed; 1 failed                             EXIT_double=101
$ rm crates/blastit-view/src/zz_double.rs && cargo test … the_deleted_transport_trait
test result: ok. 1 passed; 0 failed                                 EXIT_restored=0
```

### 3. Citation choice: both files, not one

The test's module doc and §4.2.1 now read *"`MB.36.5`'s `RESULT.md` records the
move; `MB.36.11`'s `RESULT.md` records the deviation"*. I kept the `MB.36.5`
citation — the leaf that made the decision is the right provenance for it — and
added this node's `RESULT.md` so the claim does not depend on the errata landing
(or on its wording surviving a later edit).


## Deviations

### 1. R3's ban is scoped — a **recorded deviation from `R3`**, not a reading of it

`R3`, design §4.2.1 and `ADR-0016` decision 2 — whose title is *"The worker
trait is deleted, not re-homed."* — require `WorkerRuntime`, `RequestIo`,
`RequestEnd` and `RequestIo::flush_response` to **stop existing**. The tree does
not do that: `MB.36.5` moved `RequestIo`, `RequestEnd` and `flush_response` into
`crates/blastit-view-php/src/worker.rs` (the crate that is their only
implementer, and which needs a write contract for the response it is serving)
and re-exports the two types from that crate's `src/lib.rs`. My attempt 1
citation ("design §4.2.3", "decision 2's title") was **wrong**: §4.2.3 did not
list those names and decision 2's title says the opposite. It is a deviation and
is now recorded as one:

* in the test — the module doc's *"The one exemption the scan carries: a recorded
  deviation from `R3`"* section and the `PHP_SOLE_IMPLEMENTER` doc comment both
  say so in those words, name the reason, and cite only what does say it
  (`MB.36.5`'s `RESULT.md`);
* in the design (§4.2.1's `runtime.rs` row, §4.2.3's `worker.rs` row, §7's R3
  oracle, and a pointer from R3's own bullet), corrected to the tree under the
  widened `paths`:
  - §4.2.1 now reads: the `WorkerRuntime` trait and its 2 unit cases are
    **deleted** in `MB.36.5`, while `RequestIo`/`RequestEnd`/`flush_response`
    **move** to `blastit-view-php/src/worker.rs` — their only implementer —
    "as a **recorded deviation from R3** (`MB.36.5`'s `RESULT.md` records the
    move; `MB.36.11`'s `RESULT.md` records the deviation; §4.2.3 and §7's scoped
    oracle)";
  - §4.2.3's `worker.rs` row names those three and says they move there from
    the deleted `runtime.rs` (R3's recorded deviation);
  - §7's R3 oracle is the scoped rule this suite implements: `WorkerRuntime` and
    `flush_request` in **no** file under `crates/`; `RequestIo`/`RequestEnd`/
    `flush_response` in no file except `blastit-view-php/src/{worker.rs,lib.rs}`;
    the exemption pinned positively and proved by path with a planted fixture;
    `blastit-view/src/runtime.rs` absent; `OutputStream`/`OUTPUT_LINE_CAP`
    declared once in `blastit-view/src/stream.rs`; `serve_forever` takes
    `runtime: PhpEmbedRuntime`.

The guard refuses everything the deviation does not cover, and refuses a
*stale* deviation: if those declarations vanish from `worker.rs`, the test fails
and names the line to delete from `PHP_SOLE_IMPLEMENTER`.

### 2. `BLASTIT_REQUIRE_WHEEL` is this suite's own knob

The node asks for a one-line pass with an opt-in failure, so the switch exists
only here; nothing in `.github/` sets it (outside `paths`). Filed as a
follow-up rather than wired blind.

## `dist/`, and what a clean checkout does

`dist/` is ignored (`.gitignore:8`), so a clean checkout has none: `make check`
prints the one line from proof (A) and stays offline and Docker-free. Where a
wheel *does* exist — as in this worktree, built in `MB.36.7` — `make check`
certifies the real artifact, which is the point of the node. `make check` never
built a wheel and never needed Docker for this suite.

## The reader agrees with `readelf`, on the real wheel

```console
ok   dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: 1 extension(s) with no
libpython/libphp NEEDED and no build-host search path: blastit/_blastit.cpython-314-
x86_64-linux-gnu.so [libgcc_s.so.1 libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6
ld-linux-x86-64.so.2]

$ readelf -d …/_blastit.cpython-314-x86_64-linux-gnu.so | grep -E 'NEEDED|RUNPATH|RPATH'
 (NEEDED)  Shared library: [libgcc_s.so.1]   …   (NEEDED)  Shared library: [ld-linux-x86-64.so.2]
```

Same six names, same order, and no `RUNPATH`/`RPATH` line — the std reader is not
a weaker oracle than the script it mirrors.

## Follow-ups (outside this node's `paths`)

1. **`.github/`:** if CI should demand the wheel rather than tolerate a local
   run without one, `BLASTIT_REQUIRE_WHEEL=1` belongs on the wheel legs' test
   command.
2. **`docs/knowledge/LESSONS.md`** (candidate line, not written to respect
   `paths`): *a design's grep oracle can be false against the tree that shipped
   it — read the leaf's `RESULT.md` deviations before turning a design sentence
   into a gate, and when the tree deviates, record the deviation and correct the
   design in the same node; a `crates/`-wide ban on `RequestIo`/`RequestEnd` is
   red today because `MB.36.5` moved them into the PHP worker.*
3. **Wheel-selection coupling:** this suite and
   `python/tests/runnable_docs/harness.py::wheel_path()` share the
   `dist/blastit-<version>-*.whl` rule; if `dist/` ever legitimately holds a
   3.13 *and* a 3.14 wheel for one version, both are red by design (the CI wheel
   legs build into separate trees). A `BLASTIT_WHEEL=` override would be the
   cheap escape hatch if it ever bites.
4. `MB.36.5`'s `RESULT.md` and the epic `RESULT.md` still describe the move
   without the words "recorded deviation" — left to the reviewer per the
   attempt-2 brief.
