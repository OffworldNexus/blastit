VERDICT: FAIL

# Security review, final pass — `MB.35.3` (portable `blastit-lsp`)

Scope: `docs/design/blastfile-lsp-zed.md` §1.3, §8.3a, §11, §14 and
`docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`'s `MB.35.3` amendment (the newest
amendment), against the round-3 report
`docs/design/portable-lsp-arch.review-security.round3.md` (0 BLOCKER, 3 MAJOR).

Reviewed in `.worktrees/release-0.2.0` at `143afaf` with the working-tree edits of the
`MB.35.3` amendment (`git diff`: +344 lines in the design, +207 in the ADR). Claims were
checked against `crates/blastit-manifest/src/blastfile/mod.rs`,
`crates/blastit/tests/critical_lsp.rs`, `crates/blastit/src/{cli,lsp/mod}.rs`,
`tools/critical_guard.py`, `docs/reference/cli.md`, `docs/local.md`,
`docs/troubleshooting.md`, `docs/getting-started.md` and `editors/zed/README.md`.

| Grade | Count |
|---|---|
| BLOCKER | 0 |
| MAJOR | 3 |
| MINOR | 9 |

## The three round-3 findings, one line each

| Round 3 | Now |
|---|---|
| **MAJOR 1** — the guard scanned only `lib.rs`/`hover.rs`; `std::fs`/`std::net` could be reached through a third module, a braced `std` import, or `blastfile::read` | **Half resolved.** §1.3 row 3 (line 255) now scans "**any** file under `crates/blastit-lsp/src/**`", so a third module is covered, and the crate-wide set is the right shape. The two *one-line* evasions are still not in the contract (MAJOR 1 below), and R73/S12/§14.4 still restate the two-file set (MAJOR 3). |
| **MAJOR 2** — `cfg` spellings were narrower than "one code path"; `forbid(unsafe_code)` presence was unasserted | **Half resolved.** `#[cfg(not(`, `target_family`, `cfg_attr`, `#[path` and `extern "C"` are now tokens, and row 3 makes `#![forbid(unsafe_code)]` **present** in `lib.rs` *and* `main.rs` "a positive check, so deleting the attribute is a red". The list is still an enumeration of spellings, so the property still has live holes (MAJOR 2). |
| **MAJOR 3** — ADR-0013 called the release-level `SHA256SUMS` an extension control | **Resolved.** ADR-0013:357-361 now reads "the extension's only control is HTTPS to `github.com` — the release-level `SHA256SUMS` asset is for CI and for a reader who verifies by hand, and the wasm never fetches it — and 0.2.0 ships **no signature**", matching §8.3a:1700 and R75:2955. One sentence, no residue. |

## MAJOR 1 — a braced `std` import and `blastfile::read` still pass row 3, so "opens no path / no socket / executes nothing" is still a grep you can step around in one line

**Quote.** Design §1.3 row 3, line 255: "no line in **any** file under
`crates/blastit-lsp/src/**` … contains any token of §1.2's list — `std::fs`, `std::path`,
`std::process`, `std::net`, …". Enforcement column: "line grep, one property per token
(matched as an identifier/path boundary, not an arbitrary substring …)".

**Two evasions, both green today.**

1. `use std::{fs, net};` in `lib.rs`, then `fs::read(uri)` or `net::TcpListener::bind(…)`.
   The tokens are the contiguous strings `std::fs` and `std::net`; the braced import
   contains neither, and rustfmt does not rewrite it. `use std::process;` +
   `process::Command::new(…)` is the same trick for "executes nothing".
2. `use blastit_manifest::blastfile;` then `blastfile::read(uri.as_ref())`. The public
   opener is `crates/blastit-manifest/src/blastfile/mod.rs:270`
   (`pub fn read(path: &Path) -> Result<String, ManifestErrors>`); `&str::as_ref()`
   supplies the `&Path`, so the call site names neither `std::path` nor `std::fs`. The
   linked crate is allowed, so no other rule fires. §1.3:259-268 discuss only the
   `parameterize` re-export; nothing in the revision names `blastfile::read`, a
   `use …::read as …` alias, or an allowlist of `blastit_manifest::` items.

**Failure mode.** A future "open the file the `uri` names" feature — the most natural LSP
feature there is — is one line, passes `make lsp-portable`, and falsifies S4
("A `Blastfile` is never opened"), S12 and R73. The artifact holds no capability, so this
is not a sandbox escape; it is a false invariant in the document a release reviewer
quotes, which is exactly the class round 3 raised.

**Exact fix.** Add `use std::{` (and `use core::{`) to row 3's token table with negative
twins; replace "tokens" for the manifest with a two-item allowlist —
`blastit_manifest::{blastfile::check, blastfile::fmt, parameterize}` plus the model types
the server names — with a twin that calls `blastfile::read("x".as_ref())` and must be red;
optionally add `File::open` to the same table (MINOR 1).

## MAJOR 2 — the `cfg` rule is still an enumeration, so a second behaviour on a shipped leg passes

**Quote.** Design §1.3 row 3, line 255: "nor `#[cfg(unix)]`, `#[cfg(windows)]`,
`#[cfg(not(`, `#[cfg(target_os`, `#[cfg(target_family`, `#[cfg_attr`, `#[path` …".
Why column: "one code path on three platforms: a `cfg` here is a second behaviour".

**Why it still fails.** These are one `cfg` each and contain none of the banned strings:

- `#[cfg(all(unix, target_arch = "aarch64"))] mod aarch64;` — live on
  `aarch64-unknown-linux-gnu` **and** `aarch64-apple-darwin`, two of the five shipped
  legs (ADR-0013:312-313), not a hypothetical build.
- `#[cfg(target_env = "msvc")]` (the Windows leg), `#[cfg(any(test, feature = "x"))]`,
  `#[cfg(all(not(unix), …))]`, and `if cfg!(windows) { … }` — `cfg!` is not a token at
  all in the LSP rule, though `scripts/check_zed_extension.py` asserts "no `cfg!`" for
  the extension (§8.3a:1702). The artifact has the weaker rule of the two.
- `[features] x = []` is not an "optional entry", so row 1 permits it; `#[cfg(feature = "x")]`
  then passes row 3.

**Failure mode.** The security-relevant shape is a per-arch branch that skips validation
or changes the diagnostic path — the ADR's own rejected alternative, "a validator that
validates nothing is worse than an absent one" (ADR-0013:325-326, :373). Behavioural
suites run on one host per leg and the five legs smoke the binary; a branch that is only
wrong on `aarch64-linux` can be green everywhere the tests run. Round 3's exact fix was
one blanket rule; the revision added four spellings instead, and the enumeration has
holes on shipped targets.

**Exact fix.** Row 3 bans **any `#[cfg(` except exactly `#[cfg(test)]`, and any `cfg!(`**,
in `crates/blastit-lsp/src/**`. If a legitimate `cfg` ever appears, it is one named
allowlist line in `scripts/check_lsp_portable.py`, not a token family. Twins:
`#[cfg(not(unix))]`, `#[cfg(all(unix, target_arch = "aarch64"))]`, `cfg!(windows)`.

## MAJOR 3 — R73, S12 and §14.4 still state the two-file set, and §14.4 is the text `MB.35.6`'s brief is written from

**Quote.** R73, line 2953: "no `std::fs`/`std::path`/`std::net`/`std::process`/`std::env`/
`include!` and no unsafe item or `extern "C"` in `lib.rs`/`hover.rs`". S12, line 2441:
"`src/lib.rs`/`src/hover.rs` hold no …". §14.4's `MB.35.6` row, line 3006: "§1.3's row 3
(… **with `lib.rs`/`hover.rs` as the file set** and `main.rs` pinned …)". Against row 3,
line 255: "any file under `crates/blastit-lsp/src/**` — i.e. `lib.rs`, `hover.rs`, and any
file a later change adds".

**Failure mode.** §14.4:2998-3000 says the leaf corrections "are recorded here" and "the
design wins", so the lead corrects `MB.35.6`'s brief from this row; the implementer then
writes a two-file scan and R73's proof is narrower than R73's own text. That is round 3's
evasion 1 (`mod helper;` + a clean `src/helper.rs`) restored in the one artefact that
becomes code. R71:2951 says "§1.3's third row", so the two statements conflict and the
weaker one is the one the leaf receives.

**Exact fix.** One clause each: R73 and S12 say "in `crates/blastit-lsp/src/**` (every
file; `main.rs` pinned)"; §14.4's `MB.35.6` row says the same and drops "`lib.rs`/`hover.rs`
as the file set". No test changes.

---

## Leftover MINORs (brief)

1. **S4:2433** still promises "the §1.2 grep for `std::fs`/`std::path`/`File::open`";
   `File::open` is in no list. Delete it or add it (MAJOR 1's table is the place).
2. **§7.2:1248** still says "`include_str!("../../../../docs/reference/blastfile.md")`
   (three `..`)" — it is four today, three after the move (§1.3 obligation 1 is correct).
3. **§1.2:188 and §12.1:2522** still describe the source grep over
   `include_str!("../src/lsp/mod.rs")`; §14.2:2963 says it reads
   `../../blastit-lsp/src/{lib,hover}.rs`. Use one spelling.
4. **§8.3a:1664** "always first: an explicit path is the user's decision" is the false
   half the next paragraph (1672-1678) contradicts: the extension cannot tell a user
   setting from a project one. Round 3's m6 wording is still unapplied.
5. **§8.3a:1683** "The accepted pages keep the setting to `zed: open settings`" —
   `docs/reference/cli.md:135-136` still offers a project `.zed/settings.json`. "keep" is
   still false; "gains the removal" is the honest verb.
6. **R71:2951** still does not say the epic's gate cannot pass before `MB.35.6`, so a
   stopped epic can publish an unguarded artefact (round 3's m8).
7. **§14.2:2965** calls the protected grep "13-token" and "deliberately not widened";
   `crates/blastit/tests/critical_lsp.rs` still has no `std::net` element (grep is empty),
   so the one socket token a `scripts/**` commit could drop is still unprotected.
8. **§11:2415** "It is not a trust boundary" survives although the paragraph now names
   `libpg_query`'s C parser; the intended claim is "no capability, no secret".
9. **S1:2430** says "No `[capabilities]`"; the manifest key everywhere else is
   `[[capabilities]]`. Cosmetic, but it is a grep target.

## Verified true (checked, not taken on trust)

- **ADR trust root fixed:** ADR-0013:357-361 vs §8.3a:1700 vs R75:2955 agree; no step
  fetches the release-level checksum.
- **`forbid(unsafe_code)` is now a positive assertion** on both `lib.rs` and `main.rs`
  (§1.3:255, ADR-0013:225-230), and the attribute is explicitly not a hit for the
  unsafe-item rule.
- **The new tokens are real:** row 3 gained `std::net`, `std::os::linux`, `std::os::windows`,
  `pyo3`, `nix`, `#[cfg(not(`, `target_family`, `cfg_attr`, `#[path`, `extern "C"`, and
  `#[cfg(test)]` is the one legal spelling.
- **The file set is crate-wide in the normative row**, so a module added under `src/` is
  scanned; row 5's identity rule and the new `critical_lsp_stdio.rs` are protected from
  their first commit by `tools/critical_guard.py`'s glob.
- **§8.3a still ignores `binary.env` and gives the child a `PATH`/`SYSTEMROOT` allowlist**
  (§8.3a:1679-1686, :1709-1716), and ADR decision 5 (:277-286) now says the same and states
  the layer limitation plainly.
- **No page installs the editor with privileges:** no `setcap` or `sudo` in
  `docs/reference/cli.md`, `docs/local.md`, `docs/troubleshooting.md`,
  `docs/getting-started.md`, `editors/zed/README.md`.
- **No new BLOCKER surface:** the artefact still links no privilege crate, `blastit lsp`
  keeps `hold_none()` before it reads a byte, and the download path's only stated control
  (HTTPS to `github.com`, no signature) is the one every earlier round recorded as an
  honest, stated gap.

## Could not verify

- Whether `cargo metadata --filter-platform` for a target the host lacks resolves the full
  platform closure; §1.3 row 2 hedges with "where the host resolves them". The unfiltered
  graph should contain every target-specific edge, but the guard's exact `--filter-platform`
  usage is `MB.35.6`'s to prove.
- Whether `zed_extension_api` 0.7.0's `for_worktree` exposes layer provenance (the design
  says no; the API reference is not in this repo).
