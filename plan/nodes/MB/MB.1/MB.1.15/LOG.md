# Log MB.1.15

## 2026-11-13T18:44 [test-curator]

created (task) Close the remaining blastit-manifest survivors and protect the patcher's wrong-route case

## 2026-11-13T18:53 [lead]

status ready -> active

## 2026-11-13T18:53 [lead]

attempt 1/3 started

## 2026-11-13T18:53 [lead]

base 9c523e2

## 2026-11-13T19:00 [implementer]

Read NODE/AUDIT; added 3 parse.rs error-list tests (mutation_gaps), 2 write_atomic path-branch tests (edit_branches, re-exec child for the bare name + EACCES twin), 3 protected patcher-provenance cases (critical_manifest_edit); 4 mutant probes each fail on a /var/tmp copy; full campaign running (TMPDIR=/var/tmp/mb115-mut).

## 2026-11-13T19:47 [implementer]

Done: 637 mutants 558 caught / 6 missed / 73 unviable / 0 timeouts = 98.94% (was 550/14 = 97.52%); parse.rs:197+220, edit.rs:384+411, 794/815(+→-), 818/819 all closed; 6 survivors each with an equivalence proof in RESULT.md; tests +3/+2/+5 (mutation_gaps/edit_branches/critical_manifest_edit), critical file purely additive; rs-lint, rs-doc, rustfmt, plan validate green; critical_guard authorizes this file (the one violation is the add_group leaf's critical_check_engine.rs).

## 2026-11-13T19:48 [lead]

status active -> review

## 2026-11-13T19:49 [lead]

status review -> done
