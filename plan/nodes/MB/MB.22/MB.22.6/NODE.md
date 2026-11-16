---
id: MB.22.6
title: "Audit and mutation-test blastit lsp, the manifest spans, the grammar queries and the editors guard (test-curator)"
kind: task
status: ready
owner: "test-curator"
priority: 3
depends_on: [MB.22.3, MB.22.4, MB.22.5]
paths: ["docs/testing/AUDIT-MB.22.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T09:16"
updated: "2026-11-16T09:16"
---

# Goal

Audit the new editor surface the way a release gate requires, and turn every meaningful gap
into a follow-up node rather than a note.

Read, run and judge:

- `crates/blastit/tests/critical_lsp.rs` — framing caps, malformed-frame survival, lifecycle
  exit codes, full-sync `didChange`, `didClose` clear, diagnostics severity/source/message,
  the UTF-16 `# ✓ ok` fixture, formatting (equal → `[]`, diff → one edit, broken → `null`),
  hover, stdout purity and stderr redaction. Prove each suite has teeth: the UTF-16 test must
  fail if the conversion drops to `col - 1`; the framing test must fail if the length is
  counted in chars; the formatting test must fail if `end` is the last non-empty line. A test
  that would pass with the conversion removed is fluff and must be rejected.
- `ManifestError::position()` / `message()` and the warning positions — exhaustiveness,
  `NONE` justifications, and the CLI/editor parity (the same sentence without the location
  prefix). Mutation-test `crates/blastit-manifest` (target ≥ 75 % killed on `error.rs` and the
  `blastfile` span modules); judge every meaningful survivor.
- The grammar corpus and queries: the four traps (`view view GET "/x"`, keyword as a name,
  newline after a trailing comma, multi-line raw string) actually fail when the corresponding
  rule is removed; every capture in `highlights.scm` is in the 45-theme allowlist; the
  `@variable.special @variable` pair is present; `outline.scm`/`indents.scm`/`brackets.scm`
  carry their required captures; `path_suffixes` is exact.
- `scripts/check_zed_extension.py` + `scripts/test_check_zed_extension.py`: every negative
  self-test removes the property it names and goes red (rev, `[workspace]`, license,
  `path_suffixes`, capture, `@item`/`@indent`, committed wasm); the guard cannot pass on a
  short or branch rev.
- `python/tests/critical/test_lsp_stdio.py`: both routes (standalone via
  `BLASTIT_LSP_COMMAND`, console script under `test-critical`), the EOF-without-shutdown
  exit 1, and no vacuous assertions on empty output.
- The protected-file diff: no existing `crates/*/tests/critical_*.rs` or
  `python/tests/critical/` test was weakened or deleted (`tools/critical_guard.py` against
  the base).
- The docs claims: the editor page's commands, the `format_on_save` line, and "the editor
  says what `blastit check` says" are true against the live binary; flag any claim the code
  does not make.

Report to `docs/testing/AUDIT-MB.22.md` in the `AUDIT-*` format: date, scope with commit/rev,
requirements, a **Verdict**, method, tier/teeth, mutation evidence (per-file table, command
lines, survivor list with a one-line judgement each), and the follow-up node ids. Open a
follow-up `plan.py` node for each meaningful surviving mutant or missing oracle; do not
leave it in prose.

# Context

- Inputs: `MB.22.3`–`MB.22.5` `RESULT.md`s and `LOG.md`s, the accepted design
  `docs/design/blastfile-lsp-zed.md`, `ADR-0013`, the research §5–§7, and skill `testing`
  (critical tier proves allowed **and** denied; no skip; mutation target 75 %).
- `cargo-mutants` lessons: scratch under `MUTANTS_TMP` (one campaign per checkout), read
  `mutants.out/{caught,missed,timeout,unviable}.txt` — `outcomes.txt` does not exist in
  27.1.0. The Makefile target is `make mutants-rust PKG=blastit-manifest`; for the LSP
  module use `cargo mutants -p blastit --file crates/blastit/src/lsp.rs` with the same
  `MUTANTS_TMP`, timeout and `--features python` as the Makefile.
- The grammar and extension are data + queries; mutation testing does not apply. Their
  teeth are the corpus, the query runs and the static guard's negative tests — run them,
  and try one deliberate regression each to see the gate go red.
- The audit is read-only outside `docs/testing/AUDIT-MB.22.md`; it files tasks, it does not
  fix code. Do not touch the protected tests.
- **Language rules:** never mention the retired manifest filename; never describe a PHP
  alternative transport.

# Acceptance

```console
$ cargo test -p blastit --test critical_lsp --locked
$ cargo test -p blastit-manifest --locked
$ make zed-check
$ make test-critical
$ BLASTIT_LSP_COMMAND="$PWD/target/debug/blastit lsp" uv run --no-project python -m unittest discover -s python/tests/critical -p 'test_lsp_stdio.py' -v
$ cd editors/tree-sitter-blastfile && npx --yes tree-sitter-cli@0.25 test
$ make mutants-rust PKG=blastit-manifest
$ uv run --no-project tools/critical_guard.py --base "$(git merge-base HEAD origin/develop)" --node MB.22
$ uv run --no-project tools/plan.py validate
```

1. `docs/testing/AUDIT-MB.22.md` exists, names the commit it read, and has an explicit
   **PASS/FAIL** verdict on tier, teeth and mutation.
2. Every suite named above was run; the report quotes the command and the pass/fail counts.
3. The mutation section has a per-file table for `blastit-manifest` (`error.rs` + span
   modules) and for `crates/blastit/src/lsp.rs` if the campaign was in scope, with killed %
   and every survivor classified equivalent or meaningful; every meaningful survivor is a
   new follow-up node id.
4. The "one deliberate regression per gate" list is recorded: what was broken, which test
   went red, and that it was reverted (working tree clean; no protected test edited).
5. The protected-file diff shows no weakened or deleted existing critical test; new
   `critical_lsp.rs` / `test_lsp_stdio.py` are additions only.
6. Every gap has a follow-up node id in the report; the report's own plan validation is
   green.

# Notes

- Do not accept "the test exists" as evidence. Open each test body and ask what it would
  still pass with the bound, conversion or refusal removed.
- The 45-capture allowlist is an intersection, not a preference: a capture missing from one
  bundled theme renders plain, so a "missing style" is not a query bug but a design bug —
  report it as such.
- Keep the report under ~150 lines; the survivors table is the valuable part.
