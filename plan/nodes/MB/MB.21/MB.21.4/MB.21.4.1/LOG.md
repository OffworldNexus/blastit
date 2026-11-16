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

## 2026-11-16T08:13 [lead]

status ready -> active

## 2026-11-16T08:13 [lead]

attempt 1/3 started

## 2026-11-16T08:13 [lead]

base f2773d4

## 2026-11-16T08:24 [lead]

edited fields

## 2026-11-16T08:24 [lead]

edited fields

## 2026-11-16T08:24 [lead]

attempt 1 hit the step limit mid-leaf; the flip is on disk uncommitted (18 files, lib+tests compiling). Widened paths to the two files the flip forces: crates/blastit/src/stage1.rs (stage-1 re-reads the manifest) and crates/blastit/src/dev_learn.rs (--learn writes Blastfile edits); MB.21.4.3 then keeps only the re-spellings, --json, Python package and the e2e learn case. Attempt 2 continues the fixture translation.

## 2026-11-16T08:24 [lead]

attempt 2/3 started

## 2026-11-16T09:10 [lead]

edited fields

## 2026-11-16T09:10 [lead]

measured by lead: critical_static_cli 10/12 (282s, no hang) — an_all_static_site_boots_under_dev_with_no_app and a_readable_root_under_an_owned_0700_ancestor_is_accepted fail because the scheduler serves no mount (GET / -> 404 unmatched); a_root_the_caller_cannot_list_is_refused passes in 0.01s via stage0::mount_root_refusal (dev:2993, serve:2713, check.rs:164). The runtime half of the flip is therefore part of this commit; 4.1 re-scoped to include scheduler/** and the model's mount walk, max_attempts 6 because each session reaches its step limit on this size.

## 2026-11-16T09:10 [lead]

attempt 3/6 started

## 2026-11-16T09:22 [lead]

attempt 3 green in Rust: scheduler serves mounts (MountTable longest-prefix, MountHead refusals, Missing->view->fallback->404), model static_roots/check_static_roots over mounts (Location::Mount), mount_root_refusal deleted, php_view binds every view from Manifest::php(), protected suites rewritten. critical_static_cli 12/12, php_backend 9/9, php_embed 26/26, dev_and_fail_closed 19/19, raw_ingress 5/5, smoke 31/31, scheduler 16 suites. Remaining: [route.php] sentence re-spellings, two new mount tests, the gate runs, RESULT.md.

## 2026-11-16T09:22 [lead]

attempt 4/6 started

## 2026-11-16T09:37 [lead]

edited fields

## 2026-11-16T09:37 [lead]

attempt 4 left two gate blockers, both outside the old paths: (a) python/tests/critical/test_check_contract.py stages a v1 fixture and the PyO3 binding still parses with from_toml_str (crates/blastit-py/src/audit.rs:414); (b) check::learn still applies the retired ManifestEdits through edit::apply_edits (check.rs:985, audit_v1.rs:1255/1387), so the two check --learn fences read a Blastfile as TOML. Both are the same cutover: 4.1's paths widened to blastit-py/**, audit_v1.rs, the manifest edit surface and that protected Python suite; MB.21.4.3 narrows to the --json action vocabulary, fold/pytest plugin, the e2e learn case and the sentence re-spellings.

## 2026-11-16T09:37 [lead]

attempt 5/6 started

## 2026-11-16T09:52 [implementer]

edited fields

## 2026-11-16T09:56 [lead]

status active -> review

## 2026-11-16T10:10 [lead]

status review -> done

## 2026-11-16T10:10 [lead]

commit a3ac338; reviewer PASS (own runs: make check exit 0, 164 ok, docs-test 49/49, critical_guard 14 authorized). Six MINOR findings handed to MB.21.4.2: escaped manifest path in the missing/empty sentences (config.rs:2413, check.rs:304, stage0.rs:2583/2879), fs::read_to_string -> blastfile::read at every call site (a FIFO at ./Blastfile currently blocks check), a real fmt-regression test (print.rs's test never calls fmt), RESULT.md's missing critical_rewrite behaviour note, docs/local.md:80,92 prose (MB.21.5.1), and the duplicated pattern_is_writable / is_all_static predicates.
