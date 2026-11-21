VERDICT: FAIL

# Security review, round 2 — `MB.35.3` (portable `blastit-lsp`)

Scope: `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (the `MB.35.3` amendment),
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/design/blastfile-lsp-zed.md` §1, §1.1, §1.3, §8.3, §8.3a, §11, §14, the requirement index,
`ARCHITECTURE.md` §2/§2.7. Round-1 is `docs/design/portable-lsp-arch.review-security.md`.

Reviewed in `.worktrees/release-0.2.0` (the four documents do not exist on the main checkout, which
is on `release/0.2.1`), at `143afaf`. Claims were checked against `crates/`, `scripts/`,
`tools/critical_guard.py`, `plan/nodes/MB/MB.35/**`, `docs/reference/cli.md`,
`.github/workflows/publish-pypi.yml` and the research notes.

The design text is materially better than round 1: the token list now names `std::fs`, `std::path`,
`std::net`, `std::process`, `include!`, `unsafe` and `extern "C"`; `forbid(unsafe_code)` is part of
the argument; `binary.env` is dropped and the child env is an allowlist; the project-settings layer
boundary is stated instead of implied; the trust root and the absence of a signature are written
down; and the editor's document is called untrusted data. What still fails is **ownership**: two of
the fixes exist only in this 3 000-line design, while the briefs that implement them still contain
the round-1 defect. A goldfish reading only the design would ship a safe guard; an implementer
reading its `NODE.md` ships the old one.

| Grade | Count |
|---|---|
| BLOCKER | 1 |
| MAJOR | 6 |
| MINOR | 7 |

---

## BLOCKER 1 — the guard that will actually be written is still round-1's weak guard (round-1 B1, reopened)

**Quote.** Design §1.3, line 246: "`MB.35.6` turns the five rows below into `make lsp-portable`, a
prerequisite of `make check`", and row 3 (line 253) lists the expanded token set
("`std::fs`, `std::path`, `std::process`, `std::net`, … `include!` … `unsafe`, or `extern "C"`",
plus `#![forbid(unsafe_code)]`).

**The brief that writes the script says otherwise.**
`plan/nodes/MB/MB.35/MB.35.6/NODE.md:32-34`:

> no `crates/blastit-lsp/src/**` line contains `std::os::unix`, `std::os::linux`,
> `std::os::windows`, `libc::`, `rustix`, `nix::`, `pyo3`, `#[cfg(unix)]`, `#[cfg(target_os`
> or `std::process::Command` (the LSP executes nothing);

and its mutation list (`:40-42`) is "`add rustix` / `use std::os::unix::fs` / `Command::new`".
Under that list each of these is **green** in `crates/blastit-lsp/src/lib.rs`:

- `std::fs::File::open("./Blastfile")` — the headline claim is "opens no path".
- `std::net::TcpListener::bind("0.0.0.0:0")` — "opens no socket"; `std::net` is not
  `std::os::unix`.
- `include!("../../blastit-sys/src/caps.rs")` — the included file is outside the scanned tree.
- `use std::process as p; p::Command::new("sh")` — only `std::process::Command` is named.
- `unsafe extern "C" { fn setresuid(..) -> i32; }` — `unsafe` and `extern "C"` are not grepped.

The compiler half is unowned too: `grep -n 'forbid\|unsafe' plan/nodes/MB/MB.35/MB.35.4/NODE.md`
returns nothing, so the leaf that creates the crate is never told to add
`#![forbid(unsafe_code)]`. The design already has the pattern for this problem — §8.3a
`:1705-1714` says "`MB.33`'s brief is stale on three points and this section wins" — but
`MB.35.6`'s brief, the one that implements the security guard, is not named anywhere as stale.

**Failure mode.** `MB.35.6`'s own acceptance is "`$ make lsp-portable`" and
"`$ scripts/test_check_lsp_portable.py`" with one mutation per property. Both pass with a guard
that cannot see the five bypasses above. The design's R71 ("each token with a negative twin")
is then true of the document and false of the repository, and the "opens no path, no socket,
executes nothing" sentence in §1.3/§11 S12/ARCHITECTURE §2.7 rests on it.

**Exact fix.** In §1.3, add the same paragraph §8.3a has for `MB.33`: "`MB.35.6`'s brief is stale
on the token list; §1.3's third row wins, verbatim", and add the token table to
`MB.35.6`'s `NODE.md` as a list (not prose). Add "`#![forbid(unsafe_code)]` on `src/lib.rs` and
`src/main.rs`" to `MB.35.4`'s Goal. Treat both `NODE.md` edits as a precondition of this node's
acceptance (or file them as `plan.py` follow-ups this node owns); a design sentence that says the
plan "must" change is not a change.

---

## MAJOR 1 — the leaf briefs still forbid the file edits the design says they make (round-1 M1, M2)

**Quote.** Design §1.3, lines 262-263: "**`MB.35.4` owns it and its `paths` must be widened to name
that file**"; R70 (line 2930): "which is why that file joins `MB.35.4`'s `paths`".

**Repo.** `MB.35.4`'s `paths` (lines 9-15) are `crates/blastit-lsp/**`,
`crates/blastit/src/lsp/**`, `crates/blastit/src/lib.rs`, `crates/blastit/Cargo.toml`,
`Cargo.toml`, `Cargo.lock` — neither `crates/blastit-manifest/src/lib.rs` nor
`crates/blastit/tests/critical_lsp.rs`. Its Context says the protected suite "stays
**byte-identical**" (`:50`) and "**must not be edited** — it is not in `paths`, deliberately"
(`:62`).

**Why it fails.** `cargo test -p blastit --test critical_lsp` is in that leaf's acceptance block.
After the `git mv`, the two `include_str!("../src/lsp/{mod,hover}.rs")` lines at
`crates/blastit/tests/critical_lsp.rs:2051-2054` do not resolve; the fix is the one mechanical
edit the design authorizes, and `tools/critical_guard.py` refuses it because the path is not in
`paths`. The implementer's remaining options are a second `crates/blastit/src/lsp/` copy (which
defeats the one-`run()` identity rule) or deleting the grep test — both are the security
regression round 1 was written to prevent. The ADR's "this amendment records the correction
rather than leaving the leaf to choose" (lines 243-245) does not record a correction; it records a
wish.

**Exact fix.** Add `crates/blastit-manifest/src/lib.rs` and `crates/blastit/tests/critical_lsp.rs`
to `MB.35.4`'s `paths`, replace "byte-identical" with "assertion bodies unchanged; only the two
`include_str!` path literals change", and delete "not in `paths`, deliberately". Do it with
`plan.py` in this node's commit or name it as an acceptance precondition.

## MAJOR 2 — the move breaks two more `include_str!` literals than R70 admits (new)

**Quote.** Design §1 row 10 (line 106): "`src/hover.rs` unchanged"; R70 (line 2930): "only its two
`include_str!` path literals follow the move"; ADR-0013 lines 250-252: "only the two `include_str!`
path literals follow the files the move relocates".

**Repo.** `crates/blastit/src/lsp/hover.rs:294` and `:316` (both inside `#[cfg(test)] mod tests`):

```rust
let page = include_str!("../../../../docs/reference/blastfile.md");
```

From `crates/blastit/src/lsp/` four `..` reach the repo root. From the new location
`crates/blastit-lsp/src/`, four `..` reach **above** the repository. The literal must become
`../../../docs/reference/blastfile.md`.

**Why it fails.** `cargo build -p blastit-lsp --locked` passes (the literal is test-only), but
`cargo test -p blastit-lsp` — the fourth command of `MB.35.4`'s acceptance — fails to compile. The
R70 oracle ("every changed line is one of those two literals") is blind to `hover.rs`, so the
implementer must decide unaided whether changing it violates "unchanged". Also note §7.2
(lines 1244-1247) says "three `..`" while printing four: the sentence is wrong today and
accidentally right after the move, which is how a reader gets this wrong twice.

**Exact fix.** R70/§1.3 obligation 1/ADR decision 3: "four `include_str!` path literals — the two
in `critical_lsp.rs` and the two in `hover.rs` (`../../../../docs/…` → `../../../docs/…`)"; add
`hover.rs`'s two lines to the diff oracle; rewrite §7.2 to name the literal explicitly and say
"the count changes with the move".

## MAJOR 3 — the protected oracle keeps the 13-token list, so 11 of the 24 tokens are unprotected (new)

**Quote.** Design §1.3 obligation 1 (lines 274-275): "The **assertion bodies and the property
lists** of `crates/blastit/tests/critical_lsp.rs` stay unchanged". §11.1 (lines 2423-2424):
"**§1.2's grep list, verbatim** (one assertion, one list, used by R3 /
`the_lsp_module_is_a_text_tool`)" — followed by 13 tokens ending at `include!`.

**Repo.** `crates/blastit/tests/critical_lsp.rs:2056-2069` is that 13-token list plus
`fn lex`/`fn parse`/`fn lower`.

**Why it fails.** The 11 tokens round 1 added — `std::net`, bare `std::os::unix`, `std::os::linux`,
`std::os::windows`, `nix`, `pyo3`, `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os`, `unsafe`,
`extern "C"` — are enforced **only** by `scripts/check_lsp_portable.py`. `tools/critical_guard.py`
protects `**/tests/critical/**` and `crates/*/tests/critical_*.rs` (`:8-9`, `is_protected`), not
`scripts/**`. So the one test that cannot be weakened without a plan authorization keeps the
weaker list, while the list that carries the security argument can be edited by any node with
`scripts/**` in its paths. §11.1 then calls the weaker list "one list", which invites exactly the
reading that the file already enforces §1.3's row.

**Exact fix.** Since `MB.35.4` already edits `critical_lsp.rs` for the two literals, have it
replace the array with §1.3 row 3's token table (same commit, `critical_guard.py` sees the
declared change). Delete §11.1's verbatim 13-token block or mark it "the `MB.22` list, superseded
by §1.3"; S12 already points there.

## MAJOR 4 — the project-settings execution path is real, uncited, and trust is sticky (round-1 B2, downgraded from BLOCKER)

**Quote.** §8.3a, lines 1661-1666: "`for_worktree` merges the **user** and **project** layers …
opening an untrusted clone can run the binary it names — Zed's own trust model for `lsp` settings
… not something a wasm extension can distinguish."

**What is true.** Zed does now have that trust model: worktree trust / Restricted Mode
(`zed.dev/docs/worktree-trust`, shipped Dec 2025) withholds project `.zed/settings.json`,
language-server installation and spawning until the user trusts the worktree. So round 1's
condition ("cite it in §8.3a and keep the project row") is satisfiable. The design does not cite
it, and the only note in the repo that touches the question — `docs/research/2026-11-16-zed-
blastfile-lsp.md:631` — says it is "not verified whether a project `.zed/settings.json` can
enable" the setting. A security argument that rests on an unverified platform behaviour and does
not name its source is the same class of finding round 1 rejected.

**What the design still needs to say.** (a) Trust is persistent per worktree: a repository
trusted months ago can add `binary.path`/`binary.arguments` in a later commit and run an arbitrary
program on the next open, with no new prompt. (b) Within a trusted worktree, all the env hardening
in §8.3a buys nothing against `/bin/sh -c …`. (c) `docs/reference/cli.md:135-136` still says
"`zed: open settings` for every project, **or a project's `.zed/settings.json`**", so §8.3a's
"The accepted pages keep the setting to `zed: open settings` (per user)" (lines 1670-1671) is
false of the page at this commit.

**Exact fix.** Cite the trust doc and add the sticky-trust caveat; correct the sentence about the
page (or land the page edit — `docs/reference/*.md` is in `MB.33`'s `paths`, so it is not a
follow-up that needs a new node). If the vector is to be closed rather than trusted: honour
`LspSettings` only when the user's shell environment opts in (for example
`BLASTIT_LSP_SETTINGS=1` in `worktree.shell_env()`); a project cannot set that.

## MAJOR 5 — `MB.33`'s brief still mandates the `binary.env` merge the design removes (new, round-1 B2 sub-vector)

**Quote.** §8.3a, lines 1666-1669: "`binary.env` is not honoured … the child gets an **allowlist**,
`PATH` plus `SYSTEMROOT` on Windows".

**Repo.** `MB.33`'s `NODE.md`, step 1: "`binary.env` is merged **over** `worktree.shell_env()`.
The returned `zed::Command` carries `env = shell_env + binary.env`." §8.3a's stale-brief paragraph
(lines 1705-1714) lists three disagreements (capabilities, download destination, Windows
`make_file_executable`) and not this one, so an implementer reads the list as exhaustive.

**Why it fails.** Within a trusted worktree, a later commit sets `LD_PRELOAD`/`DYLD_INSERT_LIBRARIES`
for a process that parses untrusted `libpg_query` C with the developer's environment. Blastit's
own `ALWAYS_STRIP` treats those names as loader hijacks; the leaf brief hands them over.

**Exact fix.** Add a fourth stale point quoting that sentence, and correct `MB.33` step 1 to
"`binary.env` ignored; `env` is the `PATH`/`SYSTEMROOT` allowlist", with `lib: env allowlist` in
its acceptance (the design already specifies the property).

## MAJOR 6 — the release-level `SHA256SUMS` is called a control the extension never consults (round-1 M4, residual)

**Quote.** §8.3a, line 1688: "**Trust root, stated:** the `SHA256SUMS` inside an archive … authenticates
nothing; the extension's controls are HTTPS to `github.com` **and the release-level `SHA256SUMS`
asset**"; ADR-0013 lines 345-348 repeats it.

**Why it fails.** No step in §8.3a fetches that asset: step 3 is `latest_github_release` →
`download_file` → `make_file_executable`. `zed::download_file` takes a URL, a destination and a
file type — no digest — and §14.2 has no verification row. The extension's actual controls are TLS
to `github.com` and GitHub account/CI security; the checksum file is for the human who downloads
the archive by hand. The honest "0.2.0 ships no signature" sentence is weakened by promoting an
unconsulted file to "control".

**Exact fix.** R75/§8.3a/ADR: "the release-level `SHA256SUMS` is for human verification; the
extension verifies nothing beyond TLS and GitHub". If verification is wanted, specify an
`http_request` + sha256 step and its test; otherwise say the gap is accepted.

---

## MINOR 1 — `#![forbid(unsafe_code)]` on one file or two?

ADR-0013 lines 227-229: "plus `#![forbid(unsafe_code)]` on both the library and the binary".
Design §1.3 line 253: "`#![forbid(unsafe_code)]` in `src/lib.rs`", with `main.rs` "**outside** this
rule and pinned instead to its three-line shape". An attribute line makes `main.rs` four lines, so
the two statements cannot both be acceptance-checked. Fix: say "`src/main.rs` is the three
statements in `fn main`; the crate-level attributes are not counted", or drop the binary from the
ADR.

## MINOR 2 — two of the new tokens are substring traps

`nix` matches "unix" (`u·nix`); `libc` matches "libclang", which the ADR and research texts use.
Today's `lsp/mod.rs` and `hover.rs` are clean (`0` hits for all 11 new tokens — verified), so this
is latent, not current. Fix: match `nix::`/`nix` with a word boundary and `libc` as
`libc::|libc;|libc =` (or `\blibc\b`), and add one prose negative twin ("a comment saying
`libclang` is not the `libc` crate").

## MINOR 3 — `#[path]` and `build.rs` are outside the "no line in `lib.rs`/`hover.rs`" rule

`#[path = "../../blastit-sys/src/caps.rs"] mod caps;` contains none of the tokens and pulls code
from outside the scanned tree — the same class as the `include!` round 1 closed. A `build.rs` also
needs no `[build-dependencies]` entry, so row 1 does not see it. Fix: add `#[path` to the token
list and one rule "`crates/blastit-lsp/build.rs` does not exist" (with a negative twin).

## MINOR 4 — S4's oracle names a token no list contains

§11.1 S4 (line 2413): "the §1.2 grep for `std::fs`/`std::path`/`File::open`". `File::open` is in
neither list. Either add it to the token table or drop it from the assertion text.

## MINOR 5 — §7.2's `include_str!` sentence contradicts itself

Lines 1244-1247 print `../../../../docs/reference/blastfile.md` and call it "three `..`". It is
four today and three after the move (MAJOR 2). State the literal and the reason once.

## MINOR 6 — the ADR's summary of the token set reads as exhaustive

ADR-0013 lines 331-333: "`MB.35.6` also forbids `std::process`, `std::net`, `std::fs`,
`std::path`, `include!` and `unsafe`" — omits `extern "C"`, `std::env` and the `#[cfg]` tokens.
Add "(the full list is `§1.3`'s third row)".

## MINOR 7 — the guard lands after the artifact is built

`MB.35.6` depends on `MB.35.4, MB.35.5`, so the crate and the archives exist before
`make lsp-portable` does. R74 states the matrix is the obligation until `MB.35.5` adds the family;
R71 should likewise say the epic's gate cannot pass before `MB.35.6`, so a stopped epic never
publishes an unguarded artifact.

---

## Verified true (checked, not taken on trust)

- **Re-export seam:** `crates/blastit-manifest/src/lib.rs:108` is
  `pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};`; `parameterize` is
  `pub` at `crates/blastit-sqlpolicy/src/template.rs:333` and re-exported at
  `crates/blastit-sqlpolicy/src/lib.rs:100`; `crates/blastit/src/lsp/mod.rs:55` imports
  `blastit_sqlpolicy::parameterize`. One added line in the manifest lib makes the two-dependency
  rule compile (round-1 M1's mechanism is sound).
- **Protected suite shape:** `crates/blastit/tests/critical_lsp.rs:2051-2054` has exactly the two
  `include_str!` paths quoted in R70; `:2035-2046` reads `cli.rs` for `hold_none()` before
  `lsp::run()`; `cli.rs:579` is `hold_none()`, `:586` is `crate::lsp::run()`.
- **New protected suite needs no guard listing:** `tools/critical_guard.py` protects
  `crates/*/tests/critical_*.rs` by glob, so `crates/blastit-lsp/tests/critical_lsp_stdio.rs` is
  protected from its first commit. Round-1's "add it to `critical_guard.py`" is unnecessary.
- **The expanded grep passes on today's code:** `crates/blastit/src/lsp/{mod,hover}.rs` contain
  `0` occurrences of `std::net`, `std::os::unix|linux|windows`, `nix`, `pyo3`, `#[cfg(unix)]`,
  `#[cfg(windows)]`, `#[cfg(target_os`, `unsafe`, `extern "C"`, `libc`. Applying row 3 to the moved
  files does not fail on content that is already there.
- **`forbid(unsafe_code)` is a real second half:** the workspace is `edition = "2024"`
  (`Cargo.toml:7`), where an extern block is `unsafe extern`, so the attribute and the grep agree
  (`forbid` refuses the block; the grep refuses the literal).
- **Matrix ownership:** `grep -c lsp scripts/platform_matrix.py` is `0`; ADR-0010 §4 and ADR-0013
  decision 4 now say the family is added "in `MB.35.5`" (round-1 m1 fixed).
- **Asset contract:** the five names, formats and the `blastit-lsp-<target>/` layout in §8.3a,
  ADR-0010 §4 and ADR-0013 decision 4 are identical; ADR-0010's glibc 2.28 / `manylinux_2_28` /
  Debian 12 row matches §8.3a's.
- **Release mechanics:** `.github/workflows/publish-pypi.yml` renames `dist/SHA256SUMS.<target>`
  (~299-301) and writes one release-level sorted `SHA256SUMS` (~782-805);
  `scripts/build-standalone.sh:328-338` hashes the staged files, excluding `SHA256SUMS` itself, so
  the "in-archive sums authenticate nothing" sentence is accurate.
- **ARCHITECTURE:** §2 now says "A single **serving** Rust binary … A second binary, `blastit-lsp`,
  is the **editor** surface"; §2.7 has the crate row ("exactly `blastit-manifest` and
  `serde_json`", five targets), "opens no file or socket **in its own code**", "supported
  unprivileged only", and the `libpg_query`-in-the-image caveat. Round-1 M3 is addressed.
- **No privileged install path in the pages:** `grep -rn 'setcap\|sudo ' docs/reference/cli.md
  docs/local.md editors/zed/README.md` returns nothing.
- **No new asset claim:** §11's first paragraph and R76 now call the editor's `Blastfile` untrusted
  data with exactly two parsers; the linked `blastit-manifest` file API (`pub mod edit`,
  `blastfile::read`) is real, so the "in the image" wording is conservative and correct.
- **Zed trust exists:** `zed.dev/docs/worktree-trust` documents Restricted Mode withholding
  project settings, LSP installation and spawning until trust; that is the citation MAJOR 4 asks
  for (the design's claim is true, just unsourced and missing the sticky-trust caveat).

**Could not verify:** whether `zed_extension_api` 0.7.0's `LspSettings::for_worktree` exposes layer
provenance (the design says no; the API reference is not in this repo), and whether Zed re-prompts
after a commit adds `lsp` keys to an already-trusted worktree (the trust doc says decisions persist;
nothing describes a per-commit re-gate — the basis for MAJOR 4's caveat).
