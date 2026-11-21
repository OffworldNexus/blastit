# MB.40.5.2 — RESULT

STATUS: done (items 1–9)

`blastit check`'s human output names every fact `--json` carries, `blastit fmt
--check` prints a real unified diff (trailing-newline and CRLF deviations
included), and `check --learn --dry-run` prints the writer's own document — the
exact bytes `--learn` puts on disk.

## What changed

| File | Change |
|---|---|
| `crates/blastit/src/unified.rs` (new) | The one unified diff. Lines come from `split_inclusive('\n')`, so a document's own terminator is part of the line: a missing final newline is a hunk (`-b` + `\ No newline at end of file`, `+b`) and a CRLF file's removals carry the `\r`. `--- path` / `+++ path` / `@@ -l,c +l,c @@`, 3 context lines, bounded LCS (4 Mi cells) with a prefix/suffix fallback (3000×3000 still yields one real hunk). 9 unit tests. |
| `crates/blastit/src/lib.rs` | `mod unified;` |
| `crates/blastit/src/cli/run.rs` | `fmt --check` prints the verdict **and** the diff; writes nothing, exit `1`. |
| `crates/blastit/src/audit_v1.rs` | `human()` rewritten (`view`/`trait`/`proxy` nouns, dead-grant `sql`, `on proxy <name>`, observed MIMEs, a `fix:` line per finding); `header()` pluralises and says `view`; `fix_line` fallback is a sentence, never a bare action id. `LearnPlan::proposal()` is now `Result<String, String>` returning the **writer's own bytes**; `view_block`/`view_header_named`/`brace_delta` deleted. |
| `crates/blastit/src/check.rs` | `learn()` computes one proposal with `proposal()` and hands the *same* string to `write_atomic`; a dry run prints it, an editor refusal is reported as `learn: <error>` (exit `1`), never swallowed. Duplicate doc line removed. Two tests (parity, proposal-identity). |
| `crates/blastit/tests/critical_blastfile_fmt.rs` | Authorized re-pin: stdout carries the header **and** the diff; exit `1`, stderr empty, bytes unchanged, `dir.entries() == ["Blastfile"]` verbatim. |
| `crates/blastit/tests/critical_check_engine.rs` | Authorized re-pin: the two blocks and the clean line (header plural + `view`, the two new `fix:` lines). |
| `crates/blastit/tests/fmt_command.rs` | Same stdout re-pin, plus CRLF-canonical and missing-final-newline diff tests. |
| `python/tests/runnable_docs/harness.py` | `_CLEAN` accepts `requests?/statements?` and `routes|views`; the messages say `views`. |
| `python/tests/critical/test_check_contract.py` | Authorized re-pin of the one clean header line. |

## Items 1–9

1. **Trailing newline (BLOCKER).** `split_inclusive('\n')` keeps the terminator,
   so `unified("p","a\nb","a\nb\n")` is a real hunk, and a row without a
   newline emits diff's own `\ No newline at end of file` marker. Tests:
   `a_missing_final_newline_is_a_real_hunk`, `an_empty_file_is_one_hunk_of_added_lines`,
   `crlf_input_shows_the_carriage_return_it_would_drop`, plus the binary-level
   `fmt_command::a_missing_final_newline_still_shows_a_diff`.
2. **Header plural + `view`.** `header()` writes `1 request, 1 statement, 1/1
   view, clean` / `6 requests, 5 statements, 5/6 views`; `--json` keeps
   `routes`. Pins updated at `critical_check_engine.rs:1008` and
   `python/tests/critical/test_check_contract.py:953` (both authorized by the
   addendum).
3. **`proxy_unused` fix line.** `proxy analytics: declared, 0 statements
   observed` now carries `fix: remove \`proxy analytics\` or add the call that
   uses it`; `no_observations` carries its own sentence. The parity test asserts
   `human.matches("\n      fix: ").count() == report.findings.len()`, so a
   finding with no fix line cannot slip through again.
4. **`learn: 1 route, 1 query`.** Landed in `learn_line` through
   `audit_v1::plural` (`-y` → `-ies` included), pinned for 1/1 and 2/3, with the
   `--json` keys asserted unchanged.
5. **Dry run == write.** `proposal()` returns
   `blastit_manifest::blastfile::apply_edits(source, edits)` and `learn()` hands
   that same string to `write_atomic`: a dry run prints byte-for-byte what a
   real run writes. We did **not** make `learn()` write `fmt(edited)`: `--learn`
   is a surgical edit that preserves comments and layout outside the touched
   block, `dev --learn` shares the same writer, and canonicalising the whole
   file here would rewrite documents the other path leaves alone. Pinned by
   `check.rs::tests::a_dry_run_proposes_the_bytes_learn_would_write`
   (`assert_eq!(proposal, apply_edits(..))`).
6. **No second lexer.** `view_block`/`view_header_named`/`brace_delta` are
   deleted; the fragment now comes from the editor itself (item 5). On the file
   size: `audit_v1.rs` is still ~1,880 prod LOC, because moving the reporter out
   is not enough to reach 500 (it was 1,951 before this slice). A real split of
   the audit engine (`findings` / `ingress` / `learn` / `report`) is recorded as
   Follow-up 3 rather than started here.
7. **CRLF.** Same root as item 1: `split_inclusive` keeps `\r`, so a
   canonical-layout CRLF file prints a hunk whose removals carry it (captured
   below with `od -c`), and `fmt_command::a_crlf_canonical_file_shows_the_carriage_returns_it_would_drop`
   pins it end to end.
8. **Duplicate doc line** in `check.rs` removed.
9. **`fix_line` fallback** is `fix: none — see this finding's action in
   \`--json\``; `proposal()` returns `Result` and `learn()` reports the editor's
   refusal on stderr and exits `1` instead of printing nothing.

## Evidence A — `fmt --check`, trailing newline only (real binary)

```console
$ od -c Blastfile
0000000   a   p   p       "   s   h   o   p   "
0000012
$ blastit fmt --check
./Blastfile: not in canonical layout
--- ./Blastfile
+++ ./Blastfile
@@ -1,1 +1,1 @@
-app "shop"
\ No newline at end of file
+app "shop"
exit=1
```

## Evidence B — `fmt --check`, canonical layout saved with CRLF

```console
$ printf 'app "shop"\r\n\r\nview products GET "/products" {\r\n    body json\r\n}\r\n' > Blastfile
$ blastit fmt --check
./Blastfile: not in canonical layout
--- ./Blastfile
+++ ./Blastfile
@@ -1,5 +1,5 @@
-app "shop"
-
-view products GET "/products" {
-    body json
-}
+app "shop"
+
+view products GET "/products" {
+    body json
+}
exit=1
$ blastit fmt --check | od -c | sed -n '5,7p'
0000120   5       @   @  \n   -   a   p   p       "   s   h   o   p   "
0000140  \r  \n   -  \r  \n   -   v   i   e   w       p   r   o   d   u
```

The removals carry `\r` (`\r \n` in the octal dump); exit `1`, nothing written.

## Evidence C — failing `check`, human vs `--json`, one run

Fixture `python/tests/runnable_docs/fixtures/findings`: real dbproxy in
`DynamicCheck`, real pytest suite (4 passed), `pgstub` upstream.

```console
$ blastit check            # exit 1
....                                                                     [100%]
4 passed in 0.01s
blastit check: 4 requests, 2 statements, 3/4 views, 2 proxies
  view catalog: 1 statement not granted on proxy db
    SELECT 2
      fix: add it to view catalog postgres
  view search: 1 grant never used on proxy db
    SELECT 9
      fix: remove it from view search postgres
  view checkout: never exercised
      fix: add a test that reaches it
  view upload: takes a body and declares no ingress validation; observed application/x-www-form-urlencoded
      fix: write `body form`
  request GET /robots.txt: no view matches
      fix: add a `view` whose pattern matches this request
  proxy analytics: declared, 0 statements observed
      fix: remove `proxy analytics` or add the call that uses it
blastit check: 6 problems · `--json` for the machine-readable list
```

```console
$ blastit check --json     # exit 1, one document (abridged to the facts)
{ "schema_version": 1, "ok": false, "requests": 4, "statements": 2,
  "routes": {"total": 4, "exercised": 3},
  "proxies": {"analytics": {"statements": 0, "denied": 0}, "db": {"statements": 2, "denied": 1}},
  "findings": [
    {"kind": "missing_grant", "route": "catalog", "proxy": "db", "sql": "SELECT 2",
     "fix": {"action": "add_query", "to": "view catalog postgres"}},
    {"kind": "dead_grant", "route": "search", "proxy": "db", "sql": "SELECT 9",
     "fix": {"action": "remove_query", "to": "view search postgres", "unsafe": true}},
    {"kind": "unexercised_route", "route": "checkout", "fix": {"action": "add_test"}},
    {"kind": "missing_ingress", "route": "upload",
     "observed": ["application/x-www-form-urlencoded"], "fix": {"action": "declare_ingress", "codecs": ["form"]}},
    {"kind": "unrouted_request", "method": "GET", "path": "/robots.txt", "fix": {"action": "add_route"}},
    {"kind": "proxy_unused", "proxy": "analytics", "fix": {"action": "observe"}} ] }
```

Every `sql`/`proxy`/`fix.to`/`observed` fact is on a human line, and each of the
six findings has exactly one `fix:` line (the parity test asserts both).

## Evidence D — clean run, singular header

```console
$ blastit check            # fixture python/tests/runnable_docs/fixtures/shop, exit 0
.                                                                        [100%]
1 passed in 0.01s
blastit check: 1 request, 1 statement, 1/1 view, clean
```

## Evidence E — `check --learn --dry-run` prints the writer's own document

```console
$ blastit check --learn --dry-run     # exit 0
....                                                                     [100%]
4 passed in 0.01s
learn: 1 route, 1 query proposed (dry-run) → /tmp/opencode/mb4052-capture/findings/Blastfile
learn: proposed Blastfile (nothing written):
app "findings"

entrypoint python "app:application"

postgres $DATABASE_URL

postgres analytics $ANALYTICS_URL

view catalog "/catalog" {
    postgres {
        query 'SELECT 1'
        query 'SELECT 2'
    }
}

view search "/search" {
    postgres {
        query 'SELECT 9'
    }
}

view checkout "/checkout" {}

view upload POST "/upload" {}

view robots-txt "/robots.txt" {
}
$ diff -q /tmp/opencode/mb4052-capture/findings/Blastfile \
        python/tests/runnable_docs/fixtures/findings/Blastfile
# (no output: the dry run wrote nothing)
```

Those are `apply_edits`'s bytes — the same string `learn()` writes — so the
spliced `view robots-txt` and the untouched `view catalog` are exactly what a
real `--learn` would leave on disk.

## Tests that pin this behaviour

- `crates/blastit/src/check.rs::tests::every_fact_the_json_document_carries_is_in_the_human_report`
  — one fixture, both renderings: every `sql`/`proxy`/`fix.to`/`observed` fact
  present, 5 findings, ≥9 facts, one `fix:` line per finding, no `route `/`group `
  nouns.
- `crates/blastit/src/check.rs::tests::a_dry_run_proposes_the_bytes_learn_would_write`
  — `proposal == apply_edits(..)` byte for byte, with a non-empty grant, and an
  editor refusal reported rather than swallowed.
- `crates/blastit/src/check.rs::tests::the_learn_line_names_dry_runs_and_absolute_paths`
  — `1 route, 1 query` and `2 routes, 3 queries`; `--json` keys unchanged.
- `crates/blastit/src/audit_v1.rs::tests::every_count_is_written_in_its_own_number`
  — one and many for every noun (including `query` → `queries`).
- `crates/blastit/src/unified.rs::tests::*` — trailing newline, empty file, CRLF,
  hunk numbers, far-apart hunks, fallback, quoted braces.
- `crates/blastit/tests/fmt_command.rs` — the binary-level trailing-newline and
  CRLF diffs.

## Verification (this worktree)

```
$ cargo test -p blastit --locked --lib unified --lib audit
test result: ok. 9 passed; 0 failed        (unified)
test result: ok. 12 passed; 0 failed       (audit_v1)
$ cargo test -p blastit --locked --lib
test result: ok. 277 passed; 0 failed
$ cargo test -p blastit --locked --test critical_check_engine --test critical_blastfile_fmt --test fmt_command
test result: ok. 6 passed; 0 failed
test result: ok. 20 passed; 0 failed
test result: ok. 14 passed; 0 failed
$ uv run --no-project uvx --offline ruff check python tools
All checks passed!
$ make fast-check
All checks passed! · plan OK
$ make test-critical
Ran 53 tests in 10.992s · OK
$ uv run --no-project tools/critical_guard.py --base 4787d44 --node MB.40.5.2
  M  crates/blastit/tests/critical_blastfile_fmt.rs  [authorized by crates/blastit/tests/critical_blastfile_fmt.rs]
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
  M  python/tests/critical/test_check_contract.py  [authorized by python/**]
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.40.5.2
```

`critical_guard` first failed with "not covered by any node path" for the two
Rust files the addendum authorizes: the node's `paths` had lost the two entries
the earlier attempt added. Restored through the sanctioned tool
(`plan.py edit MB.40.5.2 --paths …`, no hand-edited frontmatter); `plan.py
validate` says `plan OK`.

## Follow-ups (out of this node's `paths`)

1. `docs/ci.md:50-58` still documents the old block *and* the old header
   (`route …`, `7/8 routes`, `no route`, no `fix:` lines). `mode_findings`
   (`python/tests/runnable_docs/harness.py:1379-1401`) compares each documented
   line with real output, so `make docs-test` (in `make check`, not in this
   node's acceptance) needs that page updated. `docs/` is not in `paths`.
2. `dev --learn` still prints `learn: N routes, M queries …` (pinned by
   `python/tests/runnable_docs/harness.py:1176`); `check --learn` now says
   `1 route, 1 query`. One `plural()` call in `dev_learn.rs` closes it.
3. `audit_v1.rs` remains ~1,880 prod LOC (1,951 before this slice): the audit
   engine, the human reporter and the learn planner live in one file. A split
   into `audit/{findings,ingress,learn,report}.rs` needs its own node; moving
   only the reporter out cannot reach the 500-line bar.
