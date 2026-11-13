# AUDIT MA.2.2 — manifest 0.1.1, early upload abort, ingress enforcement, wire learning

- **Date:** 2026-11-12
- **Scope:** `crates/blastit-manifest/**` (0.1.1 schema, reserved-key guard, literal refusal,
  surgical editor), `crates/blastit-http/**` (early upload abort), `crates/blastit-scheduler/**`
  (ingress enforcement, `--learn` fallback), `crates/blastit-dbproxy/**` (learn, dynamic-check,
  redaction).
- **Commits audited:** `80638f1` (MA.2.2.1), `32f3777` (MA.2.2.2), `6771d6a` (MA.2.2.3),
  `f3f26fb` (MA.2.2.4).
- **Requirements:** R7, R8, R11, R12, R13, R14..R16 (runtime wiring), R24, R26, R28, R29, R31;
  `ADR-0002`, `ADR-0003`, `ADR-0006`; `docs/design/ma-lean-dx.md` §4.2 B/D/E.4, §5, §7.2.
- **Epic acceptance audited:** item 1 (five protected suites green), item 2 (allow + deny twin
  per suite), item 3 (no literal in `sql.denied` / `obs_fd` / check finding; `apply_edits`
  re-parses unchanged fields), item 4 (no leaf edited `docs/**`, `python/**`, root
  `Cargo.toml`/`Cargo.lock`).

## Summary

**Verdict: PASS WITH GAPS.** All five protected suites pass unprivileged, offline, with no
skip and no tautology; every requirement in §7.2's row has a deny/adversarial twin except the
two fail-closed branches listed under G2. Of 568 in-scope tests, 51 are protected critical and
517 are standard: **fluff 0, misplaced 0, broken-oracle 0**. Epic acceptance item 4 holds
exactly as claimed; item 3 holds for `sql.denied` and `obs_fd` and is not yet applicable to
`blastit check` findings (that module does not exist in this tree).

`cargo-mutants` is not installed, so `make mutants-rust` cannot run offline (exact error
below). The manual spot-check over 17 hand-picked security branches killed **12 (70.6 %)**.
One survivor is **equivalent** (removing `Stream::try_peek_response`'s early-answer report
changes no deterministic behaviour because `fill_body` observes the same frame), so the
meaningful score is **12/16 = 75 %** — on the bar but only just, and concentrated: 3/6 in
`blastit-manifest`, 2/3 in `blastit-http`, 1/3 in `blastit-scheduler`, 4/5 in
`blastit-dbproxy`. Four real gaps are tasked (MA.2.2.5 … MA.2.2.8); each proposed oracle was
reproduced in a scratch copy as green on the tip and red under its mutant, except the
dbproxy identity test, which reuses the existing harness.

## Method

1. Read all four crates' security paths and every test in scope; classified each file against
   the `testing` tiers and the §7.2 positive/negative matrix.
2. Ran the five protected suites at the audited tip (counts in Results) and confirmed
   `tools/critical_guard.py --base 80638f1` reports only *added* protected files — no
   modification or deletion.
3. Attempted the required mutation gate:

   ```console
   $ cargo mutants --version
   error: no such command: `mutants`
   $ command -v cargo-mutants
   no cargo-mutants binary
   ```

   No network was used to install it.
4. Fallback: `rsync` of the sources (no `target/`, no `.git/`) to
   `/tmp/opencode/blastit-ma22-audit` with a separate `CARGO_TARGET_DIR`, and a runner that
   applies exactly one anchor-checked replacement at a time, runs the crate's critical suite,
   then the whole crate, and restores the file from memory. The working tree was never
   mutated.
5. Each surviving mutant got a candidate oracle written in the scratch copy
   (`tests/tmp_probe_*.rs`, since deleted): green on pristine code, red under the mutant.
   Probes proved `sc.buffer_cap_off`, `sc.learn_queue_overflow_off`,
   `mf.write_atomic_create_new_off`, `mf.group_arm_guard_off` and the `$1` exemption claim;
   `db.credentials_any_uid` and `mf.validate_no_normalize` were pinned by existing tests in
   other tiers.

## Results

### Inventory & classification

| File | Tests | Class | Note |
|---|---|---|---|
| `blastit-manifest/tests/critical_manifest_011.rs` | 15 | critical | R7/R8/R11/R24 twins, both catch-all spellings, `is_query_covered_by_tables_or_groups` |
| `blastit-manifest/tests/critical_manifest_edit.rs` | 16 | critical | R28/R29 insertion, idempotence, literal/`…` refusal, atomic write + cleanup |
| `blastit-manifest/tests/docs_fences.rs` | 3 | useful | deliberately standard tier (MA.3 owns docs) |
| `blastit-manifest/tests/{parse,validate,grants,routing,uid}.rs` | 92 | useful | parser/model coverage; no allowlist short-circuit |
| `blastit-manifest/src` unit | 9 | useful | helpers, `$?` rewrite |
| `blastit-http/tests/critical_early_abort.rs` | 5 | critical | early `413` on fixed + chunked, over-cap declaration, intact normal upload |
| `blastit-http/tests/{roundtrip,rejections,bodies,forwarding,flowcontrol,limits,channel}.rs` | 101 | useful | framing/smuggling/credit behaviour |
| `blastit-http/src` unit | 76 | useful | de-chunker, headers, response reason |
| `blastit-scheduler/tests/critical_ingress_enforcement.rs` | 7 | critical | raw zero-buffer, at-cap chunked, canonical JSON, two `413`s, one learn slot |
| `blastit-scheduler/tests/{admission,ctl_channel,fairness,flow,isolation,lifecycle,memory,routing}.rs` | 102 | useful | scheduling, pressure, ctl framing |
| `blastit-scheduler/src` unit | 40 | useful | fair queue, path/route, ctl codec |
| `blastit-dbproxy/tests/critical_learn_and_redaction.rs` | 8 | critical | R26/R31 replay, redaction, root listener, 3 ctl bounds |
| `blastit-dbproxy/tests/postgres.rs` | 24 | useful | real-PostgreSQL integration; inert locally, fails in CI when unset (deliberate) |
| `blastit-dbproxy/src` unit | 70 | useful | `credentials_match`, allowlist, learning state, ctl parse |

Totals: **51 critical + 517 useful = 568**. Fluff 0, misplaced 0, broken-oracle 0.

Suite evidence at the audited tip:

```console
$ cargo test -p blastit-manifest --test critical_manifest_011 --test critical_manifest_edit
test result: ok. 15 passed; 0 failed; 0 ignored      (critical_manifest_011)
test result: ok. 16 passed; 0 failed; 0 ignored      (critical_manifest_edit)
$ cargo test -p blastit-http --test critical_early_abort
test result: ok. 5 passed; 0 failed; 0 ignored
$ cargo test -p blastit-scheduler --test critical_ingress_enforcement
test result: ok. 7 passed; 0 failed; 0 ignored
$ cargo test -p blastit-dbproxy --test critical_learn_and_redaction
test result: ok. 8 passed; 0 failed; 0 ignored; finished in 5.02s
$ grep -n 'ignore]\|pytest.mark.skip\|need_root' <the five files>
(no output)
```

### Protected-suite twin matrix

| Req | Allow twins | Deny / adversarial twins | Verdict |
|---|---|---|---|
| R7 `app`/`preload` | string `app`, string + ordered de-duplicated array `preload`, absent `app` | dotted `myshop.wsgi.application` -> `InvalidApp`, empty `app` -> `Empty`, no silent `BLASTIT_APP` fallback | complete |
| R8 reserved keys | `groups` at top level and in `[[route]]`; every author-chosen name (`[proxy.app]`, `[proxy.groups]`, `[proxy.version]`, an `app` output variable, a `groups` table) | `version`/`app`/`preload` under `[proxy.db]` and `[[route]]`, `groups` under `[route.ingress]`, undeclared `app` under `[[route]]` — exact message | complete for the asserted shapes; `[group.*]`/`[route.<proxy>]` bodies unasserted (G4) |
| R11 `[route.ingress]` | all four codecs, `allow_raw = true` on `raw`, absent ingress -> `None` | `allow_raw` on `json`/`form`/`none` -> `UnknownKey`, missing `body`, `xml`, `multipart`; both boot warnings, and their quiet twin | complete |
| R12/R13 early abort | 128 KiB upload relays intact | `CL = cap+1` refused before a stream exists; 10 MiB scripted upload stops at the early `413` (5 s bound, no `CANCEL`); chunked early `413`; chunked `cap+1` -> `413` + `Connection: close` | complete for the declared/chunked caps; scheduler-side buffered cap is only tested at the exact cap (G2) |
| R14/R15 runtime | chunked JSON at cap accepted, canonical sorted-key body, one `Content-Length`, no `Transfer-Encoding`/`Content-Encoding`/`Expect` | over-cap chunked body -> `413` (http); ingress-less and `body = "none"` -> `413` with no view frame | complete at this layer (codec rejections are `blastit-ingress`'s protected suite) |
| R16 raw | chunk 1 reaches the view before chunk 2 exists; `RouteInfo` carries the manifest policy | n/a (an allow invariant) | complete |
| R24 manifest | both `KNOWN_STATIC_SETUP_SQL` spellings (the `$1` spelling only outside this tier, G4), parameterised `$?` twin | `…` -> `RedactedQueryPlaceholder`, `'secret'` in a route and in a group -> `LiteralQuery`, all seven prefixed literal forms, no message echoes the value | complete |
| R24/R29 editor | comments/ordering/quoting preserved, both catch-all spellings, method-restricted catch-all is not a point, idempotent, grant merged not rewritten, every fixture re-parses | inline literal, `…`, undeclared proxy, route-name conflict, invalid name/regex/empty path, `unrestricted` + queries, inline grant table — none echoes the statement | complete |
| R26 db / R31 | empty-stack fixture `INSERT` -> zero lines; `SET TIME ZONE 'UTC'` + `pg_catalog.version()` recorded and replayed once; `DynamicCheck` allows an ungranted statement; `set_db` steers the next session; unrouted target attributed | `Enforce` -> `42501` and the statement never reaches the upstream; `'s3cr3t'` absent from every event JSON + `Debug` and from the whole obs byte stream; root listener without opt-in refused; malformed / 8 KiB / stalled ctl line answered or cut at 5 s | complete; uid-mismatch identity is only unit-tested (G1) |

### Mutation spot-check (manual fallback)

| # | Mutation | Verdict | Missing oracle |
|---|---|---|---|
| 1 | `parse::reserved` always `false` (R8 guard off) | **killed** (critical) | — |
| 2 | `allow_raw` polarity `==` -> `!=` | **killed** (critical) | — |
| 3 | editor's `contains('…')` refusal -> `false` | **killed** (critical) | — |
| 4 | early relay `force_close` `true` -> `false` | **killed** (critical) | — |
| 5 | de-chunker cap `max + 1` | **killed** (critical) | — |
| 6 | `AllowEmptyOnly` non-empty body -> `Ok(())` | **killed** (critical) | — |
| 7 | dbproxy mode gate inverted (learn refuses, enforce allows) | **killed** (critical) | — |
| 8 | `denial()` -> `normalized.to_string()` | **killed** (critical) | — |
| 9 | ctl deadline 5 s -> 30 s | **killed** (critical, 5 s test) | — |
| 10 | `MAX_SETUP_STATEMENTS` 8 -> 0 (no recording) | **killed** (critical) | — |
| 11 | `credentials_match`: `root_allowed && uid == spec_uid` -> `root_allowed` | killed by standard only (3 unit tests) | G1 |
| 12 | `[group.*]` reserved-key guard removed | **survived** | G4 |
| 13 | `validate` stops normalising before the setup-SQL exemption | **survived** the crate; killed by `blastit-py`'s unprotected fixture | G4 |
| 14 | `write_atomic` `create_new(true)` -> `create(true)` | **survived** | G3 |
| 15 | `try_peek_response` reports a kept frame as "no response" | **survived — equivalent** (see Follow-ups) | none |
| 16 | scheduler `Buffer` cap check removed | **survived** | G2 (oracle: `413` before `REQ_END`) |
| 17 | `--learn` queue-overflow check removed | **survived** | G2 (oracle: third unrouted request -> `503`) |

**Kill rate 12/17 (70.6 %); 12/16 (75.0 %) excluding the equivalent mutant #15.**
Probe evidence for the tasked survivors:

```console
# scratch copy, pristine tip
STATUS: 413                        # 10-byte JSON, cap 9, no REQ_END
SECOND STATUS: 503                 # queue_size = 1, third unrouted request
VICTIM: Ok("victim\n")             # planted temp symlink never written through
GROUP-VERSION: Err([NotTopLevel { key: "version" }])

# same probes under mutants #16, #17, #14, #12
buffered_cap_is_enforced_before_the_end ... FAILED (no response)
a_full_learn_queue_answers_503 ... FAILED (no response)
VICTIM: Ok("version = 1\n# edited\n")   # written through the symlink
GROUP-VERSION: Err([UnknownKey { path: "group.session.db.version" }])  # fail-closed, wrong message

# same probe under mutant #13, whole manifest crate green; blastit-py kills it
audit::tests::a_statement_connection_setup_accounts_for_is_used_rather_than_unused ... FAILED
  group "connection": queries[0] inlines a quoted literal
```

### Epic acceptance item 3 — no value-bearing literal escapes

| Surface | Construction site | Protected oracle | Verdict |
|---|---|---|---|
| `sql.denied` | `hooks.rs::denial()` is the only `SqlDenied` builder (all three call sites, plus `Bind` and `FunctionCall`) and applies `redact_literals` | `enforce_still_answers_42501_and_redacts_every_literal`: exact `SELECT … FROM orders WHERE id = 1`, `s3cr3t` absent from `encode_json` and `Debug` of **every** event | holds |
| `obs_fd` | `learn.rs::ObsBody::body()` — `sql: redact_literals(normalized)`; setup replay stores already-redacted bodies | `learning_…` asserts the three exact strings and no `s3cr3t` byte in the stream; `an_unrouted_request_carries_its_own_target` does the same, plus `verdict.redacted()` | holds |
| `blastit check` finding | not in the tree: `crates/blastit/src/check.rs` and `audit_v1.rs` do not exist yet (MA.2.3.4) | manifest-side guarantee is `LiteralQuery`/`RedactedQueryPlaceholder`, both protected | not yet applicable; re-audit at MA.2.3.4 |
| `apply_edits` re-parses unchanged fields | `edit.rs:367 verify(&manifest, &expected, &edited)?` after `render` | every edit fixture is independently re-parsed, untouched non-empty source lines asserted verbatim, idempotent byte equality, `EditError::Verification` on divergence | holds; the internal `verify` is unreachable-by-construction (equivalent mutant, no task) |

### Epic acceptance item 4 — leaf scope

`git log --name-only 80638f1..HEAD` (three commits after the MA.2.2.1 base) lists only
`crates/{blastit-manifest,blastit-http,blastit-scheduler,blastit-dbproxy}/**`, their
`tests/**`, `plan/**` bookkeeping and one `docs/knowledge/LESSONS.md` bullet (the disclosed
`obs_fd` back-pressure note). No `python/**`, no root `Cargo.toml`/`Cargo.lock`, no other
`docs/**`. The range base `80638f1` itself changed `crates/blastit-py/src/audit.rs` (declared
in MA.2.2.1's `paths`, a Rust crate — not `python/**`) and no docs at all. **Holds.**

## Gaps

Each gap is a test-only follow-up under MA.2.2; every path is a protected file the new node
owns, so `critical_guard` stays satisfiable.

1. **G1 — dbproxy socket identity is unprotected (MA.2.2.5, high).** A mutant that accepts any
   non-root uid on any listener survives `critical_learn_and_redaction`; only three in-module
   unit tests catch it. Per `security-invariants` §3 this is the wall that keeps a route from
   using another route's grants, so it belongs in the protected tier. The existing harness can
   express the refusal without root: declare `current_uid() + 1` on the listener, connect,
   expect `28000` and no upstream statement.
2. **G2 — scheduler fail-closed branches (MA.2.2.6, medium).** (a) A buffered `json`/`form`
   body is only tested at the exact cap; the per-frame cap is enforced **before** `REQ_END`
   (bounded memory, §4.3) and an unbounded-buffer mutant survives. (b) `queue_size` counts
   *waiting* requests, so a full `--learn` fallback queue (three concurrent unrouted requests
   at `queue_size = 1`) must answer `503`; the overflow branch is unprotected. Both oracles
   were proven green on the tip and red under their mutants.
3. **G3 — `write_atomic` temp-name symlink (MA.2.2.7, medium).** MA.2.2.2 promises a planted
   symlink is "refused, never written through"; no test plants one, and dropping `O_EXCL`
   writes the manifest through the link (proven: `victim.txt` overwritten). Assert the
   security property (target untouched, no `.tmp` left), not an error the tip does not return.
4. **G4 — manifest schema conformance (MA.2.2.8, low).** `[group.<g>.<proxy>]` and
   `[route.<proxy>]` bodies still fail closed but with `UnknownKey` instead of R8's documented
   message, and the `$1` spelling of the setup statement is exempt only in an unprotected
   `blastit-py` fixture. Both are cheap negative/positive twins in `critical_manifest_011`.

## Follow-ups

- **Equivalent mutant #15, no task.** `Stream::try_peek_response` returning `false` after
  keeping the frame changes no deterministic outcome: `fill_body`'s `select!` arm on
  `wait_response` keeps and reports the same frame, and `stream_chunked_body` has the same
  structure. The peek is a fast path/defence-in-depth; the 10 MiB test's
  `forwarded <= INITIAL_WINDOW` bound is deliberately loose (MA.2.2.3's RESULT documents the
  in-flight window). Removing the peek, or proving it with a timing test, is not worth a
  flaky oracle.
- **Doc drift in `write_atomic`.** The rustdoc says a planted symlink "is refused"; the code
  unlinks a stale temp file and retries, and that retry is what leaves the link's target
  intact. Either word the sentence as "removed, never written through" or return
  `AlreadyExists`; G3 pins whichever behaviour is chosen.
- **Scheduler-side `ctl_fd` client is still absent** (MA.2.2.3 follow-up 1), so the shared
  JSON-line protocol is only tested from the dbproxy end. Its scheduler half must get a
  protected test when MA.2.3.x wires it.
- **`blastit check` findings do not exist yet**, so acceptance item 3's third surface cannot be
  verified. When MA.2.3.4 lands, re-check that `missing_grant` carries the redacted `sql` and
  that `add_query` is never emitted for a statement with `…`.
- **Toolchain:** install `cargo-mutants` for the MA gate; `make mutants-rust` fails with
  `no such command: mutants` and this audit had to fall back to hand-picked branches.
