# RESULT MA.2.3.9 — pin `check --learn` refusal exit `1` and `"wrote": false`

STATUS: done

Files changed (one file, inside `paths`, additive only — `git diff --stat` reports
`159 insertions(+)`, zero deletions):

- `python/tests/critical/test_check_contract.py`
  - `LEARN_APP` / `_learn_app()` (:360, :405): the existing mini-app's socket
    round-trip, parameterised by the one statement it sends, so the two twins
    differ in nothing but that statement.
  - `LEARN_RUNNER` (:413): one gated `GET /notes` (a path no manifest route
    covers) — so the learn plan has a *route* to propose, which makes the
    refusal the only thing between the plan and a write.
  - `test_learn_refuses_an_inline_literal_with_exit_one_and_writes_nothing`
    (:1011), the missing oracle for `AUDIT-MA.2.3` F5 / mutant #18.
  - `import tomllib` (stdlib) to read the written manifest back structurally.

No existing assertion was removed, weakened or skipped; the critical guard
confirms the change is authorized by this node.

## What the test pins

**Denied twin** — the suite sends `SELECT 1 WHERE 's3cr3t' = 's3cr3t'` through
the real `blastit-dbproxy` `DynamicCheck` (learn) listener on the real wire, so the
policy's own `ObsVerdict::Refused` is what the gate reads:

- exit `1` (§1.1: "`--learn` refused a statement");
- exactly one JSON document, `"wrote": false`, `"proposed": {"routes": 1,
  "queries": 0}` — proving there **was** something worth writing, so `wrote:
  false` is caused by the refusal, not by an empty plan;
- `blastit.toml` byte-identical, no `name = "notes"` and no `SELECT 1 WHERE…`
  line added;
- stderr carries `learn: refused … bind the value with a placeholder instead`
  (the actionable fix) and the statement **redacted** (`SELECT 1 WHERE … = …`);
- R24: `s3cr3t` appears in neither the document nor stderr;
- `server.statements == [literal]` — the literal really reached the upstream
  mock, so the refusal came from observed wire SQL, not from a doctored
  observation file.

**Allowed twin** — the same request, path and statement shape, parameterised
(`SELECT 1 WHERE $1 = $2`): exit `0`, `"wrote": true`, `{"routes": 1,
"queries": 1}`, and `blastit.toml` re-parses (tomllib) to a `notes` route with
`path = ["/notes"]` and `[route.db].queries == ["SELECT 1 WHERE $? = $?"]`.
So a blanket refusal cannot satisfy the test.

## Acceptance evidence

Env: the staged tier the `Makefile` builds (`target/python-critical`:
`blastit/_blastit.cpython-314-x86_64-linux-gnu.so` +
`blastit/_bin/blastit-dbproxy` from `target/debug`), CPython 3.14.10, and
`pytest` from the uv offline cache. `make check` / `make test-critical` were
**not** run (the target lock belongs to the concurrent Rust node); the second
acceptance command was run exactly as `make test-critical` invokes it.

```console
$ PYTHONPATH=target/python-critical:python uv run --no-project --with pytest --offline \
    pytest python/tests/critical/test_check_contract.py -q
......................                                                   [100%]
22 passed in 3.66s

$ PYTHONPATH=target/python-critical:python UV_OFFLINE=1 uv run --no-project \
    python -m unittest discover -s python/tests/critical -p 'test_*.py'
..................................................
----------------------------------------------------------------------
Ran 50 tests in 3.725s

OK
```

(21 → 22 contract tests, 49 → 50 protected critical tests.)

```console
$ uv run --no-project tools/critical_guard.py --base acd9289 --node MA.2.3.9
critical-guard: OK: all 1 protected change(s) are authorized by node MA.2.3.9

$ uvx --offline ruff check python/tests/critical/test_check_contract.py
All checks passed!
$ uvx --offline ruff format --check python/tests/critical/test_check_contract.py
1 file already formatted
```

## Mutation proof (scratch copy, never the tree)

`/tmp/opencode/ma239-mutation` is an `rsync` of the tree (no `target/`, no
`.git/`) with its own `CARGO_TARGET_DIR`; `crates/blastit/src/check.rs` in the
working tree was never touched (`git diff crates/blastit/src/check.rs` is
empty). The mutation is exactly mutant #18: `check.rs:940`'s
`let clean = plan.requests > 0 && plan.refused.is_empty() && failure.is_none();`
→ `let clean = plan.requests > 0 && failure.is_none();`.

**With the mutation** (rebuilt `blastit-py`, test run against that `.so` only):

```console
mutant #18 applied
   Compiling blastit-py v0.1.0 (/tmp/opencode/ma239-mutation/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.01s
$ PYTHONPATH="stage:python" uv run --no-project --with pytest --offline \
    pytest python/tests/critical/test_check_contract.py -q --tb=line -k learn_refuses
F                                                                        [100%]
=================================== FAILURES ===================================
E   AssertionError: 0 != 1 : learn: refused SELECT 1 WHERE … = …: the statement inlines a quoted literal; bind the value with a placeholder instead
    learn: 1 routes, 0 queries added → /tmp/tmpm6wn3pan/blastit.toml
=========================== short test summary info ============================
FAILED python/tests/critical/test_check_contract.py::TestTheContract::test_learn_refuses_an_inline_literal_with_exit_one_and_writes_nothing
1 failed, 21 deselected in 0.20s
```

(The mutant even prints a phantom `learn: 1 routes, 0 queries added` line while
writing nothing — the exit code is the only channel that reports the refusal.)

**Restored `check.rs:940`, same scratch stage, incremental rebuild:**

```console
restored
   Compiling blastit-py v0.1.0 (/tmp/opencode/ma239-mutation/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.15s
$ PYTHONPATH="stage:python" uv run --no-project --with pytest --offline \
    pytest python/tests/critical/test_check_contract.py -q --tb=line -k learn
..                                                                       [100%]
2 passed, 20 deselected in 0.49s
```

So the failure is caused by the deleted `plan.refused.is_empty()` alone, and the
oracle is no longer satisfied by `plan.refused`-free code.

## Follow-ups

- The allowed twin sends `SELECT 1 WHERE $1 = $2` in the **simple** query
  protocol, which a real PostgreSQL would reject for parameters it never bound;
  the mock upstream accepts any statement, and what the oracle needs is the
  policy's verdict on the wire text. An extended-protocol
  (`Parse`/`Bind`/`Execute`) variant would be closer to production, but it
  exercises the same `check_learnable` call and no protected requirement names
  it; left as an optional hardening rather than a hollow extra run.
- Under the mutation the JSON document still says `"wrote": false` (the
  `would_write` predicate keeps its own refusal guard, `audit_v1.rs:1171`), so a
  future reviewer should keep the **exit code** as the mutation's oracle; the
  `wrote` assertion is the R29 end-to-end twin, not a mutant killer.
