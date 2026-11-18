# Implementability review: `blastit-lsp` crate boundary, assets and matrix (`MB.35.3`)

**Lens:** `implementability` (cheap-model buildability).
**Scope:** the `MB.35.3` diff in `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md` (amendment
`MB.35.3`), `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §4,
`docs/design/blastfile-lsp-zed.md` §1/§1.1/§1.3/§8.3a/§11 S12/§14 + requirement index, and
`ARCHITECTURE.md` §2/§2.7 — checked against `plan/nodes/MB/MB.35/{MB.35.3,MB.35.4,MB.35.5,MB.35.6}`,
`MB.33`, the real `crates/*` manifests and sources, `scripts/platform_matrix.py`,
`scripts/build-standalone.sh` and `.github/workflows/publish-pypi.yml`.

VERDICT: FAIL

Two findings make `MB.35.4` unimplementable as written: it must edit a file outside its `paths`,
and its "byte-identical protected test" requirement is impossible because that test
`include_str!`s the files the node deletes. Five MAJOR gaps leave `MB.35.5` guessing about the
release job, the matrix count checker, the Linux glibc floor, a platform sentence that is
factually wrong, and a ready sibling brief that contradicts the contract.

---

## BLOCKER-1 — the mandatory `blastit-manifest` re-export is outside `MB.35.4`'s `paths`, and the design says it is inside

**Quote (design §1.3, lines 260–262):**

> `name a second crate. That is one added line in `crates/blastit-manifest/src/lib.rs`, part of
> `MB.35.4`'s paths; … and `lsp`'s `use blastit_manifest::parameterize;` is the only import row 1
> changes.`

**Quote (`plan/nodes/MB/MB.35/MB.35.4/NODE.md`, lines 9–15):**

```
paths:
  - "crates/blastit-lsp/**"
  - "crates/blastit/src/lsp/**"
  - crates/blastit/src/lib.rs
  - crates/blastit/Cargo.toml
  - Cargo.toml
  - Cargo.lock
```

`crates/blastit-manifest/src/lib.rs` is **not** in that list (and no glob there reaches it).

**Why it is a blocker.** The moved source is verbatim today's module, whose line 55 is
`use blastit_sqlpolicy::parameterize;` (`crates/blastit/src/lsp/mod.rs:55`). After the `git mv`,
`crates/blastit-lsp/Cargo.toml` has exactly `blastit-manifest` + `serde_json` (§1.3's block), so
that `use` does not resolve. The implementer's three options are all red:

1. add `blastit-sqlpolicy` as a third dependency → `R69`/`R71` fail (`[dependencies]` is exactly
   `{ blastit-manifest, serde_json }`);
2. edit `crates/blastit-manifest/src/lib.rs` → outside the node's `paths` (AGENTS rule 5, and the
   plan validator refuses it);
3. stop and escalate — a cheap implementer will not.

The ADR's amendment (decision 2, lines 235–240) states the re-export but names no owning node, so
nothing corrects §1.3's false parenthetical.

**Exact fix.** Add `"crates/blastit-manifest/src/lib.rs"` to `MB.35.4`'s `paths`, and state the
edit in §1.3 verbatim: at `crates/blastit-manifest/src/lib.rs:108` change
`pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};` to
`pub use blastit_sqlpolicy::{parameterize, Columns, TableGrant, TableGrants};` (rustfmt will order
it), with the sentence "`MB.35.4` owns this line; `blastit-lsp`'s only dependency change is the
`use` above." Also correct §1.3's "part of `MB.35.4`'s paths" only after the node is updated.

---

## BLOCKER-2 — the protected test `include_str!`s the files the node deletes, so "byte-identical" + "one place" cannot both hold

**Quote (`crates/blastit/tests/critical_lsp.rs:2052–2053`, real repository):**

```rust
        include_str!("../src/lsp/mod.rs"),
        include_str!("../src/lsp/hover.rs"),
```

**Quote (design §1.3, item 1, line 270):** "`crates/blastit/tests/critical_lsp.rs` stays
**byte-identical** and keeps spawning `env!("CARGO_BIN_EXE_blastit") lsp`".
**Quote (`R70`, line 2868):** "…the protected `critical_lsp.rs` runs **byte-identical** … proved by
… `git diff --exit-code -- crates/blastit/tests/critical_lsp.rs`".
**Quote (design §1.2, line 188):** "one assertion in `critical_lsp.rs` over
`include_str!("../src/lsp.rs")`" — already the wrong path today (`../src/lsp/mod.rs`).

**Why it is a blocker.** `MB.35.4`'s own acceptance is
`git status --short` showing `crates/blastit/src/lsp/` is gone **and**
`cargo test -p blastit --test critical_lsp` green **and** `git diff --exit-code --
crates/blastit/tests/critical_lsp.rs`. After the `git mv`, the two `include_str!` arguments point
at deleted files, so the test does not compile. The only escapes are:

- keep dead duplicates at `crates/blastit/src/lsp/{mod,hover}.rs` → `the_lsp_module_is_a_text_tool`
  (S1/S11, R3) now asserts over code that is no longer compiled or run, i.e. the guard becomes
  vacuous, and it contradicts "one binary, one place" in the node's Notes;
- edit the protected test → it is deliberately **not** in `MB.35.4`'s `paths`, and `R70` forbids
  the diff.

**Exact fix (choose one and write it down).**

1. **Preferred:** add `crates/blastit/tests/critical_lsp.rs` to `MB.35.4`'s `paths` for a
   **path-only** edit — `include_str!("../../blastit-lsp/src/lib.rs")` and
   `include_str!("../../blastit-lsp/src/hover.rs")` — record it in the node as
   "two `include_str!` arguments, no assertion changed", let `tools/critical_guard.py` name the file
   in the review, and reword `R70`/§14.2 to "byte-identical except the two `include_str!` path
   arguments".
2. Or move the two text assertions into the new protected
   `crates/blastit-lsp/tests/critical_lsp_stdio.rs` (reading `../src/lib.rs`, `../src/hover.rs`) and
   delete them from `critical_lsp.rs` — same path addition, larger diff.

Either way, §1.2's `include_str!("../src/lsp.rs")` must be corrected to today's
`../src/lsp/mod.rs` and the post-move location named.

---

## MAJOR-1 — the release job cannot "stay the only publishing step": the count glob matches the new assets

**Quote (ADR-0013 amendment, Consequences, lines 307–308):**

> `Five more CI legs and one more count in the matrix; the release job's checksum rule and the
> `dist/*` upload stay the only publishing step.`

**Quote (`.github/workflows/publish-pypi.yml`):**

```
773:          pattern: standalone-*
785:          expected_bins=$(uv run --no-project scripts/platform_matrix.py --count binaries)
793:              archives=(blastit-*.tar.gz)
```

**Failure mode.** `blastit-*.tar.gz` matches `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz` — the
hyphen `*` spans `lsp-…`. If `MB.35.5` adds a third `download-artifact` for the LSP assets, that
job counts more archives than `--count binaries` and goes red; if it does not add one, the LSP
assets never reach `dist/`, the final `SHA256SUMS` omits them, and `gh release upload dist/*`
publishes them outside the checksum rule. Also, each of the five legs writing `dist/SHA256SUMS`
collides exactly as the standalone job's `mv dist/SHA256SUMS "dist/SHA256SUMS.${{ matrix.target }}"`
(`publish-pypi.yml:299–301`, `:396–398`) exists to prevent.

**Exact fix.** State in the ADR/§8.3a: the release job gains `needs.lsp`, a third
`download-artifact` with `pattern: lsp-*`, a `--count lsp` assertion, an explicit
`archives=(blastit-*-unknown-linux-*.tar.gz)` (or a `blastit-lsp-*` filter), includes the LSP
archives in the final `sha256sum` list, and the LSP job repeats the per-leg rename to
`dist/SHA256SUMS.<target>`. Replace the sentence quoted above with that list.

---

## MAJOR-2 — the matrix count checker cannot see the `lsp` family, so R74's proof is vacuous (and can misattribute claims)

**Quote (design R74, line 2872):** "The counts live in one place … proved by `make
platform-matrix-check`".
**Quote (ADR-0010 §4 note, line 47):** "the editor family is counted by `--summary`".
**Quote (`scripts/platform_matrix.py:461–477`):** the only families the gate parses are
`(?P<family>shims?|wheels?|archives?|tarballs?)`; `declared_of`/`proven_of` (lines 525–547) map
`shim`/`wheel`/`archive`/`tarball` to `shim_legs()`/`wheel_legs()`/`binary_legs()` only, and
`--matrix` is `choices=("wheels", "binaries", "shims", "images")` (line 868).

**Failure mode.** A sentence like "`6` `blastit-lsp` files" is matched by nothing (silent pass), and
a sentence like "`N` archives" (a literal count followed by the noun) is checked against `len(binary_legs())` (the serving family) and
goes red for the wrong reason. `check()` also refuses any `PROVEN` key the matrix "does not build"
(lines ~654–665), so `MB.35.5`'s five `PROVEN` entries fail until that union learns `lsp`. Nothing in
the design says which of these to change — the ADR says the opposite ("counted by `--summary`"),
while `R74` promises `--check` is the proof.

**Exact fix.** Add one paragraph to §8.3a/R74: `MB.35.5` adds a distinct family token to
`CLAIM`/`PROVEN_CLAIM` (e.g. `lsp assets?|editor binaries?`), an `lsp` branch to
`declared_of`/`proven_of`/`check()`'s declared-and-`PROVEN` unions, and `lsp` to
`--matrix`/`--leg`/`--count`; the design's asset table stays unchecked prose only if R74 says so
plainly ("printed by `--summary`, not gated by `--check`").

---

## MAJOR-3 — the Linux `blastit-lsp` glibc floor is unstated, and the chosen runner makes it 2.39 while ADR-0010 claims "none"

**Quote (ADR-0010 §4, row for `blastit-lsp`, line 42):** "Host runtime required | none — an editor
spawns it on stdin/stdout …".
**Quote (ADR-0013 amendment platform table, line 279):** "`x86_64-unknown-linux-gnu` | … |
`ubuntu-24.04`, build + smoke".

**Failure mode.** A GNU binary built on `ubuntu-24.04` carries `GLIBC_2.39` version needs; a Debian
12 workstation (glibc 2.36) gets `version 'GLIBC_2.39' not found`. The serving family treats this
as a measured, enforced number (`scripts/build-standalone.sh`'s `--max-glibc` and the
`quay.io/pypa/manylinux_2_28_<arch>` builder, `build-standalone.sh:30` and its floor block); the
editor family has no equivalent decision, and the ADR says "none". No doc tells `MB.35.5` what to
build in.

**Exact fix.** In §8.3a (and the ADR table's CI-leg column) name the Linux builder and floor, e.g.
build inside `quay.io/pypa/manylinux_2_28_<arch>` with `--max-glibc 2_28` (or `ubuntu-22.04` with a
measured floor), and change ADR-0010's "none" to "glibc `<floor>` or newer (measured); nothing
else". Add one execution check on Debian 12, as the archives already do.

---

## MAJOR-4 — ADR-0013's "`*-musl` … never had serving" is false and will be propagated

**Quote (ADR-0013 amendment, lines 292–294):**

> `Targets this epic deliberately does not build: 32-bit x86, Windows on ARM, and `*-musl`. Each
> loses the server only — none of them ever had serving, because serving is Linux-glibc/musl with
> the rest of §2's machinery and `ADR-0010`'s artifacts.`

**Failure mode.** musl **is** a serving target: `ADR-0010` §4 rows 1–2 ship `musllinux_1_2`
wheels and `blastit-{x86_64,aarch64}-unknown-linux-musl.tar.gz`, and `scripts/platform_matrix.py`'s
`LEGS` contains `x86_64-musl` and `aarch64-musl` with `PROVEN` archive entries. The sentence as
written tells a doc-writer or a `MB.35.5` implementer that musl has no Blastit at all; the CLI page
already says the opposite ("no musl" applies to the editor only).

**Exact fix.** Rewrite: "32-bit x86 and Windows on ARM get neither; `*-musl` is a serving platform
that loses only the editor server — it keeps `blastit`, the musllinux wheels and the `-musl`
archives (`ADR-0010` §4), and gains no `blastit-lsp`."

---

## MAJOR-5 — `MB.33`'s ready brief contradicts §8.3a on three points and neither document records the supersession

**Quote (`plan/nodes/MB/MB.33/NODE.md`, lines 82–90):**

> `Zed 2026 gates the download with a capability manifest. Extensions may only call
> zed_extension_api::download_file for hosts listed in extension.toml … declare **exactly** the
> capabilities the code can reach (github.com and the redirect host it follows)`

…and lines 41/43: `download_file(&asset.download_url, "<workdir>/blastit-lsp-<version>", <type>)`;
Windows executable column: "**no** `make_file_executable`".

**Quote (design §8.3a, lines 1648–1652):** "`download_file` takes a **directory** path", "the
`download_file` gate is a **user setting** … the manifest is never consulted … a block is inert, and
naming the CDN host would fail closed", "`make_file_executable` is called unconditionally … there is
**no** `if cfg!(windows)`". ADR-0013's rejected-alternatives row (line 321) says "`MB.33` declares
none".

**Failure mode.** `MB.33` is `ready`. Its brief instructs exactly the three things the design's
guard (`lib: no capabilities`, `lib: download destination`, `lib: executable`) fails on, and its
own self-correction rule ("if the design's table and this brief disagree, the design wins") is
buried inside the same stale paragraph. A cheap implementer will follow the nearer, more specific
brief and burn an attempt on a guard failure, or "fix" the design backwards.

**Exact fix.** Add to §8.3a (and R72's proof row): "`MB.33`'s Context paragraph on
`[[capabilities]]`, its `<workdir>/` download destination and its Windows-only
`make_file_executable` are stale; the three rows above win (no capability block, a relative
destination, an unconditional call)." Raise the same as a plan `human`/brief correction so
`MB.33`'s `NODE.md` is fixed before it starts; its `paths` already cover it.

---

## MINOR-1 — "the shape `scripts/build-standalone.sh` already produces" is not the shape it lists

ADR-0013 decision 4 (lines 249–252) and §8.3a (line 1647) say the archive holds the executable,
`LICENSE` and `SHA256SUMS`. `build-standalone.sh` also stages `README.md` and writes **two**
checksums: one inside the directory (`SHA256SUMS` over its files) and one at `$out/SHA256SUMS` over
the tarball. State the exact LSP list (decide whether `README.md` ships) and the per-leg
`SHA256SUMS.<target>` rename (see MAJOR-1), or drop "the shape … already produces".

## MINOR-2 — two forbidden-token lists, and `main.rs` legitimately contains `std::process::exit`

§1.2's list (lines 192–196) forbids `"std::process"`; §1.3's row (line 253) and `R71` forbid only
`"std::process::Command"`, and §1.3 explicitly allows `exit`. A `MB.35.6` guard written from the
§1.2 list over all of `crates/blastit-lsp/src/**` fails on the mandated three-line `main.rs`.
State the file set: "the §1.2 list applies to `src/lib.rs` and `src/hover.rs`; `main.rs` is exempt
for `std::process::exit` only."

## MINOR-3 — `<version>` is never defined

§8.3a uses `blastit-lsp-<version>` in both the `download_file` destination and the relaunch path but
never says which string it is (`latest_github_release(..).version` vs `CARGO_PKG_VERSION`). Name the
variable once and say it is interpolated into both places, so the cached directory and the launch
path cannot diverge.

## MINOR-4 — stale size claims in the node and §1

§1 row 1 says "`crates/blastit/src/lsp/{mod,hover}.rs` (**new**, ≤ 750 lines)"; the real files are
1591 and 337 lines, and `MB.35.4`'s Context says 1174 + 254. Correct the numbers or remove them; a
cheap implementer will otherwise plan a split.

## MINOR-5 — macOS `libclang` fallback is not decided where the leg is specified

Research §"Unverified" leaves "`libclang.dylib` presence at clang-sys's hardcoded paths" open, with
an `xcode-select -p` fallback. Neither §8.3a nor the ADR table says what `MB.35.5` does when the
Apple leg cannot find it. Add one line: set `LIBCLANG_PATH=$(xcode-select -p)/usr/lib`; if that
fails, drop the leg with `dropped_because` and the extension message.

---

## Leaf-brief notes (context, not the document diff)

- `MB.35.5`'s acceptance pipes to bare `python3` (`plan/nodes/MB/MB.35/MB.35.5/NODE.md:85`), which
  AGENTS rule 9 forbids; a cheap implementer copies the block verbatim. Use
  `uv run --no-project python`.
- `MB.35.4`'s move is 1928 lines of existing code plus a new test; that is within the ≤ 600-line
  *authoring* budget only because it is a `git mv`. Say so in the node so a reviewer does not ask
  for a split, and so the two BLOCKER fixes above are the only authored changes.

---

## Verified true

Checked against the real repository; these all hold and need no change:

- **Cargo.toml shape is complete and consistent with the workspace.** §1.3's block matches
  `crates/blastit-manifest/Cargo.toml` key for key: `version/edition/rust-version/license/
  repository/publish` as `*.workspace = true`, `[lints] workspace = true`, and the
  `[[bin]]`/`[lib]` split. `serde_json = "1.0.154"` is exactly `crates/blastit/Cargo.toml:54`;
  `publish.workspace = true` is what every crate uses (workspace `publish = false`);
  `members = ["crates/*"]` already covers the new crate; `blastit-manifest =
  { version = "0.2.0", path = "../blastit-manifest" }` matches the repo's dependency convention.
- **`blastit_lsp::run() -> u8` exists to move.** `crates/blastit/src/lsp/mod.rs:206` is
  `pub fn run() -> u8`, and the panic hook (`:207`) and `catch_unwind` sites (`:674`, `:960`,
  `:1030`) move with the file unchanged.
- **The delegation compiles untouched.** `crates/blastit/src/cli.rs:586` is `crate::lsp::run()`;
  `pub use blastit_lsp as lsp;` resolves it, and `blastit_sys::caps::hold_none()` stays before it
  (`cli.rs:579`). `crates/blastit/src/lib.rs:44` is `pub mod lsp;`.
- **The cited re-export line is real:** `crates/blastit-manifest/src/lib.rs:108` is
  `pub use blastit_sqlpolicy::{Columns, TableGrant, TableGrants};`, and
  `blastit_sqlpolicy::parameterize` is public at its crate root
  (`crates/blastit-sqlpolicy/src/lib.rs:100`), so the one-line re-export is possible.
- **Adding the new protected test is allowed.** `tools/critical_guard.py` only refuses *breaking*
  statuses (`M`/`D`/`T`) without a covering glob; additions are always allowed, so
  `crates/blastit-lsp/tests/critical_lsp_stdio.rs` needs no extra plan entry.
- **The five asset names, file types and relaunch paths are unambiguous and consistent** across
  ADR-0013 decision 4, §8.3a's table, `docs/reference/cli.md:120–126` and `MB.35.5`'s acceptance
  list; `download_file` takes a directory and extracts verbatim (research §2), so
  `blastit-lsp-<version>/blastit-lsp-<target>/blastit-lsp` is the right shape, `.exe` only on
  Windows, `Zip` only on Windows, and `make_file_executable` is unconditional.
- **The archive layout matches the accepted page.** The tar's top-level `blastit-lsp-<target>/`
  is exactly `docs/reference/cli.md:111`'s "one directory holding `blastit-lsp`", and matches
  `MB.35.5`'s staging instruction.
- **Declared-vs-proved is stated for the three open facts:** `LIBCLANG_PATH=C:\Program Files\LLVM\bin`
  (ADR table), `macos-15-intel` as the only `cargo test`-grade Intel evidence, and
  `aarch64-unknown-linux-gnu` as "declared, unbuilt" (§14.3 and MB.35.5's acceptance).
- **ADR-0010 §4's `lsp` row adds no shim, no `libphp`, no `libpython`**, uses the two formats
  correctly, and does not restate the serving counts. Its "the `lsp` family … two Linux, two Apple,
  one Windows" agrees with the five triples in §8.3a and the research table.
- **ARCHITECTURE.md §2.7 agrees with §1.3**: crate row "`blastit-lsp` … exactly
  `blastit-manifest` and `serde_json` … Linux, macOS, Windows — five targets", and the paragraph
  "The editor surface is cross-platform; serving is Linux-only" plus the
  `pub use blastit_lsp as lsp;` delegation sentence.
- **No `cfg`/`std::env`/`std::os` in today's server source:** `crates/blastit/src/lsp/*.rs` contains
  only `std::{cell,collections,io,panic}`, `blastit_manifest`, `blastit_sqlpolicy::parameterize`
  and `serde_json`, so the portability claim is a move, not a rewrite.
