# AUDIT MA.3 — runnable-docs guard, critical tiers, scoped mutants

- **Date:** 2026-11-13
- **Scope:** the `MA.3` verification tier — `python/tests/runnable_docs/**` (the docs-protection
  gate), `crates/blastit-manifest/tests/docs_fences.rs`, the protected critical tiers
  (`crates/*/tests/critical_*.rs`, `python/tests/critical/test_*.py`), their CI wiring, and a
  scoped mutation audit of `blastit-manifest` + `blastit-sqlpolicy`.
- **Base:** `dba348c` (MA.3.1 harness, MA.3.2 docs, MA.3.4 e2e evidence, MA.3.7 product fixes).
- **Requirements:** `docs/design/ma-lean-dx.md` §7.1 (execution tiers, harness isolation, offline
  recipe, `R42` zero-skip, docs-fence suite, **mutation gate ≥ 75 %**), §7.2 rows for `R7..R8`,
  `R11`, `R24..R25` (manifest + sqlpolicy), `R4..R5`, `R29..R30`, `R39..R42`; `ADR-0003`,
  `ADR-0004`; skill `testing` §1–§4 and skill `dx-first` §4 ("every snippet is tested").
- **Not re-audited (closed):** R14..R16 ingress, R12/R13/R16 abort, R26/R31 dbproxy, R27..R38
  audit engine — see `AUDIT-MA.2.1.md`, `AUDIT-MA.2.2.md`, `AUDIT-MA.2.3.md`. Docker e2e is
  `MA.3.4`'s evidence.

## Summary

**Verdict: BLOCKED at the milestone gate — the gate runs, but it is not wired to CI and it cannot
account for every snippet shape.**

Three things hold it:

1. **Five documented snippet shapes are invisible to the harness, and the inventory cannot see
   them** (`F1`). The gate counts *recognised* `console` fences and compares that count with
   the inventory, so a fence written as `~~~console`, four backticks, an indented block inside a
   list, an info-string suffix (`console title="…"`), or a `bash` block with a `$ ` prompt is neither executed nor
   demanded in `inventory.toml`. Reproduced with the real `check_inventory` on a copy of the public
   tree: every one is a silent pass. A page that zensical publishes but the nav does not name is in
   the same class (it really is published: `site/orphan/index.html` under `--strict`).
2. **`make docs-test`, `critical_guard` and `critical-skip-check` run nowhere in CI** (`F6`). No
   workflow invokes `make`; `docs-test` is local-only, and `R42`'s skip ban and the critical-test
   guard are enforced only by whoever remembers to run `make check`. The Rust and Python critical
   *tests* do run in CI (`cargo test --workspace --all-targets`, `pytest python/tests`), so this is
   a wiring gap, not a missing suite.
3. **The sqlpolicy policy oracle is unprotected** (`F8`). 11 anchored mutants in
   `check_learnable` / `is_introspection_setup_sql` / `Tables` / `redact_literals`: 10 killed, 1
   survivor, **but the protected `tests/critical_literals.rs` killed none of them** — every kill
   came from a `#[cfg(test)]` unit test in `src/`. An implementer can delete `COPY … PROGRAM`'s
   check and its only oracle in one edit and `tools/critical_guard.py` stays green.

What holds well: `blastit-manifest` killed **8 of 8** anchored policy mutants, all by the protected
suites; the harness genuinely runs what it reads (a wrong expected line, a wrong exit code and a
"zero observations" clean line all go red), the wheel actually under test is compared file-by-file
with `dist/`, and no protected test contains a skip construct.

`cargo-mutants` is **not installed** on this host (`error: no such command: mutants`; the owner was
already asked in `plan/HUMAN.md` H-002). The numbers below are an anchored mutant audit, not a
`cargo-mutants` run, and are labelled as such. The missing tool stays a blocking prerequisite for
the milestone gate's literal reading of §7.1.

## Method

1. Read the harness (`fences.py`, `harness.py`, `run.py`, `pgstub.py` — 2 677 lines, **zero
   `test_` functions**), the inventory (21 pages, 38 fences), `docs_fences.rs`, the eleven protected
   Rust suites and the three Python critical suites.
2. Attacked the harness with 15 probes against the real code: `check_inventory` / `public_pages` on
   a `/tmp` copy of the public tree plus the real `inventory.toml`, and the real `Harness.expect` /
   `mode_*` functions with a stub shell. No file in the repository was edited; `/tmp` copies only
   (`/tmp/opencode/ma3_escape.py`, `ma3_real.py`, `ma3_c.py`, `ma3_c2.py`).
3. Mapped every tier to the job that runs it: the `make check` targets and every job in
   `.github/workflows/*.yml` (`grep -rn "make \|cargo test\|pytest " .github/workflows`).
4. Attempted the required mutation runs, then fell back to an anchored mutant audit in
   `/tmp/opencode/blastit-ma3-mut` (rsync of the workspace without `target/`, `.git/`, `dist/`; a
   separate `CARGO_TARGET_DIR`). Each mutant is one exact, unique string replacement; the crate's
   whole test suite runs (`cargo test -p <crate> --locked`), failing test names are recorded, and
   the file is restored. A mutant that does not compile is recorded as *unviable* and excluded from
   the kill rate, exactly as `cargo-mutants` does.

## Inventory & classification

| Path | Count | Class | Note |
|---|---|---|---|
| `python/tests/runnable_docs/{fences,harness,pgstub,run}.py` | 2 677 lines, **0 tests** | gate, **unprotected by tests** | the gate has no oracle of its own; every escape below was found by reading |
| `python/tests/runnable_docs/inventory.toml` | 21 pages / 38 fences | gate data | 26 executed, 12 accounted (7 `static`, 2 `example`, 2 `install-external`, 1 `pgstub`) |
| `crates/blastit-manifest/tests/docs_fences.rs` | 6 manifests + 1 negative canary | useful (standard) | §7.1 places it in standard on purpose so `docs/**` edits do not trip `critical_guard` |
| `crates/*/tests/critical_*.rs` (11 files) | 190 tests (168 `#[test]` + 22 `#[tokio::test]`) | critical | every file guarded by `tools/critical_guard.py`, no skip construct |
| `python/tests/critical/test_*.py` (3 files) | 50 tests | critical | `unittest.TestCase`; pytest collects them, `make test-critical` runs them via `unittest discover` |
| `python/tests/test_*.py` (8 files) | standard | useful | the Python standard tier; only CI's `pytest-plugin` job runs it with a real PostgreSQL |

Nothing in scope is fluff, nothing is a tautology, and no test is disabled. The one **broken
oracle** in the harness is the "accounted" half of the model: `check_inventory` can only demand an
entry for a fence it can parse (`F1`), and an executing mode accepts a fence with zero commands
(`F2`).

## Tier wiring — what actually runs where

| Tier | `make check` (local) | CI job | Verdict |
|---|---|---|---|
| Rust critical (`crates/*/tests/critical_*.rs`) | `test-critical` → `rs-test-critical` | `test`: `cargo test --workspace --all-targets --locked` | **runs** |
| `R42` skip ban (`critical-skip-check`) | `test-critical` dependency | **none** | **local-only** (`F6`) |
| `critical_guard.py` (protected paths) | `critical-guard` (report mode at `HEAD`) | **none** | **local-only** (`F6`) |
| Python critical (`python/tests/critical/`) | `test-critical` (staged, offline) | `pytest-plugin`: `pytest python/tests -q` (real wheel + PG) | **runs** (not via the staged recipe) |
| Runnable docs (`make docs-test`) | yes, last-but-one in `check` | **none** | **local-only** (`F6`) |
| `docs_fences.rs` (6 manifests) | `rs-test` | `test`: `cargo test --workspace` | **runs** |
| `zensical build --strict` | `docs-check` | `docs`: `build (--strict)` | **runs** |
| Docker e2e / rootless | `make e2e` | `e2e`, `rootless` | **runs** (MA.3.4) |

`make check` is the only place the docs gate and the two critical-test guards are enforced, and no
workflow calls `make`. A pull request that weakens a protected test or ships a broken console fence
is green in CI today.

## Harness escape matrix

Every row was reproduced against the real code. "Silent pass" means `check_inventory` returned no problem and the mode returned `True` without asserting the
page's promise.

| # | Escape | Repro (observed) | Sev | Fix |
|---|---|---|---|---|
| `H1` | Fence indented two spaces (valid in a list item/admonition) | `check_inventory` → `[]`; `console_fences` → 0 | MAJOR | accept ≤3-space indentation in `_FENCE` |
| `H2` | `~~~console` fence | `check_inventory` → `[]` | MAJOR | accept `~{3,}` openers |
| `H3` | Four-backtick fence opening `console` | `check_inventory` → `[]` | MAJOR | match the closing run to the opening run's length |
| `H4` | Info-string suffix: `console title="start it"` | `check_inventory` → `[]` | MAJOR | allow an info suffix, keep the language prefix |
| `H5` | A `bash` block containing `$ ` prompts | `check_inventory` → `[]`; no page today has one (scanned all 21) | MAJOR | a `$ ` prompt inside a non-console fence is a failure, whatever the language |
| `H6` | Unclosed `console` fence appended to a page | recognised fences unchanged (1) → inventory still matches; the trailing command is dropped | MINOR | fail on an unclosed fence / a fence-shaped line with no partner |
| `H7` | A bare fence-marker line inside a body truncates it; the rest of the body (a `$ uv run blastit serve --preset nope`) is invisible | `check_inventory` → `[]` | MINOR | same scan as `H6` |
| `H8` | Published page absent from `nav` (`docs/orphan.md`) | `public_pages` = `['README.md', 'docs/a.md']`; `zensical build --strict` publishes `site/orphan/index.html`; problems `[]` | MAJOR | inventory = `docs/**/*.md` minus `exclude_docs`, not `nav` |
| `H9` | A `console` fence whose `$ ` prompts were lost, in an executing mode | `Fence.commands()` → `()`; `mode_check` → `('check reported a clean suite', True)` | MAJOR | every executing mode refuses a non-empty body with zero commands |
| `H10` | `reason = " "` (whitespace) satisfies "a written reason" | `check_inventory` → `[]` | MINOR | floor on length, reject `TODO`/`n/a` |
| `H11` | `static` deploy fence naming a nonexistent file/command | `docker compose -f /nope/compose.yaml up -d`, `docker run --frobnicate ghcr.io/x/shop:nope`, `blastit-manifest fold --manifest /nope/blastit.toml`, `doctl apps create --spec /nope/app.yaml`, `psql --frobnicate postgres://x` all pass `_static_command` | MAJOR (DX) | per-executable flag/value tables; check `--spec`/`-f`/`--manifest` paths resolve |
| `H12` | Malformed JSON boot line in a `static` fence | `_static_json_lines` skips any line that does not parse → `{"event": "boot", "stage": "ready"` passes; indented lines are skipped too | MINOR | a `{`-line in a fence must parse and carry the five keys |
| `H13` | Stale wheel: `wheel_path()` = newest **mtime** in `dist/`, untied to the source tree | `dist/` with `0.1.0` (newer mtime) + `0.1.1` → picks `blastit-0.1.0-…whl` | MINOR | require exactly one wheel whose version equals `Cargo.toml`'s |
| `H14` | YAML/ini/Dockerfile/SQL snippets are never parsed | `docs/ci.md`'s `yaml title=".github/workflows/check.yml"`, `compose.yaml`, the systemd unit, `networkpolicy.yaml`, `db/grants.sql` — nothing reads them | MAJOR (DX) | parse every fenced `yaml`/`ini`/`sql` block (tomllib already covers `deploy/*.toml`) |
| `H15` | `shape` policy normalises numbers | `docs/ci.md` fence 2 with `412` → `9999` still matches (`missing=0`); a wrong finding kind or a missing detail line does fail | MINOR | declared design (`_normalise`); pin at least one count per page or say "illustrative" in the page |

### Acceptance items (a)–(d), each with command + result

**(a) an unaccounted `console` fence makes the gate red — yes, in the five shapes it parses.**
On a `/tmp` copy of the public tree with the real `inventory.toml`:

```console
$ uv run --no-project python /tmp/opencode/ma3_real.py
pages: 21 fences: 38
(a) unaccounted fence: [… 'docs/troubleshooting.md: 1 console fences but the inventory names [];
    add or renumber the entries']
```
and the escapes above (`H1..H5`) return `[]` for the same fence. So (a) holds only for the shapes
`fences._FENCE` recognises.

**(b) deleting an inventory entry makes it red — yes.**
```console
(b) deleted inventory entry: [… 'docs/security.md: 1 console fences but the inventory names []']
(b2) inventory index 9 on a 1-fence page: [… 'docs/security.md: 1 console fences but the inventory names [9]']
```

**(c) a fence whose expected output is wrong fails — yes, where the mode compares text.**
```console
$ uv run --no-project python /tmp/opencode/ma3_c2.py
README fence 3: curl localhost:8080/whatever -> ('hello from blastit',)
  product says nothing       FAIL (caught)
  one character different    FAIL (caught)
  exact                      PASS
```
Exit-code drift is caught too (`expect(..., code=1, ...)` → `FAIL: exited 1:`), a clean line that
observed nothing is caught (`_require_nonzero`), and the `shape` modes catch a wrong word but
normalise numbers (`H15`).

**(d) `make docs-test` with `HOME`/`PATH` scrubbed — no.** It fails, for two independent reasons
(`F7`): with a fresh `HOME` and a working `cargo` the build step downloads `maturin` (10.3 MiB) and
a CPython into the new `HOME` (network, so not offline), and then the harness itself stops because
`pytest==9.2.0` is not in the *new* `HOME`'s uv cache while the harness is offline by contract:

```console
$ env -i HOME=/tmp/opencode/ma3-home RUSTUP_HOME=… CARGO_HOME=… PATH=… make docs-test
Downloading maturin (10.3MiB)
🐍 Found CPython 3.14 at /tmp/opencode/ma3-home/.cache/uv/archive-v0/…/bin/python3
…
harness: pytest==9.2.0 is not in the uv cache, and this gate never reaches the network.
Fill the cache once with `uv pip install pytest==9.2.0` and re-run:
`uv pip install --python …/base/.venv/bin/python pytest==9.2.0` failed with 1:
  cause: Because pytest was not found in the cache and you require pytest==9.2.0, …
make: *** [Makefile:117: docs-test] Error 1
```

The message is actionable and names the fix, but the acceptance item asks for a run that *works*,
so it is recorded as **not met**.

## Mutation audit

### `cargo-mutants` is absent

```console
$ make mutants-rust PKG=blastit-manifest
TMPDIR="/tmp/opencode/blastit-mutants" … cargo mutants -p "blastit-manifest" -j 2 --timeout 90
error: no such command: `mutants`

help: view all installed commands with `cargo --list`
help: find a package to install `mutants` with `cargo search cargo-mutants`
make: *** [Makefile:127: mutants-rust] Error 101
```

`cargo mutants --version` → `error: no such command: mutants`. This is the same blocker `H-002`
already records; no network was used to install the tool. **The milestone gate's literal
`make mutants-rust` evidence is therefore missing and remains a blocking prerequisite.**

### Anchored mutant audit (fallback, labelled as such)

20 mutants, one anchored replacement each, in the policy/validation paths of the two crates;
`cargo test -p <crate> --locked` after each. 19 killed, 1 survived, 0 unviable.

| Crate | Mutants | Killed | Killed by a **protected** suite | Survived |
|---|---|---|---|---|
| `blastit-manifest` | 8 | **8 (100 %)** | **8** | 0 |
| `blastit-sqlpolicy` | 11 | 10 (91 %) | **0** | 1 |
| **Both** | **20** | **19 (95 %)** | **8 (40 %)** | **1** |

`blastit-manifest` (every kill names the protected test):

| # | Mutant | Killed by |
|---|---|---|
| MF1 | `^.*$` no longer counts as a catch-all | `critical_manifest_011::both_catch_all_spellings_are_unrestricted` |
| MF2 | a method-restricted catch-all becomes an insertion point | `critical_manifest_011::both_catch_all_spellings_are_unrestricted` |
| MF3 | a new route is inserted at the top when there is no catch-all | `critical_manifest_edit::a_method_restricted_catch_all_is_not_an_insertion_point` |
| MF4 | a redacted `…` may be granted | `critical_manifest_011::a_redacted_placeholder_cannot_be_granted` |
| MF5 | the fixed-setup exemption is dropped from grants | `critical_manifest_011::the_fixed_protocol_statements_may_keep_their_literal` (+1) |
| MF6 | every ingress body warns like `raw` | `critical_manifest_011::raw_ingress_and_egress_are_boot_warnings` |
| MF7 | `egress = true` warns nothing | `critical_manifest_011::raw_ingress_and_egress_are_boot_warnings` |
| MF8 | the temp file is created without `O_EXCL` (a planted symlink is written through) | `critical_manifest_edit::a_planted_temp_name_symlink_is_never_written_through` (+1) |

`blastit-sqlpolicy` — **none** of these eleven kills came from `tests/critical_literals.rs`:

| # | Mutant | Killed by (standard tier) |
|---|---|---|
| SP1 | `COPY … PROGRAM` accepted | `learn::tests::copy_program_is_refused` |
| SP2 | an inline literal may be learned | `learn::tests::every_inline_literal_form_is_refused` |
| SP3 | multi-statement text accepted | `learn::tests::unparseable_and_multi_statement_are_refused` |
| SP4 | `CALL p(...)` hides a dangerous function | `learn::tests::a_call_and_a_three_part_name_are_refused` |
| SP5 | a schema-qualified call resolves as the catalogue function | `learn::tests::a_function_of_the_same_name_in_another_schema_is_not_refused` |
| SP6 | `SHOW …` is not replayed as setup | `learn::tests::introspection_setup_is_recognised` |
| SP7 | **any** single `SELECT` is replayed as setup | `learn::tests::introspection_setup_is_recognised` |
| SP8 | the introspection walk has no depth bound | **survived** |
| SP9 | the fixed setup statement is redacted | `literals::tests::known_static_setup_sql_is_kept_whole` |
| SP10 | a column list allows a whole-row access | `tables::tests::a_star_grant_allows_a_whole_row_read_and_the_other_way_round` (+1) |
| SP11 | `public.t` and `t` are different tables | `tables::tests::a_grant_built_from_a_pool_keeps_canonical_names` |

**SP8 is a real survivor, not an equivalent.** With the bound removed, an 18-deep nested catalogue
`SELECT` is replayed as session setup; on pristine code it is refused:

```console
$ cargo test -p blastit-sqlpolicy --test probe_depth -- --nocapture   # /tmp copy
pristine:  depth 16: parses=true replayed=true      depth 18: parses=true replayed=false
mutated:   depth 16: parses=true replayed=true      depth 18: parses=true replayed=true
```
The parser stops accepting nesting at depth ~22, so the only reachable window is 17..~22 — narrow,
and the replayed statement is still read-only catalogue SQL. Severity **MINOR** (it removes a
stack-exhaustion bound for a parseable, deeply nested tree). The candidate test (assert `false` at
depth 18) passes on the current tip and fails under the mutant; it belongs in the same new
protected file as `F8`.

## Findings

| # | Sev | Finding | Evidence |
|---|---|---|---|
| `F1` | **MAJOR** | Fence recognition is parser-relative, so `~~~`, four backticks, an indented block, an info-string suffix and a `bash` block with a prompt are all unaccounted. The inventory only ever compares against fences the regex found. | `H1..H5` |
| `F2` | **MAJOR** | A `console` fence whose `$ ` prompts are gone runs in an executing mode and passes with zero assertions. | `H9` — `mode_check` → `('check reported a clean suite', True)` |
| `F3` | **MAJOR** | A page zensical publishes but `nav` does not name is never inventoried (and `--strict` does not complain). | `H8` + `site/orphan/index.html` |
| `F4` | **MAJOR** | `static` deploy fences accept nonexistent commands, flags and file paths; the YAML/ini/Dockerfile/SQL snippets the recipes tell the reader to copy are never parsed. | `H11`, `H14` |
| `F5` | MAJOR (DX) | The gate has no tests of its own: 2 677 lines, 0 `test_` functions, so `check_inventory`, `console_fences`, `_static_command` and `report_result` are only proved by a full 38-fence run. Every escape here is a reading discovery. | `grep -c "def test_" python/tests/runnable_docs/*.py` → 0 |
| `F6` | **MAJOR** | `docs-test`, `critical_guard` and `critical-skip-check` are not in any workflow; no CI job calls `make`. | `grep -rn "make " .github/workflows` → nothing |
| `F7` | **MAJOR** | With `HOME` scrubbed the gate is not offline: the build downloads `maturin` (10.3 MiB) and a CPython into the fresh `HOME`; with `PATH` scrubbed too, `cargo` is not found. The harness also depends on the caller's `HOME` for `uv python find` and for the cached `pytest` (`install_pytest` reads `uv cache dir`). | two `env -i … make docs-test` runs, below |
| `F8` | **MAJOR** | The sqlpolicy policy oracle lives in `src/**` unit tests; the protected suite killed 0 of 11 mutants spanning `COPY … PROGRAM`, multi-statement, inline-literal refusal, `CALL`, setup replay and table extents. Deleting check + test together is invisible to `critical_guard`. | SP1..SP11 |
| `F9` | MINOR | A fence can be truncated (a bare marker line in the body / an unclosed fence) and the lost commands are neither executed nor demanded. | `H6`, `H7` |
| `F10` | MINOR | `reason` is only checked for non-emptiness; `" "` accounts a fence. | `H10` |
| `F11` | MINOR | A malformed or indented JSON boot line is skipped by `_static_json_lines`; `wheel_path()` picks the newest mtime, not the workspace version. | `H12`, `H13` |
| `F12` | MINOR | `shape` policy normalises every number, so a page may print any request/statement count (only `≠ 0` is required). | `H15` |
| `F13` | INFO | The Python critical tier runs in CI through `pytest python/tests`, not through `make test-critical`'s staged offline recipe, so the staging path itself is unexercised in CI. | tier table |

`F7` evidence:

```console
$ env -i HOME=/tmp/opencode/ma3-home PATH=/usr/bin:/bin make docs-test
scripts/build-wheel.sh: line 43: cargo: command not found            # PATH scrubbed

$ env -i HOME=/tmp/opencode/ma3-home PATH=/home/remy/.local/bin:/home/remy/.cargo/bin:/usr/bin:/bin make docs-test
error: rustup could not choose a version of cargo to run …           # HOME scrubbed, RUSTUP_HOME not

$ env -i HOME=/tmp/opencode/ma3-home RUSTUP_HOME=…/.rustup CARGO_HOME=…/.cargo PATH=… make docs-test
Downloading maturin (10.3MiB)                                        # network, not offline
🐍 Found CPython 3.14 at /tmp/opencode/ma3-home/.cache/uv/archive-v0/…/bin/python3
harness: pytest==9.2.0 is not in the uv cache, and this gate never reaches the network. …
make: *** [Makefile:117: docs-test] Error 1
```

Three different `HOME`s, three different answers: the gate is only reproducible on the machine whose
`HOME` already holds the uv-managed CPython and a `pytest==9.2.0` cache entry.

## Gaps → tasks

| # | Severity | Gap | Task |
|---|---|---|---|
| F1, F3, F5, F9, F10 | MAJOR | fence recognition, orphan pages, reason floor, and the missing unit tests for the gate | MA.3.9 |
| F2, F4, F7, F11 | MAJOR | executing modes accept zero commands; static checks accept nonexistent commands/files; YAML/ini/SQL snippets unchecked; HOME/offline isolation; wheel version | MA.3.10 |
| F6 | MAJOR | no CI job runs `make check`'s docs gate or the critical-test guards | MA.3.8 |
| F8 (SP8) | MAJOR | protected-tier twins for the sqlpolicy learn/table policy | MA.3.11 |
| F12 | MINOR | declared design ("numbers and absolute paths may differ"); no task | proposed |
| F13 | INFO | staging path exercised only locally; consider a `make test-critical` step in CI | proposed |

`MA.3.8..MA.3.11` are siblings of this node and depend on it only for the findings, not on its
completion; `MA.3.6` (the 0.1.1 bump) already depends on `MA.3.5` and should not start before the
CI wiring lands, or the release ships with a docs gate that CI does not run.

## Follow-ups

- `MA.3.7`'s product fix note: `harness.unserved()` still names `MA.3.7 (a)` (the
  `BLASTIT_MEM_HIGH_WATERMARK` spawn gap) — the product fix landed, so the hint text is stale and
  should be re-pointed or removed when `harness.py` is next touched (part of `MA.3.7`).
- `docs_fences.rs` pins `count == 6` manifests; a seventh complete `blastit.toml` in the docs needs
  that constant bumped in the same commit (§7.1).
- `make check`'s `docs-test` builds a **release** wheel through `scripts/build-wheel.sh`, so the
  local gate is several minutes even when every fence passes; `F6`'s CI job should cache `dist/`
  and the uv cache rather than re-build per PR.

VERDICT: BLOCKED — unprotected gaps remain (`F1..F8`, tasks MA.3.8..MA.3.11), and the milestone
gate's `make mutants-rust` evidence is unavailable on this host (`H-002`).
