# Result MA.2

## Summary

All five 0.1.1 DX upgrades are implemented and proven: automatic `./blastit.toml` discovery with `app`/`preload` inside the manifest, the unprivileged `blastit dev` supervisor with the stage-0 `execve` secret barrier, wire-level `--learn` at dbproxy/scheduler, the strict framework-agnostic `blastit check` (human + `--json`), and ingress validation at HTTP and scheduler boundaries. Every sub-epic audit is closed and no test-curator BLOCKER/MAJOR gap is open.

## Evidence

Run by `lead` at `d105fdb` (see each child RESULT.md for per-leaf tails):

```console
$ make check
exit 0 — guard, critical-guard, fmt, clippy -D warnings, doc -D warnings, workspace tests,
         11 Rust critical suites + 50 Python critical tests, ruff check + format, zensical --strict, plan OK
$ rm -rf dist && make e2e
EXIT=0 — app leg (Django demo 6x "PASS: 0 failure(s)", SQLAlchemy leg 0 failures,
         blastit log 0 failures) + rootless capability leg (boot mode capabilities,
         7 per-uid views with empty cap sets + no_new_privs, cross-uid capable copy refused EACCES)
```

Requirement coverage per `docs/testing/AUDIT-MA.2.1.md`, `AUDIT-MA.2.2.md`, `AUDIT-MA.2.3.md`: R1–R42 each mapped to a protected test; the four MA.2.1 audit gaps (G1 strict-string JSON, G2 raw `;` form, G3 64 KiB sample cap, G4 schema-qualified introspection replay) were closed by `MA.2.1.4`–`MA.2.1.7`, and every MA.2.3 gap by `MA.2.3.6`–`MA.2.3.12`.

Acceptance mapping:

- **Production fails closed / `dev` unprivileged:** `crates/blastit/tests/critical_dev_and_fail_closed.rs` (13 tests) — banner + `"privilege": "unprivileged-dev"`, stage-0 `execve` stripping, per-route socket dirs, and the non-zero-exit prod twin.
- **Framework-agnostic `--learn` + `check`:** Django leg end-to-end in `make e2e`; raw-WSGI and Flask-shaped `.wsgi_app` in `python/tests/critical/test_check_contract.py`; SQLAlchemy leg in e2e; both human and `--json` reports pinned.
- **ruff gates:** `make py-lint` runs `ruff check` + `ruff format --check` over `python` and `tools`.
- **Manifest/discovery:** `crates/blastit/tests/critical_manifest_011.rs`, `critical_config_and_presets.rs`, `critical_manifest_editor.rs`.

## Files

`crates/blastit-ingress/**`, `crates/blastit-sqlpolicy/**`, `crates/blastit-dbproxy/**`, `crates/blastit-manifest/**`, `crates/blastit/src/**`, `crates/blastit-py/**`, `python/blastit/**`, `python/tests/**`, `e2e/**`, `docs/testing/AUDIT-MA.2.*.md`.

## Follow-ups

- `cargo-mutants` is not installed on this host; `make mutants-rust` cannot run. Asked via `HUMAN.md` (`H-002`); the milestone gate needs it for `blastit-manifest` / `blastit-sqlpolicy`.
- `e2e/*.py` is not `ruff format`-clean (mechanical node).
- The 85 % memory watermark that makes `blastit dev` answer `503 queue_timeout` with no hint deserves a boot warning (MA.3 docs).
