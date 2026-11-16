# Log MB.21.4.1

## 2026-11-16T00:38 [planner]

created (task) Discover ./Blastfile and refuse blastit.toml/TOML input with one migration sentence at every entry point

## 2026-11-16T01:49 [planner]

edited fields

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T08:04 [planner]

edited fields

## 2026-11-16T08:05 [lead]

status ready -> active

## 2026-11-16T08:05 [lead]

attempt 1/3 started

## 2026-11-16T08:05 [lead]

base b4c8344

## 2026-11-16T08:11 [lead]

needs_split at attempt 1: no code landed (only RESULT.md). Measured: (a) the flip breaks every crates/blastit/tests boot fixture (9 files) and every python/tests/runnable_docs fixture, (b) docs-test stages fixture manifests EXCEPT docs/local.md's dev --learn fence, which stages the page's own block via tomllib, (c) error::{clipped,escape_control} are pub(crate) behind a private mod error, so cli.rs cannot call them without lib.rs+error.rs. Re-scoping 4.1 to the atomic cutover (product flip + every make-check-reachable fixture + docs/local.md's executed block + the runnable-docs harness) and narrowing 4.4 to e2e/scripts/Makefile.

## 2026-11-16T08:11 [lead]

status active -> ready

## 2026-11-16T08:12 [planner]

edited fields

## 2026-11-16T08:13 [planner]

rescoped after attempt 1's needs_split: this leaf is now the atomic cutover. One discovery function (config::manifest_path pub(crate), cli::fmt_path deleted) called by Config::from_env, stage0, check::read_manifest and fmt; Manifest::parse_blastfile at every entry point; design 15.1's two sentences verbatim with exit 1 for boot/dev and 2 for check/fmt; --manifest refusal reworded; fmt re-parses its own output (R25); cli.rs folds through error::{clipped,escape_control}, so crates/blastit-manifest/src/{lib,error}.rs are in paths. Fixture translation is atomic with the flip: 9 crates/blastit/tests suites, the 6 python/tests/runnable_docs fixtures, the harness (staged name, READER_FILES, page-manifest, dev-learn) and docs/local.md's one harness-executed block plus its one docs_fences PAGES row. attempts reset to 0: attempt 1 landed no code because the brief's paths forbade the work, not because the work failed.
