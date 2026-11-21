# Log MB.33

## 2026-11-17T12:57 [planner]

created (task) Resolve blastit lsp without uv: Zed LspSettings -> which(blastit) -> GitHub Release download, with the exact [[capabilities]] the host grants and a guard that fails on extras

## 2026-11-17T13:18 [planner]

edited fields

## 2026-11-18T05:21 [lead]

PLAN CORRECTION from MB.35.1 + its reviewer (this NODE.md is wrong; the brief you will receive overrides it): (a) an extension must declare NO download capability — zed_extension_api-0.7.0 capability_granter.rs:49-65 never consults the extension.toml manifest for download_file (unlike allow_exec at :28); the gate is a USER setting (default host="*") checked pre-flight. Declaring hosts FAILS CLOSED. So drop the "[[capabilities]] with the exact hosts / guard that fails on extras" obligation and instead prove the manifest declares none. (b) the browser release URL 302s to release-assets.githubusercontent.com (verified num_redirects=1), not objects.githubusercontent.com. (c) resolution order stays LspSettings -> which(blastit-lsp) -> which(blastit) lsp -> release download; Zip EXTRACTS, so the archive root must hold blastit-lsp.exe; make_file_executable is a no-op off Unix.

## 2026-11-18T07:26 [lead]

status ready -> active

## 2026-11-18T07:26 [lead]

attempt 1/3 started

## 2026-11-18T07:26 [lead]

base ef93363

## 2026-11-18T07:45 [lead]

status active -> review

## 2026-11-18T07:45 [lead]

owner done: settings -> which(blastit-lsp) -> which(blastit) lsp -> Release download (GzipTar x4, Zip on Windows, relative <version>/<target>/), make_file_executable unconditional, PATH/SYSTEMROOT allowlist, binary.env ignored; extension.toml declares NO capabilities; guard 59 properties / 41 self-tests (15 new twins); wasm32-wasip2 build 0 warnings; make check EXIT=0.

## 2026-11-18T07:55 [lead]

REVIEW FAIL (attempt 1): MAJOR editors/zed/src/lib.rs:169 — latest_github_release runs BEFORE the cache hit at :179, so an offline/rate-limited machine that already has blastit-lsp-<version>/<target>/ fails with no server; NODE.md:65 and design §8.3a require "a second start does no network I/O" and the amendment that records the code instead gives a wrong reason (a read_dir entry IS both paths). Plus MINORs: lib: cache pins only the order, not the predicate; no twin for a binary.env regression; lib: arguments does not pin the empty-args arms.

## 2026-11-18T07:55 [lead]

status review -> ready

## 2026-11-18T07:55 [lead]

attempt 2/3 started

## 2026-11-18T08:20 [lead]

reviewer 2: VERDICT PASS — cache-first verified (metadata before latest_github_release, version-free EXTRACT_DIR so both paths name one file), promise consistent in code/page/design incl. the refresh cost, guard pins CACHE_HIT verbatim + 44 tests, wasm build 0 warnings, make check EXIT=0. Two MINORs: document the "clear the work dir to refresh" remedy on the CLI page; README remains unattributed.

## 2026-11-18T08:55 [lead]

status ready -> done

## 2026-11-18T08:55 [lead]

committed 233c959
