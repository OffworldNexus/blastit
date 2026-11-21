VERDICT: FAIL

# Consistency review — `MB.35.3` (portable `blastit-lsp`)

Lens: **consistency**. Reviewed at `release/0.2.0` (worktree `.worktrees/release-0.2.0`,
where the `MB.35.3` documents exist):

- `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (whole file, incl. the `MB.35.3` amendment)
- `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4
- `docs/design/blastfile-lsp-zed.md` §1, §1.1, §1.3, §8.3, §8.3a, §8.4 (R37), §11, §14
- `ARCHITECTURE.md` §2, §2.7
- `docs/VISION.md`; accepted pages `docs/reference/cli.md` `## blastit lsp` / `## blastit-lsp`,
  `docs/local.md` `## Editors`, `editors/zed/README.md`, `docs/troubleshooting.md`
- `plan/nodes/MB/MB.35/{NODE.md,MB.35.3,MB.35.4,MB.35.5,MB.35.6}/NODE.md`,
  `plan/nodes/MB/MB.33/NODE.md`, `plan/nodes/MB/MB.35/MB.35.2/NODE.md`
- `scripts/platform_matrix.py`, `scripts/build-standalone.sh`, `scripts/check_zed_extension.py`,
  `.github/workflows/publish-pypi.yml`, `Cargo.toml`/`Cargo.lock`, the LSP sources.

Counts: **2 BLOCKER, 8 MAJOR, 10 MINOR.**

The architecture decision itself is coherent: ADR-0013's reversal is dated and keeps its
record, ADR-0010 §4's row, ARCHITECTURE §2/§2.7, §1.3's boundary and §8.3a's asset table agree
on the crate name, the library name, the five asset names, the archive layout, the two
dependencies and the "editor cross-platform, serving Linux-only" sentence. The failures are in
the seams: the only implementing leaf for the environment contract (`MB.33`) says the opposite
of the decision; the extraction leaf (`MB.35.4`) cannot execute the design under its own
`paths`; and two accepted pages plus §11 still carry the pre-amendment `uv` story.

---

## BLOCKER

### C1 — BLOCKER — `MB.33` implements the environment contract the ADR removed, and §8.3a says only three brief points are stale

The decision of record is explicit:

> ADR-0013 (MB.35.3 amendment), §5: “`binary.env` is **not** honoured (the server reads no
> environment, so the field has no purpose and is the one loader-hijack vector a settings file
> could add), and the child receives an allowlist — `PATH`, plus `SYSTEMROOT` on Windows —
> instead of the whole shell environment.”

§8.3a says the same twice:

> §8.3a step 1: “`binary.arguments`, defaulting to `[]` when unset. `binary.env` is **ignored**”
>
> §8.3a, “The child's environment is an allowlist.”: “Steps 1 and 2 hand the server `PATH` (plus
> `SYSTEMROOT` on Windows) and nothing else — not `worktree.shell_env()`, not `binary.env`, not a
> `BLASTIT_*` name.”

`MB.33` — the only leaf that writes `editors/zed/src/lib.rs` — says the exact opposite, twice:

> `MB.33` Goal, step 1: “`binary.env` is merged **over** `worktree.shell_env()`. The returned
> `zed::Command` carries `env = shell_env + binary.env`.”

The design knows the brief is stale, but undercounts:

> §8.3a: “**`MB.33`'s brief is stale on three points and this section wins.**” — the three listed
> are the capability block, the `download_file` destination and `make_file_executable`. The env
> contradiction is a fourth, material one.

The design is also inconsistent with itself: R37 still requires the old behaviour.

> §8.4 R-ids: “R37 (`lib.rs`: `which` order, the six argv literals, **one `shell_env()`
> assignment**, no process execution, no capability)”

while R72 requires the allowlist:

> §14.1 R72: “`binary.env` is ignored and the child env is the `PATH`/`SYSTEMROOT` allowlist”

and `MB.33`'s new property list omits the guard property §8.3a assigns to it
(“`MB.33`'s guard gains `lib: env allowlist`”) — it lists `lib: which blastit-lsp`, `lib: no uv`,
`extension: capabilities`, `lib: asset map`, `lib: zip windows`, `lib: executable`, `lib: cache`,
`lib: status`, `lib: platform`, and no `lib: env allowlist`.

**Failure mode:** an implementer follows `MB.33` (the leaf it was assigned), re-adds
`shell_env()` (hence `HOME`, `LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`, `GCONV_PATH`, `BLASTIT_*`)
to a process the ADR calls an attack surface, and the guard the design promised is not written.
The security review's m6 answer (“hand it over verbatim on purpose”) is superseded by §8.3a
without R37/`MB.33` being updated.

**Fix (exact):**
1. `MB.33` step 1: replace the `binary.env` sentence with “`binary.env` is ignored; `Command.env`
   is the literal allowlist `PATH` (+ `SYSTEMROOT` on Windows).”
2. `MB.33`'s property list: add `lib: env allowlist` (`assert` no `shell_env()` assignment to
   `Command.env`, and the two-name literal).
3. §8.4 R37: replace “one `shell_env()` assignment” with “the `PATH`/`SYSTEMROOT` allowlist”.
4. §8.3a's “stale on three points” becomes “four points”, naming the env contract.

---

### C2 — BLOCKER — `MB.35.4` cannot be executed as written: two required `paths` are absent, it says “byte-identical”, and it omits the import change the crate needs

The ADR and design are precise about what the extraction edits:

> ADR-0013 §3: “There is **one** `run()` behind both binaries; the *assertion bodies* of the
> protected `crates/blastit/tests/critical_lsp.rs` stay unchanged against `blastit lsp` — only the
> two `include_str!` path literals follow the files the move relocates …”
>
> design §1.3 obligation 1: “… `critical_lsp.rs` joins `MB.35.4`'s `paths`,
> `tools/critical_guard.py` sees the change as declared”
>
> ADR-0013 §2: “`MB.35.4` owns that one line, and its `paths` must name
> `crates/blastit-manifest/src/lib.rs` — the plan's list does not yet”

`MB.35.4`'s `paths` are `crates/blastit-lsp/**`, `crates/blastit/src/lsp/**`,
`crates/blastit/src/lib.rs`, `crates/blastit/Cargo.toml`, `Cargo.toml`, `Cargo.lock`. Neither
`crates/blastit-manifest/src/lib.rs` nor `crates/blastit/tests/critical_lsp.rs` is listed, and
the brief forbids the edit outright:

> `MB.35.4`: “the 1880-line `crates/blastit/tests/critical_lsp.rs` stays **byte-identical** and
> keeps proving the delegation.”
>
> `MB.35.4` Context: “**Protected files.** `crates/blastit/tests/critical_lsp.rs` is protected and
> **must not be edited** — it is not in `paths`, deliberately.”

That is not merely a documentation wording gap: the suite's two literals name a directory the
design deletes (`§1.3` row 5: “`crates/blastit/src/lsp/` does not exist”), so the test does not
compile after the move. `tools/critical_guard.py` will also refuse an undeclared edit.

The brief's premise is factually wrong as well:

> `MB.35.4` Context: “`mod.rs` (1174 lines) + `hover.rs` (254) are already pure `std` +
> `blastit_manifest` + `serde_json` (verify with `grep -n 'use ' crates/blastit/src/lsp/mod.rs`).”

Measured: `mod.rs` is **1591** lines, `hover.rs` is **337**, and `mod.rs:55` is
`use blastit_sqlpolicy::parameterize;`. Moving the module without changing that import does not
compile in a crate whose only dependencies are `blastit-manifest` and `serde_json`. The ADR
assigns the fix to `MB.35.4` (“one line, `pub use blastit_sqlpolicy::parameterize;`”), and
`MB.35.4` never mentions it (nor `crates/blastit-manifest/src/lib.rs`).

**Failure mode:** `MB.35.4` starts, hits the protected-file guard and a missing dependency, and
must either violate its lane or stop. The design's own acceptance gate — “The design is accepted
when `MB.35.4` … can be written without re-deciding a name, a format, a target or an invariant”
(`MB.35.3` NODE.md) — is not met.

**Fix (exact):**
1. `MB.35.4.paths` += `crates/blastit-manifest/src/lib.rs` and
   `crates/blastit/tests/critical_lsp.rs`.
2. Replace “stays **byte-identical**” with “the assertion bodies and property lists are
   unchanged; four `include_str!` literals move (two in the test, two in `hover.rs` — see M1)”.
3. Add to the Goal: `crates/blastit-manifest/src/lib.rs` gains
   `pub use blastit_sqlpolicy::parameterize;`; `mod.rs`'s
   `use blastit_sqlpolicy::parameterize;` becomes `use blastit_manifest::parameterize;`.
4. Refresh the stale counts and the `cli.rs:572` reference (actual `crate::lsp::run()` is at
   `cli.rs:586`).

---

## MAJOR

### M1 — MAJOR — The move leaves `hover.rs` “unchanged”, but its two `include_str!` paths break

> §1 row 10: “`src/lib.rs` = today's `lsp/mod.rs`, `src/hover.rs` unchanged”

`hover.rs` contains two test-time includes that are relative to its location:

> `crates/blastit/src/lsp/hover.rs:294` and `:316`:
> `let page = include_str!("../../../../docs/reference/blastfile.md");`

From `crates/blastit-lsp/src/hover.rs` the same literal resolves outside the repository. `cargo
build` still passes (both are under `#[test]`), but `cargo test -p blastit-lsp` — one of
`MB.35.4`'s acceptance commands — fails to compile until they become
`include_str!("../../../docs/reference/blastfile.md")`.

The design counts only the test's two literals (“only the two `include_str!` path literals
follow the files the move relocates”) and never mentions `hover.rs`'s two; the diff oracle in
§1.3 is scoped to `crates/blastit/tests/critical_lsp.rs` only, so this edit has no recorded
proof.

**Fix:** change §1 row 10 to “`src/hover.rs` = the same file with its two
`docs/reference/blastfile.md` `include_str!` depths reduced by one”; add the two literals to
§1.3's equivalence obligation 1, R70 and `MB.35.4`.

---

### M2 — MAJOR — The accepted pages do not state the three-step order, and two still send the reader to `uv`

§8.3a makes a claim about the accepted DX that is not true of the pages:

> §8.3a: “`MB.33` implements this order and nothing else; the accepted pages state the same three
> steps in the same order (`docs/local.md` `## Editors` step 2, `docs/reference/cli.md`
> `## blastit-lsp`, `editors/zed/README.md`'s four rows …)”

- `editors/zed/README.md`'s four rows are correct, but two lines later the page says:
  > “Until the release that ships `blastit-lsp`, this extension runs a `blastit`/`uv` already on
  > `PATH`.”
- `docs/reference/cli.md` `## blastit-lsp` never names step 2's first name and presents
  `blastit lsp` as the PATH answer:
  > “Zed's extension downloads this binary itself, once per Blastit release, and reuses the cached
  > copy. On Linux, `blastit lsp` is the same server: the extension runs it when `blastit` is on
  > the `PATH` it sees, and downloads otherwise.”
- `docs/local.md` step 2 defers to a page that does not state the order:
  > “The extension finds the server itself, in a fixed order ([the order](reference/cli.md#blastit-lsp));
  > there is nothing to configure.”

That also contradicts ADR-0013's summary of the new order — “**the second name is the first
name.** `blastit-lsp` wins by settings or by `PATH`, and `blastit lsp` is the Linux fallback” —
and the CLI page's “downloads otherwise” only holds if `blastit-lsp` is not on the PATH.

**Failure mode:** a Mac/Windows reader follows the CLI page, never learns that an unpacked
`blastit-lsp` on `PATH` is the intended first answer, and a Linux reader is told `blastit lsp`
wins; `MB.33`'s own acceptance grep
`! git grep -n 'uv run blastit lsp\|uv add blastit' -- … editors/zed/README.md` does not match
“runs a `blastit`/`uv` already on `PATH`”, so the stale sentence survives a green leaf.

**Fix:** (a) put the three steps in `docs/reference/cli.md` `## blastit-lsp` in order (settings →
`blastit-lsp` on `PATH` → `blastit lsp` → download) and delete “and downloads otherwise”;
(b) delete README lines 23–24 (the release ships the server, so the caveat is false);
(c) either state the order in `docs/local.md` step 2 or soften §8.3a's claim to “the README states
the four rows; the other pages link to it”; (d) widen `MB.33`'s grep to `\buv\b` on the three
pages.

---

### M3 — MAJOR — `MB.35.5` builds an archive with no `LICENSE` and no `SHA256SUMS`, and smokes the build tree, not the extracted file

The format is decided by R75 and stated three times:

> design §8.3a: “the archive's one top-level directory is `blastit-lsp-<target>/` holding exactly
> the executable, `LICENSE` and a `SHA256SUMS` over those files”
>
> ADR-0013 §4: “beside `LICENSE` and a `SHA256SUMS` over those files”
>
> ADR-0010 §4 row: “beside `LICENSE` and an in-archive `SHA256SUMS`”
>
> §14.1 R75: “each asset carries `LICENSE` and an in-archive `SHA256SUMS`”

`MB.35.5` stages one file and asserts one file:

> Goal 2: “stages `blastit-lsp-<target>/blastit-lsp(.exe)` and archives it”
>
> Acceptance: `tar -tzf /tmp/opencode/lsp-dist/blastit-lsp-*.tar.gz   # one dir, one executable`

and its smoke runs `target/release/blastit-lsp`, not the extracted artifact:

> Acceptance: `uv run --no-project scripts/lsp_smoke.py --binary target/release/blastit-lsp`

while §14.2 requires “extract to a temp dir, assert the three entries, then smoke **the extracted
file**”.

**Failure mode:** the release ships an Apache-2.0 binary without its licence and without the
in-archive checksum the ADR tells users to verify; the layout the Zed extension joins on
(`blastit-lsp-<version>/blastit-lsp-<target>/blastit-lsp`) is never proved from a produced
archive.

**Fix:** `MB.35.5` Goal 2 stages `LICENSE` + `SHA256SUMS` (excluding itself) + the executable;
acceptance extracts to a temp dir, asserts exactly those three entries, and points
`lsp_smoke.py --binary` at the extracted path.

---

### M4 — MAJOR — `MB.35.5` is not told to add the `lsp` count noun or `--count lsp`, nor about the `blastit-*.tar.gz` collision that will fail the release job

> ADR-0013 §4: “`MB.35.5` also adds it to `--matrix`/`--summary`/`--check` and to **the clause
> noun the count gate reads**, so R74's proof is a real gate and not a green line about other
> families.”
>
> ADR-0013 Consequences: “a third `download-artifact` with `pattern: lsp-*`, a `--count lsp`
> assertion, the archive glob narrowed (today's `blastit-*.tar.gz` also matches
> `blastit-lsp-*.tar.gz`)”
>
> §14.1 R74: “a distinct count noun so an LSP asset is never counted against the standalone
> family”

`MB.35.5` Goal 1 promises only “`--matrix lsp`, `--summary`, and the declared/proved split”; its
acceptance checks only that `--matrix lsp` has five entries. The existing release job will fail
on the first release that adds the family:

> `.github/workflows/publish-pypi.yml:793` — `archives=(blastit-*.tar.gz)` (matches
> `blastit-lsp-*.tar.gz`)
>
> `.github/workflows/publish-pypi.yml:785` — `expected_bins=$(… --count binaries)` (still four)

**Failure mode:** either the release job fails (`9 != 4`), or the implementer “fixes” it by
counting LSP archives against the standalone family — exactly what R74 forbids — or the leaf
ships without touching the check and the release breaks on the tag.

**Fix:** add to `MB.35.5`: the `lsp` count noun in `scripts/platform_matrix.py`'s scan regexes,
`--count lsp`, and the narrowed release glob (or an explicit array excluding `blastit-lsp-*`),
with `packaging-gates-test` mutations for each.

---

### M5 — MAJOR — The `getcap` assertion is assigned to `MB.35.5` by the ADR and §1.3 but appears in no leaf

> ADR-0013: “**the portable artifact has nothing in it that drops or uses a privilege, and it is
> supported unprivileged only.** … It is unsupported — `MB.35.5` asserts `getcap` is empty for
> every Linux asset and no page installs it that way”
>
> design §1.3: “`MB.35.5` asserts `getcap` is empty for every Linux asset, no page installs it
> with `setcap` or `sudo`”

Neither `MB.35.5` nor `MB.35.6` mentions `getcap`, file capabilities, or the per-asset assertion.
`docs/testing/E2E-MB.35.md` (MB.35.6) is the only place that could record it, and its brief does
not name it.

**Failure mode:** the “supported unprivileged only” claim — the one that answers the
`sudo`/`setcap` objection in ADR-0013's comparison table — has no oracle.

**Fix:** add to `MB.35.5`'s acceptance (Linux legs) `getcap` on each produced binary and a
`packaging-gates-test` negative twin; or move it to `MB.35.6`'s E2E and cite it there instead of
claiming `MB.35.5` does it.

---

### M6 — MAJOR — `MB.35.6`'s guard enforces a subset of R71/R73/S12, and the compiler half is missing

§1.3's third row and R73 name the whole set:

> §1.3 row 3: “no line in `crates/blastit-lsp/src/lib.rs` or `src/hover.rs` contains any token of
> §1.2's list — `std::fs`, `std::path`, `std::process`, `std::net`, …, `std::env`, `include!` —
> nor `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os`, `unsafe`, or `extern "C"`”
>
> §1.3 row 1: “**every other dependency table** … is absent or empty”
>
> ADR-0013 Consequences: “`MB.35.6` also forbids `std::process`, `std::net`, `std::fs`,
> `std::path`, `include!` and `unsafe` in `lib.rs`/`hover.rs`”
>
> §14.1 R73: “no `std::fs`/`std::path`/`std::net`/`std::process`/`std::env`/`unsafe` … and
> `#![forbid(unsafe_code)]`”

`MB.35.6` implements:

> “no `crates/blastit-lsp/src/**` line contains `std::os::unix`, `std::os::linux`,
> `std::os::windows`, `libc::`, `rustix`, `nix::`, `pyo3`, `#[cfg(unix)]`, `#[cfg(target_os` or
> `std::process::Command`”
>
> “`[dependencies]` is exactly `{ blastit-manifest, serde_json }` — any other runtime dependency
> is red … (`blastit-sys`/`rustix`/`nix`/`libc`/`pyo3`/`blastit-php-sapi` are the named forbidden
> set …)”

Missing: `std::fs`, `std::path`, `std::net`, `include!`, `unsafe`, `extern "C"`,
`#[cfg(windows)]`; `std::process` is narrowed to `std::process::Command` (so `std::process::id()`
or a future `std::process::abort` pass); only `[dependencies]` is parsed (a
`[target.'cfg(windows)'.dependencies] libc` row is the exact negative twin §1.3 asks for); and
the named forbidden set omits the seven serving crates (`blastit`, `blastit-dbproxy`,
`blastit-scheduler`, `blastit-http`, `blastit-view`, `blastit-py`).

Worse, `#![forbid(unsafe_code)]` — the compiler half that makes the grep non-optional — is
required by ADR-0013 decision 1 (“plus `#![forbid(unsafe_code)]` on both the library and the
binary”) but appears in neither `MB.35.4` nor `MB.35.6`; §1.3 mentions it for `src/lib.rs` only,
not `main.rs`.

**Failure mode:** the guard goes green while a `use std::net::TcpListener`, an `extern "C"` +
`unsafe` shim, or a target-scoped `libc` arrives — the “enforced, not remembered” claim (R71)
is then a comment.

**Fix:** drive the guard from one token table equal to §1.3 row 3 (one negative twin per token),
parse every dependency table with `tomllib`, use the twelve-name forbidden set, and add
`#![forbid(unsafe_code)]` to both `lib.rs` and `main.rs` in `MB.35.4` + §1.3 row 3.

---

### M7 — MAJOR — `MB.33`'s acceptance cannot pass against the record it is told to preserve

> `MB.33` Acceptance #2:
> `$ ! git grep -n 'which("uv")' -- docs/design docs/decisions`

That command returns matches in the dated record the same brief says must stay:

> ADR-0013:69 — “`Worktree::which("blastit")` then `Worktree::which("uv")` with”
>
> design §8.3:1604 — `None => match worktree.which("uv")`
>
> `MB.33` Context: “(Dated record rows in `docs/design/blastfile-lsp-zed.md` and `ADR-0013` keep
> the old argv inside the amendment/history; they must not read as an instruction.)”

A second, smaller drift: §8.3a's assertion column names `lib: no capabilities`,
`lib: download destination`, `lib: asset url`, `lib: failure`, `lib: env allowlist`, while
`MB.33`'s property list names `extension: capabilities` and omits the other four — and
`scripts/test_check_zed_extension.py` pins the property names **and the count exactly**
(`assertEqual(len(properties), 47)` today). The guard and the contract cannot both be green.

**Fix:** scope the grep to `editors/zed/src/lib.rs` and `scripts/check_zed_extension.py` (never
`docs/design`/`docs/decisions`), and reconcile the property names/count with §8.3a in one list.

---

### M8 — MAJOR — ADR-0013 says a musl host “loses only the editor server”; it does not

> ADR-0013, “Targets this epic deliberately does not build”: “`*-musl` is different and must not
> be described with them: it is a **serving platform** … so it keeps `blastit`, `dev`, `check`
> and `fmt`, **loses only the editor server**, and gains no `blastit-lsp` in 0.2.0.”

`blastit lsp` is not feature-gated: `crates/blastit/src/cli.rs:50` has `Lsp`, `mod.rs` is
unconditional, and the musl standalone is built
`--no-default-features --features php` (`ADR-0010` §4) — so the musl `blastit` still answers
`blastit lsp`, and the extension's step 2 (`which("blastit")` → `["lsp"]`) resolves there. The
musl host loses the **portable asset**, not the editor experience. `docs/reference/cli.md` only
says “no musl” for the `blastit-lsp` table, which is correct.

**Fix:** “it keeps `blastit` (including `blastit lsp`), so the editor still works through the
Linux binary; it gains no `blastit-lsp` asset in 0.2.0.”

---

## MINOR

### m1 — MINOR — §11 still describes the pre-move module, contradicting §1.3 row 5 and §14.2

> §11.1 S5: “`include_str!("../src/lsp/mod.rs")` contains `set_hook` and `catch_unwind`”
>
> §11.1 S11: “`lsp.rs` contains `blastfile::check` and `blastfile::fmt`”
>
> §1.3 row 5: “the server has one home: `crates/blastit/src/lsp/` does not exist”
>
> §14.2: “the source-grep test now reads `../../blastit-lsp/src/{lib,hover}.rs`”

Fix: mark S5/S11's paths as the record and point at §14.2's post-move literals (same pass as
C2/M1), or write `../../blastit-lsp/src/lib.rs` and “`lib.rs`” directly. Also S3's “`lsp.rs`
reads no environment” should say `lib.rs`.

### m2 — MINOR — ADR-0013's §14 reference stops at R69–R74, but §14 defines R69–R76

> ADR-0013 References: “§14 (R69–R74 and the test mapping)”

R75 (trust root) and R76 (untrusted document) are requirements the amendment relies on in its
own Consequences. Fix: “R69–R76”.

### m3 — MINOR — ADR-0013 quotes the design row non-verbatim

> ADR-0013: “privilege surface: ‘the same call in a second binary — splitting the binary buys
> nothing here’”
>
> design §1.1, actual: “the same call in a second binary — the capabilities belong to the process
> image either way, so splitting the binary buys nothing here”

Fix: quote the row exactly, or drop the quotation marks.

### m4 — MINOR — ADR-0010 §4 restates the target counts its own note says live in `scripts/platform_matrix.py`

> ADR-0010 §4 row: “the `lsp` family in `scripts/platform_matrix.py` (`MB.35`): **two Linux, two
> Apple, one Windows**”
>
> ADR-0010 §4 note: “the five targets, the runner per leg and any `dropped_because` live in that
> module — the same rule the two serving families follow.”

`MB.35.3`'s own brief said “Do not restate the matrix in two places”. Fix: “the `lsp` family
(five targets, listed once in that module)”.

### m5 — MINOR — `MB.35.5` miscounts the families and uses bare `python3` in acceptance

> `MB.35.5` Context: “a **sixth** family means either a second explicit count or a renamed
> pattern”

There are four declared families today (wheels, binaries, shims, images); `lsp` is the fifth.
Also `scripts/platform_matrix.py --matrix lsp | python3 -c …` contradicts the repository's
“never bare `python3`” rule (AGENTS.md #9); use `uv run --no-project python -c …`.

### m6 — MINOR — `MB.35.5`'s smoke note is stale: `MB.34.5` is done and `codeActionProvider` already exists

> `MB.35.5` Goal 3: “assert the capability document (and, after `MB.34.5`, `codeActionProvider`)”

`plan/nodes/MB/MB.34/MB.34.5/NODE.md` is `status: done`, and `mod.rs:540` already dispatches
`textDocument/codeAction`. Fix: assert it unconditionally.

### m7 — MINOR — §8.3a says the pages keep the setting per-user, then says the CLI page offers the project alternative

> §8.3a: “The accepted pages keep the setting to `zed: open settings` (per user); the
> project-file alternative in `docs/reference/cli.md` `## blastit-lsp` is `MB.33`'s to remove”

`docs/reference/cli.md:134-136` offers both (“`zed: open settings` for every project, or a
project's `.zed/settings.json`”). The second clause admits the first is false; fix the first to
“the pages offer the per-user setting (and, today, a project one `MB.33` removes)”.

### m8 — MINOR — `MB.35.2` is still `ready` while ADR-0013/design call its pages “Accepted DX”

> ADR-0013: “**Accepted DX:** `docs/reference/cli.md` `## blastit-lsp`, `docs/local.md`
> `## Editors`, `editors/zed/README.md` (`MB.35.2`)”
>
> `plan/nodes/MB/MB.35/MB.35.2/NODE.md`: `status: ready`

Either the status or the word “accepted” is wrong. Relatedly, `MB.35.2`'s Context says
`## blastit lsp` “says `uv run blastit lsp` when the CLI is not installed”, which the page no
longer does. Fix: mark the node's state accurately (or drop “Accepted”) and refresh its Context.

### m9 — MINOR — `docs/VISION.md` never mentions the editor artifact

VISION's Milestone B enumerates static/SPA, embedded PHP, Vite/Node and Tomcat; Milestone E is
coding-agent integrations. The authority for a cross-platform editor artifact is an owner quote
inside ADR-0013, not VISION. One sentence in Milestone B (“the `Blastfile` editor surface ships
as a portable `blastit-lsp` for Linux, macOS and Windows; serving stays Linux-only”) would keep
the north star and the ADR aligned and give §14.3 a citable home.

### m10 — MINOR — §14.3's macOS/Windows sentence says “no server” in a paragraph about the server

> §14.3: “**macOS Apple silicon** — the editor only: colours, SQL diagnostics …, hover,
> format-on-save; no `blastit`, no `dev`, no `check`, no server.”

Read literally, “no server” denies the language server the sentence just promised. Fix:
“no **serving** server” (or “no application server”).

---

## What would make this PASS

1. C1 fixed in `MB.33` + R37 + §8.3a's stale list, and C2 fixed in `MB.35.4` (paths, wording,
   re-export + import line) — these are the two acceptance gates the node names.
2. M2's page corrections in `editors/zed/README.md` and `docs/reference/cli.md` (and §8.3a's
   claim about them), M3/M4/M5 in `MB.35.5`, M6 in `MB.35.6`, M7 in `MB.33`, M8 in ADR-0013.
3. The MINORs folded into the same edit pass; none changes a decision.

The decision record itself — ADR-0013's dated reversal, ADR-0010 §4's row, ARCHITECTURE §2.7 and
§1.3/§8.3a/§14's names, formats, five targets and two-dependency invariant — is coherent and
needs no new decision. The failures are leaf-brief drift, one wrong platform paragraph and a handful of stale sentences — not a wrong
architecture.
