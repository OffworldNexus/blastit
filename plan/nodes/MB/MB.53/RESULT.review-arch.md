# MB.53 — architecture / security / test review

Base `32d781b`. Read in full: `plan/nodes/MB/MB.53/{NODE,RESULT}.md`, the whole
`crates/blastit/src/check.rs` diff, `bundled_python`/`bundle_root`/
`platform_env`/`prepare_env`/`require_pytest`/`imports`/`run_pytest`,
`crates/blastit-env/src/env.rs` (`build_in`, `Prepared::app_env`, strip order),
`crates/blastit-audit/src/obs.rs` (`HttpObsLine`, `SCHEMA_VERSION`),
`python/tests/critical/test_check_contract.py` (`Project` stub construction,
`run_check`'s `PYTHONPATH`), `python/tests/{test_check,test_pytest_plugin,
test_release_scripts,live_project/conftest}.py`, `.github/workflows/ci.yml`
(`pytest-plugin` job), `e2e/universal_binary.py::leg_check`,
`Makefile` (`test-critical`, `install`'s printed recipe), and every
`PYTHONPATH` site in `crates/ python/ e2e/ scripts/ Makefile`.

Read-only commands I ran myself:

```
git status --short / git diff --stat 32d781b
  M crates/blastit/src/check.rs, M plan/{BUDGET,JOURNAL,STATE}.md, ?? plan/nodes/MB/MB.53/
git diff 32d781b -- python/          -> empty
grep -rn PYTHONPATH crates/ python/ e2e/ scripts/ Makefile
  -> exactly one builder of the bundle join: check.rs:1008
uv run --no-project tools/critical_guard.py --base 32d781b --node MB.53
  critical-guard: 32d781b..working tree, 4 changed file(s)
  node MB.53: paths ['crates/blastit/src/check.rs', 'python/blastit/**', 'python/tests/**']
    no change to a protected critical test
# import-precedence demo, /tmp/opencode/pp-demo (caller/blastit/{__init__,pytest}.py, bundle/blastit/{__init__,_check_pytest}.py)
PYTHONPATH=caller:bundle python -c 'import blastit; ...'
  blastit from .../caller/blastit/__init__.py caller       # caller package wins, as intended
  stub pytest found
PYTHONPATH=caller:bundle python -c 'import blastit._check_pytest'
  ModuleNotFoundError: No module named 'blastit._check_pytest'   # exit 1
PYTHONPATH=bundle:caller python -c 'import blastit._check_pytest; print("gate ok")'
  gate ok
```

## Verdict

**APPROVE — no blockers, no majors.** The precedence reasoning is correct and
the fix is at the single right place:

- Python resolves `sys.path` as *cwd (`-m`) → `PYTHONPATH` in order → stdlib →
  `site-packages`*, so `{caller}:{bundle}` is the only order in which the
  project's `stub/pytest` (`python/tests/critical/test_check_contract.py`'s
  `Project`, a *package* `pytest/` with `__main__.py`) shadows the installed
  one while the bundle still shadows the child interpreter's own
  `site-packages`. Both halves of the contract hold at once only in this order;
  the demo above confirms caller-first resolves `blastit` and `pytest` from the
  caller and bundle-first does not.
- The wheel/`.deb`/`.rpm`/`uv tool` leg is unaffected: with no caller
  `PYTHONPATH` the helper returns the bundle alone (`check.rs:1145-1152`), and
  `PYTHONPATH` always precedes the child's `site-packages`, so
  `-p blastit._check_pytest` stays importable on an interpreter that never
  installed `blastit` (`bundle_root` already requires both markers,
  `check.rs:1157-1166`).
- Purity: `pythonpath_with_bundle` reads no environment, holds no state, and
  the only process-environment read stays where it was —
  `platform_env`→`std::env::vars_os()` at the command's composition root
  (`check.rs:371-374`), not inside the helper. No `OnceLock`, no
  `cfg(any(test, debug_assertions))` hook, no mock trait.
- Cohesion: one join site (`check.rs:1008`); `Makefile:372` and
  `e2e/universal_binary.py:743` set `PYTHONPATH` outright with no caller entry
  to order, and `stage0/mod.rs` / `visibility.rs` / `config/*` only mention it
  in prose. Nothing to keep consistent, nothing duplicated.
- Tests: `python/` is byte-identical to the base; the protected contract tier
  is what proves the behaviour (22 passed inside the image), the new unit test
  only pins the string, which is the right split.
- Evidence: RESULT's counts are internally consistent and reconcile with the
  baseline (`MB.52`: 12 failed + 446 passed + 1 skipped = 459; `MB.53`:
  458 + 1 = 459) — 12 failures became 12 passes with no test added, removed or
  newly skipped. `0 failed, 0 errors` is claimed and matches that arithmetic.

## Findings (all non-blocking)

1. **`[MINOR]` `crates/blastit/src/check.rs:1001-1008` — the whole `blastit`
   package is now resolved by the caller, so a caller `PYTHONPATH` entry that
   itself ships a `blastit` package wins outright.** A Python package is not
   merged across `sys.path` entries, so if that copy lacks `_check_pytest.py`
   the gate cannot be found anywhere (`-p blastit._check_pytest` → pytest exits
   4, `blastit check` exits non-zero), and if it has a stale copy the gate comes
   from the stale one. Demonstrated above. This is **fail-closed**, and none of
   the shipped layouts hit it — `make install`'s printed recipe
   (`Makefile:975`) puts the bundle's own `python` dir on `PYTHONPATH`,
   `make test-critical` (`Makefile:372`) stages a package that carries the gate,
   a wheel/`.deb`/`.rpm` run with no `PYTHONPATH` never sets `existing`, and an
   old distro package in `site-packages` loses to the bundle as designed. It is
   a narrow new failure surface, not a hole. Cheap hardening, in the shape the
   file already uses for `require_pytest` (`check.rs:390`): after building the
   child environment, probe `imports(python, …, "blastit._check_pytest")` and
   refuse with one sentence naming the shadowing entry (`your PYTHONPATH's
   blastit package shadows check's gate: <dir>`) instead of letting pytest fail
   with a bare `no module named` traceback. Out of this node's goal; record as a
   follow-up rather than reopening the fix — reverting to bundle-first would
   re-break the protected contract.

2. **`[MINOR]` `crates/blastit-audit/src/obs.rs:25-40` + `check.rs:1008` — the
   gate itself is now caller-selectable, and version skew is silent.** Every
   optional field of `HttpObsLine` is `#[serde(default)]` and the schema version
   is stated by the orchestrator, not the line, so an older caller `blastit`'s
   `_check_pytest` produces a parseable but weaker observation set (no
   `sample`/`body_len`) rather than an error. Pre-existing — a project-local
   `blastit/` in the cwd already wins under *both* orderings, because `-m`
   prefixes the cwd before `PYTHONPATH` — but this change widens the surface
   from "the cwd" to "any `PYTHONPATH` entry". If the audit is meant to be
   tamper-evident even in this dev-time command, the durable fix is an origin
   check: the gate records `blastit._check_pytest.__file__`, or `check.rs`
   compares it against `bundled_python()`/`bundle_root` before trusting the
   report. Follow-up only; `blastit check` runs the project's own suite with the
   project's own code, so this is inside that command's trust boundary today.

3. **`[MINOR]` `plan/nodes/MB/MB.53/RESULT.md` — the single skip is
   misattributed.** "The skip is `smoke`-style privilege self-skip inside the
   Python tier" does not describe anything the job's command collects: the only
   skip that can fire under `pytest python/tests -q` with `DATABASE_URL` set,
   `pytest-django`/`django`/`psycopg` installed and `dist/` holding a wheel is
   `python/tests/live_project/tests/test_live.py::test_own_live_server_wins`,
   gated on the opt-in `BLASTIT_TEST_OWN_LIVE_SERVER` (`live_project/conftest.py:18`,
   set only by `test_live_server.py:384`'s nested run). The privilege skips were
   removed by `R39`. The substantive claim still holds — the count is identical
   to `MB.52`'s baseline, so nothing was newly skipped by this change — but the
   evidence record should name the real skip, so a later reader does not go
   looking for a privilege self-skip that no longer exists.

4. **`[MINOR]` `crates/blastit/src/check.rs` — 1,449 production LOC** (2,248 with
   its test module), against the <500 / 1,000 budget; this node net-adds ~17
   production lines to it. Unavoidable here (the join exists only there, and the
   node's `paths` name this file), but `PytestSpawn` + `run_pytest` +
   `python_program` + `bundled_python`/`bundle_root` are a self-contained child
   -environment unit that belongs in `src/check/pytest.rs`, and the string helper
   would then sit beside its only caller. Follow-up, not this node.

5. **Nit (no action required)** — in
   `a_callers_pythonpath_precedes_the_bundle_directory` (`check.rs:1499-1514`)
   the `assert!(joined.starts_with("/project/stub:"))` is implied by the
   `assert_eq!` immediately above it; if the intent is to assert *ordering*, the
   structural form (split on `:`, compare indices of the two entries) survives a
   future change of join primitive and reads as the contract rather than as a
   string. The behavioural contract is already proven by the protected tier, so
   this test is correctly a cheap pin — no change needed to land the node.

## Checked and clean

- Zero secret in the new path: neither the helper nor the comment interpolates
  `PYTHONPATH` values, `DATABASE_URL` or any credential into a message; the
  helper's output is only ever handed to `Command::env`.
- Zero `unwrap()`/`expect()` added on untrusted input (`map(String::as_str)` +
  `unwrap_or_default()` is total); the new `expect`s are confined to the
  existing `#[cfg(test)]` module.
- No panic path, no new `unsafe`, no new global, no new trait.
- Protected critical tests untouched (`critical_guard` output above), and
  `python/` is unchanged by this node.
