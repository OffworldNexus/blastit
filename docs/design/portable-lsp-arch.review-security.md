VERDICT: FAIL

# Security review — `MB.35.3` (portable `blastit-lsp`, ADR-0013 amendment + design §1.3/§8.3a/§11 S12/§14)

Scope: `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (newest amendment),
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/design/blastfile-lsp-zed.md` §1/§1.3/§8.3/§8.3a/§11 S12/§14, `ARCHITECTURE.md` §2/§2.7.
Reviewed in `.worktrees/release-0.2.0` (the four documents do not exist on the main checkout).
Every claim below was checked against `crates/`, `scripts/`, `Cargo.lock`, the Zed guard, and the
research note; "could not verify" is stated where it applies.

The amendment's *direction* is right: extraction is the only honest way to make the closure
portable, and one `run()` behind two binaries is the correct equivalence obligation. What fails
is the **proof**: the new guard does not enforce most of the properties the security argument
rests on, one accepted safety finding (cloned-repository code execution) returns through step 1
of the resolution order, and the `setcap`/release-integrity claims are asserted rather than
checked.

---

## BLOCKER 1 — `make lsp-portable` does not enforce "opens no path, no socket, executes nothing"

**Quote.** Design §14.1 R71:

> the source greps (`std::os::unix/linux/windows`, `libc::`, `rustix`, `nix::`, `pyo3`,
> `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os`, `std::process::Command`, `std::env`) —
> each with a negative twin

and §1.3:

> `Command`/`env` are how "executes nothing" and "reads no environment" would break.

**Why it fails.** The list R71 makes normative drops five tokens that the *existing* protected
list contains — `crates/blastit/tests/critical_lsp.rs:2056-2069` checks
`std::fs`, `std::path`, `std::process`, `std::os::unix::net`, and `include!` (plus `fn lex`,
`fn parse`, `fn lower`). With R71 as written, each of these compiles green in
`crates/blastit-lsp/src/`:

- `std::fs::File::open(...)` — the artifact's headline claim is "opens no path" (§1.3, ARCHITECTURE
  §2.7, ADR-0010 §4).
- `std::net::TcpListener::bind(...)` — "opens no … socket"; `std::net` is not `std::os::unix`.
- `include!("../../blastit-sys/src/caps.rs")` — the guard greps only lines under
  `crates/blastit-lsp/src/**`; a line containing `include!` is not in the list, and the included
  file is outside the scanned tree.
- `use std::process as p; p::Command::new("sh")` — only the literal `std::process::Command` is
  banned, so a one-line alias spawns a child.
- `extern "C" { fn setresuid(ruid: u32, euid: u32, suid: u32) -> i32; }` + `unsafe` — the ADR's
  reason ("because no crate that wraps them is in its graph", ADR line 272) is not a property of
  Rust source: libc is already in the process (see MAJOR 3), and an FFI declaration needs no
  crate.

So the four security sentences in §1.3 ("links **no privilege crate** … no code in the image can
call `capset`, `setresuid`, `seccomp`, `fork` or `execve` … **cannot become a serving binary by
accident**") are, for the bypass classes above, asserted. The closure check proves the *crate*
claim; nothing proves the *source* claim.

**Exact fix.** In design §1.3 (table, row 3) and §14.1 R71, make the rule §1.2's list verbatim
over `crates/blastit-lsp/src/**`: "no line contains `std::fs`, `std::path`, `std::net`,
`std::os::unix`, `std::os::linux`, `std::os::windows`, `std::process` (the single
`std::process::exit` line in `main.rs` is the named exception), `libc`, `rustix`, `nix`, `pyo3`,
`blastit_sys`, `crate::stage0`, `crate::stage1`, `crate::host`, `crate::config`, `std::env`,
`include!`, `#[cfg(unix)]`, `#[cfg(windows)]`, `#[cfg(target_os`, `unsafe`, or `extern "C"`",
add `#![forbid(unsafe_code)]` to `lib.rs` and `main.rs`, and add one negative twin per token in
`scripts/test_check_lsp_portable.py`. Add the sentence to R73: "the grep is the enforcement of
'opens no path, no socket, executes nothing'; the closure check is only the crate half."

## BLOCKER 2 — project-controlled `.zed/settings.json` still executes a cloned repository's code, and injects the loader environment

**Quote.** Design §8.3a, step 1:

> `zed::settings::LspSettings::for_worktree("blastit-lsp", worktree)`'s `binary.path` — an
> unpacked archive, `target/release/blastit-lsp`, or the Linux `blastit` … `binary.env` merged
> **over** `worktree.shell_env()` … always first: an explicit path is the user's decision.

**Why it fails.** `for_worktree` is the *worktree* (project) layer: a repository can commit

```json
{ "lsp": { "blastit-lsp": { "binary": { "path": "/bin/sh", "arguments": ["-c", "…"] } } } }
```

and opening a `Blastfile` in that clone runs it. That is exactly the class ADR-0013 decision 6
closed for `uv` ("opening a cloned repository's `Blastfile` would execute that repository's build
code", line 74), and the amendment keeps the heading "The extension resolves the server **without
executing repository code**" (line 68) while reintroducing the same outcome through a different
key. It is not "the user's decision": the user decided to open a folder, not to run its shell.

The same row merges `binary.env` over `shell_env()`. A project can therefore set
`LD_PRELOAD`/`DYLD_INSERT_LIBRARIES` (or `LD_AUDIT`, `GCONV_PATH`) for the child; Blastit's own
barrier treats those as loader hijacks (`ADR-0010` §1.3 `ALWAYS_STRIP`) and strips them from every
worker. The editor path hands them over unfiltered, then also hands the full shell environment to
a process that "reads none of it" (§8.3a, last paragraph) — pure downside: a parser bug in
`libpg_query`/`serde_json` inherits every token the developer exports.

`docs/reference/cli.md` (`## blastit-lsp`) makes this worse by inviting the project file: "name
its absolute path … in your settings: `zed: open settings` for every project, **or a project's
`.zed/settings.json`**".

**Exact fix.** In §8.3a step 1 and §8.3, replace the sentence with: "Step 1 reads the **user**
settings layer only; a worktree's `.zed/settings.json` may not select `binary.path`,
`binary.arguments` or `binary.env`. The child receives an allowlist — `PATH`, and `SYSTEMROOT` on
Windows — never `shell_env()` and never `binary.env`; `LD_*`, `DYLD_*`, `GCONV_PATH` and
`BLASTIT_*` are stripped." Delete the project-file alternative from
`docs/reference/cli.md`/§8.3a. If Zed gates worktree settings behind an explicit trust prompt,
cite it in §8.3a and keep the project row; nothing in the research note establishes that, so
today the safe rule is user-only. Add one `check_zed_extension.py` property:
`lib: env allowlist` (no `shell_env()` assignment to `Command.env`; the allowlist is a literal).

## MAJOR 1 — the two-dependency rule is true only after a re-export that no leaf owns

**Quote.** Design §1.3:

> That is one added line in `crates/blastit-manifest/src/lib.rs`, part of `MB.35.4`'s paths.

**Why it fails.** It is not. `MB.35.4`'s `paths` are `crates/blastit-lsp/**`,
`crates/blastit/src/lsp/**`, `crates/blastit/src/lib.rs`, `crates/blastit/Cargo.toml`, `Cargo.toml`,
`Cargo.lock` — `crates/blastit-manifest/src/lib.rs` is absent, and MB.35.3 may not write Rust. The
real import today is `use blastit_sqlpolicy::parameterize;`
(`crates/blastit/src/lsp/mod.rs:55`), so a literal `git mv` leaves `blastit-lsp` needing a third
direct dependency; R69/`make lsp-portable` then goes red and the implementer must either edit a
file outside its lane or break the invariant.

The *mechanism* is sound: `parameterize` is public (`crates/blastit-sqlpolicy/src/lib.rs:100`), and
`crates/blastit-manifest/src/lib.rs:108` is the existing `pub use blastit_sqlpolicy::{Columns,
TableGrant, TableGrants};`, so one added name makes `use blastit_manifest::parameterize;` work and
the two-dep claim true. What is missing is the ownership, not the design.

**Exact fix.** Add `crates/blastit-manifest/src/lib.rs` to `MB.35.4`'s `paths`, and in §14.1 R69
write: "`blastit-manifest` re-exports `parameterize` (one line in `lib.rs`, owned by `MB.35.4`)
before the extraction, so `crates/blastit-lsp/Cargo.toml` has exactly two direct dependencies."
Keep ADR-0013 decision 2 as is once the path exists.

## MAJOR 2 — R70's "byte-identical" protected suite cannot compile after the move

**Quote.** Design §14.1 R70:

> the protected `critical_lsp.rs` runs **byte-identical** against `blastit lsp` | … `git diff
> --exit-code -- crates/blastit/tests/critical_lsp.rs`

**Why it fails.** `crates/blastit/tests/critical_lsp.rs:2051-2054` compiles the module in:

```rust
let sources = [
    include_str!("../src/lsp/mod.rs"),
    include_str!("../src/lsp/hover.rs"),
];
```

`MB.35.4` `git mv`s both files into `crates/blastit-lsp/src/{lib,hover}.rs` and deletes
`crates/blastit/src/lsp/`. The protected suite then fails to compile — and `MB.35.4` cannot fix it
(the file is protected and not in `paths`). `cargo test -p blastit --test critical_lsp`, the
acceptance command, fails for a reason unrelated to behaviour. The security consequence is that
the one grep that proves "the LSP module is a text tool" would have to be deleted or moved under
time pressure, which is how the BLOCKER 1 regression becomes permanent.

**Exact fix.** Reword §1.3's equivalence obligation 1 and R70 to: "the assertion bodies of
`critical_lsp.rs` stay unchanged; the two `include_str!` paths and the `the_lsp_module_is_a_text_tool`
property list move with the code into `crates/blastit-lsp/tests/critical_lsp_stdio.rs`." Add both
test files to `MB.35.4`'s `paths`, and add the new file to `tools/critical_guard.py` in the same
commit. `git diff --exit-code` cannot be an acceptance line for a file whose include targets move.

## MAJOR 3 — `setcap`/root is asserted harmless, and the argument is false at the image level

**Quote.** ADR-0013, "What the old comparison table's costs became":

> **the portable image holds none to drop.** … a `setcap` on the portable binary would grant a
> process that opens no path, no socket and no child

and §1.3:

> no code in the image can call `capset`, `setresuid`, `seccomp`, `fork` or `execve`

**Why it fails.** Three separate errors.

1. "holds none to drop" is an installation assumption, not a property. `sudo blastit-lsp` (a
   realistic debugging move) or a distro/`setcap cap_setuid+ep blastit-lsp` runs the server with
   privileges. `blastit lsp` clears them (`blastit_sys::caps::hold_none()` → `capset(0,0,0)` then
   verify, `crates/blastit-sys/src/caps.rs:598`); `blastit-lsp` has no such call, so the same
   server has two different privilege postures and the portable one fails **open**.
2. "opens no path" is false of the image: `blastit-manifest` — linked by design — contains
   `edit.rs` (`std::fs::{File, OpenOptions}`, `rename`, `remove_file`) and `blastfile::read`
   (`File::open`, `metadata`). Unreachable from `run()` today; present in the process, reachable
   from any memory-safety bug or future caller.
3. "no code in the image can call … `fork` or `execve`" is false at the image level: `std` links
   libc; the `libc` *crate* absence in the cargo graph is not the absence of libc. The process
   parses the developer's SQL with `libpg_query` (a C parser, 42 packages in the normal closure),
   which is precisely the kind of code a privileged process must not parse.

**Exact fix.** In ADR-0013's row, §1.3's four sentences, §11 S12 and ADR-0010 §4's row, replace
"a `setcap` on the portable binary would grant a process that opens no path" with: "`blastit-lsp`
is supported unprivileged only. Its own code opens no path; the linked manifest crate's file API
and `libpg_query` are in the image. `MB.35.5` asserts `getcap` is empty for every Linux archive, the
docs never install it with `setcap` or `sudo`, and a Linux-only startup check refuses
`geteuid() != getuid()` (one reviewed `extern "C"` exception, covered by the same guard and a
test); `blastit lsp` keeps `hold_none()`." If the check is judged too costly, keep the sentence
but say explicitly "`sudo`/`setcap` use is unsupported and not defended" — do not claim the
privilege would be inert.

## MAJOR 4 — release-asset integrity is not verified; the shipped `SHA256SUMS` authenticates nothing

**Quote.** Design §8.3a:

> each asset … is `blastit-lsp-<target>.tar.gz` … the archive's one top-level directory is
> `blastit-lsp-<target>/` holding the executable, `LICENSE` and `SHA256SUMS`

and ADR-0013 Consequences:

> the release job's checksum rule and the `dist/*` upload stay the only publishing step

**Why it fails.** `scripts/build-standalone.sh:332-338` writes `SHA256SUMS` *inside* the archive,
excluding itself — a consumer who has the archive already has the file it would verify. The
useful artefact is the **release-level** `SHA256SUMS` asset written by
`.github/workflows/publish-pypi.yml:782-805`, and the extension never fetches or checks it:
`zed::download_file` extracts straight into the work directory and
`make_file_executable` runs the result. The trust root is therefore TLS to `github.com` and
GitHub account security alone — no signature, no pinned hash, no notarization/Authenticode
decision anywhere in §8.3a/§14, even though ADR-0013's rejected-alternative table still describes
the artifact as one "to build, **sign**, publish". The macOS quarantine story exists only in
`docs/reference/cli.md` ("A macOS browser download is quarantined: `xattr -d
com.apple.quarantine blastit-lsp`") and does not cover the extension's own download, which never
gets the xattr.

**Exact fix.** In §8.3a add a **Trust root** row: "The in-archive `SHA256SUMS` is for a human who
unpacked by hand; it does not authenticate the download. The extension's controls are HTTPS to
`github.com` and the release-level `SHA256SUMS` asset; there is no signature in 0.2.0." Add a
`MB.35.5` acceptance line (`tar -tzf` → exactly `blastit-lsp`, `LICENSE`, `SHA256SUMS`; `getcap`
empty), and either add a detached signature (minisign/sigstore) checked by the release or record
the acceptance in §14.3 in one sentence. Move the quarantine sentence into §8.3a/§14 as the
macOS install caveat, since the CLI page already promises it.

## MAJOR 5 — "a `Blastfile` is policy, not input" does not survive a cloned repository on macOS/Windows

**Quote.** Design §11:

> A `Blastfile` is policy, not input: whoever can write it can already grant `unrestricted`
> … The editor therefore inherits the manifest's threat model and adds no new asset.

**Why it fails.** That sentence is true for the **boot**: the manifest applied to a server is
written by the deployer. The editor opens manifests the developer has not applied — from a clone,
a review, a downloaded archive — which are attacker-controlled *data* to this process. The
reachable parsers are now `blastit-manifest` (including `regex`) and `libpg_query`'s C parser, in a
process with the developer's environment (BLOCKER 2) and no seccomp/uid boundary (MAJOR 3). Saying
"adds no new asset" is the one conclusion the amendment's own premise ("a text tool on
macOS/Windows") invalidates.

**Exact fix.** Replace §11's opening paragraph with: "In the boot a `Blastfile` is policy; in the
editor it is untrusted data. The only parsers are `blastit-manifest`'s (safe Rust) and
`libpg_query`'s (C, parse-only, never executed); `MB.35.6` records the 42-package normal closure
and a `cargo audit`/`cargo deny` row for it, and `pg_query` stays pinned." Add one R to §14.1:
"the portable artifact's runtime closure has no advisory open at release time" with the command
as proof.

## MINOR 1 — `platform_matrix.py` has no `lsp` family yet, but ADR-0013 states it in the present tense

ADR-0013 decision 4: "the counts and the runner per leg **are declared** once in
`scripts/platform_matrix.py`'s `lsp` family". Verified: `grep -n lsp scripts/platform_matrix.py`
returns nothing; `ADR-0010` §4 correctly says "**gains** the `lsp` family in `MB.35.5`". Fix
ADR-0013 to "are declared once, in `MB.35.5`, in …'s `lsp` family".

## MINOR 2 — `MB.33`'s brief contradicts the capability decision

Design §8.3a says no `[[capabilities]]` and quotes research §3 (the gate is a *user setting*,
default `host = "*"`, checked pre-redirect). `plan/nodes/MB/MB.33/NODE.md` says the opposite:
"Zed 2026 gates the download with a capability manifest … declare **exactly** the capabilities
the code can reach (github.com and the redirect host it follows)". One of the two is wrong; the
research is sourced, so correct MB.33's Context (it is a sibling node, not editable here; record it
as a follow-up). Consequence if uncorrected: an implementer either ships an inert block or a
`host = "github.com"` block that fails closed on the CDN redirect.

## MINOR 3 — ARCHITECTURE §2 says "A single Rust binary … plays every role" and adds a second one in the same paragraph

§2's first sentence is unchanged and now false; the entry-point table has four rows and omits
`blastit lsp` / `blastit-lsp`. Add the row (`blastit lsp` → editor, Linux, delegation) or reword
the first sentence to "One serving binary, `blastit`, plays every role; `blastit-lsp` is the
editor surface." Also in §2.7, "opens no file or socket" needs the qualifier from MAJOR 3
("its own code").

## MINOR 4 — the Linux asset's glibc floor is unmeasured

`scripts/build-standalone.sh` refuses a binary above `--max-glibc 2_28`; `MB.35.5`'s
`scripts/build-lsp.sh` has no floor and builds on `ubuntu-24.04` (glibc 2.39), so the archive will
not start on Debian 12/Ubuntu 22.04. §14.3 says "the released archive runs from a clean directory"
— true on the runner only. Add one sentence to §8.3a: "the Linux assets are built in the same
`manylinux_2_28` image (or `--max-glibc 2_28` is checked), and `MB.35.5` records the floor", or
state glibc ≥ 2.39 as the supported floor. Not a CVE; it is the portability claim the whole
amendment rests on.

## MINOR 5 — "no `libc` in the closure" is a crate-graph fact presented as a runtime fact

R71 and ARCHITECTURE §2.7 say `libc` is absent "from its dependency graph"; `std` always links
libc on Linux/macOS, and an `extern "C"` declaration needs no crate (BLOCKER 1). Fix: add
"(the `libc` *crate*; `std` still links the C library, which is why `forbid(unsafe_code)` is part
of the rule)" once, in §1.3, and let R71 reference it.

---

## Verified true (checked, not taken on trust)

- **Closure:** `cargo tree -p blastit-manifest --edges normal --offline` contains no `libc`,
  `rustix`, `nix`, `pyo3`, `blastit-sys`; 42 unique packages; `pg_query`'s `cc`/`bindgen`/`glob`/
  `prost-build` are build-only. "Two direct dependencies" is true as a Cargo claim.
- **Imports:** `crates/blastit/src/lsp/mod.rs` imports `std`, `blastit_manifest`,
  `blastit_sqlpolicy::parameterize`, `serde_json`; the only `crate::` match is a comment.
- **Re-export seam:** `crates/blastit-manifest/src/lib.rs:108` is the existing `TableGrant/Columns`
  re-export; `parameterize` is `pub` (`crates/blastit-sqlpolicy/src/lib.rs:100`), so the proposed
  one-line re-export does make the two-dep rule compile.
- **No path from a document today:** `blastfile::check` (mod.rs:329) calls `parse`, `lower`,
  `Manifest::validate`, `sites`; `Manifest::validate` does not stat. `check_static_roots`
  (`model.rs:1288`) — which does `std::fs::metadata`/`read_dir` — is deliberately separate and not
  called by the LSP. The claim holds for code reachable from `run()`.
- **Capability drop on `blastit lsp`:** `cli.rs:579` calls `blastit_sys::caps::hold_none()` before
  `crate::lsp::run()`; `hold_none` → `clear()` → `capset(0,0,0)` + verify
  (`crates/blastit-sys/src/caps.rs:598-628`). The Linux path does drop, as S3 says.
- **The old guard list:** `crates/blastit/tests/critical_lsp.rs:2056-2069` contains
  `std::fs`, `std::path`, `std::process`, `std::os::unix::net`, `include!` and the three `fn`
  names — so R71 is a regression, not an oversight in the original design.
- **Extension capability gate:** research §3 (sourced) confirms `download_file` is gated by a user
  setting (`host = "*"` default), checked on the pre-redirect URL, the manifest never consulted,
  and `fetch` has no check; design §8.3a's "no `[[capabilities]]`" is correct.
- **Checksums:** `scripts/build-standalone.sh:332-338` hashes the archive contents (excluding
  `SHA256SUMS`) inside the archive; `publish-pypi.yml:782-805` writes a release-level
  `SHA256SUMS` over every asset. Both facts used in MAJOR 4 are current.
- **Asset names/layout:** the five names in §8.3a, ADR-0010 §4 and `MB.35.5`'s acceptance block
  are identical; `download_file` taking a directory path and `make_file_executable` being a no-op
  off Unix match research §2.

**Could not verify:** Zed's treatment of worktree-scoped settings (whether a trust prompt gates
`binary.path`/`binary.env`) — no cited source establishes it, which is why BLOCKER 2 states the
user-only rule; `pg_query`'s Windows/macOS CI results (taken from research §1); whether Rust's
linker applies an ad-hoc signature to the arm64 macOS asset and whether Zed's downloader sets the
quarantine xattr (neither is stated in §8.3a, hence MAJOR 4).
