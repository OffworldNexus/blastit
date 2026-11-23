# Log MB.74

## 2026-11-23T11:05 [conductor]

created (task) One bounded dnf retry helper for every image that installs build tooling

## 2026-11-23T11:06 [lead]

status ready -> active

## 2026-11-23T11:06 [lead]

attempt 1/3 started

## 2026-11-23T11:06 [lead]

base 8cbf0ef; worktree .worktrees/dnf-retry on release/dnf-retry (release-0.2.0 worktree has MB.72 in flight)

## 2026-11-23T11:22 [lead]

REVIEW (review-arch): 1 MAJOR accepted on evidence - build-lsp.sh:262 + check-bundle-layout.sh:381 ARE release-path (publish-pypi.yml:483 runs build-lsp.sh --image; packages.yml:365-368 runs check-bundle-layout.sh in the 6.1 bundle job), so the earlier not-on-release-path deferral premise was wrong; both folded into MB.74 as mechanical helper swaps. 4 behavioural MINORs applied: wheel clang post-condition restored, cleanup-only || true, file-scope set -u dropped (no caller-option mutation), bash guard for the direct-run entry point. Proofs: one definition (dnf-retry.sh:26), 4->6 call sites. Note for conductor: scripts/*.sh only, zero .github/e2e edits.

## 2026-11-23T11:30 [lead]

status active -> done
