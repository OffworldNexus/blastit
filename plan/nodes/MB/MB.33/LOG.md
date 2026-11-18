# Log MB.33

## 2026-11-17T12:57 [planner]

created (task) Resolve blastit lsp without uv: Zed LspSettings -> which(blastit) -> GitHub Release download, with the exact [[capabilities]] the host grants and a guard that fails on extras

## 2026-11-17T13:18 [planner]

edited fields

## 2026-11-18T05:21 [lead]

PLAN CORRECTION from MB.35.1 + its reviewer (this NODE.md is wrong; the brief you will receive overrides it): (a) an extension must declare NO download capability — zed_extension_api-0.7.0 capability_granter.rs:49-65 never consults the extension.toml manifest for download_file (unlike allow_exec at :28); the gate is a USER setting (default host="*") checked pre-flight. Declaring hosts FAILS CLOSED. So drop the "[[capabilities]] with the exact hosts / guard that fails on extras" obligation and instead prove the manifest declares none. (b) the browser release URL 302s to release-assets.githubusercontent.com (verified num_redirects=1), not objects.githubusercontent.com. (c) resolution order stays LspSettings -> which(blastit-lsp) -> which(blastit) lsp -> release download; Zip EXTRACTS, so the archive root must hold blastit-lsp.exe; make_file_executable is a no-op off Unix.
