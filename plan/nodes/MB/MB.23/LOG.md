# Log MB.23

## 2026-11-16T23:12 [planner]

created (epic) Reload what changed in blastit dev: unprivileged fanotify + exact loaded-file tracking (Python & PHP), on by default with --no-reload

## 2026-11-16T23:19 [conductor]

Owner/architecture simplification: DO NOT do selective single-route restarts — reload ALL view workers together (while keeping :8080 and proxies up) whenever any tracked loaded file changes! Tracking the union of files opened across preload + all Python/PHP route workers still tells fanotify the exact set of loaded source/template files (ignoring .git/target/logs and catching editable external packages), while eliminating stale preload/fork/shared-module skew across views and simplifying Ctl::Reload.

## 2026-11-16T23:20 [planner]

edited fields

## 2026-11-17T00:47 [planner]

edited fields

## 2026-11-17T06:20 [lead]

lead: 23.1+23.2 done (055607b, 0c2c117). Final gate: validate/guard green and every make check target green EXCEPT rs-doc, which fails on another node's uncommitted crates/blastit-manifest/src/error.rs:1523 (/ intra-doc links) — no Rust changed in 23.1/23.2/25.1. docs-test stays red on the new --no-reload fence until 23.5; 23.6 owns that run.

## 2026-11-17T06:20 [lead]

lead: correction to the line above — the rs-doc failure is two broken intra-doc links on `prefix` and `Display` at `crates/blastit-manifest/src/error.rs:1523`, in another node's uncommitted edit.
