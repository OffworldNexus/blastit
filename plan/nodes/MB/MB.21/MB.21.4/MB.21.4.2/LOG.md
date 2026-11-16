# Log MB.21.4.2

## 2026-11-16T00:38 [planner]

created (task) Prove runtime parity on Blastfile: static precedence, ingress modes, entrypoint php, traits, grants

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T08:04 [planner]

edited fields

## 2026-11-16T08:12 [planner]

edited fields

## 2026-11-16T08:13 [planner]

narrowed: runtime parity plus design 4.3's removals only. The fixture-translation framing (rewrite/replace fixtures in the crates/blastit/tests suites) is deleted because MB.21.4.1 converts them atomically with the flip; the enumerated test paths are replaced by the existing crates/blastit/tests/** glob, and acceptance gains a no-fixture-translation check. Parity suite, sentence re-spellings, scheduler and manifest work unchanged.

## 2026-11-16T09:11 [planner]

edited fields

## 2026-11-16T10:38 [planner]

edited fields

## 2026-11-16T10:39 [planner]

re-scoped to what MB.21.4.1 (a3ac338) actually landed: 4.1 absorbed design 4.3's runtime half, so this leaf's removals work list is gone. Measured on the tree: the scheduler serves static mounts (MountTable longest prefix, MountHead refusals, Missing->view->fallback GET/HEAD->404, 405 after views, RouteInfo::{static_policy,static_forward} deleted), model static_roots/check_static_roots iterate mounts (Location::Mount), stage0::mount_root_refusal deleted, php_view binds every view from Manifest::php(), SpaLost->FallbackMissing with 9.4's url_path join, per-mount immutable caps + index/fallback dotfile refusal in validate, and every make-check fixture is a Blastfile. Route::{static_policy,with_static,php,with_php} survive because blastit-manifest/src/parse.rs (deleted in 4.5) and validate's route arms read them, so 4.3's removal row is MB.21.4.5's now; the acceptance grep is scoped to exclude model.rs/parse.rs and the item says so, instead of demanding an absence 4.2 cannot produce. New scope: (a) crates/blastit-scheduler/tests/critical_blastfile_parity.rs - one named runtime case per capability-map row through Options::blastfile/World; (b) only the boot/check sentences 4.3 does not own (stage0 static_app_refusal + libphp-absent, check's nothing-to-learn and the R14 unrequested-root hint, which is dead on a Blastfile today because it filters route.static_policy); 13.2/10.2/11.3 stay 4.3's; (c) the five MINORs 4.1's reviewer handed here (escaped manifest path via error::clipped, fs::read_to_string -> blastfile::read so a FIFO cannot block check, a real fmt-regression test, one pattern_is_writable, the disagreeing static-only predicates). depends_on [MB.21.4.6], max_attempts 5, paths widened to the files the work touches (stage1.rs for the same read/escape sites, scheduler src only to close a gap the suite proves).

## 2026-11-16T10:39 [lead]

status ready -> active

## 2026-11-16T10:39 [lead]

attempt 1/5 started

## 2026-11-16T10:39 [lead]

base e223753

## 2026-11-16T11:09 [lead]

status active -> review

## 2026-11-16T11:15 [lead]

REVIEW FAIL (attempt 1): [MAJOR] critical_blastfile_parity.rs claims every case boots through common::Options::blastfile but 5 of 14 rows (trait grants, unrestricted, table/column grants, normalised query, entrypoint php) only call Manifest::effective_grants — the lowering, already proved by the manifest crate's suite — so the set stays green if every view gets every grant; boot the row and assert what the product hands the view, or move the four grant rows to the manifest suite and rewrite the header/RESULT claim. [MINOR] dev_learn.rs:441,444,468 still read_to_string + manifest.display(); [MINOR] route.rs:31-32 promises 'no [route.static] policy' for a field deleted in 4.1 and is unrecorded.

## 2026-11-16T11:15 [lead]

status review -> ready

## 2026-11-16T11:15 [lead]

attempt 2/5 started

## 2026-11-16T11:31 [lead]

status ready -> review

## 2026-11-16T11:43 [lead]

reviewer PASS (round 2). Gate flake noted: dev_boots_five_unprivileged_roles_and_strips_secrets failed once inside the full make check (POST /env answered 200 where the test reads 413 from its dev client), passed 3/3 in isolation and the re-run make check exited 0 with 166 suites — a load flake in the dev test client's response pairing, not a product refusal difference.

## 2026-11-16T11:43 [lead]

status review -> done
