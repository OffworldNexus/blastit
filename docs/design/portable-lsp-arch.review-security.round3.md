VERDICT: FAIL

# Security review, round 3 — `MB.35.3` (portable `blastit-lsp`)

Scope: `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (the `MB.35.3` amendment),
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/design/blastfile-lsp-zed.md` §1.1, §1.2, §1.3, §8.3, §8.3a, §11, §11.1, §11.2, §12.1, §14,
and `ARCHITECTURE.md` §2, §2.7. Round 2 is
`docs/design/portable-lsp-arch.review-security.round2.md`; round 1 is
`docs/design/portable-lsp-arch.review-security.md`.

Reviewed in `.worktrees/release-0.2.0` at `143afaf` with the working-tree documentation edits of
06:15 (the four documents do not exist on the main checkout, which is on `release/0.2.1`). Claims
were checked against `crates/`, `scripts/`, `tools/critical_guard.py`,
`plan/nodes/MB/MB.35/**`, `plan/nodes/MB/MB.33/NODE.md`, `docs/reference/cli.md`,
`docs/local.md`, `docs/troubleshooting.md`, and `.github/workflows/publish-pypi.yml`.

Round 2's BLOCKER is now a *written* hand-off rather than a silent contradiction: §14.4 names
`MB.35.6`'s ten-token list as stale, and R71 says the brief "has to be corrected to this rule
first — §14.4 lists the hand-off". The three failures below are not hand-offs: they are holes in the
design's own guard contract, plus one ADR sentence that contradicts the design on the one thing a
security reader will quote — the trust root of the download.

| Grade | Count |
|---|---|
| BLOCKER | 0 |
| MAJOR | 3 |
| MINOR | 10 |

## Round-2 findings, one line each

| Round 2 | Now |
|---|---|
| **B1** — `MB.35.6`'s brief keeps the weak token list | **Resolved as a document matter.** §14.4's `MB.35.6` row names the §1.3 row 3 set, the file set, the twins and the evidence states; R71 makes the correction a precondition. The remaining weakness is the *content* of row 3 (MAJOR 1, MAJOR 2), not the hand-off. |
| **M1** — `MB.35.4`'s `paths` forbid the edits the design requires | **Resolved as a hand-off.** §1.3 lines 264-266 and §14.4's `MB.35.4` row add `crates/blastit-manifest/src/lib.rs` and `crates/blastit/tests/critical_lsp.rs`; "byte-identical" is replaced by "assertion bodies unchanged, four literals moved". |
| **M2** — the move breaks four `include_str!` literals, not two | **Resolved.** §1.3 obligation 1, R70 and ADR decision 3 all say four, two files, with the old and new strings. §7.2 still miscounts (MINOR 3). |
| **M3** — the protected oracle keeps the 13-token list | **Resolved by an explicit decision, with a residue.** §14.2 line 2957 now says the protected grep "stays as the *behaviour-side* record and is deliberately not widened", and the token table is pinned in the scripts test. The cheap residue is `std::net` (MINOR 9). |
| **M4** — project settings can run a binary; trust is sticky | **Resolved in substance.** §8.3a no longer relies on Zed filtering the project layer: it says the extension cannot tell the layers apart, that a clone can name the binary, and that `binary.env` is ignored. Residues are wording (MINOR 6, MINOR 10). |
| **M5** — `MB.33` still merges `binary.env` | **Resolved as a hand-off.** §14.4's `MB.33` row says "`binary.env` ignored and the `PATH`/`SYSTEMROOT` allowlist"; §8.3a says it twice. The stale-brief paragraph still says "four points" and lists three (MINOR 5). |
| **M6** — the release-level checksum is called a control the extension consults | **Not resolved in the ADR.** §8.3a is fixed; ADR-0013 lines 356-359 still calls the release-level `SHA256SUMS` an extension control. MAJOR 3. |
| **m1** — `forbid(unsafe_code)` on one file or two? | **Resolved.** §1.3 row 3 and §14.4 put the attribute on both `src/lib.rs` and `src/main.rs`; the "one shape" paragraph excludes the attribute from the line count. |
| **m2** — `nix`/`libc` substring traps | **Resolved for `nix`.** Row 3 now says "matched as an identifier/path boundary, not an arbitrary substring"; the same matcher covers `libclang`. |
| **m3** — `#[path]` and `build.rs` are outside the rule | **Resolved.** `#[path` is a token; row 1 requires no `build.rs` and empty `[build-dependencies]`/`[target.*]`, with a negative twin. |
| **m4** — S4 names `File::open`, no list contains it | **Persists.** MINOR 2. |
| **m5** — §7.2's `include_str!` sentence contradicts itself | **Persists.** MINOR 3. |
| **m6** — the ADR's token summary reads as exhaustive | **Resolved.** ADR lines 341-344 end with "the exhaustive token list and file set are design §1.3's third row". |
| **m7** — the guard lands after the artifact is built | **Persists.** R71/R74 do not say the epic gate cannot pass before `MB.35.6`. MINOR 8. |

---

## MAJOR 1 — the guard scans two named files and not the crate, so a third file (or the linked `blastfile::read`) opens a path with no red

**Quote.** Design §1.3, line 255: "no line in `crates/blastit-lsp/src/lib.rs` or `src/hover.rs`
contains any token of §1.2's list". ADR-0013, lines 341-344: "`MB.35.6` also forbids … in
`lib.rs`/`hover.rs` (with `main.rs` pinned to its attribute plus one call) — the exhaustive token
list and file set are design §1.3's third row". Design §1.3, lines 314-316: "It **cannot become a
serving binary by accident**, because the serving crates are not in its graph: there is no
listener to bind, no stage 0 to enter, and no `std::process` with which to spawn one."

**The repo.** `crates/blastit-manifest/src/blastfile/mod.rs:270` is a public path opener in the
one crate the LSP links:

```rust
pub fn read(path: &Path) -> Result<String, ManifestErrors> {
    let metadata = std::fs::metadata(path).map_err(unreadable)?;
    ...
    let file = std::fs::File::open(path).map_err(unreadable)?;
```

`blastit_manifest::blastfile::read("/etc/passwd".as_ref())` compiles without the token
`std::path` (type inference supplies `&Path`) and without `std::fs` (the call site never names
it). No rule in §1.3 bans `blastfile::read` or an alias of it, and no rule asserts the crate's
`src/` file set. A future "open the file from the URI" feature — the most natural LSP feature
there is — can therefore be implemented in one line, pass `make lsp-portable`, and falsify S4
("A `Blastfile` is never opened") and the §14.1 R73 claim.

**Three evasions, all green under row 3 as written.**

1. `mod helper;` in `lib.rs` plus `src/helper.rs` containing `use std::net::TcpListener;` and
   `std::fs::read(...)`. The only scanned files stay clean; row 5 only forbids a second module
   under `crates/blastit/src/`, not under `crates/blastit-lsp/src/`.
2. `use std::{fs, net};` in `lib.rs`. The tokens are the contiguous strings `std::fs` and
   `std::net`; the brace-grouped import contains neither.
3. `blastit_manifest::blastfile::read("…".as_ref())` as above.

**Failure mode.** The artifact still has no privilege crate, so this is not a sandbox escape. It
is a false invariant: the design's "opens no path" sentence is enforced by a grep that a normal
refactor can step around, and the one linked API that opens a path is not named. The review file
that accepts the crate would quote S12 and be wrong.

**Exact fix.**

- Row 3's file set becomes "every file under `crates/blastit-lsp/src/**`", and a new row asserts
  the set is exactly `src/lib.rs`, `src/hover.rs`, `src/main.rs` (negative twin: add
  `src/extra.rs` + `mod extra;` and require a red).
- Add `blastfile::read` (and any `use …::read as …` alias) to the token list, or allowlist the
  `blastit_manifest::` items to `check`, `fmt` and the model types; negative twin: a `lib.rs`
  line calling `blastit_manifest::blastfile::read("x".as_ref())`.
- Add `use std::{` to the token list (the crate has no legitimate grouped `std` import); negative
  twin: `use std::{fs, net};`.
- Assert the literal `#![forbid(unsafe_code)]` at the top of **both** `src/lib.rs` and
  `src/main.rs` — today row 3 says the attribute is the compiler half but no assertion requires
  it to be present.

## MAJOR 2 — the `cfg` and `unsafe` spellings are narrower than the "one code path" claim, so a second behaviour passes the grep

**Quote.** Design §1.3, line 255: the rule bans "`#[cfg(unix)]`, `#[cfg(windows)]`,
`#[cfg(target_os`" and "an unsafe item or block (`unsafe {`, `unsafe fn`, `unsafe impl`, `unsafe
trait`)". The same cell's "Why" column: "one code path on three platforms: a `cfg` here is a
second behaviour".

**Why it fails.** The spellings are not the space of second behaviours:

- `#[cfg(not(unix))] mod windows;` contains none of the three banned `cfg` strings.
- `#[cfg(target_family = "unix")]` and `#[cfg(target_env = "msvc")]` likewise pass; only
  `target_os` is named.
- `#[cfg_attr(windows, path = "win.rs")]` passes; `#[path` is banned, but `path =` inside
  `cfg_attr` is not.
- `unsafe extern "C" { … }` is caught by `forbid(unsafe_code)` **only while the attribute is
  present**; deleting the attribute is not a red (MAJOR 1's last bullet), and `unsafe` inside a
  new module is not scanned.

The guard is the artifact-level gate that `make check` runs; §14.2 line 2957 explicitly makes it
the place the full property list lives. A property list with four holes in a five-line rule is an
underspecified seam, not a spelling preference.

**Exact fix.** Replace the three `cfg` spellings with one blanket rule: **any `#[cfg` or `cfg!`
in `crates/blastit-lsp/src/**` is a red** (the crate has one code path; `main.rs` needs none),
with a negative twin using `#[cfg(not(unix))]`. Keep the unsafe-item list, add `unsafe extern`,
and make the `forbid` attribute's presence an assertion. If a legitimate `cfg` ever appears, the
allowlist is one named line in the script, not a token family.

## MAJOR 3 — ADR-0013 still calls the release-level `SHA256SUMS` an extension control

**Quote.** ADR-0013, lines 356-359:

> **Trust root stated, not implied.** The `SHA256SUMS` inside an archive is for a human who
> unpacked it; it authenticates nothing. The extension's controls are HTTPS to `github.com` and
> the release-level `SHA256SUMS` asset, and 0.2.0 ships **no signature** …

**The design says the opposite.** §8.3a, line 1699: "the extension's only control is HTTPS to
`github.com` (the release-level `SHA256SUMS` asset is for CI and for a reader who verifies by
hand — the wasm never fetches it)". No step in §8.3a fetches that asset: step 3 is
`latest_github_release` → `download_file` → `make_file_executable`, and `download_file` takes a
URL, a destination and a file type, not a digest. R75 likewise says the release job "checksums
every archive at release level" — a producer statement, not a consumer check.

**Failure mode.** The ADR is the decision record a security reviewer and the release notes quote.
It tells them the editor verifies an integrity artifact that the wasm never requests. That is a
false control, and it is the same class of finding round 2 raised (M6); the design was corrected
and the ADR was not.

**Exact fix.** Replace the sentence with the design's: "The extension's only control is HTTPS to
`github.com`; the release-level `SHA256SUMS` is for CI and for a human who verifies by hand — the
wasm never fetches it; 0.2.0 ships no signature." One sentence, no new test.

---

## MINOR 1 — §11.2 still documents the removed `uv` error

Design §11.2, line 2454: "| `blastit` not installed | the extension's one-line error naming
`uv add blastit` | …". §8.3a step 4 says the error names the supported targets and
`lsp.blastit-lsp.binary.path`, and §8.3a's "no `uv`" row makes `uv` a red in the extension.
Fix: "the extension's one-line error naming the supported targets and
`lsp.blastit-lsp.binary.path`".

## MINOR 2 — S4 names `File::open`, which no list contains

Design §11.1 S4, line 2425: "the §1.2 grep for `std::fs`/`std::path`/`File::open`". `File::open`
is in neither §1.2's list nor §1.3 row 3. `File::open` is a path opener reachable with
`use std::fs::File;` (which row 3 catches via `std::fs`) or with a fully qualified
`std::fs::File::open` (also caught). The assertion text promises a token the guard does not have.
Fix: add `File::open` to row 3's token table with a negative twin, or delete it from S4's text and
name the two tokens that are checked.

## MINOR 3 — §7.2 prints four `..` and calls them three

Design §7.2, lines 1246-1249: "`include_str!("../../../../docs/reference/blastfile.md")` (three
`..`, because the file lives at `crates/blastit/src/lsp/{mod,hover}.rs`)". It is four today (five
path components from `crates/blastit/src/lsp` to the root), and three after the move to
`crates/blastit-lsp/src`. §1.3 obligation 1 has the correct old and new strings. Fix: delete "three
`..`" and state "four today, three after the move" once.

## MINOR 4 — §1.2 and §12.1 still describe the protected test with the deleted path

Design §1.2, line 188: "over `include_str!("../src/lsp/mod.rs")` (`the_lsp_module_is_a_text_tool`)";
§12.1, line 2514: "`include_str!("../src/lsp/mod.rs")` contains none of §11.1's list". After
`MB.35.4` the test reads `../../blastit-lsp/src/lib.rs` and `hover.rs`. §1.3 is correct; these two
descriptions are not. Fix: use the post-move literals in both cells, or say "the two literals
§1.3 obligation 1 names".

## MINOR 5 — "stale on four points" enumerates three, and `binary.env` is not one of them

Design §8.3a, line 1717: "**`MB.33`'s brief is stale on four points and this section wins.**" The
paragraph then lists capabilities, the download destination and the Windows
`make_file_executable` — three. `binary.env` is the fourth *security* correction (§14.4 lists it;
§8.3a says it twice elsewhere), and it is the loader-hijack vector
(`LD_PRELOAD`/`DYLD_INSERT_LIBRARIES`/`GCONV_PATH`). Fix: make the count five (adding `uv` and
`binary.env`) and list `binary.env` in the paragraph, quoting `MB.33` step 1.

## MINOR 6 — "an explicit path is the user's decision" is the sentence the project-settings vector contradicts

Design §8.3a, step 1, line 1663: "always first: an explicit path is the user's decision." The very
next paragraph says `for_worktree` merges the layers and "if a repository's `.zed/settings.json`
can name `binary.path`/`binary.arguments`, opening an untrusted clone can run the binary it
names". The step-1 rationale is the false half. Fix: "an explicit path wins because it is the
most specific setting, whether the user or a trusted project set it — the extension cannot tell
which".

## MINOR 7 — "It is not a trust boundary" is the wrong sentence for a process that parses a clone's bytes

Design §11, lines 2406-2408: "It is not a trust boundary, and this section's job is to prove it
never becomes one." The process holds no capability and (with the allowlist) no secret, which is
the intended claim, but it does parse bytes written by someone else with `libpg_query`'s
memory-unsafe C parser in the developer's uid. §11 and R76 already say the parser is C and no
advisory gate is claimed; the opening sentence should not flatten that. Fix: "It holds no
capability and no secret; the only boundary it crosses is a clone's bytes into a process running
as the developer. `libpg_query` is the one memory-unsafe parser in the image; 0.2.0 pins 6.2.1,
has no advisory gate (R76), and the process is unprivileged and env-cleared for that reason."

## MINOR 8 — R71/R74 still do not say the epic cannot pass before the guard exists

Design §14.1 R71, line 2943, makes `make lsp-portable` a prerequisite of `make check`; R74, line
2946, says the matrix "prints no `lsp` line" until `MB.35.5` adds the family. `MB.35.6` depends on
`MB.35.4, MB.35.5`, so the crate and the published editor files exist before the guard does.
Nothing says a stopped epic must not publish them. Fix: add to R71 "the epic's gate cannot pass
before `MB.35.6`, so a stopped epic never publishes an unguarded artifact" (the same shape R74
already uses for the matrix).

## MINOR 9 — the protected 13-token list omits `std::net`

Design §14.2, line 2957: "The protected `critical_lsp.rs`'s 13-token source grep … is deliberately
not widened". `crates/blastit/tests/critical_lsp.rs:2056-2069` has `std::os::unix::net` but not
`std::net`; a `TcpListener::bind` needs `std::net`. The scripts guard is not under
`tools/critical_guard.py`, so `std::net` is the one socket token that a commit with `scripts/**`
can remove without a plan authorization. Fix: add `std::net` to the protected array (one element,
same file `MB.35.4` already edits); this is not "widening" the grep.

## MINOR 10 — §8.3a says the accepted pages "keep the setting to `zed: open settings`", but the page still offers the project file

Design §8.3a, lines 1682-1684: "The accepted pages keep the setting to `zed: open settings` (per
user); the project-file alternative in `docs/reference/cli.md` `## blastit-lsp` is `MB.33`'s to
remove". `docs/reference/cli.md:135-136` currently says "`zed: open settings` for every project,
or a project's `.zed/settings.json`". "Keep" is false today, and the two clauses contradict each
other. This is a hand-off (§14.4 lists the page edit), so not a defect of the design's decision —
but the sentence should say "the page gains the removal" rather than "keeps".

---

## Verified true (checked, not taken on trust)

- **One `run()`, four literals.** `crates/blastit/src/lsp/hover.rs:294` and `:316` hold
  `include_str!("../../../../docs/reference/blastfile.md")` (four `..`); the only moving literals
  in `crates/blastit/tests/critical_lsp.rs` are `:2052-2053`
  (`../src/lsp/{mod,hover}.rs`). `:1419`, `:2036` and `:2093` are relative to the test's own
  location and do not move. §1.3's four-literal statement is exact.
- **Re-export seam.** `crates/blastit-manifest/src/lib.rs:108` is
  `pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};`, so the one added
  `parameterize` line is the same shape; `blastfile::read` at `mod.rs:270` is public (the basis of
  MAJOR 1).
- **The new suite is protected from its first commit.** `tools/critical_guard.py` protects
  `crates/*/tests/critical_*.rs` by glob; `crates/blastit-lsp/tests/critical_lsp_stdio.rs`
  matches.
- **The matrix has no `lsp` family today.** `grep -c lsp scripts/platform_matrix.py` is `0`; ADR-0010
  §4 and ADR-0013 decision 4 both say `MB.35.5` adds it, with its own count noun.
- **The five names and formats agree.** Design §8.3a's table, ADR-0010 §4's row and ADR-0013
  decision 4 use the same asset names, `GzipTar`/`Zip` split, top-level
  `blastit-lsp-<target>/` and glibc 2.28 / Debian 12 floor.
- **No page installs the editor with privileges.** `grep -rn 'setcap\|sudo '` over
  `docs/reference/cli.md`, `docs/local.md`, `docs/troubleshooting.md`,
  `docs/getting-started.md` and `editors/zed/README.md` returns nothing.
- **The old order is marked, not deleted.** §1.1 and §8.3 both carry a "Superseded 2026-11-18"
  paragraph that states the `--no-sync` finding and points at §1.3/§8.3a. The `uv` argv survives
  only inside the superseded record.
- **ARCHITECTURE §2.7 matches the design.** The row says "exactly `blastit-manifest` and
  `serde_json`", "Linux, macOS, Windows — five targets", "opens no file or socket **in its own
  code**", "supported unprivileged only", and names the linked manifest API and `libpg_query` as
  the reason the claim is about the code path.
- **ADR-0010 §4's editor row is consistent.** One executable inside one top-level directory,
  beside `LICENSE` and an in-archive `SHA256SUMS`, no shim, no `libphp`, no `libpython`, tar.gz on
  Linux/macOS and zip on Windows, and the same glibc floor.
- **The design fixed round 2's trust-root finding.** §8.3a line 1699 now says the wasm never
  fetches the release-level checksum; only the ADR retains the old sentence (MAJOR 3).

## Hand-offs recorded in §14.4 (not findings)

- `MB.35.4`'s `paths` do not yet include `crates/blastit-manifest/src/lib.rs` or
  `crates/blastit/tests/critical_lsp.rs`; §14.4 and §1.3 both say they must. The leaf is `ready`,
  so the lead applies this before it starts.
- `MB.35.5`'s packaging acceptance is still `tar -tzf` by eye; §14.4 adds the extraction
  assertion, the glibc floor and the matrix family.
- `MB.35.6`'s brief still has the ten-token list; §14.4 and R71 make §1.3 row 3 the contract.
- `MB.33`'s brief still merges `binary.env`, declares a `[[capabilities]]` block, uses an
  absolute-looking destination and a Windows-only `make_file_executable`; §14.4 lists all five
  corrections, including the CLI page's project-file alternative.
- `docs/reference/cli.md:135-136` still offers a project's `.zed/settings.json`; §14.4 gives the
  removal to `MB.33`, whose `paths` include that page. Until then the page recommends the exact
  vector §8.3a describes — a hand-off, not a design defect.

## Could not verify

- Whether `zed_extension_api` 0.7.0's `LspSettings::for_worktree` exposes layer provenance (the
  design says no; the API reference is not in this repo).
- Whether Zed re-prompts after a commit adds `lsp` keys to an already-trusted worktree. The
  design says no research fact establishes that project `lsp` keys are withheld, and it does not
  rely on that; §8.3a's "opening an untrusted clone" sentence understates a later commit on a
  trusted worktree, which is MINOR 6's neighbourhood rather than a new control gap.
- Whether `zed::download_file` performs any digest check (the design says no; the API reference
  is not in this repo, and MAJOR 3 is about the claim, not the API).
