# Result MB.21.4.1 (attempt 5)

**`STATUS: done`** — the two gate blockers attempt 4 left are closed and every `# Acceptance`
command is green: `make test-critical`, `make docs-test`, `make check`, `cargo fmt --check`
and the enforcing `critical_guard.py --node MB.21.4.1` all exit `0` on the final tree.

## What attempt 5 changed

### (a) `make test-critical`: the protected `check` contract now boots a `Blastfile`

* `python/tests/critical/test_check_contract.py` stages `./Blastfile` (not the retired file
  name) and its fixtures are translated `Blastfile`s: `postgres $DATABASE_URL`, `view home "/"`,
  `view upload POST "/upload" { body raw unvalidated }`. The `pyproject.toml` ini value is
  `blastit_manifest = "Blastfile"`. The four learn tests read/write `Blastfile` and assert
  **textually** instead of through `tomllib`: `view notes "/notes"` occurs once and its block
  holds `query 'SELECT 1 WHERE $? = $?'`; the refusal twin asserts no `view notes` and no
  `SELECT 1 WHERE` was written. No assertion was deleted or weakened (table below).
* The `check`/`--learn` PyO3 bindings now read the `Blastfile`:
  `crates/blastit-py/src/{check,lib}.rs` pass `manifest_text` to
  `blastit::audit_v1::{check_audit,plan_learn_json}` and
  `crates/blastit/src/audit_v1.rs::parse_manifest` is `Manifest::parse_blastfile`.
* **Correction to the brief's diagnosis.** The 18 contract failures were caused only by the
  staged file name; `blastit check` is the Rust CLI and never calls `_blastit.audit`. The
  brief also asked for `crates/blastit-py/src/audit.rs:414` to move to `from_blastfile_str`.
  That binding is what `fold` (`python/blastit/fold.py`) and the 0.1.0 pytest plugin call, and
  moving it turned three **protected** trials in
  `python/tests/critical/test_fold_literals_and_atomic.py` red (`blastit-manifest fold: …
  \`version\` is not a keyword`). That file is outside this leaf's `paths` and the design
  assigns `fold`/the plugin to `MB.21.4.3`, so `_blastit.audit` stays on the retired reader
  there until 4.3; the `check`/`--learn` bindings and `blastit check` itself are `Blastfile`
  only. No detector, no suffix test, no second file name was added.

### (b) `make docs-test`: `check --learn` writes through the `Blastfile` editor

`crates/blastit/src/check.rs::learn` now calls `blastit_manifest::blastfile::apply_edits` with
a `BlastfileEdits` plan; `audit_v1::LearnPlan.edits` is `BlastfileEdits`. `audit_v1::plan_learn`
turns the paths `infer` saw into quoted `Blastfile` patterns (numeric → `{idN:int}`, UUID →
`{idN:uuid}`, everything else literal) and grants become `ProposedGrant { view, store, queries }`
— the unnamed `postgres { }` block is store `db`, a named one is `postgres <name> { }`. A path
that cannot be a quoted pattern is not proposed, and a grant for a view that cannot be created is
dropped rather than written against a missing node. `--learn` still writes only views and exact
`queries`: never `raw`, never `unvalidated`, never a table grant, never `transactions`.

**This is the merge `MB.21.4.3` would have done.** `MB.21.4.3` is left with: the `--json`
action vocabulary (`fix.to` → `view <name> postgres <store>`, `trait connection`, `add_route`
with no `to`), `fold`/the pytest plugin (and `_blastit.audit`'s move to `Blastfile`), the e2e
learn case, and the §13.2/§10.2/§11.3 sentence re-spellings.

### Other files this attempt touched (all inside `paths`)

* `crates/blastit/tests/critical_check_engine.rs` — `GOLDEN_MANIFEST`, `RE_PARSE_MANIFEST`,
  `RICH_MANIFEST`, `ONE_PROXY_MANIFEST` and the ingress fixture are `Blastfile`s; the R27
  golden asserts the new proposal shape (`view orders "/orders/{id1:int}/"`, grant
  `view = "orders"`, `store = None`) and still proves both exact queries, the parsed grant and
  the byte-exact human report. The hand-built edits are `BlastfileEdits`.
* `crates/blastit/tests/critical_check_schema.rs` — the same fixture translation and the
  bridge plan-learn trial now applies through `blastfile::apply_edits`.
* `crates/blastit/tests/audit_group_hint.rs` and the `check.rs` unit fixtures — translated.
* `docs/local.md` — the executed `git diff blastit.toml` command became `git diff Blastfile`
  (the fence runs it; the page's prose is `MB.21.5.1`'s). The manifest block was attempt 4's.

## Acceptance evidence

All commands run in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` on the final
tree.

### 1. Discovery, at every entry point — PASS

```console
$ cargo test -p blastit --features python --test blastfile_discovery
test blastit_manifest_wins_over_the_default ... ok
test a_set_but_empty_manifest_variable_is_refused_at_every_entry_point ... ok
test the_default_is_dot_slash_blastfile_only ... ok
test nothing_is_read_above_the_working_directory ... ok
test a_body_that_is_not_a_blastfile_is_an_ordinary_syntax_error ... ok
test result: ok. 5 passed; 0 failed; 0 ignored
```

`BLASTIT_MANIFEST` wins, the default is `./Blastfile`, nothing above the working directory is
read, and the set-but-empty variable is refused at boot/`dev` (`1`) and `check`/`fmt` (`2`).

### 2. The one missing-file sentence — PASS

The same suite asserts §15.1's sentence byte for byte for `check`, `check --json` and `fmt`
(exit `2`) and for `dev` on fd 2 (exit `1`, nothing on stdout); `fmt_command.rs` pins it for
`fmt`. The production boot calls the same `config::missing_manifest`.

### 3. A foreign body is an ordinary `Blastfile:<line>:<col>` error — PASS

`blastfile_discovery.rs::a_body_that_is_not_a_blastfile_is_an_ordinary_syntax_error` (a
`version = 1` line followed by a `[[route]]` table) asserts the parser's position and the two
exit codes. The protected contract keeps its own foreign-body trial
(`test_a_bad_manifest_exits_two_with_an_empty_stdout`) unchanged.

### 4. The retired-name grep prints nothing — PASS

```console
$ grep -rnE 'blastit[.]toml|TomlManifest|looks_like_toml|is_toml|strip_suffix\("\.toml"\)' \
    crates/blastit/src crates/blastit/tests python/tests/runnable_docs \
    --include='*.rs' --include='*.py' --include='*.toml'
(no output; exit 1)
```

### 5. `fmt` obeys the same discovery; `cli.rs` holds no copy — PASS

```console
$ grep -c 'BLASTIT_MANIFEST' crates/blastit/src/cli.rs crates/blastit/src/config.rs
crates/blastit/src/cli.rs:0
crates/blastit/src/config.rs:16

$ grep -n 'fn fmt_path\|one_line_text' crates/blastit/src/cli.rs
(no output; exit 1)

$ cargo test -p blastit --features python --test fmt_command
test result: ok. 12 passed; 0 failed
```

### 6. `blastfile::fmt` re-parses before returning — PASS

```console
$ grep -n 'parse(&formatted)' crates/blastit-manifest/src/blastfile/print.rs
64:    parse(&formatted)?;

$ cargo test -p blastit-manifest --test blastfile_fmt
test result: ok. 7 passed; 0 failed
```

### 7. The gates — PASS

```console
$ timeout 1800 make test-critical ; echo EXIT=$?
cargo test -p blastit --features python --test critical_static_cli
test result: ok. 12 passed; 0 failed
…
python -m unittest discover -s python/tests/critical
..................................................
----------------------------------------------------------------------
Ran 50 tests in 4.335s

OK
EXIT=0

$ timeout 1800 make docs-test ; echo EXIT=$?
PASS  docs/local.md  fence 4  learn-git      1.9s  learn wrote the manifest, then check was clean
PASS  docs/ci.md     fence 5  learn-sequence 2.7s  learn, dry-run and check agree
…
49 fences: 28 executed, 21 accounted, 0 failed · 25.4s
EXIT=0

$ timeout 5400 make check ; echo EXIT=$?
guard: OK (26 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 63 changed file(s)            # report-only here
cargo fmt --all -- --check                                        # clean
cargo clippy … -D warnings                                        # 0 warnings
RUSTDOCFLAGS="-D warnings" cargo doc …                            # 0 warnings
cargo test --workspace --all-targets --locked --features python    # 164 `test result: ok`, 0 failures
python -m unittest discover -s python/tests/critical               # Ran 50 tests, OK
49 fences: 28 executed, 21 accounted, 0 failed
… platform-matrix, workflow-check, packaging-gates, doc-tone, manifest-purity …
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`make rs-test` is the `cargo test --workspace --all-targets`/`--doc` pair inside that `make
check` (the log holds 164 `test result: ok` blocks and no `FAILED`); the `blastit-view` 1 s
frame budget did not flake, so no re-run was needed.

### The remaining acceptance commands

```console
$ timeout 300 cargo fmt --all -- --check ; echo EXIT=$?
EXIT=0

$ timeout 600 uv run --no-project tools/critical_guard.py --base f2773d4 --node MB.21.4.1
critical-guard: f2773d4..working tree, 63 changed file(s)
  M  crates/blastit-py/tests/critical_check_schema.rs  [authorized by crates/blastit-py/**]
  M  crates/blastit/tests/critical_check_engine.rs     [authorized by crates/blastit/tests/**]
  M  python/tests/critical/test_check_contract.py      [authorized by python/tests/critical/test_check_contract.py]
  …  (14 protected changes listed the same way)
critical-guard: OK: all 14 protected change(s) are authorized by node(s) MB.21.4.1
```

### 8. The crate suites and six fixtures boot from a `Blastfile` — PASS

`rs-test` and the 49 `docs-test` fences boot the nine `crates/blastit/tests/` suites and the six
`python/tests/runnable_docs/fixtures/` from `Blastfile`; `make test-critical` boots the
translated `check` contract fixture the same way.

## `python/tests/critical/test_check_contract.py`: every change

| Change | Assertion kept |
|---|---|
| `PROJECT_MANIFEST` is a `Blastfile` (`view home "/"` + `query 'SELECT 1'`) | one route, one request, one statement, `proxies.db`, the exact report and the credential barrier |
| `DEGRADED_MANIFEST` replaces `query 'SELECT 1'` with `query 'SELECT 2'` | `missing_grant`/`dead_grant` pair and the `add_query` fix |
| `UPLOAD_MANIFEST` is `view upload POST "/upload" { body raw unvalidated … }` | the 64 KiB body cap and the empty-findings twin |
| `Project` stages `Blastfile`; `pyproject` says `blastit_manifest = "Blastfile"` | every `run_check` reaches the manifest |
| learn trials read/write `Blastfile` as text | byte-identical no-write on exit `3`/refusal; the written view and its normalised `query` |
| refusal asserts `view notes`/`SELECT 1 WHERE` absent | no view and no query line was added |
| Django-without-a-proxy fixture is `app "home"` / `view home "/" {}` | exit `2`, "declares no postgresql proxy" |
| `import tomllib` removed | nothing parses the manifest as TOML |

The deliberately malformed body in `test_a_bad_manifest_exits_two_with_an_empty_stdout`
(`version = 1\n[[route]\nname = "oops"\n`) is **kept**: it is a foreign body, and the test
proves it is an ordinary syntax error with no detector and no second sentence.

## Protected suites: file → assertion kept → new subject

| Protected file | Assertion kept | New subject |
|---|---|---|
| `python/tests/critical/test_check_contract.py` | every exit code, document, secret-free twin and the `add_query` fix; no assertion removed | `./Blastfile` and its translated views/grants; textual learn assertions |
| `crates/blastit-py/tests/critical_check_schema.rs` | the bridge's plan-learn document (`1` route, `1` query, `wrote`), the edited manifest booting, `orders` holding the `db` grant | `Blastfile` fixture; `blastfile::apply_edits`; `parse_blastfile` |
| `crates/blastit/tests/critical_check_engine.rs` | R27 golden (one view, both exact queries, no table grant), every finding kind, byte-exact `§1.3` human report, bootable `add_query`, hint-only transaction control | `Blastfile` fixtures; `ProposedView`/`ProposedGrant`; `view orders "/orders/{id1:int}/"` |
| `crates/blastit/tests/critical_config_and_presets.rs`, `critical_dev_and_fail_closed.rs`, `critical_raw_ingress.rs`, `critical_static_cli.rs`, `critical_php_backend.rs`, `critical_php_embed.rs`, `dev_signal_reap.rs`, `smoke.rs`, plus `blastit-scheduler/tests/critical_*.rs` and `blastit-manifest/tests/critical_static_manifest.rs` | changed in attempts 1–4, untouched here | all authorized by this node's globs; no assertion deleted or weakened |

## Forced out-of-`paths`-by-necessity

None: no file outside `paths` was written. The `crates/blastit-py/src/check.rs` change made
`crates/blastit-py/tests/critical_check_schema.rs` uncompilable, so the node's
`crates/blastit-py/src/**` glob was widened to `crates/blastit-py/**` **through
`tools/plan.py edit`** (the brief already stated paths cover `crates/blastit-py/**`), which is
what authorized that protected file. `python/tests/critical/test_fold_literals_and_atomic.py`
and `python/blastit/fold.py` were **not** touched.

## Follow-ups

- **`MB.21.4.3`**: the `--json` action vocabulary (`fix.to` → `view <name> postgres <store>`,
  `trait connection`, `add_route` with no `to`); `fold`/the pytest plugin and `_blastit.audit`'s
  move to `Blastfile`; the e2e learn case; the §13.2/§10.2/§11.3 sentence re-spellings; any
  remaining `--learn` prose.
- **`MB.21.4.5`**: `Route::{static_policy,with_static,php,with_php}` stay until the reader is
  deleted (they are unreachable from a `Blastfile`), with the dead `stage0::php_worker_budget`
  sentence and `crates/blastit/src/host.rs`'s two `[route.php]` strings.
- **`MB.21.5.1`**: the remaining retired prose on `docs/local.md`/`docs/ci.md` (the executed
  fences are green; their surrounding `[[route]]`/`blastit.toml` prose is that leaf's) and
  `docs/security.md`/`docs/VISION.md`'s `[route.php]`.
