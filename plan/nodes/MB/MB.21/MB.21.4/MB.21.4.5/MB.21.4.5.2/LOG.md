# Log MB.21.4.5.2

## 2026-11-16T16:57 [planner]

created (task) Prove the static mount's and the php entrypoint's grammar in the two protected suites

## 2026-11-16T17:38 [lead]

status ready -> active

## 2026-11-16T17:38 [lead]

attempt 1/3 started

## 2026-11-16T17:38 [lead]

base 2ef786ce381dce05208d8d4d09a50f8eb4d7589b

## 2026-11-16T17:46 [lead]

REVIEW FAIL: (1 MAJOR) control-byte exemplars in critical_static_manifest.rs:211,215,386,426 + critical_php_manifest.rs:124 moved from NUL/0x1F boundaries to \n/\t interiors, unpinning validate.rs has_control_byte mutants; RESULT's 'a Blastfile string can only carry \" \\ \n \t' claim is false (lexer.rs:288 copies any non-quote/non-backslash byte; critical_blastfile_entrypoint.rs:72 and blastfile_errors.rs:72 already use raw \u{1}). (2 MINOR) fallback family asserts 3 of fallback_rule's 6 branches: add fallback "" / a//b / a\\b / control-byte. (3 MINOR) static-block-without-root denial lost its ManifestError::MissingKey{variant} equality, now only a rendered string.

## 2026-11-16T17:46 [lead]

status active -> ready

## 2026-11-16T17:46 [lead]

status ready -> active

## 2026-11-16T17:46 [lead]

attempt 2/3 started

## 2026-11-16T17:53 [implementer]

attempt 2: restored raw control-byte boundary exemplars (`\u{0}` on root/index/immutable/fallback/php entry, `\u{1f}` on root/index/immutable/php entry), added the four missing `fallback_rule` branches, and restored `StaticRootMissing`'s variant equality. 15/11 tests pass; critical-guard clean for the two files (sibling leaves still trip it in the shared tree); `cargo fmt --all -- --check` and clippy clean; hand-mutation in a throwaway worktree kills `byte < 0x0B`, `byte <= 0x1E` and `byte == '\n' || '\t'`.

## 2026-11-16T17:58 [implementer]

review PASS follow-ups: both `.php` `Hidden` pointers now name `blastit-scheduler/tests/critical_static_entry.rs::php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates` (the resolve suite has no `.php` row); `fallback "a\u{1f}b"` added so the family pins both ends of `byte < 0x20` (hand-mutation of `has_control_byte` to `byte <= 0x1E` now fails `a_fallback_is_…`); root/php-entry comments name `blastfile/lower.rs`'s `root_rule`/`check_php_entry` instead of `validate.rs`'s `has_control_byte`. Greps empty, 15/11 pass, fmt-all clean.

## 2026-11-16T18:17 [lead]

pre-commit gates: plan.py validate OK; guard.py OK; make check exit 0, 4619 lines (/tmp/opencode/mb2145-batch-makecheck.log). Tree QUIESCENT at commit time — shared attestation across the 5.1-5.5 batch, five separate per-node commits off it.

## 2026-11-16T18:17 [lead]

status active -> review

## 2026-11-16T18:17 [lead]

status review -> done
