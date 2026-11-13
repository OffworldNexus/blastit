# Log MB.1

## 2026-11-13T10:42 [strategist]

created (epic) MA Gate Follow-ups: Mutation Coverage, Startup Diagnostics & Harness Hygiene

## 2026-11-13T15:49 [lead]

MERGE HANDOVER for whoever drives this subtree next: origin/develop is ec47752 (Release v0.1.2, PR #63 merged). Merge it into release/0.2.0 ONLY when the tree is committed and no cargo-mutants run is in flight (a merge rewrites crates/*/Cargo.toml under a running mutants process and yields bogus caught/missed counts). Previewed read-only with git merge-tree: 9 conflicts, all mechanical. Resolve crates/blastit[-dbproxy|-env|-http|-manifest|-py|-scheduler|-view]/Cargo.toml to develop's side (intra-workspace requirement version 0.1.2); resolve scripts/dev-version.sh to HEAD/ours (MB.1.9's hardened version also scans the workspace-root Cargo.toml and rejects versionless path requirements - a strict superset of develop's); Cargo.toml [workspace.package] version, Cargo.lock and CHANGELOG.md auto-merge cleanly (root version 0.1.2, 0.1.2 section above the 0.2.0 work). Then verify: cargo metadata --locked --no-deps, make check, scripts/check-release-tag.sh v0.1.2.

## 2026-11-13T15:59 [lead]

DUAL-DRIVER CONFLICT (16:00): a second LIVE opencode session is driving this same tree right now (its shell dir ~/.local/share/opencode/shell/ba94ef7b*, PIDs 3500174/3500178/3500191). It authored the stranded MB.1.2/MB.1.3 edits (its /tmp/opencode/mb12-model-list.txt 15:37, corpus.diff 15:45) and resumed after the restart: it killed my cargo-mutants run (exit 143) and is now running 'make mutants-rust PKG=blastit-manifest MUTANTS_TMP=/tmp/opencode/mb12-mut-tmp' (Found 637 mutants, started 15:59). I stood down rather than race it for git and the measurement. WHAT I DID LEAVE IN THE TREE (all safe for the owner to keep): (a) cargo fmt fix in crates/blastit-sqlpolicy/tests/lexer_progress.rs (import order only; tree now passes 'cargo fmt --all -- --check'); (b) MB.1.10 docs work complete (docs/ci.md, docs/design/ma-lean-dx.md, docs/knowledge/LESSONS.md + RESULT.md), docs gates green; (c) an independent read of the stranded MB.1.2/MB.1.3 diffs: sound - production additions have no unwrap/expect/panic, all scans are structurally bounded (lex() has a .max(i+1) floor, skip_doubled/skip_block_comment/parse_form strictly shrink), the new tests pair allowed with denied cases; 'cargo test -p blastit-manifest -p blastit-sqlpolicy --locked' is green (36+17+18+3+17+13+17+33+11+6+29 tests). NOTE my aborted probes are not evidence: 206/298 lexer mutants and 637/630 manifest mutants are post-refactor counts, not measurements, and the lexer run died at 30 caught before any ratio was valid.

## 2026-11-13T18:39 [lead]

INCIDENT 17:31: a concurrent process reverted crates/blastit-sqlpolicy/src/{lru,statements,tables}.rs to HEAD and deleted the untracked tests/{policy_module_edges,policy_module_tables}.rs (mtime 17:31:05), destroying MB.1.4/MB.1.5 deliverables after their campaigns. MB.1.2 (7a41f61) and MB.1.10 (d84141f) were already committed and survived. Recovery: no scratch copy and no dangling git blob exists; both implementations are being re-created from RESULT.md + the stale 17:03 test binaries, then committed immediately. Policy from now on: commit a reviewed node the moment it passes, never leave a deliverable uncommitted across a gate.

## 2026-11-13T18:39 [lead]

OWNER DIRECTIVE: new node MB.1.11 (Bound mutants memory and keep build scratch off tmpfs) created and dispatched before any further long campaign; MB.1.3's lexer campaign must run under the new bounded invocation.
