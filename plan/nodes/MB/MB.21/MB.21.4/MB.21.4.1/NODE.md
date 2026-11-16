---
id: MB.21.4.1
title: Discover ./Blastfile and refuse blastit.toml/TOML input with one migration sentence at every entry point
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.6]
paths:
  - crates/blastit/src/config.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/check.rs
  - crates/blastit/tests/critical_config_and_presets.rs
  - crates/blastit/tests/critical_manifest_migration.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T00:38"
---

# Goal

Discover `./Blastfile` (with `BLASTIT_MANIFEST` still overriding), keep "no upward search", and make
every entry point answer a `blastit.toml` or TOML input with one sentence that names the new file.

# Context

- Design §10. Precedence is unchanged (`ADR-0002`): `BLASTIT_MANIFEST` else `./Blastfile` in the working
  directory; a bare name means the working directory; nothing walks upwards.
- Code sites measured today (`crates/blastit/src/`): `config.rs:2357-2386` (`manifest_path`, the
  `./blastit.toml` default and the empty-variable message), `config.rs:1019` (the `Config` default),
  `config.rs:3035-3042` (the doc comment), `stage0.rs` (the missing-file sentence surfaced by
  `MB.1.6` — an `app.output` event on stdout in production, raw text on fd 2 for `dev`), `cli.rs:264`
  and `cli.rs:331/357` (the `--manifest` refusal: keep the refusal, reword it), `check.rs:294-300`
  (`read_manifest`'s "…blastit.toml, or set BLASTIT_MANIFEST").
- The migration classifier is a small pure function tested in isolation: given the path and the first
  bytes, a `.toml` suffix **or** TOML-shaped content (`version = 1`, `[[route]]`, a line starting `[`
  with `]` on it) is `ManifestError::TomlManifest { path, line }`, whose sentence is
  `"<path> is a TOML manifest; Blastit reads ./Blastfile now (or BLASTIT_MANIFEST): see docs/reference/blastfile.md"`.
  It must never reach the lexer, so the message can never say "unknown keyword" or "expected `{`".
- Tests: extend `crates/blastit/tests/critical_config_and_presets.rs` (path precedence + defaults) and
  create `crates/blastit/tests/critical_manifest_migration.rs` for the refusal at each entry point
  (boot, `dev`, `check`, and the production stage-0 message on **stdout** as a JSON `app.output`
  event — see `LESSONS.md` on the two streams).

# Acceptance

```console
$ make test-critical      # includes critical_config_and_presets + critical_manifest_migration
$ make rs-test
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.1
$ make check
```

1. `BLASTIT_MANIFEST=other-blastfile blastit check` reads that file; with no variable, `./Blastfile` is
   read; nothing is found above the working directory (a fixture with a parent `Blastfile` proves it).
2. A `blastit.toml` given four ways — present in the directory, named by `BLASTIT_MANIFEST`, passed as a
   path argument, TOML content in a file called `Blastfile` — produces the same one-sentence refusal
   with the `Blastfile` name and the reference link, no parser vocabulary, and exit code 1.
3. `blastit dev` in an empty directory names `./Blastfile` (not `BLASTIT_APP`) on fd 2; the production
   boot names it inside the `app.output` event on fd 1.
4. `--manifest`'s refusal sentence names `Blastfile` and `BLASTIT_MANIFEST`.
5. `make check` exit 0; `critical_guard.py` clean.

# Notes

- `MB.21.3.5`/`MB.21.3.6` are ancestors, so `Manifest::from_toml_str` still exists — that is fine; this
  leaf only refuses TOML **at the door**, it does not delete the reader (`MB.21.4.4` does).
- Do not create a root `Blastfile` for the repository: it is not an app, and nothing boots it.
