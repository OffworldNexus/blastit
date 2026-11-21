# Log MB.3.62

## 2026-11-15T20:57 [lead]

created (task) The two-package image must prove what it claims, and its probe table must not be able to drift

## 2026-11-15T20:57 [lead]

status ready -> active

## 2026-11-15T20:57 [lead]

attempt 1/3 started

## 2026-11-15T20:57 [lead]

base 71dcaf8

## 2026-11-15T21:06 [lead]

NOTE: two runner failures on 71dcaf8 (MB.3.63 = standalone read-only CARGO_HOME; MB.3.64 = the shim-swap race in smoke (root)) are release-blocking and take the gate after you. Your two items are unchanged and still worth landing, but they must not delay MB.3.63/64 if the tree gets busy — report promptly.

## 2026-11-15T21:40 [lead]

attempt 2/3 started

## 2026-11-15T21:40 [lead]

attempt 2/3 started; attempt 1 died with no work on disk (git status clean under crates/blastit/tests and e2e/php-sapi). base 71dcaf8

## 2026-11-15T21:46 [lead]

status active -> review

## 2026-11-15T21:51 [lead]

reviewer VERDICT: PASS (base 71dcaf8; drift bite reproduced at entry 4; make check exit 0 at review time). LEAD make check after review: exit 2 on ONE finding outside this node — python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_php_vocabulary -> plan/nodes/MB/MB.20/NODE.md:142 'FastCGI'. MB.20/ is an UNTRACKED sibling brief created 21:50 (after the reviewer's green run; depends_on MB.3.65), it is not part of HEAD and touches none of MB.3.62's files. Not a .62 defect; must be reworded by MB.20's own writer so every MB.3 gate stops inheriting it.

## 2026-11-15T21:51 [lead]

status review -> done

## 2026-11-15T21:51 [lead]

commit 646c4aa
