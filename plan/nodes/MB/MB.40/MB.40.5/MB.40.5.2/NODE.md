---
id: MB.40.5.2
title: "Actionable human check output (dead-grant SQL, proxy, MIME, fix.to), fmt --check unified diff & --learn --dry-run proposal"
kind: task
status: done
owner: implementer
priority: 3
depends_on: [MB.40.5.1]
paths:
  - "crates/blastit/src/**"
  - "python/**"
  - crates/blastit/tests/critical_blastfile_fmt.rs
  - crates/blastit/tests/critical_check_engine.rs
  - crates/blastit/tests/fmt_command.rs
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-19T04:27"
updated: "2026-11-19T05:13"
---

# Goal
Make `blastit check` and `blastit fmt` hand the developer the fix, not a puzzle: human `check` prints every actionable field `--json` already computes (dead-grant `sql`, `proxy`, observed MIME types, exact `fix.to`), `fmt --check` prints a real unified diff, and `check --learn --dry-run` prints the proposed `Blastfile` block(s).

# Context
`MB.40.5` group 4, split from group 3. The `--json` output already carries everything; the human path throws it away, so a developer sees a failure without the one line they must paste.

# Exact Targets (`file:line` pre-diagnosed in `/tmp/opencode/audit-ux.md`)
- `crates/blastit/src/audit_v1.rs:1189-1307` (human `check` report): render, for every problem, the dead-grant `sql`, the `proxy` name, the observed MIME types, and the exact `fix.to` (e.g. `view catalog postgres`). Use `Blastfile` nouns (`view`, `trait`), never internal ids, and correct singular/plural (`1 request`, `1 statement`, `1 problem`, `2 statements`).
- `crates/blastit/src/cli/run.rs` / `crates/blastit/src/check.rs`: human output must name the same facts `--json` emits for the same run — add a test that runs one fixture both ways and asserts each human line's facts appear in the JSON.
- `blastit fmt --check`: when the file is non-canonical, print a unified diff (`--- <path>` / `+++ <path>` with hunks) instead of a bare "not canonical".
- `blastit check --learn --dry-run`: print the proposed `Blastfile` block(s) (copy-pasteable, non-empty grants) rather than a count.

# Authorization (protected critical tests)
None. Do not touch `crates/*/tests/critical_*.rs` or `python/tests/critical/`. If a protected test pins a sentence this slice must change, STOP and report `STATUS: blocked` with the exact `file:line` — do not edit it.

# Acceptance
- `make fast-check`
- `make test-critical`
- Evidence in RESULT.md: captured real output of a failing `check` (human vs `--json` for the same run), a `fmt --check` diff, and a `--learn --dry-run` proposal; plus whatever non-protected tests pin them.

# Notes
Single Cargo writer. Reuse the existing `Colour` for ANSI; no re-probing TTY. Files <500 prod LOC; zero `unsafe`; no `unwrap()` on untrusted input.

# Authorization (protected critical tests — exactly these two, added by the lead after the blocker report)

Both files pin the *old* UX bytes that `AGENTS.md` §4 replaces ("`fmt --check` shows a diff"; human `check` must print dead-grant `sql`, `proxy`, and `fix.to`). They pin UX, not security invariants.

1. `crates/blastit/tests/critical_blastfile_fmt.rs:126-132` (`check_writes_nothing_at_all`) — the `assert_eq!` on `"./Blastfile: not in canonical layout\n"` may become an assertion that stdout carries the header **and** a unified diff for `./Blastfile` (`--- `, `+++ `, at least one `@@` hunk). Every other assertion in that test stays verbatim and must still pass: exit `1`, **`stderr` empty**, the file's bytes unchanged, `dir.read("Blastfile") == MESSY`, `dir.entries() == ["Blastfile"]` (no temp file left behind, in a directory it could not write to).
2. `crates/blastit/tests/critical_check_engine.rs:881-905` and `:920-930` — the two whole-block `assert_eq!(…, expected)` expectations may be rewritten to the new human text (each finding now also naming the dead-grant `sql`, the `proxy`, the observed MIME types and the exact `fix.to`). Preserve: the same findings, the same counts in the header/footer (`6 requests, 5 statements, 5/6 routes, 2 proxies` / `11 problems`; `0 requests … 4 problems`), one line per finding in the same order, `view`/`trait`/`proxy` nouns, correct plurals, and both negative assertions verbatim: `!human.contains("body = \"")` and `!human.contains("allow_raw")`.

Nothing else in either file may change: no test renamed, deleted, `#[ignore]`d, no count changed. `uv run --no-project tools/critical_guard.py --base 4787d44 --node MB.40.5.2` must pass.

# Authorization (addendum — the `check` header, added by the lead after review)

The header is the one sentence a developer reads first, and it currently violates two charter rules at once (`AGENTS.md` §4: "proper singular/plural (`1 request`)" and "using `view`/`trait` nouns"). Two protected tests freeze the wrong bytes, so both are authorized for this one sentence only:

3. `crates/blastit/tests/critical_check_engine.rs:1008` — `"blastit check: 1 requests, 1 statements, 1/1 routes, clean\n"` becomes `"blastit check: 1 request, 1 statement, 1/1 view, clean\n"`, and every other header expectation in that file that pluralises a count of one must move the same way (`N requests`/`N statements` stay plural for N>1; `3/4 routes` becomes `3/4 views`).
4. `python/tests/critical/test_check_contract.py:953` — the same single line, same change.

Preserve in both: the exact header/footer shape, the counts themselves, the `clean`/`N problems` suffix, the `--json` document (machine keys never pluralise, and `routes` stays the JSON key), and every other assertion in both files. No test renamed, deleted or `#[ignore]`d; no count changed. `uv run --no-project tools/critical_guard.py --base 4787d44 --node MB.40.5.2` must pass.
