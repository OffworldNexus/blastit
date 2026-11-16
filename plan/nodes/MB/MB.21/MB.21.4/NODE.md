---
id: MB.21.4
title: "Cut the product over to Blastfile: discovery, runtime parity, check/--learn, Python, Makefile, e2e, and the erasure of the retired reader"
kind: epic
status: ready
priority: 1
depends_on: [MB.21.3]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:16"
---

# Goal

Cut the running product over to `Blastfile`: discovery, runtime parity, `check`/`--learn`/`--json`, the
Python package, the Makefile, every `e2e/` fixture and — last — the deletion of the retired reader.
After this epic no code path in the repository reads the retired format.

# Context

- Design: `docs/design/blastfile-dsl.md` §15 (discovery and the erasure of the retired format) and
  `ADR-0002` (precedence: `BLASTIT_MANIFEST` else the working directory, no upward search).
- Measured entry points that must move: `Config::from_env` (`crates/blastit/src/config.rs:2357-2386`
  and its working-directory default), `stage0`'s missing-file sentence, `cli.rs`'s `--manifest`
  refusal (keep the refusal, reword it), `check.rs`'s `read_manifest`/`learn_line`, `dev_learn.rs`,
  `audit_v1.rs`'s fix actions, `python/blastit/{fold,pytest_plugin}.py` help text and defaults,
  `Makefile`'s `DOCS_SERVE_MANIFEST`, `e2e/**`, and `benchmark/` if it names the file.
- **There is no detector, no sniffer and no migration sentence.** A body that is not a `Blastfile` is an
  ordinary `Blastfile:<line>:<col>` syntax error whatever it contains, and a missing file says
  `missing ./Blastfile` (or names `BLASTIT_MANIFEST`). Nothing in this epic classifies an input by
  format, no sentence explains a change to a user, and no entry point has a second file name to fall
  back on (boot, `dev`, `check`, `fmt`, `blastit-manifest fold`).
- Ordering: `MB.21.4.1` the atomic cutover (one discovery function, `Blastfile` parsing at every entry
  point, and every fixture `make check` reaches — translation and flip cannot be green apart) →
  `MB.21.4.2` runtime parity and §4.3's removals → `MB.21.4.3` `check`/`--learn`/`--json` and the Python
  package → `MB.21.4.4` the `e2e/`, script and Makefile fixtures, which takes `make e2e` green again →
  `MB.21.4.5` the Rust erasure sweep.
  Only `MB.21.4.5` touches `Cargo.toml`/`Cargo.lock`, so the dependency removal cannot collide with a
  fixture rewrite.
- Protected suites that encode the boot/learn contracts are rewritten to Blastfile fixtures with every
  assertion kept (`critical_guard.py --node` clean for each leaf that touches one):
  `crates/blastit/tests/{critical_config_and_presets,critical_dev_and_fail_closed,critical_check_engine,critical_learn_executor_blocks,critical_php_backend,critical_static_cli}.rs`,
  `crates/blastit-manifest/tests/{critical_manifest_011,critical_php_manifest,critical_static_manifest}.rs`,
  and the Python `python/tests/critical/**` that writes a manifest.
- Parity is proven, not assumed: the capability map in `MB.21` gets one end-to-end assertion per row
  (a static mount's four-step precedence, an ingress `body` mode, `prefork`, `egress any`, a trait's
  grants, `entrypoint php`, `HEAD` from `GET`).

## Children

| Node | What | Depends |
|---|---|---|
| `MB.21.4.1` | The atomic cutover: one discovery function, `Blastfile` parsing at every entry point, §15.1's sentences and exit codes, `fmt`'s re-parse, and every fixture `make check` reaches | `MB.21.3.6` |
| `MB.21.4.2` | Runtime parity and design §4.3's removals: precedence, ingress modes, executor, grants — protected end-to-end suites, no fixture translation | `MB.21.4.1` |
| `MB.21.4.3` | `check` / `dev --learn` / `--json` fix actions against Blastfile, plus the Python package (`fold`, the pytest plugin, the PyO3 editor binding) | `MB.21.4.2` |
| `MB.21.4.4` | The remaining `e2e/` and `python/tests/` fixtures, the scripts and the Makefile's `docs-serve` stage on Blastfile — `make e2e` green again, no deletion work | `MB.21.4.3` |
| `MB.21.4.5` | The Rust erasure sweep: the reader, `Manifest::from_toml_str`, the TOML synthesis in `stage1`, the dependency, every mention | `MB.21.4.4` |

# Acceptance

```console
$ uv run --no-project tools/plan.py tree --under MB.21.4
$ make check
$ make e2e
$ grep -rnE 'blastit[.]toml' crates/ python/ e2e/ Makefile scripts/ --include='*.rs' --include='*.py' --include='Makefile' --include='*.sh'
$ find e2e python/tests \( -name 'Blastfile' -o -name '*.toml' \) | sort
```

1. Every child is `done` with evidence; `plan.py tree` shows no open child.
2. `make check` exit 0 and `make e2e` exit 0 — both are the epic gate's, and `make e2e` is red from
   `MB.21.4.1`'s commit until `MB.21.4.4`, which is expected; no leaf weakens a gate to cover that gap.
3. The `grep` above prints nothing: no retired file name as a path, a default, a fixture, a script line
   or a Makefile stage, and no test that names it next to a refusal either.
4. The reader, the patcher and the dependency are gone — `Manifest::from_toml_str` no longer exists, no
   `toml`-shaped path remains in `blastit-manifest`, and `cargo tree -p blastit-manifest | grep -ci toml`
   prints `0` — all proven by `MB.21.4.5`, the only leaf that owns `Cargo.toml`/`Cargo.lock`.
5. Each capability-map row has a protected assertion; the six rewritten boot/learn suites pass with no
   weakened assertion, and `critical_guard.py --node` is clean for every leaf.

# Notes

- Serialize on `Cargo.toml`/`Cargo.lock`: exactly one child (`MB.21.4.5`) owns both files, so no other
  leaf may add, remove or bump a dependency.
- Keep the release honest: `docs/` still shows the retired openings until `MB.21.5.1`, and `make
  docs-test` builds a wheel. `MB.21.4.1` moves the one page block the harness *executes*
  (`docs/local.md`'s manifest) together with the staged file name; every other page stays `MB.21.5.1`'s,
  and no leaf edits docs prose to make a fence pass.
- A leaf that finds the retired name in a file its `paths` do not list records the hit in `RESULT.md`
  and names the sibling that owns it; it does not widen itself silently.
