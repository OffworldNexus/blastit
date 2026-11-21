VERDICT: FAIL

# Implementability review, round 2 — `MB.35.3` (portable `blastit-lsp`)

**Lens:** `implementability` (cheap-model buildability).
**Round 1:** `docs/design/portable-lsp-arch.review-implementability.md`.
**Scope:** `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (the `MB.35.3` amendment),
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/design/blastfile-lsp-zed.md` §1, §1.3, §8.3a, §14 (including §14.4), and
`ARCHITECTURE.md` §2/§2.7, checked against `plan/nodes/MB/MB.35/{MB.35.4,MB.35.5,MB.35.6}/NODE.md`,
`MB.33`'s `NODE.md`, `scripts/platform_matrix.py`, `scripts/check_workflows.py`,
`.github/workflows/publish-pypi.yml`, `tools/critical_guard.py`, and the LSP sources.

Reviewed in `.worktrees/release-0.2.0` at 06:20 CET (design `b00c0755…`, `ADR-0013` `db6a5480…`,
`ADR-0010` `042d7411…`, `ARCHITECTURE.md` `5ead128a…`). The tree is changing during this review;
the hashes pin what was read.

**Counts: 0 BLOCKER, 2 MAJOR, 7 MINOR.** Both round-1 BLOCKERs are resolved in the documents and
handed off in §14.4; `MB.35.4` is buildable from the design plus §14.4's corrections. `MB.35.5` is
still one step short: the numeric claim R74 says lives in `ADR-0010` is absent, and the
drop-a-leg rule edits several documents that no leaf's `paths` cover.

---

## Round-1 disposition

| Round-1 | State | Evidence |
|---|---|---|
| **BLOCKER-1** — re-export outside `MB.35.4`'s `paths` | **resolved in the documents** (hand-off to the plan) | Design §1.3: "**`MB.35.4` owns it and its `paths` must be widened to name that file** (today's list does not …)" (lines 262–266). ADR decision 2: "`MB.35.4` owns that one line, and its `paths` must name `crates/blastit-manifest/src/lib.rs` — the plan's list does not yet" (lines 243–245). §14.4's `MB.35.4` row repeats both path additions. The node is stale; that is the hand-off, not a document gap. |
| **BLOCKER-2** — protected test `include_str!`s deleted files | **resolved** | Design §1.3 obligation 1 names all four changed literals and says "`critical_lsp.rs` must join them" (lines 276–287); ADR decision 3 says "four `include_str!` path literals follow the move (two source paths in the test, two doc paths in `hover.rs`)" (lines 251–254); §14.4 says "assertion bodies unchanged, four literals moved". |
| **MAJOR-1** — release job cannot remain the only publishing step | **resolved** (residual MINOR-1) | ADR consequences list "a third `download-artifact` with `pattern: lsp-*`, a `--count lsp` assertion, the archive glob narrowed …, the LSP archives added to the final `SHA256SUMS`, and the per-leg `dist/SHA256SUMS.<target>` rename" (lines 351–355); §8.3a's release row (line 1698) now repeats it and names the `needs.matrix.outputs.lsp` shape. |
| **MAJOR-2** — matrix count checker cannot see `lsp`; R74 vacuous | **resolved in R74, but its numeric oracle is missing — see MAJOR-1** | R74 (line 2947) now names the `CLAIM`/`PROVEN_CLAIM` noun, `declared_of`/`proven_of`, `check()`'s `declared_keys`, `--matrix`/`--count`/`--leg`/`--summary`, and `enabled`/`dropped_because`. The claim it points at in `ADR-0010` is absent. |
| **MAJOR-3** — Linux glibc floor unstated; `ADR-0010` said "none" | **resolved** | `ADR-0010` §4: "on Linux glibc **2.28** or newer — built in `quay.io/pypa/manylinux_2_28_<arch>` with a measured `--max-glibc 2_28` floor and smoke-tested on Debian 12 (2.36)" (line 42); §8.3a's floor row (line 1695) adds the `check_workflows.py` rule; ADR-0013's platform table explains the `ubuntu-24.04`/`GLIBC_2.39` failure. |
| **MAJOR-4** — `*-musl` described as never having serving | **resolved** | ADR-0013: "`*-musl` is different and must not be described with them: it is a **serving platform** … so it keeps `blastit`, `dev`, `check` and `fmt`, loses only the editor server" (lines 327–331). |
| **MAJOR-5** — `MB.33`'s ready brief contradicts §8.3a | **resolved as a hand-off** (residual MINOR-2) | §8.3a (line 1717) says the brief is stale and this section wins; ADR decision 6 (line 292) and §14.4's `MB.33` row both name `binary.env` and the pages' three sentences. |

The round-1 MINORs are resolved in the documents: the archive is "exactly the executable,
`LICENSE` and a `SHA256SUMS` over those files (no `README.md`)" (line 1696), the guard's file set is
`lib.rs`/`hover.rs` with `main.rs` pinned (line 255), `<version>` is `latest_github_release(…).version`
interpolated into both places (lines 1667–1669), the stale counts are corrected (§1 row 1:
"1591 + 337"; §14.4: "1591/337/2477"), and the macOS `LIBCLANG_PATH` fallback is in ADR-0013's
table with the drop rule.

---

## MAJOR-1 — R74's "one numeric claim" is not in `ADR-0010`, and `MB.35.5` may not add it

**Quote (design R74, line 2947):**

> "The counts live in one place: `ADR-0010` §4 carries the `lsp` family row with the one numeric
> claim (`5` `blastit-lsp` assets), and `MB.35.5` adds the family to `scripts/platform_matrix.py`
> — … the page noun `blastit-lsp assets?` in `CLAIM`/`PROVEN_CLAIM` …"

**Quote (`ADR-0010` §4, the lsp-family paragraph, line 47):**

> "The editor family is counted by `--summary` and, once `MB.35.5` adds its own count noun, gated
> by `--check` like the others (never by renaming its assets "archives", …)"

There is no `5` and no `blastit-lsp assets` claim anywhere in `ADR-0010`; the sentence still says
"once `MB.35.5` adds its own count noun", i.e. it describes a future gate, not a claim. `CLAIM`
matches only digit + qualifier + `shims?|wheels?|archives?|tarballs?` (`platform_matrix.py`
lines 461–478), so with no new sentence `--check` prints **no** `lsp` note and §14.2's "a stated
`lsp` count above the declared family" cannot fire. R74's proof is then green about a claim that
does not exist — the exact vacuity round-1 MAJOR-2 identified.

`MB.35.5`'s `paths` (its `NODE.md` lines 9–16) do not include `docs/decisions/ADR-0010-…` or
`docs/design/blastfile-lsp-zed.md`, so the leaf cannot add the sentence; §14.4's `MB.35.5` row
does not hand the claim to `MB.35.3` either.

**Exact fix.** Add the claim to `ADR-0010` §4 (owned by the active `MB.35.3`) before `MB.35.5`
starts, e.g.:

> "The editor family is **`5` `blastit-lsp` assets** — two Linux, two Apple, one Windows — counted
> by `--summary` and gated by `--check` through the `blastit-lsp assets?` noun."

Keep the claim in the same commit as the drop (see MAJOR-2). If the family is meant to have no
prose claim, change R74 to say so and stop claiming `--check` reads one.

---

## MAJOR-2 — dropping a failed leg requires document edits that `MB.35.5` is forbidden to make

**Quote (ADR-0013, lines 316–322):**

> "**If a leg fails at `MB.35.5`**, it is dropped from the `lsp` family in
> `scripts/platform_matrix.py` with `dropped_because`, and — in the same commit — from every
> place that states the targets: this amendment's platform table, design §8.3a's asset table and
> §14.3's five bullets, `docs/reference/cli.md` `## blastit-lsp`'s table,
> `docs/troubleshooting.md`'s "no asset for that pair" row, and `ADR-0010` §4's row."

**Quote (`MB.35.5/NODE.md`, lines 9–16 and 74):**

```
paths:
  - .github/workflows/ci.yml
  - ".github/workflows/publish-pypi.yml"
  - scripts/platform_matrix.py
  - scripts/check_workflows.py
  - scripts/test_packaging_gates.py
  - "scripts/build-lsp.sh"
  - scripts/lsp_smoke.py
…
- **Do not touch** `crates/**`, `editors/**`, `docs/**` (except a version/count sentence that
  `--check` proves inconsistent — list the page in `paths` if so) …
```

`docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`, `docs/design/blastfile-lsp-zed.md`,
`docs/reference/cli.md`, `docs/troubleshooting.md` and `ADR-0010` are not in that list, and none is a
"version/count sentence that `--check` proves inconsistent": until the claim of MAJOR-1 exists, `--check` proves nothing about the target table.
`MB.35.2` (the pages) is in `review`; `MB.35.3` (the design and ADRs) is `active`; neither will be
available when `MB.35.5` runs. §14.4's `MB.35.5` row lists the matrix, floor, CI and checksum
corrections but not this one, so the design's own hand-off table is silent on the only edit the
failure path requires.

**Failure mode.** The Apple legs are explicitly "buildable, unproven". A cheap implementer hits a
red leg, follows the ADR, edits several documents, and the plan validator/`AGENTS.md` rule 5 refuses
the commit — at the moment the node should ship four targets and a recorded `dropped_because`.
The alternative (ship the matrix without the docs) leaves every page promising a target that has
no asset, which is the failure the same paragraph says must not happen.

**Exact fix.** Give the drop an owner. Either add `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`,
`docs/design/blastfile-lsp-zed.md`, `docs/reference/cli.md`, `docs/troubleshooting.md` and
`docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` to `MB.35.5`'s `paths` (and delete
`docs/**` from the blanket "do not touch" line for this case), or split a `MB.35.5a` doc-sync leaf
with exactly those paths and say in ADR-0013 that the drop lands in `MB.35.5a`'s commit, not
`MB.35.5`'s. Add the same sentence to §14.4's `MB.35.5` row.

---

## MINOR-1 — the release job's "narrowed" glob has no exact spelling, and the natural guess still matches `blastit-lsp-*`

**Quote (design §8.3a, line 1698):** "an archive glob that does **not** also match the standalone
family (today's `blastit-*.tar.gz` matches `blastit-lsp-*.tar.gz`)".

`blastit-*-unknown-linux-*.tar.gz` is the obvious "fix" and still matches
`blastit-lsp-x86_64-unknown-linux-gnu.tar.gz`. The final `SHA256SUMS` also has to include the
Windows `.zip`, which today's `archives=(blastit-*.tar.gz)` never would.

**Fix.** State the arrays, e.g. `archives=(blastit-x86_64-unknown-linux-*.tar.gz
blastit-aarch64-unknown-linux-*.tar.gz)` and `lsp_assets=(blastit-lsp-*)`, with
`--count binaries` checked against `archives` and `--count lsp` against `lsp_assets`.

## MINOR-2 — §8.3a says "four points" and then enumerates three

**Quote (line 1717):** "**`MB.33`'s brief is stale on four points and this section wins.**" The
paragraph then lists the capability block, the download destination and the Windows
`make_file_executable` row. `binary.env` is the fourth, but it appears only in the allowlist
paragraph above and in §14.4.

**Fix.** Change "four" to "three", or add the `binary.env` bullet to the enumerated paragraph.

## MINOR-3 — `MB.35.5`'s acceptance still invokes bare `python3` and the script directly

**Quote (`MB.35.5/NODE.md`, line 85):** "`$ scripts/platform_matrix.py --matrix lsp | python3 -c
'import json,sys; …'`". The script's first line is `#!/usr/bin/env python3`, so the direct call is
bare Python too. `AGENTS.md` rule 9 forbids both; §14.4's `MB.35.5` row does not mention it, while
the `MB.35.6` row explicitly says `uv run --no-project python`.

**Fix.** Add to §14.4's `MB.35.5` row: "`uv run --no-project scripts/platform_matrix.py --matrix lsp
| uv run --no-project python -c …`; never bare `python3`."

## MINOR-4 — `--max-glibc` is a `build-standalone.sh` flag; `build-lsp.sh` is new

**Quote (design §8.3a, line 1695):** "built in `quay.io/pypa/manylinux_2_28_<arch>` with a measured
`--max-glibc 2_28` floor and smoke-tested on Debian 12". `--max-glibc` is parsed by
`scripts/build-standalone.sh` (line 49), not by a new `build-lsp.sh`.

**Fix.** Say "reuse `build-standalone.sh`'s `readelf --version-info` floor block (or factor it into
a helper) and give `build-lsp.sh` the same `--max-glibc 2_28` check", so the flag is not assumed
to exist.

## MINOR-5 — "declared, unbuilt" contradicts "the archive the release publishes"

**Quote (design §14.3, lines 2970–2971):** "until a smoke run exists it is **declared, unbuilt**,
and the archive the release publishes is the claim under test." The matrix's word for built but
not exercised is *declared, unproven* (`PROVEN` is the built-and-exercised set). If the archive
exists, the target was built.

**Fix.** Say "declared, unproven (no `PROVEN` entry); the archive the release builds is the claim
under test."

## MINOR-6 — `MB.35.4`'s brief cites `cli.rs:572`; the call is at `:586`

**Quote (`MB.35.4/NODE.md`, line 38):** "so `crates/blastit/src/cli.rs:572`'s `crate::lsp::run()`
keeps compiling untouched". The call is `crates/blastit/src/cli.rs:586`. §14.4's `MB.35.4` row
does not correct the line number.

**Fix.** Add "change `cli.rs:572` to `:586`" (or drop the number) to §14.4's `MB.35.4` row.

## MINOR-7 — `MB.35.6` says "list `docs/**` here" but its `paths` do not allow a docs fix

**Quote (`MB.35.6/NODE.md`, line 68):** "If a documented command is wrong, the fix is on the page
(list `docs/**` here) and the transcript shows both." Its `paths` (lines 9–13) are the two guard
scripts, `Makefile`, and `docs/testing/E2E-MB.35.md`; `docs/reference/cli.md` is a lane violation.

**Fix.** Add `"docs/**"` to `paths`, or reword to "report the page error in `RESULT.md`; do not
edit it" and make the fix a follow-up. §14.4's `MB.35.6` row should say which.

---

## Verified true (no change needed)

- **The re-export is possible and the cited line is real.** `crates/blastit-manifest/src/lib.rs:108`
  is `pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};`; `parameterize` is public at
  `crates/blastit-sqlpolicy/src/lib.rs:100` (re-exported from `template`). `#![deny(missing_docs)]`
  does not reject a re-export of a documented item (the existing three prove the shape).
- **The moved source is portable as claimed.** `crates/blastit/src/lsp/mod.rs` and `hover.rs`
  contain no `std::fs`, `std::path`, `std::process`, `std::net`, `std::env`, `blastit_sys`,
  `rustix`, `libc`, `pyo3`, `crate::stage0`, `crate::stage1`, `crate::host`, or `crate::config`;
  the only SQL import is `use blastit_sqlpolicy::parameterize;` (`mod.rs:55`). The protected grep
  at `critical_lsp.rs:2049–2080` passes today and passes against `lib.rs` after the one import
  change.
- **`run()` moves cleanly.** `crates/blastit/src/lsp/mod.rs:206` is `pub fn run() -> u8`; the
  panic hook (`:207`) and the three `catch_unwind` sites move with it. `cli.rs:586` is
  `crate::lsp::run()`, with `blastit_sys::caps::hold_none()` before it.
- **The four changed `include_str!` literals are exactly four.** `hover.rs:294,316` are the two doc
  paths; `critical_lsp.rs:2052–2053` are the two source paths. The test's other three
  `include_str!`s (`:1419`, `:2036`, `:2093`) stay valid from `crates/blastit/tests/`.
- **The normal closure has no `libc` crate.** `cargo tree -p blastit-manifest --edges normal
  --offline` resolves `blastit-ingress`, `blastit-sqlpolicy`, `pg_query`, and no `libc`,
  `rustix`, `nix` or `pyo3` line; `pg_query`'s `cc`/`bindgen` are build-only, as §1.3 says.
- **`crates/blastit-lsp/Cargo.toml` matches the workspace conventions** key for key
  (`version/edition/rust-version/license/repository/publish.workspace`, `[lints] workspace`, the
  `[lib]`/`[[bin]]` split); `serde_json = "1.0.154"` matches `crates/blastit/Cargo.toml:54`;
  `members = ["crates/*"]` already covers the crate.
- **The structural identity rule is implementable.** `tools/critical_guard.py` allows additions and
  refuses only `M`/`D`/`T` without a covering glob; `crates/blastit-lsp/tests/critical_lsp_stdio.rs`
  needs no plan entry, and `critical_lsp.rs` needs the `MB.35.4` path addition §14.4 states.
- **The round-1 literal oracle is real.** `critical_guard.py --base <commit>` exists and reports a
  changed protected file (with `--node` it enforces; without it, report-only). Use
  `--node MB.35.4` once the path is added, or keep the `git diff` as the stated oracle.
- **The asset names, file types and relaunch paths are unambiguous** across ADR-0013 decision 4,
  §8.3a's table, `docs/reference/cli.md:111–126` and `MB.35.5`'s five-name list; the archive's
  `blastit-lsp-<target>/` root matches the extension's join, and `.exe`/`Zip` are Windows-only.
- **`ADR-0010` §4 and `ARCHITECTURE.md` §2.7 agree** with §1.3: `blastit-lsp` has exactly
  `blastit-manifest` + `serde_json`, builds on five targets, is editor-only, and `blastit lsp`
  delegates through `pub use blastit_lsp as lsp;`.

---

## Note on the hand-off boundary

This review treats the plan's stale `paths`/wording as §14.4 hand-offs, as instructed. That is why
the two round-1 BLOCKERs are not re-raised even though `MB.35.4/NODE.md` still says
"byte-identical" and omits both paths. The findings above are the ones where the **design** (or its
own hand-off table) is still incomplete: the unowned numeric claim, the unowned drop-a-leg edits,
and the small text corrections.
