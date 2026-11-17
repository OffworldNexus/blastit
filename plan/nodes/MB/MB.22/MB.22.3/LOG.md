# Log MB.22.3

## 2026-11-16T09:16 [planner]

created (task) Add structured manifest spans and implement blastit lsp (stdio JSON-RPC) with protected framing/UTF-16/diagnostics/formatting tests

## 2026-11-17T06:03 [lead]

status ready -> active

## 2026-11-17T06:03 [lead]

attempt 1/3 started

## 2026-11-17T06:03 [lead]

base 053b56e

## 2026-11-17T06:23 [implementer]

implemented: ManifestError::message() + exhaustive position() (34 explicit arms, no wildcard), blastfile::sites + check()/Checked, tests/warning_sites.rs; new lsp.rs (framing/UTF-16/diagnostics/formatting/hover) + Command::Lsp with caps::hold_none before run; protected tests/critical_lsp.rs (41 tests). cargo test -p blastit-manifest (517 passed), critical_lsp (41), blastit --lib (225), clippy -D warnings, fmt --check, plan validate, make rs-doc all green.

## 2026-11-17T06:28 [lead]

status active -> review

## 2026-11-17T06:39 [lead]

edited fields

## 2026-11-17T06:39 [lead]

paths widened to crates/blastit/src/lsp/** so the 940-line lsp.rs can split its hover data out (§1's 750-line bound)

## 2026-11-17T06:43 [implementer]

minor round: removed the masking empty-body frames (over-cap and junk-header firings now each read their own -32700); Session::drain_stdout + empty-after-exit assertions in both stderr sessions, with drain_stdout_sees_an_unread_frame as the positive control; children_of now panics on an unreadable /proc/<pid>/task/<tid>/children instead of skipping; HOVER_TABLE/hover_text/env_var_paragraph/is_env_name moved to crates/blastit/src/lsp/hover.rs (lsp.rs -> lsp/mod.rs, data only). 42 critical tests, 517 manifest tests, clippy/fmt/rs-doc/rs-no-python green; mod.rs 749 non-comment lines.

## 2026-11-17T06:43 [lead]

status review -> done
