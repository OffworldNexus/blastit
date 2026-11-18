# Log MB.36.5

## 2026-11-17T14:07 [planner]

created (task) Extract crates/blastit-view-php (worker, SAPI, discovery, pinning, probe), delete WorkerRuntime/runtime.rs, and shrink stage0.rs by the PHP half

## 2026-11-17T15:57 [planner]

edited fields

## 2026-11-18T15:14 [lead]

status ready -> active

## 2026-11-18T15:14 [lead]

attempt 1/3 started

## 2026-11-18T15:14 [lead]

attempt 1/3 started; base df6a5bf. NODE Context corrected before dispatch:  stays blastit-scheduler/static (design 4.2.5, already landed), and critical_loaded_files.rs left crates/blastit in MB.36.3.

## 2026-11-18T15:14 [lead]

attempt 1/3 started; base df6a5bf. NODE Context corrected before dispatch: `static` stays blastit-scheduler/static (design 4.2.5, already landed in MB.36.2), and critical_loaded_files.rs left crates/blastit in MB.36.3 so that paths entry is a no-op.

## 2026-11-18T15:51 [lead]

status active -> review

## 2026-11-18T15:52 [lead]

edited fields

## 2026-11-18T15:52 [lead]

paths widened: crates/blastit/src/host.rs. The leaf gates `pub use crate::stage0::deployment_boot_check` with #[cfg(feature = "php")] (a php-less build has no gate to expose; stage0::no_php_support refuses an `entrypoint php` with one sentence instead). RESULT.md must say this instead of "host.rs keeps compiling unchanged".

## 2026-11-18T16:17 [lead]

status review -> done
