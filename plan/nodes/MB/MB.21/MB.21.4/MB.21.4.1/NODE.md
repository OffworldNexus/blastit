---
id: MB.21.4.1
title: "Discover ./Blastfile (BLASTIT_MANIFEST, no upward search) and fail a missing file with the plain missing-file sentence — there is no format detection and no migration"
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
  - crates/blastit/tests/critical_blastfile_discovery.rs
  - crates/blastit/tests/blastfile_discovery.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:16"
---

# Goal

Discover `BLASTIT_MANIFEST` else `./Blastfile` at every entry point, name `./Blastfile` when nothing
is there, and treat every other body as an ordinary `Blastfile:<line>:<col>` syntax error. There is no
detector, no sniffer, no suffix test, no "migration" classifier and no second file name anywhere in
the product.

# Context

- Design §15 is authoritative. Precedence is unchanged (`ADR-0002`): `BLASTIT_MANIFEST` else
  `./Blastfile` in the working directory; a bare name means the working directory; nothing walks
  upwards.
- Owner directive (2026-11-16): the v1 format is buried exactly like the retired PHP transport
  (`MB.3.17`/`MB.3.30`). Delete any classifier that recognises it. A v1 body is not a special case —
  the lexer from `MB.21.3.1` meets `version = 1` and reports a syntax error like it would for any
  other stray token.
- Code sites measured today (`crates/blastit/src/`), listed by
  `grep -rnE 'blastit[.]toml' crates/blastit/src`: `config.rs:2357-2386` (`manifest_path`, the
  working-directory default and the empty-variable sentence), `config.rs:1019` (the `Config`
  default), `config.rs:3035-3042` (the doc comment), `stage0.rs` (the missing-file sentence surfaced
  by `MB.1.6` — an `app.output` event on stdout in production, raw text on fd 2 for `dev`),
  `cli.rs:264` and `cli.rs:331/357` (the `--manifest` refusal: keep the refusal, reword it),
  `check.rs:294-300` (`read_manifest`).
- Sentences after this node (one shape each, so a reader can copy them):
  * missing file, `dev`: `missing ./Blastfile in the working directory (or set BLASTIT_MANIFEST)`.
  * missing file, production stage 0: the same sentence inside the `app.output` event on fd 1.
  * `--manifest`: keep the refusal, and name `Blastfile` plus `BLASTIT_MANIFEST`.
  * foreign body: whatever the parser says — for a v1 body that is the refusal
    `docs/design/blastfile-dsl.md` §6.7 pins for its first offending token, e.g.
    ``Blastfile:1:9: `=` is not Blastfile syntax; write the value after the keyword``, with no
    second sentence and no mention of another format.
- Tests: extend `crates/blastit/tests/critical_config_and_presets.rs` (path precedence + defaults) and
  create `crates/blastit/tests/critical_blastfile_discovery.rs` for the missing-file sentence and the
  syntax error at each entry point (boot, `dev`, `check`, `fmt`). The production stage-0 sentence is
  asserted on **stdout** as a JSON `app.output` event (`LESSONS.md`, the two streams).

# Acceptance

```console
$ make test-critical
$ make rs-test
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.1
$ grep -rnE 'blastit[.]toml|TomlManifest|looks_like_toml|is_toml|strip_suffix\(".toml"\)' crates/blastit/src
$ make check
```

1. `BLASTIT_MANIFEST=other-blastfile blastit check` reads that file; with no variable, `./Blastfile`
   is read; a fixture with a `Blastfile` in a parent directory proves nothing above the working
   directory is read.
2. In an empty directory `blastit dev` names `./Blastfile` on fd 2 and the production boot puts the
   same sentence inside the `app.output` event on fd 1; neither sentence names a second file name.
3. A body that is not a Blastfile — a `version = 1` line followed by a `[[route]]` table included —
   fails at boot, `dev`, `check` and `fmt` with the ordinary `Blastfile:<line>:<col>` syntax error
   naming the file and the position, exit code 1, and nothing else: no detection, no format name, no
   second sentence.
4. The `grep` above prints nothing: no retired file-name default, no detector, no
   `TomlManifest`/`is_toml` symbol, no `.toml` suffix test in `crates/blastit/src`.
5. `make check` exit 0 and `critical_guard.py` clean.

# Notes

- The repository-wide emptiness of the retired name across every crate, `python/`, `e2e/`, `scripts/`
  and `Makefile` is `MB.21.4.4`'s and `MB.21.4.5`'s acceptance, and `MB.21.5.2` makes it permanent. This
  node leaves the discovery and messaging surface clean and lists, in `RESULT.md`, every remaining hit
  it did not own and which sibling owns it.
- `MB.21.3.5`/`MB.21.3.6` are ancestors, so the retired reader still exists in the tree — that is fine;
  it is deleted by `MB.21.4.5`. This leaf only stops the product from ever reaching it.
- Do not create a root `Blastfile` for the repository: it is not an app, and nothing boots it.
- Never add a sentence that explains the change to a user. A file that does not parse is a file that
  does not parse.
