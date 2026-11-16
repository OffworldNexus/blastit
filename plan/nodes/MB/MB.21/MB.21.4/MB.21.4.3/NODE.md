---
id: MB.21.4.3
title: "Map check, dev --learn and the --json fix actions onto Blastfile nodes, re-spell §13.2/§10.2/§11.3, close §18 and grow the editor's grant scope"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.4.2]
paths:
  - crates/blastit/src/check.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/tests/**"
  - crates/blastit/tests/critical_check_engine.rs
  - crates/blastit/tests/critical_learn_executor_blocks.rs
  - crates/blastit/tests/critical_raw_ingress.rs
  - crates/blastit/tests/audit_group_hint.rs
  - "crates/blastit-py/**"
  - "python/blastit/**"
  - "python/tests/**"
  - e2e/php2shell.py
  - "docs/design/blastfile-dsl.md"
attempts: 3
max_attempts: 5
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T13:23"
---

# Goal

Make `blastit check`, `blastit dev --learn` and the `--json` contract write and read `Blastfile`:
`fix.action` keeps the accepted vocabulary (`add_query`, `add_group`, `remove_query`, `declare_ingress`,
`fix_ingress`, `add_route`, `raw_ingress`) and, where `fix.to` appears at all, it is a Blastfile **node
locator** (`view restaurants postgres db`, `trait connection`, `view upload body`) — never a directive to
paste. The surgical editor from `MB.21.3.5` is what applies the edits. The sentences that §13.2/§10.2/§11.3
re-spell, the §18 error-contract gaps, the `manifest_toml` → `manifest_text` renames and the editor's one
public entry point also land here, because every one of them is pinned by a suite outside the 3.x leaves
that wrote the strings.

**One leaf for the Rust contract.** Six files assert the strings and shapes they move, four of them
protected, and the editor needs a field it did not have; the implementer's proposed 4.3a/4.3b/4.3c split
was rejected because three more nodes would have bought three reviews for one commit's worth of work.
That ruling stands and is not re-litigated.

**The editor grows here, and `MB.21.4.7` consumes it.** `ProposedGrant` (design §14.5) was
`{view, store, queries}` and cannot express the `--prefer-tables` scope or the `functions` grants `fold`
writes today, so `fold --manifest` cannot be re-pointed without either dropping a documented flag
(`docs/reference/cli.md:95`) or growing the editor. **The editor grows**: `ProposedGrant` gains the
table/function scope, and §14.5's row in `docs/design/blastfile-dsl.md` gains that field with its refusal
shape in the same commit. `docs/reference/cli.md` and `--manifest PATH` do not change — the flag keeps
working. The Python caller that exercises the new field is `MB.21.4.7`.

**This leaf's scope is the Rust half only.** The Python cutover (`python/blastit/{fold,pytest_plugin}.py`,
`_blastit.audit()`'s parse and the four Python suites) is `MB.21.4.7`; `e2e/php2shell.py::check_learn`'s
fixture translation is `MB.21.4.4`'s. Nothing here waits on them.

# Context

- Design §13.3/§14 and `ADR-0003`/`ADR-0004` (the `--json` v1 schema and the literal-redaction rules are
  **unchanged**; only the `to`/`fix` strings that name a node change).
- Code sites: `crates/blastit/src/{check,dev_learn,audit_v1}.rs`, the `learn_line` helper
  (`check.rs:1364-1379`), `read_manifest` (`check.rs:294-300`), and the executor glue that calls the
  manifest editor. `dev --learn` writes grants; `check --learn` proposes new views only — keep that
  asymmetry exactly (`MB.1.6`, `docs/VISION.md`).
- Rename the `manifest_toml` parameters to `manifest_text` and fix their docstrings:
  `crates/blastit-py/src/lib.rs:156,179,302,326-333` (a positional PyO3 signature — keep it positional so
  `python/blastit`'s call sites keep working), `crates/blastit-py/src/audit.rs:412-414`,
  `crates/blastit-py/src/check.rs:25,39`, `crates/blastit/src/audit_v1.rs:545,1447`.
- Mapping rules to implement and pin:
  * `missing_grant` → `add_query`, `to` is `view restaurants postgres db` (a named store appends its
    name: `view catalog postgres analytics`), and the editor inserts `query 'SELECT …'` inside that
    store's grant block, creating the block when absent;
  * connection-setup SQL → `add_group` with `to` = `trait connection` (the `MB.1.6` group hint keeps its
    group name; the node it writes is a `trait` plus a `use` line on every view);
  * `unrouted` → `add_route` with no `to` — `fix` carries `method`/`path`, and the view is inserted above
    the method-unrestricted catch-all `view`;
  * a body seen on a view without a codec → `declare_ingress`, whose `fix` is `fix_ingress` and whose
    `to` is `view upload body`;
  * `raw` is never written, `unvalidated` never appears in a `fix`, and a statement inlining a literal is
    still refused rather than written.
- Tests: `crates/blastit/tests/critical_check_engine.rs` and
  `crates/blastit/tests/critical_learn_executor_blocks.rs` (rewrite fixtures to Blastfile; assertions
  stay or get stronger), `python/tests/critical/test_check_contract.py` (the `--json` document shape and
  the `fix.to` locators), and `crates/blastit/tests/audit_group_hint.rs` (now a trait hint).
- **Sentence re-spellings pinned outside every 3.x leaf** (`MB.21.3.7`/`MB.21.3.8` could not do them — the
  asserting suites were not theirs). Each keeps its trigger, its order and its count; only the nouns
  change:
  * §13.2's five existing boot warnings, all in `crates/blastit-manifest/src/error.rs`, were spelled for
    the retired file: `UnattachedGroup`, `UngrantedProxy`, `UnrestrictedRoute`, `RawIngress`,
    `EgressRoute` → `trait`/`view`/`store` and `body raw`/`egress any`; `EntrypointNotReached` is **new**
    (it existed only as a doc comment, `model.rs:870`) and keeps §12.2's wording. Pinned by
    `crates/blastit-manifest/tests/{validate,mutation_gaps}.rs`;
  * §10.2's production `raw_ingress_refusals` sentence → ``view "<name>": body raw hands the request body
    to the application unvalidated, and the production path refuses it without the acknowledgement;
    write `body raw unvalidated` in that view, or declare the codec it takes (json, form, none)`` — same
    function, same call sites, same order. Pinned by `crates/blastit/tests/critical_raw_ingress.rs`;
  * §11.3's `group` → `trait` noun anywhere the sweep above does not reach (`error.rs:434`'s
    `UnrestrictedGroup`, and `Location::Group` rendering `trait "…"`), pinned by
    `crates/blastit-manifest/tests/blastfile_traits.rs` and its protected twin
    `critical_blastfile_traits.rs`;
  * **§13.2 re-spells only the prefix of two sentences**: `body = "raw"` → `body raw` and
    `egress = true` → `egress any`, so `passes the request body through unvalidated` and
    `opens any outbound address and port` **stay**. §12.2 pins `EntrypointNotReached` as
    ``entrypoint php "…" is reached by no view: declare a `view` that forwards to it``. No acceptance
    command may ask for the *absence* of the shared tail — that is a re-spelling the design does not make;
  * these suites assert what the sweep moves, so each was edited **here** and never weakened:
    `crates/blastit-manifest/tests/{validate,mutation_gaps,blastfile_traits,blastfile_errors,parse}.rs`;
    protected `crates/blastit-manifest/tests/{critical_blastfile_traits,critical_blastfile_parity,
    critical_blastfile_errors,critical_php_manifest}.rs` (the parity suite's exhaustive
    `ManifestWarning` `match` at `:1132-1138` compiles only with the sixth `EntrypointNotReached` arm, and
    `critical_php_manifest.rs:349-352` was `PhpEntryInvalid`'s echo);
    `crates/blastit/tests/critical_raw_ingress.rs`; the internal test in
    `crates/blastit/src/stage0.rs:6339-6342`; and `python/tests/test_pytest_plugin.py:461`, which pins
    the `EgressRoute` sentence inside the plugin's `manifest.warnings` and is rewritten in place.
- **§18's error-contract gaps:** `ManifestErrors`' `Display` joins one line per problem (no trailing
  newline, no double prefix), `Location::View` stops printing "(index 0)" (it names the view), and
  `InvalidApp`/`PhpEntryInvalid` stop echoing their value — they name the position and the expected shape
  (`module:attribute`, a relative `.php`), because a pasted DSN or token can land in either slot. Add
  `ManifestError::position()` so `cli.rs`'s `report_manifest_errors` reads the position instead of
  splitting the rendered string, and keep `error::clipped`/`escape_control` as the only folding code
  (`MB.21.4.1` deletes `cli.rs`'s copy). The old rendering was asserted by
  `crates/blastit-manifest/tests/blastfile_errors.rs` and `tests/parse.rs` (a `manifest has N error(s)`
  header and `  - ` prefixed lines — exactly the header and the double prefix the design deletes), and
  `PhpEntryInvalid`'s echo by protected `crates/blastit-manifest/tests/critical_php_manifest.rs`. All
  three are inside `crates/blastit-manifest/tests/**`: rewrite the rendering and the changed assertions,
  never the strength.
- **One editor entry point:** `blastfile::apply_edits` over `BlastfileEdits` is *the* editor; the retired
  TOML patcher's `blastit_manifest::edit::apply_edits` stops being a second public path. Re-point
  `check.rs:979` (`check --learn`) and `dev_learn.rs:459` (`dev --learn`) at the Blastfile entry point,
  keep one `write_atomic`, and keep the patcher's own `EditError` mapping (`LiteralQuery`,
  `RedactedQuery`, `Verification`, `Source`) compiling and asserted until `MB.21.4.5` deletes its reader
  — its sentences are pinned by `edit_branches.rs` and `critical_manifest_edit*.rs`.
- **The editor's proposal type grows (ruling, landed here).** `ProposedGrant` gains the scope `fold`
  needs: a per-store table grant (each table with its `select`/`insert`/`update`/`delete` mode, columns or
  all, spelled as §13.1 spells them) and a `functions` list, merged into the view's existing
  `postgres [store] { }` block for that store (or the new block, as any grant), so P4's postcondition stays
  `equivalent()` and a repeated proposal adds nothing. The refusals are the grant block's own: a store the
  view does not declare is `UnknownProxy`/`MissingRoute` naming the view and the store, an empty column
  list and a function name the grammar cannot write are refused before anything is spliced, and the
  structural ban stays (`unrestricted`, `use`, `transactions` and `body` remain unproposable — `fold`
  never writes `unrestricted`, §13.3). §14.5's row in `docs/design/blastfile-dsl.md` gains the field
  **and** its refusal shape; `docs/reference/cli.md:95`'s flags do not change.
- **`dev --learn` end to end:** `dev_learn.rs` builds `BlastfileEdits` for each recorded finding and
  performs one atomic write; the live `blastit dev --learn` case in `e2e/php2shell.py::check_learn` is
  **expectation-only** here — `MB.21.4.4` translates the fixture it runs against and owns `make e2e`, so
  only the case's expectations (the file survives, comments survive, a second run adds nothing) belong to
  this leaf.

# Acceptance

```console
$ make test-critical
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/critical/test_check_contract.py python/tests/test_pytest_plugin.py -q
$ uv run --no-project tools/critical_guard.py --base 537eb2d --node MB.21.4.3
$ cargo test -p blastit-manifest --locked
$ grep -rnE 'attached to no route|no grant in any route or group|may run any statement on proxy|body = "raw"|egress = true' crates/blastit-manifest/src/error.rs
$ grep -rn 'blastit_manifest::edit::apply_edits\|edit::apply_edits' crates/ --include='*.rs'
$ make check
```

1. `dev --learn` on a Blastfile fixture adds exactly one `query` line inside the right store block, keeps
   every comment and blank line, and a second run adds nothing.
2. `check --learn` still proposes views only; `--json` bytes match the documented v1 schema with the
   page's action names (`add_query`/`add_group`/`declare_ingress`/`add_route`) and the node locators in
   `fix.to`, and the literal-refusal case still refuses.
3. The connection-setup finding is `add_group` with `to` = `trait connection`, and the text it writes is
   a `trait` block plus a `use` line on every view (assert the exact strings).
4. `raw` is never emitted by any learn path and `unvalidated` appears in no `fix` object (assert on the
   written text of every fixture and on the `--json` document). The editor's new scope works: a
   `ProposedGrant` carrying `tables` writes the smallest `select`/`insert`/`update`/`delete` directive
   inside the view's `postgres [store] { }` block, a wider column list **rewrites** that directive rather
   than adding a second one, a `functions` list is written and merged, comments above a directive stay
   where the author put them, a repeated proposal adds nothing, an unwritable name and an empty column
   list are refused before anything is spliced, and the design's §14.5 row names exactly that field, that
   merge rule and those refusals — nothing more and nothing less (pinned by `edit_branches.rs`).
5. `make test-critical` (zero skips) and `make check` exit 0; `critical_guard.py --base 537eb2d --node
   MB.21.4.3` reports all ten protected changes authorized.
6. The §13.2 warnings, §10.2's production sentence and §11.3's nouns are re-spelled exactly as the
   design's tables say, with triggers, order and counts unchanged. Each new sentence is asserted
   **verbatim** — `crates/blastit-manifest/tests/validate.rs` and `mutation_gaps.rs` for the five
   §13.2 sentences plus the sixth `EntrypointNotReached`, `crates/blastit/tests/critical_raw_ingress.rs`
   for §10.2's, `blastfile_traits.rs`/`critical_blastfile_traits.rs` for `UnrestrictedGroup` and
   `Location::Group` — not merely that a warning fired. The `grep` prints nothing: it asks only for the
   retired fragments (`group … attached to no route`, `no grant in any route or group`, `may run any
   statement on proxy`, `body = "raw"`, `egress = true`). The shared tails `passes the request body
   through unvalidated` and `opens any outbound address and port` are **not** re-spelled by the design and
   must still match with their new prefixes; no command asserts their absence.
7. §18 holds: one line per problem from `ManifestErrors` (no `manifest has N error(s)` header, no `  - `
   prefix, no trailing newline, `""` for none), `Location::View` names the view, neither `InvalidApp` nor
   `PhpEntryInvalid` echoes its value (assert on a `$TOKEN`-shaped and a DSN-shaped value), and `cli.rs`
   reads `ManifestError::position()` instead of splitting the rendered string — with
   `blastfile_errors.rs`, `parse.rs` and protected `critical_php_manifest.rs` asserting the new rendering.
8. One editor entry point: the second `grep` prints nothing outside `edit.rs`'s own definition, `check
   --learn` and `dev --learn` both write through `blastfile::apply_edits` + one `write_atomic`, and the
   patcher's `EditError` mappings still assert. `e2e/php2shell.py::check_learn`'s expectations are updated
   for the Blastfile fixture it will run against; `make e2e` is `MB.21.4.4`'s gate, not this leaf's.

# Notes

- **The Python half is `MB.21.4.7`.** `python/blastit/{fold,pytest_plugin}.py`'s readers, `_blastit.audit`'s
  parse and the four Python suites' fixtures are that leaf's; `e2e/php2shell.py::check_learn`'s fixture
  translation is `MB.21.4.4`'s. Attempt 2 landed every Rust item above and left the tree green, so this
  node is narrowed to what it landed — the split is one session's worth of Python, not a design defect.
- **The landed tree is the acceptance evidence:** `make check` exit 0, both greps silent, `make
  test-critical` zero skips, and `critical_guard.py --base 537eb2d --node MB.21.4.3` authorizing all ten
  protected changes. A re-run proves the same, and `RESULT.md` lists every changed assertion with why it is
  not weaker.
- `python/tests/critical/test_check_contract.py` is protected: it is in `paths` for the `fix.to` locators
  and the fixture format only; do not relax a schema assertion.
- **The design row and the code must agree.** §14.5's `ProposedGrant` line is edited in the same commit as
  the struct, and describes exactly what the code does (field, merge rule, refusal). The page also
  corrects §13.2's "every sentence lives in `crates/blastit-manifest/src/validate.rs`": the six sentences
  are `error.rs`'s `thiserror` attributes, which is why `error.rs` — not `validate.rs` — is the file the
  sweep rewrites.
- **The globs are wider than this leaf.** `crates/blastit-manifest/tests/**`, `crates/blastit-py/**`,
  `python/blastit/**` and `python/tests/**` are in `paths` because the asserting suites, the PyO3 surface,
  the plugin's help text and the sentence pinned in `test_pytest_plugin.py` sit inside them; they also
  cover files that are **not** this leaf's — `tests/docs_fences.rs` (`MB.21.5.1`),
  `crates/blastit-py/tests/critical_check_schema.rs`, `python/tests/{project,packaged_project,
  live_project}/**`, `python/tests/runnable_docs/**` (`MB.21.4.4`), and
  `python/blastit/{fold,pytest_plugin}.py`'s readers (`MB.21.4.7`). A glob authorises; it does not assign.
- **Protected files inside `paths` are edited, never weakened**, and every changed assertion is listed in
  `RESULT.md` with the reason it is not weaker: `critical_blastfile_parity.rs:1132-1138` (an exhaustive
  `ManifestWarning` `match` — it compiles only with the new `EntrypointNotReached` arm),
  `critical_php_manifest.rs:349-352` (`PhpEntryInvalid`'s echo becomes the position+shape assertion),
  `critical_blastfile_traits.rs:61` (the `view` noun), `critical_blastfile_errors.rs` (the §18 shape),
  `critical_manifest_edit*.rs` (constructors only), `critical_check_engine.rs` (locators), and
  `test_check_contract.py` (`add_query`'s locator).
- `crates/blastit/src/stage0.rs` is in `paths` for §10.2's sentence and its own unit test (`:6339-6342`)
  only; its retired-format fixtures and reader call are `MB.21.4.5`'s.
- Keep the human-readable finding text stable where a docs page quotes it (`docs/ci.md` is rewritten by
  `MB.21.5.1`, so say in `RESULT.md` which sentences moved; `docs/reference/manifest.md:118,318` quotes
  the §13.2 sentences and is not in `paths` — record it as a follow-up, do not edit it).
- The re-spellings are renames, not redesigns: a warning that fires twice must still fire twice, and a
  test that only checks "a warning exists" is not proof of a sentence — assert the string.
- `crates/blastit-manifest/src/error.rs` is in `paths` (the five warning strings and the two echoes live
  there), which is why `MB.21.4.5`'s erasure sweep must leave the new nouns alone.
