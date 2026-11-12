# Result MA.2.3

## Summary

The CLI, the unprivileged `blastit dev` runtime and the framework-agnostic `blastit check` are implemented, tested and proven at the epic gate: 3-line dev banner with `"privilege": "unprivileged-dev"`, production `blastit` still fails closed, `check` runs the app's own pytest under a hermetic dbproxy (JSON purity, exit `0/1/2/3`, pytest-django `set_db` handshake), and `--learn` writes only inferred routes plus exact queries, refusing literal-bearing SQL. `make check` and `make e2e` are green on this host (CPython 3.14, Docker).

## Evidence

Gate verification (all run by `lead` at `012190a`):

```console
$ cargo test -p blastit --test critical_dev_and_fail_closed --locked
test result: ok. 13 passed; 0 failed        # dev banner/privilege, execve barrier, prod fail-closed, socket ownership
$ cargo test -p blastit --test critical_check_engine --locked
test result: ok. 16 passed; 0 failed        # R27 golden, 13 inference rows, nine finding kinds, three mutation-killed rules
$ cargo test -p blastit-py --test critical_check_schema --locked
test result: ok. 6 passed; 0 failed
$ PYTHONPATH=target/python-critical:python uv run --no-project --with pytest pytest python/tests/critical -q
50 passed                                   # check contract (JSON purity, exits 0/1/2/3, learn gate, R24 redaction, Django handshake)
$ uvx --offline ruff check python tools && uvx --offline ruff format --check python tools
All checks passed! / 43 files already formatted
$ make check
exit 0 (guard, critical-guard, fmt, clippy -D warnings, doc -D warnings, workspace tests,
        11 Rust critical suites + 50 Python critical, ruff, zensical --strict, plan OK)
$ rm -rf dist && make e2e
EXIT=0   app leg "PASS: 0 failure(s)" x6 (demo, sqlalchemy, logs), rootless leg PASS 0 failures
         (capabilities boot mode, 7 per-uid views, empty cap sets + nnp=1, capable copy refused cross-uid)
```

Test-curator epic audit: `docs/testing/AUDIT-MA.2.3.md` — 90 protected tests, 0 fluff, 0 broken oracle; its three BLOCKERs (red `pytest-plugin` job, `test_pytest_plugin` drift, `ctl.chan` EPIPE exit 1) and two MAJORs (R29 learn-refusal oracle, three surviving `audit_v1` mutants) are all closed by `MA.2.3.6`..`MA.2.3.12`; `make mutants-rust` could not run (`cargo-mutants` is not installed on this host — recorded as a tooling gap).

## Files

- `crates/blastit/src/{check.rs,audit_v1.rs,infer_routes.rs,cli.rs,supervise.rs,lib.rs}`, `crates/blastit-py/src/**`
- `crates/blastit/tests/{critical_check_engine.rs,critical_dev_and_fail_closed.rs}`, `crates/blastit-py/tests/critical_check_schema.rs`
- `python/blastit/{_check_pytest.py,pytest_plugin.py}`, `python/tests/{critical/**,test_check.py,test_pytest_plugin.py,test_live_server.py,test_plugin_helpers.py}`
- `e2e/**` (interpreter-agnostic prebuilt images, demo ingress declarations)
- `docs/testing/AUDIT-MA.2.3.md`, `docs/knowledge/LESSONS.md`

## Follow-ups

- `cargo-mutants` is not installed: `make mutants-rust` cannot run locally; the milestone audit needs it for the `blastit-manifest`/`blastit-sqlpolicy` bar.
- `e2e/*.py` is not `ruff format`-clean (mechanical cleanup node).
- The `blastit dev` scheduler refuses to spawn views above an 85 % memory watermark, which surfaces as `503 queue_timeout` after 10 s with no hint; only the test fixture pins 100. A boot warning belongs in the MA.3 docs pass.
- `e2e/demo/blastit.toml` had drifted from the runtime `[route.ingress]` gate since MA.2.2.3; a "fixtures declare their bodies" assertion would have caught it without a Docker run.
- `test_pytest_plugin.py`'s happy-path "repair converges to clean" oracle was lost to the R24 literal rule; a parameterised `application_name` in the fixture app would restore it.
