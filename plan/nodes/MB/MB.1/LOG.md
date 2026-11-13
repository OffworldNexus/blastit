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

## 2026-11-13T18:52 [lead]

MERGED origin/develop v0.1.2 as 9c523e2 (9 conflicts: 8 crates/*/Cargo.toml -> develop 0.1.2, scripts/dev-version.sh -> ours). Verified cargo metadata --locked --no-deps, check-release-tag.sh v0.1.2, release-notes --require-dated v0.1.2, make check exit 0 (136s).

## 2026-11-13T18:57 [lead]

GATE NOTE: after the develop merge bumped the workspace to 0.1.2, 'make e2e' refused to start - 'a prebuilt e2e image installs exactly one wheel from dist, found 2' (a stale blastit-0.1.1 wheel beside the new 0.1.2 one). dist/ is gitignored build output, so the fix was 'rm -f dist/blastit-0.1.1-*.whl' then re-run; worth a LESSONS bullet (and it is the kind of thing a version bump always causes locally).

## 2026-11-13T19:02 [lead]

GATE EVIDENCE: make e2e exit 0 at 9c523e2 + dist 0.1.2 (130s): 'ok a route granting queries = [] refuses an ordinary statement (SQLSTATE 42501)', the log check 'the queries = [] route refused an ordinary statement with SQLSTATE 42501', and D6 'an unprivileged boot with no capabilities is refused' with 'init runs as uid 10000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid'. Closes audit gap G7 for e2e.

## 2026-11-13T19:56 [lead]

status ready -> done

## 2026-11-13T19:56 [lead]

GATE CLOSED 06013a6. Two test-curator audits of the same diff exist (docs/testing/AUDIT-MB.1.md F1-F10 committed at 9018a8b by a concurrent session, and AUDIT-MB1.md C1-C9/G1-G7 committed with this gate); every gap either audit filed is closed by MB.1.12-MB.1.17, so the duplication is documentation debt only - one of the two should be folded or deleted at the next plan hygiene pass.
