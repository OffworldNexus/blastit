# Log MB.35.5

## 2026-11-17T13:17 [planner]

created (task) Build, smoke and release blastit-lsp on five targets from platform_matrix, with workflow and packaging gates kept honest

## 2026-11-18T06:43 [lead]

status ready -> active

## 2026-11-18T06:43 [lead]

attempt 1/3 started

## 2026-11-18T06:43 [lead]

base 143afaf

## 2026-11-18T06:57 [lead]

status active -> review

## 2026-11-18T06:57 [lead]

owner done: lsp family in platform_matrix (5 legs, --matrix/--count/--leg/--summary, PROVEN per target), build-lsp.sh (manylinux, getcap empty, LICENSE+SHA256SUMS), lsp_smoke.py (4:15-4:20 severity-1, exit 0, 0 stderr), ci.yml lsp job + publish lsp-* family with --count lsp; 54 packaging cases. Linux x86_64 PROVED (smoked in Debian 12 too); 4 legs declared-unproven NOT RUN. Also: matrix CLAIM walk now reads zensical exclude_docs, so a review file can no longer red the gate.

## 2026-11-18T07:06 [lead]

REVIEW FAIL (attempt 1): Linux x86_64 is proved (layout, glibc 2.28, getcap empty, no libpython, 0-stderr smoke) and the review-file gate works both ways, but "one table" is broken twice: build-lsp.sh:74-93,356 re-spells all five triples + exe/format map + archive name, and publish-pypi.yml:881 hardcodes the linux triples. Plus --summary takes no family, NOT RUN names no ci_command, non-nullglob arrays, 4 unformatted scripts, NODE.md:85 include key.

## 2026-11-18T07:06 [lead]

status review -> ready

## 2026-11-18T07:06 [lead]

attempt 2/3 started

## 2026-11-18T07:22 [lead]

status ready -> review

## 2026-11-18T07:22 [lead]

attempt 2: one table restored (build-lsp.sh matches OS shapes, assets come from --archive/--dir/--executable; release counts blastit-*.tar.gz minus lsp), --summary [FAMILY], per-leg ci_command, nullglob, ruff clean, rule 17/18 drift guard self-tested both ways. 447 checks / 56 cases / make check exit 0. Reviewer 2 dispatched.

## 2026-11-18T07:35 [lead]

reviewer 2: VERDICT PASS — no full triple outside platform_matrix.py, release job target-free with nullglob, --summary lsp 5 legs each with its ci_command, 447 checks / 56 cases, drift guard proved BOTH ways (add a leg -> 6 legs, workflows unchanged; re-spell a triple -> red at build-lsp.sh:465). Three MINORs recorded.

## 2026-11-18T07:55 [lead]

reviewer 2: VERDICT PASS — 0 full triples outside the table (only comments/derived names), release count target-free with nullglob, --summary lsp 5 legs with verbatim ci_command, 449 checks / 56 cases, drift proved both ways; the Apple sha256sum blocker was fixed mid-review (shasum selector + per-leg sha_verify) and verified with both tools. Six MINORs recorded (PYTHON_VERSION env, --image re-exec name forwarding, rule 17 not scanning ci.yml, NODE.md:85, RESULT predates the fix, 4 foreign ruff files).

## 2026-11-18T08:55 [lead]

status review -> done

## 2026-11-18T08:55 [lead]

committed 1d42a49
