---
id: MB.21.4
title: "Cut the product over to Blastfile: discovery, migration refusal, runtime parity, check/--learn, Python, Makefile, e2e"
kind: epic
status: ready
priority: 1
depends_on: [MB.21.3]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Cut the running product over to `Blastfile`: discovery, the migration refusal, runtime parity,
`check`/`--learn`/`--json`, the Python package, the Makefile and every `e2e/` fixture. After this epic
no code path in the repository reads `blastit.toml`.

# Context

- Design: `docs/design/blastfile-dsl.md` §10 (discovery + migration error) and `ADR-0002` (precedence:
  `BLASTIT_MANIFEST` else the working directory, no upward search).
- Measured entry points that must move: `Config::from_env` (`crates/blastit/src/config.rs:2357-2386`
  and its `./blastit.toml` default), `stage0`'s missing-file sentence, `cli.rs`'s `--manifest` refusal
  ("put `blastit.toml` in the working directory"), `check.rs`'s `read_manifest`/`learn_line`,
  `dev_learn.rs`, `audit_v1.rs`'s fix actions, `python/blastit/{fold,pytest_plugin}.py` help text and
  defaults, `Makefile`'s `DOCS_SERVE_MANIFEST`, `e2e/**`, and `benchmark/` if it names the file.
- **The migration error is a feature**, not a stack trace: a `blastit.toml` path, or content that is
  TOML (`version = 1`, `[[route]]`, a leading `[`), produces one sentence naming `Blastfile`,
  `./Blastfile` and `docs/reference/blastfile.md` — never a lexer error, never "unknown keyword".
  Every entry point gets the sentence (boot, `dev`, `check`, `fmt`, `blastit-manifest fold`).
- Protected suites that encode the boot/learn contracts are rewritten to Blastfile fixtures and are
  listed in `paths` on purpose:
  `crates/blastit/tests/{critical_config_and_presets,critical_dev_and_fail_closed,critical_check_engine,critical_learn_executor_blocks,critical_php_backend,critical_static_cli}.rs`,
  `crates/blastit-manifest/tests/critical_manifest_011.rs`, `crates/blastit-manifest/tests/critical_php_manifest.rs`,
  and the Python `python/tests/critical/**` that writes a manifest.
- Parity is proven, not assumed: the capability map in `MB.21` gets one end-to-end assertion per row
  (a static mount's four-step precedence, an ingress `body` mode, `prefork`, `egress any`, a trait's
  grants, `entrypoint php`, `HEAD` from `GET`).

## Children

| Node | What | Depends |
|---|---|---|
| `MB.21.4.1` | `./Blastfile` discovery, `BLASTIT_MANIFEST`, the migration refusal at every entry point | `MB.21.3.6` |
| `MB.21.4.2` | Runtime parity: precedence, ingress modes, executor, grants — protected end-to-end suites | `MB.21.4.1` |
| `MB.21.4.3` | `check` / `dev --learn` / `--json` fix actions against Blastfile | `MB.21.4.2` |
| `MB.21.4.4` | Python package, Makefile and every `e2e/` fixture on Blastfile; delete the TOML reader | `MB.21.4.3` |

# Acceptance

```console
$ uv run --no-project tools/plan.py tree --under MB.21.4
$ make check
$ make e2e
$ grep -rn 'blastit\.toml' crates/ python/ e2e/ Makefile scripts/ --include='*.rs' --include='*.py' --include='Makefile' --include='*.sh'
$ find e2e python/tests -name 'Blastfile' -o -name 'blastit.toml' | sort
```

1. Every child is `done` with evidence; `plan.py tree` shows no open child.
2. `make check` exit 0 and `make e2e` exit 0.
3. The `grep` above finds no `blastit.toml` as a **path or default** (a migration test may name the
   string once, next to the refusal assertion, and `RESULT.md` names that file).
4. The TOML reader and patcher are gone: `Manifest::from_toml_str` no longer exists and no
   `toml`-shaped code path remains in `blastit-manifest`.
5. Each capability-map row has a protected assertion; the six rewritten boot/learn suites pass with no
   weakened assertion, and `critical_guard.py --node` is clean for every leaf.

# Notes

- Serialize on `Cargo.toml`/`Cargo.lock`: if deleting the TOML reader or adding a module needs a manifest
  edit, exactly one child owns both files.
- Keep the release honest: `docs/` still shows `blastit.toml` until `MB.21.5.1`, and `make docs-test`
  builds a wheel — if a docs fence breaks because the binary now wants `Blastfile`, `MB.21.4.4` is the
  child that fixes the fixture, not the page.
