---
id: MB.21.5.3
title: "Audit and mutation-test the Blastfile parser, CST and editor (test-curator)"
kind: task
status: ready
owner: "test-curator"
priority: 3
depends_on: [MB.21.4.4, MB.21.5.2]
paths: ["docs/testing/AUDIT-MB.21.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Audit the new parsing surface the way a release gate requires: `test-curator` reads the lexer, parser,
pattern compiler, CST printer and surgical editor, and `cargo-mutants` measures `blastit-manifest` on
those modules — every meaningful surviving mutant becomes a follow-up node, not a note.

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
$ make mutants-rust PKG=blastit-manifest
$ wc -l $(find . -path '*mutants.out*' -name 'caught.txt' -o -path '*mutants.out*' -name 'missed.txt' | head -2)
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
5. `make check` still exit 0 after the audit.

# Notes

- One campaign per checkout: do not run `make mutants-clean` or a second campaign while this one runs.
- If a mutant survives only because a fixture is too small, add the fixture in the same node and say so;
  do not defer a one-line fix to a follow-up.
