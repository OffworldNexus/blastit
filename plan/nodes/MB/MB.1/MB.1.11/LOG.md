# Log MB.1.11

## 2026-11-13T18:38 [lead]

created (task) Bound mutants memory and keep build scratch off tmpfs

## 2026-11-13T18:39 [lead]

status ready -> active

## 2026-11-13T18:39 [lead]

attempt 1/3 started

## 2026-11-13T18:39 [lead]

base 51dced9

## 2026-11-13T18:47 [implementer]

implemented: MUTANTS_TMP on disk (XDG cache), prune+preflight+retention, systemd-run --user --scope MemoryMax=12G MemorySwapMax=0 with logged fallback, mutants-clean/scope-probe/print-mutants-tmp; measured blastit-env 2-job campaign: 2.2 GB scratch, 2.9 GiB scope memory.peak, memory.max=12884901888 inside the scope; .gitignore documents /.worktrees/ and guard.py proven to skip it. NOTE for lead: NODE.md paths should gain '.gitignore' (owner's second directive) - frontmatter is not mine to edit.

## 2026-11-13T18:49 [lead]

status active -> review

## 2026-11-13T18:50 [lead]

status review -> done
