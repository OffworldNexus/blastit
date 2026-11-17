# Log MB.31.4

## 2026-11-17T12:33 [planner]

created (task) Hover and tree-sitter teach bare immutable and regex: regenerate parser.c, then re-pin the Zed grammar rev

## 2026-11-17T12:50 [conductor]

Polish in editors/zed/src/lib.rs while touching editors/: check zed::settings::LspSettings::for_worktree('blastit-lsp', worktree) binary.path/arguments/env first before falling back to worktree.which('blastit') -> uv, so users can also point Zed directly at a local build path if not on PATH.
