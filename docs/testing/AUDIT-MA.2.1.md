# AUDIT MA.2.1 — `blastit-ingress` + `blastit-sqlpolicy` critical tests

- **Date:** 2026-11-12
- **Scope:** `crates/blastit-ingress/**` (body codecs: `plan_body`, `transcode_buffered`,
  `parse_mime`, `probe_sample`, `suggest_fix`) and `crates/blastit-sqlpolicy/**` (literal
  redaction, `check_learnable`, `is_introspection_setup_sql`).
- **Commits audited:** `bbb5827` (MA.2.1.2 ingress), `f571f91` (MA.2.1.3 sqlpolicy).
- **Requirements:** R14, R15, R16, R24, R25, R31, R36; `ADR-0003`, `ADR-0006`;
  `docs/design/ma-lean-dx.md` §4.2 A/C, §7.2.

## Summary

**Verdict: PASS WITH GAPS.** Both protected suites are real: every §4.2 A row and every R24/R25
rule has an allow twin and a deny twin, no test is skipped, and none would pass with the validator
deleted. 114 in-scope tests were read (69 protected, 36 new-module unit tests, 9 doctests); one
test is a **partial oracle** (`form_reencodes_semicolon`), no fluff, no misplaced test, no broken
oracle.

`cargo-mutants` is not installed, so `make mutants-rust` cannot run offline (exact error below).
The fallback manual spot-check killed **4 of 12** hand-written mutants (33 %); all four kills were
controls, and all **8 meaningful survivors** are branches the protected suites never name. Each
survivor is killed by a candidate test that passes on the current tip — reproduced in a scratch
copy and turned into follow-up tasks MA.2.1.4 … MA.2.1.7.

## Method

1. Read both crates' sources and every test in scope; classified each suite against the skill
   `testing` tiers and the §7.2 positive/negative matrix.
2. Checked the protected paths are actually guarded (`tools/critical_guard.py --base HEAD`:
   `no change to a protected critical test`) and that neither suite contains `#[ignore]`/skip.
3. Attempted the required mutation run:

   ```console
   $ make mutants-rust PKG=blastit-ingress
   ... cargo mutants -p "blastit-ingress" -j 2 --timeout 90
   error: no such command: `mutants`
   make: *** [Makefile:96: mutants-rust] Error 101
   ```

   `cargo-mutants` is absent from the offline toolchain; no network was used to install it.
4. Fallback: `rsync` of the sources (no `target/`, no `.git/`) to
   `/tmp/opencode/blastit-ma21-audit`, a separate `CARGO_TARGET_DIR`, and a Python runner that
   applies exactly one anchor-checked replacement at a time, runs the protected suite, and restores
   the file. The working tree was never mutated. A candidate-gap test file per crate
   (`tests/probe_ingress.rs`, `tests/probe_sql.rs`, scratch only) was then run on pristine code
   (green) and under each surviving mutant (red), proving the proposed test is the missing oracle.
5. One candidate mutant was discarded as **equivalent** (removing the `InsertStmt`/`UpdateStmt`/
   `DeleteStmt` arms of `introspection_node`): the sampled write CTE over an allowlisted catalogue
   is still refused by the relation allowlist, before and after. It is not counted and not reported
   as a gap.

## Results

### Inventory & classification

| File | Tests | Class | Note |
|---|---|---|---|
| `blastit-ingress/tests/critical_ingress.rs` | 46 | 45 critical, 1 partial oracle | all §4.2 A rows in both directions |
| `blastit-ingress/src/form.rs` | 2 | useful | empty segments, duplicate order |
| `blastit-ingress/src/mime.rs` | 2 | useful | quoted params, essence cap |
| `blastit-ingress/src/lib.rs` doctest | 1 | useful | runnable `plan_body` example |
| `blastit-sqlpolicy/tests/critical_literals.rs` | 23 | critical | R24/R25/R31 twins |
| `blastit-sqlpolicy/src/learn.rs` | 12 | useful | fast mirrors of the protected suite |
| `blastit-sqlpolicy/src/literals.rs` | 4 | useful | fast mirrors |
| `blastit-sqlpolicy/src/lexer.rs` | 16 | useful | the only oracle for `N'…'`, `U&'…'`, `$tag$` scanning (2 new) |
| sqlpolicy doctests | 8 | useful | runnable `redact_literals`/`check_learnable`/predicate examples |

Totals: **68 critical + 1 partial + 45 useful = 114**. Fluff 0, misplaced 0, broken-oracle 1.

Weak spots found by reading, not by mutation:

- **Partial oracle — `form_reencodes_semicolon`.** Its comment promises "`;` is literal in a form
  body and must come back as `%3B`", but the input is `a=%41%3B1` (already percent-encoded), so the
  raw-`;` differential it names is never exercised. Mutant M3 below confirms this.
- **List-derived assertion — `every_dangerous_function_is_refused`.** It iterates the same
  `DANGEROUS_LEARN_FUNCTIONS` constant the implementation checks, so deleting an entry would not
  fail the loop. Three names are pinned externally and five more via `CALL`/three-part tests, and
  §7.2 only asks for "every entry", so this is acceptable — noted, not tasked.
- **R16 is proved at the plan level only here** (`StreamPassthrough`, `Raw` identity). The real
  "0 B buffered in `blastit-scheduler`" claim is the scheduler suite (§7.2 R12..R13/R16, MA.2.2.3)
  and is unprotected until that node lands.
- No `IngressReject` carries a body byte: every payload-bearing field is a capped MIME/encoding
  string, and the error text never derives from the body. Verified by inspection; no test asserts
  it, but no body byte is reachable.

### Protected-suite twin matrix

| Req | Allow twins | Deny twins | Verdict |
|---|---|---|---|
| R14 JSON | vendor `+json` + charset canonicalized, sorted keys, `u64::MAX`, `i64::MIN`, depth 64, distinct keys, surrogate pair, `{}` | `u64::MAX+1`, `i64::MIN-1`, depth 65, duplicate keys (incl. `\u0061`), BOM, trailing bytes, lone surrogate, invalid UTF-8, empty body, `1e400`, CT mismatch ×2, `charset=ibm037` | complete for §4.2 A; strict-string branches unasserted (G1) |
| R15 form | `;`→`%3B` (via `%3B`), empty body, duplicate order | `%ZZ`, `%4`, `%00` (escaped and raw), `%FF`, CT mismatch, `charset=ibm037` | complete except raw `;` (G2) |
| R16 raw | `StreamPassthrough` (POST/PUT), identity transcode | n/a (an allow invariant) | sufficient at this crate's boundary; end-to-end owned by MA.2.2.3 |
| R24 redact | quoted/`U&"` identifiers, `$1`→`$?`, `KNOWN_STATIC_SETUP_SQL`, comments, idempotence, no literal byte | all 8 literal forms, prefix included, comment quotes not literals | complete |
| R25 learn | parameterised, quoted/`U&"` identifiers, static setup, `pg_catalog.version()`, `SHOW`, other-schema `pg_read_file` | 8 inline forms, `…`, 5 unparseable, 4 multi-statement, 3 `COPY … PROGRAM`, every dangerous function, `CALL`/three-part names, no refusal prints a literal | complete |
| R31 replay | `SET`/`SHOW`, `version()`, `current_schema()`, `to_regtype($?)`, `pg_type`, `current_setting(...)` | unqualified `tables`/`columns`/`schemata`, `information_schema.pg_type`, `public.*`, `pg_get_userbyid`, `auth_user`, `pg_read_file`, `pg_authid`, `nextval`, `INTO`, `FOR UPDATE`, DDL/DML, multi-statement, empty | complete for the named cases; function/relation schema edge untested (G4) |
| R36 check | sample validity gated by declared essence, truncation, malformed sample, 6 `suggest_fix` cases | bad JSON, bad form, non-UTF-8 charset | complete except exact cap boundary (G3) |

### Mutation spot-check (manual fallback)

| # | Mutation | Protected suite | Candidate test |
|---|---|---|---|
| M1 | `probe_sample`: `total_len > cap` → `>=` | survived | cap-length body is not truncated |
| M2 | JSON emitter: emit a decoded `\` raw | survived | quote/backslash round-trip |
| M3 | form `decode`: also split on `;` | survived | raw `a=1;b=2` is one pair |
| M11 | JSON emitter: emit a decoded newline raw | survived | `\n` re-escapes |
| M5 | `function_name`: accept any 2-part schema | survived | `!is_introspection_setup_sql("SELECT public.version()")` |
| M9 | `is_introspection_relation`: drop the `catalogname` guard | survived | `!is_introspection_setup_sql("SELECT * FROM otherdb.pg_catalog.pg_type")` |
| M8 | `transcode_buffered`: drop the bodyless-method 413 | survived | `GET` body on the buffered API is 413 |
| M13 | `is_utf8_charset`: drop the `utf8` alias | survived | (no task: alias is undocumented; see Follow-ups) |
| C1 | JSON depth `>=` → `>` | **killed** | control |
| C2 | dangerous-function schema check → `true` | **killed** | control (`public.pg_read_file`) |
| C3 | redaction stops exempting static setup | **killed** | control |
| C4 | form rejects uppercase hex | **killed** | control |

**Kill rate: 4/12 (33 %).** The sample is deliberately aimed at branches the suites do not name,
so it is a lower bound on the true score, not an estimate of it; every survivor except M13 is a
real requirement/security gap and is tasked.

### Requirement → test map

| Requirement / promise | Protected by | Status |
|---|---|---|
| R14 canonical JSON + rejections | `critical_ingress.rs` (+ no unit tests in `json.rs`) | protected; G1 |
| R15 canonical form + rejections | `critical_ingress.rs` + `form.rs` unit | protected; G2 |
| R16 `Raw` zero scheduler buffer | plan-level `critical_ingress.rs` | end-to-end owed to MA.2.2.3 |
| R24 redaction | `critical_literals.rs` + `literals.rs` unit | protected in sqlpolicy; manifest `LiteralQuery`/`RedactedQueryPlaceholder` and `dbproxy` wiring are MA.2.2.1 / MA.2.2.4 |
| R25 learnable verdicts | `critical_literals.rs` + `learn.rs` unit | protected as a predicate; `fold`/CLI wiring is MA.2.3.1 |
| R31 replay predicate | `critical_literals.rs` + `learn.rs` unit | protected as a predicate; max-8 replay and wiring are MA.2.2.4; G4 |
| R36 sample/suggestion | `critical_ingress.rs` | protected as a function; the three findings are MA.2.3.4; G3 |
| DX doctests (`plan_body`, `redact_literals`, `has_quoted_literal`, `check_learnable`, `is_introspection_setup_sql`) | crate doctests, run by `make check` | green |

## Gaps

Each gap is a follow-up task under MA.2.1 (test-only, protected file already listed in `paths`).

1. **G1 — canonical JSON string handling (MA.2.1.4, medium).** The emitter's escaping of `"`, `\`
   and control characters and the parser's rejection of a raw `0x01` / unknown escape are unasserted.
   An emitter regression here puts invalid JSON on the wire (R14).
2. **G2 — raw `;` in a form body (MA.2.1.5, high).** ADR-0006/R15 say `;` is data; the current test
   feeds `%3B`, and a mutant that splits on `;` survives. This is the `parse_qs` parser differential
   the re-encode exists to remove.
3. **G3 — check-sample boundary and buffered 413 (MA.2.1.6, low).** `body_len == cap` is never
   sampled; `transcode_buffered`'s bodyless-method 413 is only tested through `plan_body`.
4. **G4 — replay schema qualification (MA.2.1.7, high).** `SELECT public.version()` and
   `otherdb.pg_catalog.pg_type` are replayable under their respective mutants. Replay runs without a
   grant, so a route-defined function can be re-executed by `dbproxy` outside the policy.

## Follow-ups

- **Out-of-scope but required for the epic's claim** (do not close MA.2.1 without them):
  R16 end-to-end zero-buffer (MA.2.2.3), R24 manifest rejection + `dbproxy` redaction
  (MA.2.2.1, MA.2.2.4), R25 `fold`/CLI (MA.2.3.1), R31 replay max-8/wiring (MA.2.2.4),
  R36 findings (MA.2.3.4).
- **Minor survivor, no task:** `is_utf8_charset` accepts the undocumented `charset=utf8` alias; a test
  would bless behaviour §4.2 A does not promise. Either document the alias or delete it.
- **Strictness branches, no task:** leading zeros (`{"a":01}`) and a lone `-` fail through
  `JsonTrailingBytes`/`JsonSyntax` (safe), and `MAX_INTROSPECTION_DEPTH` is unreachable within
  PostgreSQL's own parse limits.
- **Production observation for MA.2.2.1:** `mime::cap` truncates the essence to 64 B *before*
  `is_json_essence` matches, so a valid `application/vnd.<long>+json` essence longer than 64 bytes is
  a `400`. Fail-closed, but tighter than §4.2 A; decide whether the cap belongs only in the report.
- **Toolchain:** install `cargo-mutants` for the MA gate, or record per-crate manual spot-checks like
  this one; the `Makefile` target otherwise fails with `no such command: mutants`.
