# Result MB.21.4.3 (attempt 1)

**`STATUS: needs_split`** — this node cannot reach its own `# Acceptance` from its own `paths`:
**six** files that assert the strings and shapes items 2, 3 and 4 change are outside `paths`, and
**four** of them are protected, so `critical_guard.py` would call the edit a violation. The two
sentences this leaf owns are therefore *still in the tree*, and the fold/pytest-plugin cutover — the
largest piece — is blocked by the very suite the dispatch says is mine to move
(`python/tests/critical/test_fold_literals_and_atomic.py`, which is **not** in `paths`).

What **did** land is complete, green and verifiable on its own: the `--json` `fix.to` node locators
(item 1), the §18 pieces whose asserting suites *are* in `paths` (`Location::View`,
`ManifestError::position()`, `InvalidApp`), item 5's renames and both doc follow-ups, and item 8's
"one editor" wording so its grep prints nothing. §3 below is the concrete split; §4 is the plan
defects with file:line, so the next dispatch needs no search of its own.

## 1. What landed, and the test that pins it

| Node item | Landed | Pinned by |
|---|---|---|
| 1 — `--json` `fix.to` is a **node locator** | `grant_locator`/`trait_locator`/`body_locator` in `crates/blastit/src/audit_v1.rs`: `view <view> postgres[ <store>]` for `add_query`/`remove_query`, `trait <name>` for `add_group` and a trait's `remove_query`, `view <view>[ <METHOD>] body` for `raw_ingress` (method only when one endpoint owns the `body`, `HEAD` dropped as implied); `DEFAULT_STORE` is public so the crate spells the unnamed block the way a file writes it | `critical_check_engine::{every_add_query_reparses_under_parse_and_validate, connection_setup_sql_names_the_connection_trait_grant, ordinary_statements_keep_the_per_view_add_query, only_connection_setup_names_a_trait_and_never_a_refusal, the_body_finding_names_the_view_body_node_and_never_unvalidated, every_finding_kind_and_its_interaction_counts}`, `audit_group_hint::*`, **protected** `python/tests/critical/test_check_contract.py::test_the_report_names_the_fix` |
| 3 (part) — `Location::View` names the view | `error.rs`: `view "x"`, never `view "x" (index 0)`; the sandbox index stays in the variant but is not printed | `validate::{locations_read_well_in_errors, a_view_refusal_names_the_view_and_no_index}` (the second is the end-to-end rendering of a `Blastfile` grant error) |
| 3 (part) — `ManifestError::position()` | new `pub fn position(&self) -> Position` over all 63 positioned variants, `Position::NONE` otherwise; `cli.rs::report_manifest_errors` reads it and `positioned()` (the split-and-sniff helper) is **deleted** | `critical_blastfile_fmt` (path-prefixed refusals, `./Blastfile:2:10: …` and `<path>: <message>`), `blastfile_discovery` (one escaped line at every entry point) |
| 3 (part) — `InvalidApp` stops echoing its value | `app: must be \`module:attribute\`` — the value is compared, never rendered (a pasted `$TOKEN` ends up in that slot) | `mutation_gaps::a_refusal_never_carries_the_app_it_refused` (`$BLASTIT_TOKEN` and `ghp_s3cr3tToken`, plus the allowed twin), `critical_manifest_011::app_must_be_module_attribute` unchanged |
| 5 — `manifest_toml` → `manifest_text` | `crates/blastit-py/src/lib.rs::audit_report`, `crates/blastit-py/src/audit.rs::audit` (signatures stay positional), docstrings re-written to say what the text is and that the retired reader is the next leaf's; `check.rs`/`audit_v1.rs` were already `manifest_text` | doc build (`RUSTDOCFLAGS=-D warnings`) + `make check` |
| 5 — the two 4.2 follow-ups | `cli.rs::one_line` **deleted**; `fmt` uses `config::display_path` (one home for §18's escaping/cut). `crates/blastit-manifest/src/lib.rs:22-24` no longer says "every route is static" | `critical_blastfile_fmt`, `docs-test` |
| 8 (part) — one editor, one wording | `check --learn` (`check.rs:994`) and `dev --learn` (`dev_learn.rs:464`) write through `blastfile::apply_edits` + one `edit::write_atomic`; `blastfile/mod.rs`, `edit.rs`, `lib.rs` no longer present `edit::apply_edits` as the editor, so acceptance 8's second grep prints **nothing** | the grep in §5.6 |

`fix.to` spelling: the design's §13.3 table and `docs/reference/blastfile.md:451` both write the
default store **without a name** (`view catalog postgres`) and a named store appended
(`view catalog postgres analytics`); the node's `view restaurants postgres db` is that same rule with
a store literally named `db`. I followed the accepted page.

## 2. Why the rest cannot land from this node's `paths`

Every line below is an assertion that *is* the contract the item changes; each is outside `paths`,
and the ones marked **P** are protected (`tools/critical_guard.py` would refuse the edit).

| Blocked item | File:line | What it pins |
|---|---|---|
| §13.2 `EgressRoute` sentence (and with it the whole five-sentence sweep) | `python/tests/test_pytest_plugin.py:461` | `route "note-cache": egress = true opens any outbound address and port` in the plugin's `manifest.warnings`. `python/tests/**` is `MB.21.4.4`'s; re-spelling the warning turns this suite red until that leaf lands |
| §13.2 #6 `EntrypointNotReached` (the new variant) | **P** `crates/blastit-manifest/tests/critical_blastfile_parity.rs:1132-1138` | an exhaustive `match` over `ManifestWarning`; a sixth variant does not compile in that file |
| §10.2 `raw_ingress_refusals` | `crates/blastit/src/stage0.rs:6339-6342` | asserts `route "upload"`, `allow_raw = true`, `[route.ingress]`. `stage0.rs` is not in `paths` (`stage1.rs` is) |
| §11.3 `UnrestrictedGroup` (`route`→`view`) | **P** `crates/blastit-manifest/tests/critical_blastfile_traits.rs:61`, `crates/blastit-manifest/tests/blastfile_traits.rs:115` | `unrestricted is only allowed inline on a route` |
| §18 `ManifestErrors`' `Display` (one line per problem, no header) | `crates/blastit-manifest/tests/blastfile_errors.rs:109-116`, `crates/blastit-manifest/tests/parse.rs:461,466,547` | `lines.len() == errors.len() + 1` and the `manifest has N error(s)` header |
| §18 `PhpEntryInvalid` stops echoing its entry | **P** `crates/blastit-manifest/tests/critical_php_manifest.rs:349-352` | the exact `route "app" (index 0): php entry "index.php": …` rendering |
| item 4 — the whole Python cutover | **P** `python/tests/critical/test_fold_literals_and_atomic.py` (486 lines: it calls `fold.plan(tomllib.loads(MANIFEST), …)`, `fold._Addition`, `fold._write_atomic`, `fold.main` on a TOML file) plus `python/tests/test_pytest_plugin.py`, `python/tests/test_plugin_helpers.py:97-121` | the fixture format *and* the fold/pytest-plugin API. This is the file the dispatch calls mine to move; it is not in `paths` |
| item 6 — `e2e/php2shell.py::check_learn` | `e2e/**` fixtures | `MB.21.4.4` translates them (its `# Context`: `make e2e` is red between 4.1 and 4.4); the case cannot be green before that, and `make e2e` needs Docker |

Two further facts make item 4 undecidable here, both real:

1. **`BlastfileEdits` cannot express fold's policy.** `ProposedGrant` is `{view, store, queries}`
   (§14.5) — there is no table or function field, so `fold --prefer-tables` and the `functions`
   grants it writes today have no `Blastfile` form. Re-pointing `fold` means either dropping a
   documented flag (`docs/reference/cli.md:95`) or growing the editor's proposal type, which no
   accepted doc authorises. That is an owner/planner decision, not an implementer's.
2. **§13.2's acceptance grep cannot print nothing.** It greps for `passes the request body through
   unvalidated` and `opens any outbound address`, which §13.2's own table says are re-spelled only in
   the *prefix* (`body = "raw"`→`body raw`, `egress = true`→`egress any`). Either the acceptance
   command is stale or the two sentences must change more than the table says.

## 3. Proposed split (concrete)

| New node | Scope | `paths` it needs |
|---|---|---|
| **4.3a** — the §13.2/§10.2/§11.3 sentence sweep, exactly as the design's tables spell it | the five `ManifestWarning` sentences + the new `EntrypointNotReached`, `raw_ingress_refusals`, `UnrestrictedGroup`; assert each string (not "a warning happened"), keep trigger/order/count | *existing* plus `crates/blastit/src/stage0.rs`, `crates/blastit-manifest/tests/{parse.rs,blastfile_errors.rs,blastfile_traits.rs}`, **`crates/blastit-manifest/tests/critical_blastfile_parity.rs`**, **`crates/blastit-manifest/tests/critical_blastfile_traits.rs`**, **`crates/blastit-manifest/tests/critical_php_manifest.rs`**, **`crates/blastit-manifest/tests/critical_manifest_011.rs`**, `python/tests/test_pytest_plugin.py` |
| **4.3b** — §18's remaining contract | `ManifestErrors`' `Display` (one line per problem), `PhpEntryInvalid` stops echoing, the §13.2/§18 sentence assertions moved with 4.3a | the same test files as 4.3a's last row |
| **4.3c** — fold/`_blastit.audit`/pytest-plugin cutover **and** the new PyO3 binding `blastfile_apply_edits(source, edits_json) -> str` | re-point `fold.py`'s reader/writer at the editor, delete its `tomllib` scan/verify, keep `--manifest PATH` and the never-literal/never-unrestricted/never-twice policy; `pytest_plugin.py`'s default and help text; `python/tests/{test_check,test_fold}.py`; `e2e/php2shell.py::check_learn` | `python/tests/**` (the whole tree, incl. the protected fold suite), `crates/blastit-py/tests/**`, `e2e/**`; **plus** a decision on `--prefer-tables`/`functions` under `BlastfileEdits` |

Splitting that way keeps every acceptance command satisfiable by the node that owns it: 4.3a owns
the first grep, 4.3b the `Display`/echo assertions, 4.3c the fold wall and the e2e case.

## 4. Acceptance evidence (final tree)

All commands in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0`, foreground, under
`timeout`. `make check` was run **after the last edit**.

### 5.1 `make test-critical` — exit 0, zero skips, 50 Python tests

```console
$ timeout 2400 make test-critical ; echo EXIT=$?
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
cargo build --locked -p blastit-py -p blastit-dbproxy --features blastit/python
python -m unittest discover -s python/tests/critical
..................................................
Ran 50 tests in 4.788s
OK
EXIT=0
```

### 5.2 The protected Python contract suite — 22 passed

```console
$ timeout 1200 env UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 \
    python -m pytest python/tests/critical/test_check_contract.py -q
......................                                                   [100%]
22 passed in 4.03s
```

(`LESSONS.md` records why this needs the in-tree `python/blastit/_blastit*.so` refreshed first: a bare
`pytest` imports that built extension, and a stale one runs the previous commit's CLI. I refreshed it
from `target/debug/lib_blastit.so`; it is gitignored build residue, not source.)

### 5.3 `critical_guard.py --base 537eb2d --node MB.21.4.3` — clean

```console
$ timeout 600 uv run --no-project tools/critical_guard.py --base 537eb2d --node MB.21.4.3
critical-guard: 537eb2d..working tree, 20 changed file(s)
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
  M  python/tests/critical/test_check_contract.py  [authorized by python/tests/critical/test_check_contract.py]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.21.4.3
```

No assertion was relaxed: the contract test's `fix` assertion now names the documented locator
(`{"action": "add_query", "to": "view home postgres"}`), and `critical_check_engine` gained a test
(§1) rather than losing one.

### 5.4 `cargo test -p blastit-manifest --locked --test validate --test mutation_gaps` — green

```console
$ timeout 900 cargo test -p blastit-manifest --locked --test validate --test mutation_gaps
running 21 tests   # mutation_gaps
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
running 30 tests   # validate
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 5.5 The first grep — still prints, because §13.2 is blocked (§2)

```console
$ grep -rnE 'attached to no route|no grant in any route or group|may run any statement on proxy|passes the request body through unvalidated|opens any outbound address' \
    crates/blastit-manifest/src/error.rs crates/blastit/tests/
crates/blastit-manifest/src/error.rs:1556:        "group {} is attached to no route and no global groups: it grants nothing",
crates/blastit-manifest/src/error.rs:1565:    #[error("proxy {} has no grant in any route or group: it is unusable", quoted(.proxy))]
crates/blastit-manifest/src/error.rs:1574:        "route {} may run any statement on proxy {} (unrestricted = true)",
crates/blastit-manifest/src/error.rs:1590:        "route {}: body = \"raw\" passes the request body through unvalidated",
crates/blastit-manifest/src/error.rs:1600:        "route {}: egress = true opens any outbound address and port",
crates/blastit/tests/critical_raw_ingress.rs:647:        stdout.contains("passes the request body through unvalidated"),
crates/blastit/tests/critical_raw_ingress.rs:674:        stdout.contains("passes the request body through unvalidated"),
```

I drafted the five re-spellings and **reverted them**: without `EntrypointNotReached` (blocked) and
without `raw_ingress_refusals` (blocked) the boot would say `view … body raw` while the production
refusal still said `route … allow_raw = true`, and the `EgressRoute` re-spell turns
`test_pytest_plugin.py` red — a regression in a suite this node cannot fix. No sentence moved.

### 5.6 The second grep — prints nothing

```console
$ grep -rn 'blastit_manifest::edit::apply_edits\|edit::apply_edits' crates/ --include='*.rs'
(no output; exit 1)

$ grep -n 'blastfile::apply_edits\|write_atomic' crates/blastit/src/check.rs crates/blastit/src/dev_learn.rs
crates/blastit/src/check.rs:994:        match blastit_manifest::blastfile::apply_edits(source, &plan.edits) {
crates/blastit/src/check.rs:996:                match blastit_manifest::edit::write_atomic(&config.manifest, &edited) {
crates/blastit/src/dev_learn.rs:464:        let edited = match apply_edits(&source, &edits) {
crates/blastit/src/dev_learn.rs:472:            && let Err(err) = write_atomic(manifest, &edited)
```

(`dev_learn.rs:19` imports `blastit_manifest::blastfile::apply_edits`.)

### 5.7 `make check` — exit 0

```console
$ timeout 5400 make check ; echo EXIT=$?
guard: OK (15 staged/untracked file(s) checked)
cargo fmt --all -- --check                                    # clean
cargo clippy --workspace --all-targets … -- -D warnings        # Finished, 0 warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps …   # Finished, 0 warnings
cargo test --workspace --all-targets --locked --features python # 166 `test result: ok`, 0 failed
python -m unittest discover -s python/tests/critical           # Ran 50 tests, OK
49 fences: 28 executed, 21 accounted, 0 failed · 25.9s
… rs-no-python, seam-check, platform-matrix, workflow-check, packaging-gates, doc-tone,
  manifest-purity (17 sources, no std::env) …
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

(166 `test result: ok` blocks, no `FAILED`; the only `warning:` line is cargo's pre-existing "doc tests
are not supported for crate type(s) `cdylib`".)

## 6. Behaviour that changed, and its test

| Change | Test |
|---|---|
| every `fix.to` is a Blastfile node locator; a named store appends its name | §1 |
| `raw_ingress`'s `fix` names `view <view> [<METHOD>] body`, and no `fix` object carries `unvalidated` or a retired `[route.…]`/`[group.…]` path | `the_body_finding_names_the_view_body_node_and_never_unvalidated` |
| a view's grant refusal says `view "x"`, not `view "x" (index 0)` | `validate::a_view_refusal_names_the_view_and_no_index`, `locations_read_well_in_errors` |
| `cli.rs` prefixes the path by `ManifestError::position()`; `fmt` renders a path with `config::display_path` | `critical_blastfile_fmt`, `blastfile_discovery` |
| `app: must be \`module:attribute\`` (the refused value is never echoed) | `mutation_gaps::a_refusal_never_carries_the_app_it_refused` |

## 7. Files outside `paths`

Only `docs/knowledge/LESSONS.md` (one line, §5.2's gotcha) and the gitignored build residue
`python/blastit/_blastit*.so` (re-copied from `target/debug/lib_blastit.so` so a hand-run of the
protected suite grades this tree). No `plan/` frontmatter was hand-edited; the `plan/` and
`docs/research/` diffs in `git status` were already dirty when this attempt started.

## Follow-ups

- **`python/tests/test_check.py` is stale in a way that predates this attempt:** its `MANIFEST` and
  `DJANGO_MANIFEST` are still TOML, its project writes `blastit.toml`, and `blastit check` now parses
  only a `Blastfile` — the suite is `skipif(not DATABASE_URL)`, which is why nobody saw it. Its
  `fix.to` assertion (line 224) is the old `[route.db].queries` form. It is `4.3c`'s, together with
  `test_fold.py`; both need PostgreSQL or a rewritten fold to run.
- **`docs/reference/manifest.md:118,318` and `docs/testing/AUDIT-MA.2.3.md:172` quote the sentences
  that have not moved yet.** They become wrong exactly when `4.3a` lands; `MB.21.5.1` rewrites the
  reference pages, so whoever lands `4.3a` should either move the quotes with it or note it there.
- **`set_transaction_control`**: `docs/design/learn-single-pass.md` R18 pins a `fix.action` and
  `fix.to == "[route.db].transaction_control"`; the 0.2.0 engine emits `observe` for transaction
  control (`audit_v1`'s module doc says so). Not this leaf's, and not a `--json` v1 action today, but
  the design page and the engine disagree — worth a planner's look before the v1 schema is frozen.
- **`EntrypointNotReached` is still only a doc comment** (`model.rs:870`, `stage0.rs:317`,
  `critical_blastfile_entrypoint.rs:100`). `4.3a` adds the variant; the scheduler parity suite's php
  row should then assert it (§4.2's follow-up, still open).
