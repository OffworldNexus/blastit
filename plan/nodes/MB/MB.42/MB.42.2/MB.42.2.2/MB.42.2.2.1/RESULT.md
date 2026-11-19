# `MB.42.2.2.1` — `crates/blastit-audit`: one grant-diff owner

**Status: done.** `crates/blastit-py/src/audit.rs` (3,683 LOC) and
`crates/blastit/src/audit_v1.rs` (2,156 LOC) are one crate now, split into
cohesive modules, with one implementation of "executed but not granted / dead
grant" that both presenters read. `crates/blastit` and `crates/blastit-py` both
compile against it, and the Python extension still exports every name it did.

## The shared pass, and the two presenters

The merge shares the *answer*, not the document:

| piece | module | what it owns |
|---|---|---|
| `Ledger` | `ledger.rs` | per route per `(view, proxy)`: `record()` normalises one statement, asks the pair's check and files it under `matched` **or** `missing`. That pair of sets **is** "executed but not granted / dead grant". |
| `GrantCheck` | `diff.rs` | the allowlist dbproxy enforces, the granted statements (normalised, deduped) and the `transactions` flag — one constructor for both paths, which used to be two `Policy` types with two `of()`s. |
| `TcProbe` | `diff.rs` | the §5.2 transaction-control recognition, one copy. |
| locators | `diff.rs` | `grant_locator`, `trait_locator`, `body_locator` — `grant_locator` was duplicated byte-for-byte in both files and is now one function. |
| `Occurrence`, `EgressTrace`, `UnroutedTrace` | `ledger.rs` | the examples/counts a report keeps, moved with the diff so both callers' loop bodies are just `record()`. |

So `audit()` (`pytest.rs`) and `evaluate()` (`evaluate.rs`) both run the same
`Ledger::record`. The plugin presenter reads each pair's `matched` set; `check`'s
dead-grant rule asks the route-level question through
`Ledger::matched_contains(route, sql)`, which walks those same per-proxy sets —
there is no second `matched` map anywhere in the crate.

**Decision: shared pass + two thin presenters, not one document.** The two
reports are genuinely different artefacts — the pytest plugin's document and
`blastit check --json`/`--learn` (findings, fixes, `schema_version`) — and they
group dead grants differently *by contract*:

* `audit()` files a named group's dead grant under each route that names the
  group, reports global groups once in `unused_global`, and skips statements a
  `connection_setup` entry accounts for (`D3c`);
* `evaluate()` reports a group's dead grant once per group and suppresses one on
  an unexercised route in favour of `unexercised_route` (`R34`, `R35`).

That difference is grouping, and it is pinned by the moved suites in both
directions (e.g. `a_trait_every_view_uses_is_reported_per_view_not_once_globally`
vs. `dead_group_grants`), so unifying the documents would have changed user-visible
output for no gain. What is shared is what was duplicated: normalisation, the
allowlist decision, the matched/missing sets, the transaction-control probe and
the locators. This is the option the node's §2 explicitly allows.

## Modules and size (prod LOC = non-comment, non-blank lines)

`crates/blastit-audit/src/`

| file | prod LOC | holds |
|---|---|---|
| `lib.rs` | 42 | crate docs, module list, re-exports, three constants |
| `normalize.rs` | 1 | `normalize`, `has_quoted_literal`, `redact_literals` (policy re-exports) |
| `error.rs` | 61 | `AuditError`, `ObsError` |
| `analyze.rs` | 51 | `analyze`, `covering`, `classify`, `LOCK_NEEDS_A_COLUMN` |
| `model.rs` | 92 | `blastfile_model`, `blastfile_apply_edits` |
| `obs.rs` | 78 | `SCHEMA_VERSION`, `HttpObsLine`, `CheckObsV1`, JSONL readers |
| `observe.rs` | 258 | the plugin's observations document and its reader |
| `diff.rs` | 144 | `GrantCheck`, `TcProbe`, `grant_of`, `resolve_proxy`, locators |
| `ledger.rs` | 238 | `Ledger`, `Decision`, `ProxyLedger`, `RouteLedger`, `Occurrence`, egress/unrouted traces |
| `pytest.rs` | 99 | `audit()` (the plugin report entry point) |
| `assemble.rs` | 292 | the report document, assembled from the ledger |
| `sections.rs` | 247 | global-group state, `unused_global`, `connection_setup` |
| `evaluate.rs` | 212 | `parse_manifest`, `check_audit`, `evaluate` |
| `findings.rs` | 196 | `Finding`, `Fix`, `Severity`, counts, `Report` |
| `dead.rs` | 138 | `dead_grant` / `unexercised_route` (`R34`, `R35`) |
| `static_roots.rs` | 27 | unrequested static mounts (`R14`) |
| `ingress.rs` | 81 | `missing_ingress` / `ingress_mismatch` / `raw_ingress` (`R36`) |
| `human.rs` | 255 | `human`, `plural`, `probe_ingress_json` |
| `learn.rs` | 206 | `LearnPlan`, `plan_learn`, `plan_learn_json` |
| `patterns.rs` | 41 | the `Blastfile` pattern a path is written as |
| `infer_routes.rs` | 217 | moved verbatim from `crates/blastit/src/infer_routes.rs` |
| `tests.rs` + `tests/*.rs` | 0 prod / 1,863 lines | the audit suites, split by area |
| `check_tests.rs` | 0 prod / 307 lines | the check suites, moved |

Every file is under 500 prod LOC (largest: `assemble.rs`, 292).
`crates/blastit-audit/tests/grant_diff_parity.rs` adds the parity suite (0 prod).

## The equality test (the point of the merge)

`crates/blastit-audit/tests/grant_diff_parity.rs` feeds one fixture through both
entry points — the plugin's observations document and the gate's
`http_obs.jsonl`/`sql_obs.jsonl` — and asserts:

* `audit()`'s `missing[].sql` set **equals** `check_audit()`'s `missing_grant`
  `sql` set (`DELETE FROM "notes" WHERE "notes"."id" = $?`);
* `audit()`'s `unused` set **equals** `check_audit()`'s `dead_grant` `sql` set
  (`SELECT "notes"."title" FROM "notes" LIMIT 21`);
* both agree the run is not clean, and both count the same two observed
  statements.

The fixture has no groups and no connection setup — exactly the case where the
two groupings must coincide, so any remaining divergence would be a second
implementation showing through.

## Callers rewired

* `crates/blastit/src/audit_v1.rs` **deleted**; `crates/blastit/src/lib.rs` now
  has `pub use blastit_audit as audit_v1;`. Every existing call site
  (`check.rs`, `dev_learn.rs`, `cli/**`, `dev --learn`) and every suite —
  including the protected `critical_check_engine.rs`, `critical_static_cli.rs`
  and `blastit-py/tests/critical_check_schema.rs` — names the same path and
  reads `blastit-audit`, which is why none of those files changed.
* `crates/blastit/src/infer_routes.rs` moved to the new crate and is re-exported
  the same way (`pub use blastit_audit::infer_routes;`), so
  `blastit::infer_routes::infer` still resolves.
* `crates/blastit-py/src/audit.rs` **deleted**. `src/check.rs` calls
  `blastit_audit::*` directly, and `src/lib.rs`'s fourteen `#[pyfunction]`s are
  now one-liners onto `blastit_audit` (`normalize`, `has_quoted_literal`,
  `redact_literals`, `audit`, `analyze`, `covering`, `classify`, `check_audit`,
  `plan_learn`, `probe_ingress`, `blastfile_apply_edits`, `blastfile_model`).
  **`crates/blastit-py`, `Host::wheel` and `Install::Wheel` are untouched**, as
  the node requires: `python/blastit/__main__.py` still imports
  `blastit._blastit`. Their removal is `MB.42.2.2`'s first step.
* `crates/blastit/src/cli/internal_json.rs` **does not exist yet** (the CLI has
  `help.rs`/`parse.rs`/`preset.rs`/`run.rs`); design §3 gives it `<150` LOC and
  `MB.42.2.2` creates it. Nothing was thinned here because nothing is there.

## Invariants preserved verbatim

* **Secret redaction** — unchanged: every statement a report or `stderr` line
  names is normalised or `blastit_sqlpolicy::redact_literals`-ed before it gets
  there, and the move added no new text path (`human.rs`, `assemble.rs`,
  `learn.rs` are the moved bodies).
* **`E'..'`/`U&'..'` backslash handling (`MB.2.3.1`)** — not re-implemented: the
  crate owns no lexer, `normalize.rs` re-exports `blastit_sqlpolicy`'s three
  functions, and `Ledger::record` calls that one `normalize`.
* **Atomic write with mode preservation** — `blastfile_apply_edits` still
  delegates to `blastit_manifest::blastfile::apply_edits` (the verified editor);
  the atomic write stays `blastit_manifest::edit::write_atomic` on the `check`
  side.
* **`audit()` and `covering()` return JSON *text*** — unchanged signatures
  (`Result<String, AuditError>` / `Result<String, String>`), so `fold.py`'s
  `json.loads` keeps working; `check_audit`/`plan_learn_json` still return text.
* **No `unwrap()`/`expect()` on untrusted input** — the only `unwrap()` in the
  production modules is the pre-existing `serde_json::to_value(...).unwrap_or_else`
  fallback in `ledger::table_grants_json`; the single `expect()` in the crate is
  in `infer_routes.rs`'s `#[cfg(test)]` block.
* **`<500` prod LOC, `<=6` args, no globals** — `Ledger::record` takes six
  arguments (self included); `EMPTY_MATCHED` is an immutable empty `BTreeSet`
  moved with the code (no `OnceLock`, no `static Mutex`, no mock trait, no
  `#[cfg(test)]` env seam was added).

## Evidence

```
$ cargo tree -p blastit-audit --locked -e normal | grep -c pyo3
0

$ cargo test -p blastit-audit -p blastit -p blastit-py --locked     →  0 failed
... test result: ok. 286 passed; 0 failed   (blastit lib)
... test result: ok. 78 passed; 0 failed    (blastit-audit lib: 56 audit + 22 check)
... test result: ok. 3 passed; 0 failed     (grant_diff_parity)
... test result: ok. 31 passed; 0 failed    (critical_check_engine)
... test result: ok. 29 passed; 0 failed    (critical_static_cli)
... test result: ok. 6 passed; 0 failed     (critical_check_schema)
(all other suites: ok, 0 failed)

$ cargo test -p blastit --test critical_check_engine        → ok. 20 passed; 0 failed
$ cargo test -p blastit --test critical_static_cli          → ok. 12 passed; 0 failed
$ cargo test -p blastit-py --test critical_check_schema     → ok. 6 passed; 0 failed

$ cargo clippy --workspace --all-targets --locked --features python -- -D warnings
(no output — clean)

$ RUSTDOCFLAGS="-D warnings" cargo doc -p blastit-audit -p blastit -p blastit-py --no-deps --locked --features python
(no output — clean)

$ cargo fmt --all -- --check
(clean)
```

`make fast-check` (fmt, clippy, doc, workspace tests, Python lint, plan
validation) is the gate this slice ran last and it passed; its tail is in the
report to the orchestrator.

## Review fixes (`review-arch` + `review-ux`, post-approval)

Both reviewers approved the extraction; these are the consolidated items and
what was done with each.

1. **One `matched` owner (MAJOR).** `evaluate()` no longer keeps a
   `BTreeMap<usize, BTreeSet<String>>`: it calls `ledger.request(route, false)`
   for each routed HTTP line (so request counts live in the ledger too) and
   deletes the map. `dead_route_grants`, `dead_group_grants` and
   `unexercised_routes` take `(&Manifest, &Ledger, …)` and ask
   `Ledger::matched_contains(route, sql)` / `Ledger::requests(route)`, which
   walk the per-proxy sets without flattening or cloning them. The set-returning
   `Ledger::matched()` is **deleted** (it had no callers), the per-statement
   clone+insert on the `check` hot path is gone, and the two comments that
   described a guarantee the code did not implement were rewritten to say what
   is true (both presenters read one `record`; `check`'s dead-grant rule is the
   route-level question asked of those same sets).
2. **`assemble` takes 5 arguments (MAJOR).** A `#[derive(Copy)] struct Run<'a> {
   manifest, ledger, global }` bundles the three inputs; `assemble(run, warnings,
   observations, unrouted, summary)` destructures it back into the names the body
   already used, so the diff is the signature plus one line.
3. **The `granted`-gate change is pinned, not reverted.** Old `audit.rs`
   inserted into `matched` only when the grant *listed* the statement; the shared
   `record` inserts when the grant **allowed** it. I kept the new meaning —
   `matched` is "the pair's grant allowed this statement", which is exactly what
   `check`'s dead-grant rule needs (it used `allowed` too) and what a route
   actually ran — and pinned it in
   `tests::groups::an_unrestricted_grant_counts_an_allowed_statement_as_matched`:
   an `unrestricted` grant's `granted()` is empty, `allows("SELECT 1")` is true,
   and `matched_contains` is true. The reviewer's `unused_global` consequence
   cannot be observed through a `Blastfile` today because `global_groups()` is
   always empty for one (§7.1, `crates/blastit-manifest` sets it to `Vec::new()`
   for the file reader); the test therefore pins the semantic where it *is*
   observable rather than inventing a retired-TOML fixture.
4. **The presenter divergence is pinned.** New case
   `tests::groups::a_groups_dead_grant_and_the_plugins_unused_line_name_the_same_statement`:
   a trait whose table grant covers its own `query`, used by one view. The plugin
   reports that statement in the route's `unused`; `check` reports it **once** as
   `dead_grant { group: "session", fix.to: "trait session" }`; and the route's own
   query, covered by the same table grant, is dead in neither. Written against
   the two real entry points (`audit` and `check_audit`), so a change on either
   side fails it.
5. **`blastit-manifest` is a dev-dependency.** `crates/blastit-py/Cargo.toml`
   moves it under `[dev-dependencies]` with a comment naming the one suite that
   needs it; the protected test is untouched. The extension's own dependency list
   is now `blastit`, `blastit-audit`, `pyo3`, `serde_json`.
6. **Stale docs / duplicated work.** `pytest.rs` no longer describes a
   `normalize_sql` it does not have; the probe is built once —
   `Ledger::new` owns a `TcProbe` that `record` uses, instead of one per
   statement.
7. **`human.rs` doc deduplicated.** The "every finding names the `--json` facts"
   paragraph stays on the module; the `human` fn doc keeps only what is specific
   to the function (the clean one-liner vs. header + findings + footer).
8. **Parity suite limitation noted.** `grant_diff_parity.rs` now says in its
   module doc that, because both entry points share `record`, the cross-assertion
   catches *presenter* divergence, while the hard-coded statement literals are
   what cover a change inside the shared pass — so those literals must stay.

## Follow-ups

* `crates/blastit-py/Cargo.toml` still needs `blastit-manifest` for the protected
  `tests/critical_check_schema.rs` (now `[dev-dependencies]` only); `MB.42.2.2`
  deletes that crate and the entry with it.
* `evaluate()`'s dead-grant rule still answers at route granularity while
  `Ledger` keeps the sets per proxy; if `R34` ever wants "dead on this store
  only", `matched_contains` grows a proxy argument and nothing else changes.
* The audit suites were split across `src/tests/*.rs` at test boundaries; two
  files (`setup.rs`, `egress.rs`) still hold helper fixtures their neighbours
  import (`SETUP`, `GLOBAL`) — moving those to `tests/common.rs` would read
  better but is churn for a later sweep.
* `pub use blastit_audit as audit_v1` in `crates/blastit/src/lib.rs` is the
  compatibility path for this slice; retiring it (and the protected suites'
  `blastit::audit_v1::…` imports with it) is `MB.42.2.2.2`.
* The `proxy <name>: declared, 0 statements observed` fix-text in
  `human.rs` is `MB.42.6`'s, deliberately untouched here.
