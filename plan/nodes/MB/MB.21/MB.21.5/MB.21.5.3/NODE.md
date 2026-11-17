---
id: MB.21.5.3
title: "Audit and mutation-test the Blastfile parser, CST and editor (test-curator)"
kind: task
status: done
owner: "test-curator"
priority: 3
depends_on: [MB.21.4.4, MB.21.5.2]
paths: ["docs/testing/**", "docs/testing/AUDIT-MB.21.5.3.md", "plan/nodes/MB/MB.21/MB.21.5/**"]
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-17T05:00"
---

# Goal

Audit the new parsing surface the way a release gate requires: `test-curator` reads the lexer, parser,
pattern compiler, CST printer and surgical editor, and `cargo-mutants` measures `blastit-manifest` on
those modules — every meaningful surviving mutant becomes a follow-up node, not a note. The three
gaps the toolchain already measured (the purity needle's nested-`use` evasion, the P4 no-write arms,
the documented nits) are part of the pass, not folklore to repeat.

# Context

- Skill `testing`: critical tier proves allow **and** deny; a test that passes with the policy removed is
  fluff. `docs/knowledge/LESSONS.md` warns that a `cargo-mutants` campaign needs its own scratch dir
  (`MUTANTS_TMP=/tmp/opencode/MB.21.5.3-mut-tmp` is wrong here — use the Makefile default under the
  cache dir, one campaign per checkout) and that `outcomes.txt` does not exist in 27.1.0: read
  `mutants.out/{caught,missed,timeout,unviable}.txt`.
- Scope: `crates/blastit-manifest/src/blastfile/**` (lexer, parser, pattern, CST, printer, editor glue)
  plus `edit.rs`. Target **≥ 75 %** killed on those modules; timeouts must be bounded, not tolerated
  (a mutant that cannot terminate is an untested loop — `LESSONS.md` on `i += 1`).
- Also audit the protected suites for weakened assertions after the fixture rewrites
  (`critical_manifest_edit*.rs`, `edit_branches.rs`, `critical_blastfile_*.rs`): every rewrite must
  assert as much as it did before.
- Report to `docs/testing/AUDIT-MB.21.md`: scope, per-file ratio table, the command lines, the surviving
  mutant list with a one-line judgement each, and the follow-up node ids you opened.

# Acceptance

```console
$ make mutants-rust PKG=blastit-manifest MUTANTS_TMP=$(make -s print-mutants-tmp)/MB.21.5.3
$ wc -l $(make -s print-mutants-tmp)/MB.21.5.3/results-*/mutants.out/{caught,missed,timeout,unviable}.txt
$ UV_OFFLINE=1 uv run --no-project python scripts/test_manifest_purity.py
$ make test-critical
$ uv run --no-project tools/plan.py tree --under MB.21.5
$ ls docs/testing/AUDIT-MB.21.md
```

1. `docs/testing/AUDIT-MB.21.md` contains the ratio computed from `mutants.out/*.txt` (≥ 75 % killed on
   the parsing/editor modules), the exact commands, and a table of survivors with a verdict each.
2. Every survivor that can let a malformed or hostile document through is filed as a new task under
   `MB.21.5` (or the next release's epic) with the mutant's file:line — a survivor that is merely an
   equivalent mutant says why in one line.
3. Zero timeouts remain, or each is bounded and explained.
4. `make test-critical` exit 0 with zero skips; no protected suite lost an assertion (the report lists
   each rewritten suite and what it now asserts).
5. The purity needle's nested-`use` evasion is *measured* (the scratch fixture and `check_manifest_purity.py`'s
   output are pasted in the report) and filed as a follow-up node with the fixture text and the needle
   that would catch it; the two P4 no-write gaps are filed the same way. Nothing in this list is only a
   sentence in the report.
6. `make check` still exit 0 after the audit.

# Notes

- One campaign per checkout: do not run `make mutants-clean` or a second campaign while this one runs.
- If a mutant survives only because a fixture is too small, add the fixture in the same node and say so;
  do not defer a one-line fix to a follow-up.
- **Purity-gate evasion (measure it, do not assume it).** `scripts/check_manifest_purity.py` matches a
  literal needle, so R24 is evadable by a nested use list: `use std::{env};` plus `env::var(..)` never
  spells the banned path in one piece. Prove it with a scratch fixture under `/tmp/opencode/` (this
  node's `paths` are `docs/testing/**`, so the committed fixture and the widened needle belong to a
  follow-up node — file it with the exact fixture text and the needle that catches it). A rule a
  one-line edit evades is not a gate, so this is a finding with a node id, never a footnote.
- **The P4 no-write arms.** `write.rs::first_difference` has no `unresolved_grants` arm, so a P4
  refusal is blamed on "the top-level keys"; and `BlastfileEdits::apply_to` has no direct test at all,
  so a mutation of it is killed by nothing. Both are ordinary gaps, not equivalent mutants — file them
  with the mutant's `file:line`.
- **The documented nits, each with a verdict** (a survivor here is either filed with its mutant id or
  declared equivalent with one sentence): `UnrestrictedWithContent` is unreachable through the grammar;
  `PgGrant::with_unrestricted` is `#[doc(hidden)] pub` and unused outside the crate;
  `Manifest::from_parts` still takes `version`/`global_groups` (always `1`/empty from a `Blastfile`);
  `lower.rs` carries one unjustified `#[allow(clippy::type_complexity)]`;
  `critical_blastfile_fmt.rs`'s write-failure test returns silently under uid 0, so on that host it
  proves nothing; `fmt` reuses `TooLarge` for the *formatted* length; and a comment hoisted out of an
  unwrapped list is not covered by §14.3's owner oracle.
- Two nits already have owners and must not be re-filed here: `cli.rs`'s `one_line_text` duplicate and
  `fmt`'s missing re-parse are `MB.21.4.1`'s (R25), and the two `edit.rs` entry points plus the §18
  echo fixes are `MB.21.4.3`'s. Confirm they landed before crediting them as killed mutants.
