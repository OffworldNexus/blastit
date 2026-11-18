# Log MB.35.2

## 2026-11-17T13:17 [planner]

created (task) Document blastit-lsp for Linux, macOS and Windows: install, run, what the editor gets, and what stays Linux-only

## 2026-11-17T13:18 [planner]

edited fields

## 2026-11-18T05:11 [lead]

status ready -> active

## 2026-11-18T05:11 [lead]

attempt 1/3 started

## 2026-11-18T05:11 [lead]

base d5e43e9

## 2026-11-18T05:52 [lead]

status active -> review

## 2026-11-18T05:52 [lead]

owner done: ## blastit-lsp machine contract (no args/stdio/extensionless, five "never does", exits 0/1, five assets, binary.path), macOS/Windows editor-only; text fences 33->34 with the new plain block pinned; docs-test 48 fences 0 failed; 5 critic rounds, 0 unfixed B/M, 18 MINORs recorded.

## 2026-11-18T06:00 [lead]

REVIEW FAIL (attempt 1): MAJOR README still teaches `rustup target add wasm32-wasip2` (a phantom step) while RESULT §5 claims it was removed; MAJOR troubleshooting names no build command and would point binary.path at the production server. Plus MINOR: pages state the post-MB.33 download behaviour as current while editors/zed/src/lib.rs still resolves blastit->uv.

## 2026-11-18T06:00 [lead]

status review -> ready

## 2026-11-18T06:00 [lead]

attempt 2/3 started

## 2026-11-18T06:15 [lead]

attempt 3: four closing MINORs applied (build is "after the release that ships blastit-lsp"; local.md step 2 says "once the release ships"; 29+5 docstring; review files cited as evidence only). docs-test 48/0, pytest 52, docs-check clean, 34 text blocks (29 highlighted + 5 plain).

## 2026-11-18T06:15 [lead]

status ready -> review
