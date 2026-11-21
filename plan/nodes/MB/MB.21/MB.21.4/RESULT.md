# Result MB.21.4

## Summary

The product reads one format at every entry point: boot, `dev`, `check`, `fmt`, the PyO3 binding,
`fold`, the pytest plugin and the Makefile's docs-serve stage. Discovery is one function
(`BLASTIT_MANIFEST` else `./Blastfile`, no upward search); a missing file says `missing ./Blastfile`
and a foreign body is an ordinary `Blastfile:<line>:<col>` syntax error, so no detector, sniffer or
second file name exists anywhere in the running tree.

Runtime and tooling follow the same reader: the scheduler serves `static` mounts and binds one
file-level `entrypoint php` to every view; `check`, `dev --learn` and `--json` name Blastfile node
locators (`view <name> postgres[ <store>]`, `trait <name>`, a view's `body` node), and the report's
table scope is pasteable directives under `table_grants.blastfile`. `--learn` writes only through the
lossless editor, never `raw`, `unvalidated`, a table grant or `transactions`.

The retired reader, its patcher, ten reader-only `ManifestError` variants and the `toml` dependency
are gone. The retired words survive only where they are records: `LOG.md`/`RESULT.md`, finished
briefs, ADRs and the `MB.21.5.2` guard — which is the point.

Ten children are `done`; four of them also fixed product defects the cutover exposed (the
`AllowEmptyOnly` `413`-vs-`200` race, the dev test client's response pairing, the budgets a generated
file needs, and the last user-facing sentences).

## Evidence

Each `# Acceptance` item, on the final tree (`335f564`).

**1. Every child is `done`.**

```console
$ uv run --no-project tools/plan.py tree --under MB.21.4 --depth 1 --all
○ MB.21.4 [epic/ready] …
  ● MB.21.4.1 [task/done]  ● MB.21.4.2 [task/done]  ● MB.21.4.3 [task/done]  ● MB.21.4.4 [task/done]
  ● MB.21.4.5 [epic/done]  ● MB.21.4.6 [task/done]  ● MB.21.4.7 [task/done]  ● MB.21.4.8 [task/done]
  ● MB.21.4.9 [task/done]  ● MB.21.4.10 [task/done]
```

`MB.21.4.5`'s six sub-leaves (`5.1`–`5.6`) are `done`. Commits: `a3ac338` 4.1 · `537eb2d` 4.2 ·
`fa6dc39` 4.3 · `c228390` 4.4 · `201c7c2`…`f390bd2` + `fa7f2ef` gate 4.5 · `1cfdace` 4.6 · `621626c`
4.7 · `eeccd9f` 4.8 · `2ef786c` 4.9 · `335f564` 4.10.

**2. `make check` and `make e2e` exit 0.**

```console
$ make check        → exit 0, 166 × "test result: ok", plan OK (guard · rs-fmt · rs-lint · rs-doc ·
                      test-critical · py-lint · docs-unit · docs-check · docs-test · rs-no-python ·
                      manifest-purity · plan validate)
$ make e2e          → exit 0, 0 FAIL lines, 411 ok lines, 1116 log lines
```

`make e2e` was red from `a3ac338` until `c228390` by design; no leaf weakened a gate to cover it.

**3. No retired file name as a path, default, fixture, script line or Makefile stage.**

```console
$ grep -rnE 'blastit[.]toml' crates/ python/ e2e/ Makefile scripts/ \
    --include='*.rs' --include='*.py' --include='Makefile' --include='*.sh'
(no output)

$ find e2e python/tests \( -name 'Blastfile' -o -name '*.toml' \) | sort
15 × Blastfile · python/tests/runnable_docs/inventory.toml · e2e/php-sapi/seccomp-exec/Cargo.toml
```

The two `.toml` files are not manifests: `inventory.toml` is the runnable-docs inventory and the other
is a fixture crate's own `Cargo.toml`.

**4. The reader, the patcher and the dependency are gone.**

```console
$ cargo tree -p blastit-manifest --locked --edges normal | grep -ci toml      → 0
$ git grep -n '^mod parse;' crates/                                          → (empty)
$ ls crates/blastit-manifest/src/parse.rs                                    → No such file or directory
$ git diff --stat 2ef786c -- Cargo.toml Cargo.lock
 Cargo.lock | 55 -------------------------------------------------------
```

`Manifest::from_toml_str`, `parse_and_validate`, the retired `edit.rs` patcher and the ten
reader-only variants are deleted; `stage1`'s TOML synthesis is `RoutingTable::for_learn`. The
workspace `Cargo.toml` was never touched (it never carried `toml`), and only
`toml`/`toml_datetime`/`toml_parser`/`toml_writer`/`serde_spanned`/`winnow` left the lock.

**5. Parity is proved and the protected suites were not weakened.**

```console
$ cargo test -p blastit-scheduler --test critical_blastfile_parity      10 rows, all booted
$ cargo test -p blastit-manifest  --test critical_blastfile_parity      4 grant rows
$ uv run --no-project tools/critical_guard.py --base <leaf-base> --node <leaf>
critical-guard: OK: all N protected change(s) are authorized by node(s) <leaf>
```

`critical_guard.py` is clean at every leaf (14, 2, 10, …, 9 for the `4.5` epic, 2 for `4.10`
authorized as its reviewer recorded). Covered rows include a static mount's four-step precedence,
`body json|form|none|raw[ unvalidated]`, `prefork`, `egress any`, a trait's grants, `entrypoint php`
and `HEAD` from `GET`, each with its deny side. The mutation bar on the erased modules:
validate+error 77.2 %, routing 77.3 %, lower 93.0 % (`docs/testing/AUDIT-MB-21-4-5.md`).

## Files

No path list: the change is a crate-wide cutover.

- `crates/blastit-manifest` — the `blastfile/` module is the only reader; `parse.rs` deleted;
  `model`, `validate`, `error`, `routing`, `edit`, `blastfile/lower` carry the re-spellings; six
  budgets raised (16 MiB source, 4 194 304 tokens/trivia, 4 096-byte patterns, 4 096 list items,
  256-char app name).
- `crates/blastit` — `config` (one discovery function, `display_path`, `read`, `manifest_text`),
  `stage0` (missing-file sentence, `entrypoint php` binding, raw gate), `cli` (`--manifest`
  refusal reworded), `check`, `dev_learn`, `audit_v1` (node locators, §13.2 warnings, one-line
  errors), `host`, `infer_routes`, `php_view`.
- `crates/blastit-scheduler` — mounts served and the head-time `413` / `REQ_END` dispatch rule
  (4.9), `Source::Toml` deleted, fixtures translated.
- `crates/blastit-py` + `python/blastit` — `fold` and the pytest plugin read via
  `blastfile_model` and write via `blastfile_apply_edits`; `audit()` off the one reader; hints and
  help text in Blastfile.
- `crates/blastit-sqlpolicy` (`tables`, `learn`), `crates/blastit-view` (`php.rs` sentences),
  `crates/blastit-env`, `crates/blastit-http` and `crates/blastit-dbproxy` follow the model.
- `e2e/`, `scripts/`, `Makefile` and the `Dockerfile`s/compose files: the fifteen `Blastfile`
  fixtures (six under `e2e/`, nine under `python/tests/`), one `BLASTFILE` heredoc per script,
  `DOCS_SERVE_MANIFEST := …/Blastfile`.
- `docs/`: `docs/local.md`'s executed block and `git diff Blastfile` command (4.1); `ADR-0012`
  decision 16 (`table_grants.blastfile`); `docs/knowledge/LESSONS.md`.
- Protected suites rewritten with every assertion kept: the six boot/learn `critical_*` files, the
  three manifest suites, `critical_blastfile_parity` (both crates),
  `critical_policy_surface`, `critical_check_engine`, `critical_ingress_enforcement`, and the Python
  `python/tests/critical/**`.

## Follow-ups

### (a) Hand-offs to `MB.21.5.1` (docs prose and the report's page)

1. The eight retired pages still open with the retired format: no page block moved in this epic
   except `docs/local.md`'s executed one.
2. `table_grants.blastfile` is undocumented. `docs/reference/blastfile.md` never names
   `table_grants` (measured: zero hits), so the report's new key has no reference row, and the
   renderer's advice for a `SELECT count(*)` has no page either.
3. `docs/ci.md`'s `--json` example still shows the old fix payload.
4. `docs/local.md`: the block is migrated and its `git diff Blastfile` fence runs, but the prose
   around it is `MB.21.5.1`'s.
5. Reference-page sentence for an empty column list: a table grant takes at least one column, so the
   renderer prints advice and no directive rather than widening to `select t`.

### (b) Hand-offs to `MB.21.5.2` (the permanent guard)

1. `docs_fences`' strict flip: `THE_RETIRED_OPENING_IS_GONE` is still `false`
   (`crates/blastit-manifest/tests/docs_fences.rs:64`), so the eight retired pages are counted and
   named, not parsed as live fences. `4.5.3` narrowed this deliberately and recorded it.
2. The exhaustive `docs/**` scan after the pages are rewritten.
3. The retired-name guard itself (`test_the_real_tree_carries_no_retired_toml_manifest`), including
   its negative self-tests.

### (c) Hand-offs to `MB.21.5.3` (mutation audit)

1. `grant_locator`'s named-store header is executed by `4.10`'s tests but never asserted, so a mutant
   that drops the store from `view <name> postgres <store>` would survive.
2. The table-scope hint tests run in CI's pytest job, not a `make` target: `test_pytest_plugin.py` is
   `skipif(not DATABASE_URL)` and skips on a host without PostgreSQL. The PostgreSQL-free half lives
   in `test_fold.py::TestTheTableScopeHint`.
3. The ~99 comment-only mentions of the retired name recorded by `4.10` in `crates/**/*.rs` (plus
   `check.rs`'s unreachable "no input variables" sentence guard and `pytest_plugin.py:1189`'s
   comment). Harmless: none is rendered, and they are records.

### (d) Out of scope, recorded for a reader

1. `blastit-view`'s 1 s frame budget (`php.rs::wait_for`) flakes under load. Pre-existing, untouched
   by every child, and it needs its own leaf in the milestone that owns that crate.
2. Two cases in `crates/blastit/tests/critical_dev_and_fail_closed.rs` are host-memory conditioned:
   above `BLASTIT_MEM_HIGH_WATERMARK` `blastit dev` correctly refuses, and the same case fails
   identically at the leaf's base commit. Environment, not product.
3. `crates/blastit/tests/smoke.rs:315` keeps its own read-to-EOF-and-parse-a-prefix client (the
   `4.8` defect class) and wants the same reader when a leaf owns that file.
4. `Proxy::output` and the per-route `static`/`php` accessors remain unreachable from a `Blastfile`;
   `docs_fences`' retired-page predicate still reads the pair, so removing them waits on `5.1`.
