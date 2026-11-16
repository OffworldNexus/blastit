---
id: MB.21.4.3
title: "Map check, dev --learn, fold and --json fix actions onto Blastfile nodes, re-spell §13.2/§10.2/§11.3 and close §18"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.2]
paths:
  - crates/blastit/src/check.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/stage1.rs
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/edit.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/src/model.rs"
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit-manifest/tests/validate.rs"
  - "crates/blastit-manifest/tests/mutation_gaps.rs"
  - "crates/blastit-manifest/tests/edit_branches.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit.rs"
  - "crates/blastit-manifest/tests/critical_manifest_edit_executor.rs"
  - crates/blastit/tests/critical_check_engine.rs
  - crates/blastit/tests/critical_learn_executor_blocks.rs
  - crates/blastit/tests/critical_raw_ingress.rs
  - crates/blastit/tests/audit_group_hint.rs
  - python/tests/critical/test_check_contract.py
  - "crates/blastit-py/src/lib.rs"
  - "crates/blastit-py/src/audit.rs"
  - "crates/blastit-py/src/check.rs"
  - python/blastit/fold.py
  - python/blastit/pytest_plugin.py
  - python/blastit/__init__.py
  - python/tests/test_check.py
  - python/tests/test_fold.py
  - e2e/php2shell.py
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T08:04"
---

# Goal

Make `blastit check`, `blastit dev --learn`, `blastit-manifest fold` and the `--json` contract write
and read `Blastfile`: `fix.action` keeps the accepted vocabulary (`add_query`, `add_group`,
`remove_query`, `declare_ingress`, `fix_ingress`, `add_route`, `raw_ingress`) and, where `fix.to`
appears at all, it is a Blastfile **node locator** (`view restaurants postgres db`, `trait connection`,
`view upload body`) — never a directive to paste. The surgical editor from `MB.21.3.5` is what applies
the edits. The sentences that §13.2/§10.2/§11.3 re-spell, the §18 error-contract gaps and the editor's
one public entry point also land here, because every one of them is pinned by a suite outside the 3.x
leaves that wrote the strings.

# Context

- Design §13.3/§14 and `ADR-0003`/`ADR-0004` (the `--json` v1 schema and the literal-redaction rules are
  **unchanged**; only the `to`/`fix` strings that name a node change).
- Code sites: `crates/blastit/src/{check,dev_learn,audit_v1}.rs`, the `learn_line` helper
  (`check.rs:1364-1379`), `read_manifest` (`check.rs:294-300`), and the executor glue that calls the
  manifest editor. `dev --learn` writes grants; `check --learn` proposes new views only — keep that
  asymmetry exactly (`MB.1.6`, `docs/VISION.md`).
- The Python package is this leaf's (`MB.21.4.4` keeps to fixtures and the Makefile): `fold.py` keeps
  its required `--manifest PATH` (now a Blastfile, `ADR-0002`), re-pointed at the new PyO3 binding
  `blastfile_apply_edits(source, edits_json) -> str` over `apply_edits`, so the one editor stays the
  one editor; `pytest_plugin.py`'s default and help text name `Blastfile`; `python/tests/{test_check,
  test_fold}.py` assert the new defaults.
- Rename the `manifest_toml` parameters to `manifest_text` and fix their docstrings:
  `crates/blastit-py/src/lib.rs:156,179,302,326-333` (a positional PyO3 signature — keep it positional
  so `python/blastit`'s call sites keep working), `crates/blastit-py/src/audit.rs:412-414`,
  `crates/blastit-py/src/check.rs:25,39`, `crates/blastit/src/audit_v1.rs:545,1447`. That is why
  `crates/blastit-py/src/check.rs` is in `paths` in addition to §21's `{lib,audit}.rs`.
- Mapping rules to implement and pin:
  * `missing_grant` → `add_query`, `to` is `view restaurants postgres db` (a named store appends its
    name: `view catalog postgres analytics`), and the editor inserts `query 'SELECT …'` inside that
    store's grant block, creating the block when absent;
  * connection-setup SQL → `add_group` with `to` = `trait connection` (the `MB.1.6` group hint keeps its
    group name; the node it writes is a `trait` plus a `use` line on every view);
  * `unrouted` → `add_route` with no `to` — `fix` carries `method`/`path`, and the view is inserted
    above the method-unrestricted catch-all `view`;
  * a body seen on a view without a codec → `declare_ingress`, whose `fix` is `fix_ingress` and whose
    `to` is `view upload body`;
  * `raw` is never written, `unvalidated` never appears in a `fix`, and a statement inlining a literal
    is still refused rather than written.
- Tests: `crates/blastit/tests/critical_check_engine.rs` and
  `crates/blastit/tests/critical_learn_executor_blocks.rs` (rewrite fixtures to Blastfile; assertions
  stay or get stronger), `python/tests/critical/test_check_contract.py` (the `--json` document shape),
  and `crates/blastit/tests/audit_group_hint.rs` (now a trait hint).
- **Sentence re-spellings pinned outside every 3.x leaf** (`MB.21.3.7`/`MB.21.3.8` could not do them —
  the asserting suites were not theirs). Each keeps its trigger, its order and its count; only the
  nouns change:
  * §13.2's six boot warnings, all in `crates/blastit-manifest/src/error.rs`, still spelled for the
    retired file today: `UnattachedGroup` "group {} is attached to no route and no global groups",
    `UngrantedProxy` "proxy {} has no grant in any route or group", `UnrestrictedRoute` "route {} may
    run any statement on proxy {}", `RawIngress` "route {}: body = \"raw\" passes the request body
    through unvalidated", `EgressRoute` "route {}: egress = true opens any outbound address and port"
    → `trait`/`view`/`store` and `body raw`/`egress any`; `EntrypointNotReached` keeps its §12.2
    wording. Pinned by `crates/blastit-manifest/tests/{validate,mutation_gaps}.rs`;
  * §10.2's production `raw_ingress_refusals` sentence → ``view "<name>": body raw hands the request
    body to the application unvalidated, and the production path refuses it without the
    acknowledgement; write `body raw unvalidated` in that view, or declare the codec it takes (json,
    form, none)`` — same function, same call sites, same order. Pinned by
    `crates/blastit/tests/critical_raw_ingress.rs` (lines around 651/678);
  * §11.3's `group` → `trait` noun anywhere the sweep above does not reach.
- **§18's error-contract gaps:** `ManifestErrors`' `Display` joins one line per problem (no trailing
  newline, no double prefix), `Location::View` stops printing "(index 0)" (it names the view), and
  `InvalidApp`/`PhpEntryInvalid` stop echoing their value — they name the position and the expected
  shape (`module:attribute`, a relative `.php`), because a pasted DSN or token can land in either slot.
  Add `ManifestError::position()` so `cli.rs`'s `report_manifest_errors` reads the position instead of
  splitting the rendered string, and keep `error::clipped`/`escape_control` as the only folding code
  (`MB.21.4.1` deletes `cli.rs`'s copy).
- **One editor entry point:** `blastfile::apply_edits` over `BlastfileEdits` is *the* editor; the
  retired TOML patcher's `blastit_manifest::edit::apply_edits` stops being a second public path.
  Re-point `check.rs:979` (`check --learn`) and `dev_learn.rs:459` (`dev --learn`) at the Blastfile
  entry point, keep one `write_atomic`, and keep the patcher's own `EditError` mapping
  (`LiteralQuery`, `RedactedQuery`, `Verification`, `Source`) compiling and asserted until
  `MB.21.4.5` deletes its reader — its sentences are pinned by `edit_branches.rs` and
  `critical_manifest_edit*.rs`.
- **`dev --learn` end to end:** `dev_learn.rs` builds `BlastfileEdits` for each recorded finding and
  performs one atomic write; the live `blastit dev --learn` case in `e2e/php2shell.py::check_learn` is
  re-pointed at the Blastfile fixture (4.4 translates the fixtures, this leaf owns the case's
  expectations: the file survives, comments survive, a second run adds nothing).

# Acceptance

```console
$ make test-critical
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/critical/test_check_contract.py -q
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.3
$ cargo test -p blastit-manifest --locked --test validate --test mutation_gaps
$ grep -rnE 'attached to no route|no grant in any route or group|may run any statement on proxy|passes the request body through unvalidated|opens any outbound address' crates/blastit-manifest/src/error.rs crates/blastit/tests/
$ grep -rn 'blastit_manifest::edit::apply_edits\|edit::apply_edits' crates/ --include='*.rs'
$ make check
```

1. `dev --learn` on a fixture adds exactly one `query` line inside the right store block, keeps every
   comment and blank line, and a second run adds nothing.
2. `check --learn` still proposes views only; `--json` bytes match the documented v1 schema with the
   page's action names (`add_query`/`add_group`/`declare_ingress`/`add_route`) and the node locators in
   `fix.to`, and the literal-refusal case still refuses.
3. The connection-setup finding is `add_group` with `to` = `trait connection`, and the text it writes is
   a `trait` block plus a `use` line on every view (assert the exact strings).
4. `raw` is never emitted by any learn path, and `unvalidated` appears in no `fix` object (assert on the
   written text of every fixture and on the `--json` document).
5. `make test-critical` (zero skips) and `make check` exit 0; `critical_guard.py` clean.
6. The §13.2 warnings, §10.2's production sentence and §11.3's nouns are re-spelled exactly as the
   design's tables say; the first `grep` prints nothing, `validate.rs`/`mutation_gaps.rs` and
   `critical_raw_ingress.rs` assert the new sentences (not merely that a warning happened), and the
   triggers, order and counts are unchanged.
7. §18 holds: one line per problem from `ManifestErrors`, `Location::View` names the view, neither
   `InvalidApp` nor `PhpEntryInvalid` echoes its value (assert on a `$TOKEN`-shaped and a DSN-shaped
   value), and `cli.rs` reads `ManifestError::position()` instead of splitting the rendered string.
8. One editor entry point: the second `grep` prints nothing outside `edit.rs`'s own definition, `check
   --learn` and `dev --learn` both write through `blastfile::apply_edits` + one `write_atomic`, the
   patcher's `EditError` mappings still assert, and `e2e/php2shell.py::check_learn` passes.

# Notes

- `python/tests/critical/test_check_contract.py` is protected: it is in `paths` for the fixture format
  change only; do not relax a schema assertion.
- Keep the human-readable finding text stable where a docs page quotes it (`docs/ci.md` is rewritten by
  `MB.21.5.1`, so say in `RESULT.md` which sentences moved).
- The re-spellings are renames, not redesigns: a warning that fires twice must still fire twice, and a
  test that only checks "a warning exists" is not proof of a sentence — assert the string.
- `e2e/php2shell.py` is in `paths` for the `--learn` case only; `MB.21.4.4` owns every other fixture in
  that tree and runs after this leaf, so nothing is rewritten twice.
- `crates/blastit-manifest/src/error.rs` is in `paths` (the six warning strings and the two echoes live
  there), which is why `MB.21.4.5`'s erasure sweep must leave the new nouns alone.
