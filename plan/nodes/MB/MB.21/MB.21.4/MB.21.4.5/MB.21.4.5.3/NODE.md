---
id: MB.21.4.5.3
title: "Translate the last manifest suites and keep the docs-fence harness honest without the retired opening"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.5.1]
paths:
  - "crates/blastit-manifest/tests/common/mod.rs"
  - "crates/blastit-manifest/tests/validate.rs"
  - "crates/blastit-manifest/tests/mutation_gaps.rs"
  - "crates/blastit-manifest/tests/routing.rs"
  - "crates/blastit-manifest/tests/grants.rs"
  - "crates/blastit-manifest/tests/uid.rs"
  - "crates/blastit-manifest/tests/docs_fences.rs"
  - "crates/blastit-manifest/tests/static_mounts.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T16:57"
updated: "2026-11-16T16:57"
---

# Goal

Every remaining suite in `crates/blastit-manifest/tests/` reads `Blastfile`, `tests/common/mod.rs` exposes
Blastfile fixtures instead of two retired documents, and `docs_fences` keeps its promise — every
*documented document a reader can copy today* still boots — while the pages that still carry the retired
opening are counted, not parsed.

# Context

- Measured at planning: `tests/validate.rs` 781 lines / 32 fixtures / 6 reader calls,
  `tests/mutation_gaps.rs` 714 / 16 / 7, `tests/routing.rs` 259 / 11 / 7, `tests/grants.rs` 261 / 5 / 5,
  `tests/uid.rs` 127 / 3 / 2, `tests/common/mod.rs` 172 / 2 / 6, `tests/docs_fences.rs` 539 / 3 / 0.
- `tests/common/mod.rs` is the crate's shared fixture module. `MB.21.4.5.1` deleted its third user, so the
  two that remain (`validate`, `routing`) are yours. Keep the exported names (`texts`, `text_map`,
  `parse_errors`, `validate_errors`, `validate_warnings`, `section_6`, `fixture`, `corpus`,
  `CORPUS_SIZE`, `fixture_directory`) and re-point their bodies at `from_blastfile_str` /
  `parse_blastfile`; the other suites and the plan's other leaves rely on those names.
- `SECTION_6` and `SECTION_5_1` become Blastfile documents of the same application: the four views
  (`product-detail`, `checkout`, `about`, `fallback`), the two stores, and one `trait session` instead of
  the top-level group. A top-level `groups` list that attaches to every route has no Blastfile spelling —
  the tests that assert it (`fallback_gets_the_global_groups_only`, `texts(manifest.global_groups()) ==
  ["session"]`, `check_global_groups`) must assert the Blastfile subject instead: a `use <trait>` that is
  written is honoured, one that is omitted is not inherited, an unknown trait is refused, and
  `global_groups()` is empty for every Blastfile.
- `docs_fences.rs` collects three openings: the two Blastfile ones (executed through
  `Manifest::parse_blastfile`) and the retired one (executed through the reader until now). Once
  `MB.21.4.5.6` deletes the reader there is no parser for the third, so:
  - keep every per-page count pinned, the retired opening's count included, and leave
    `THE_RETIRED_OPENING_IS_GONE` at `false` for `MB.21.5.2` to flip;
  - stop **executing** the retired fences (they are counted, and named in the message, but not parsed);
  - re-home what they newly stop proving, at model level, in the same commit: the `InvalidRegex` canary
    becomes a Blastfile `view` whose `regex '/products/('` cannot compile, and the documented static-site
    shape (`is_the_documented_static_site`) becomes a `static` mount fixture in `tests/static_mounts.rs`
    pinning the same `root` / `index` / `fallback` / `immutable`. The Django `[route.static]` fragment
    test can no longer be parsed at all: delete it with a comment naming `MB.21.5.1`'s rewrite, and keep
    the immutable-depth fact as the same mount fixture.
  - every message that spells the retired opening composes it from two literals (`"blastit"` + `".toml"`),
    so the tree never carries the contiguous name and `MB.21.5.2`'s guard needs no exemption.
- Cross-leaf contract: `ManifestError::MissingVersion` / `UnsupportedVersion` are deleted by
  `MB.21.4.5.6`. The two assertions that construct them (`tests/validate.rs`, `tests/mutation_gaps.rs`)
  must use a variant the lexer still produces and keep the same rendering assertion.

# Acceptance

```console
$ grep -rnE 'from_toml_str|toml::' crates/blastit-manifest/tests/
$ grep -rnE 'blastit[.]toml' crates/blastit-manifest/tests/
$ cargo test -p blastit-manifest
$ make docs-unit
```

1. Both greps print nothing over the whole test directory, `cargo test -p blastit-manifest` exits `0`, and
   `make docs-unit` exits `0`.
2. `docs_fences` still prints/pins one row per documented page, the retired opening's count included, and
   `THE_RETIRED_OPENING_IS_GONE` is still `false` — paste the per-page table.
3. `RESULT.md` names, one line each: the canary's Blastfile replacement, the static-site shape's new home
   in `tests/static_mounts.rs`, the deleted Django fragment test and the fact `MB.21.5.1` restores it, and
   the two error-variant assertions that changed subject.
4. No page under `docs/` is edited by this leaf — paste `git status --porcelain docs/` (expected: empty).

# Notes

- `tests/grants.rs`, `tests/uid.rs` and `tests/routing.rs` currently assert `Route::static_policy()` /
  `route.php()` shapes. Rebuild those fixtures as mounts / `entrypoint php` and assert on
  `manifest.mounts()` / `manifest.php()`: the per-route accessors are a recorded follow-up and this suite
  must survive it.
- Where a Blastfile cannot express a retired shape at all, build the model through its `#[doc(hidden)]`
  builder and say in the test's doc comment why — never resurrect a document.
- Do not touch `crates/blastit-manifest/src/**` (5.6's) or `docs/**` (`MB.21.5.1`'s).
