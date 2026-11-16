---
id: MB.21.4.1
title: "Cut every entry point over to ./Blastfile in one atomic commit: one discovery function, Blastfile parsing, the §15.1 sentences and exit codes, fmt re-parse, and every fixture make check reaches"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.21.3.6]
paths:
  - crates/blastit/src/config.rs
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/cli.rs
  - crates/blastit/src/check.rs
  - crates/blastit/src/dev_learn.rs
  - crates/blastit/src/audit_v1.rs
  - crates/blastit/src/php_view.rs
  - "crates/blastit/tests/**"
  - "crates/blastit-py/**"
  - "crates/blastit-manifest/src/**"
  - "crates/blastit-manifest/tests/**"
  - "crates/blastit-scheduler/src/**"
  - "crates/blastit-scheduler/tests/**"
  - "python/tests/runnable_docs/**"
  - python/tests/critical/test_check_contract.py
  - docs/local.md
attempts: 5
max_attempts: 6
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T10:10"
---

# Goal

Cut the running product over to `Blastfile` in **one atomic commit**: every entry point reads through
**one** discovery function and parses with `Manifest::parse_blastfile`, the §15.1 sentences and exit
codes hold at boot, `dev`, `check` and `fmt`, `blastit fmt` re-parses the text it is about to write, and
`cli.rs` folds errors through `error.rs`'s helpers instead of its own copies. The same commit translates
every fixture `make check` reaches, because the flip and the fixture translation cannot be green apart.

There is no detector, no sniffer, no suffix test, no "migration" classifier and no second file name
anywhere in the product. A body that is not a `Blastfile` is an ordinary `Blastfile:<line>:<col>` syntax
error, whatever it contains.

# Context

- Design §15/§15.1 and `ADR-0002` are authoritative. Precedence is unchanged: `BLASTIT_MANIFEST` else
  `./Blastfile` in the starting working directory; a bare name means the working directory; nothing walks
  upwards.
- **Why this leaf is atomic (measured at attempt 1, do not re-derive).** The product still calls
  `Manifest::parse_and_validate` in `config.rs`, `stage0.rs` and `check.rs`, so a v1 body still boots. The
  flip turns red every fixture that boots the binary: nine suites under `crates/blastit/tests/`
  (`smoke.rs`, `critical_php_embed.rs`, `critical_config_and_presets.rs`, `critical_dev_and_fail_closed.rs`,
  `critical_raw_ingress.rs`, `critical_static_cli.rs`, `no_python_build.rs`, `dev_signal_reap.rs`,
  `critical_php_backend.rs`) and the six fixtures under `python/tests/runnable_docs/fixtures/`
  (`hello`, `shop`, `learn`, `denial`, `findings`, `static-site`). `Blastfile` parsing already exists as
  `Manifest::parse_blastfile` / `Manifest::from_blastfile_str`, so the cutover is confined to
  `crates/blastit/src/{config,stage0,check,cli}.rs` plus those fixtures. Translation and flip land
  together or neither is green.
- **One discovery function (first law).** `config::manifest_path` (`config.rs:2363`) is private, returns
  `Result<PathBuf, ConfigError>` and is the only place that reads `BLASTIT_MANIFEST`, refuses a
  set-but-empty value and defaults to the working directory — but `cli::fmt_path` (`cli.rs:471-509`)
  re-implements the whole rule a second time. Make `manifest_path` `pub(crate)`, have `Config::from_env`
  (`config.rs:1141`), `stage0`, `check`'s `read_manifest` and `fmt_path` all call it, and delete
  `fmt_path`'s copy. The default, the empty-variable refusal and the missing-file sentence then exist
  exactly once, and `fmt` can never disagree with `dev`/`check` about which file it read. Nothing
  inspects the body to pick a path.
- **The parse cutover.** Replace `Manifest::parse_and_validate`/`from_toml_str` at every product entry
  point with `Manifest::parse_blastfile`/`from_blastfile_str`: `Config::from_env`'s `manifest_wiring`,
  `stage0::run`/`stage0::run_dev`, and `check`'s `read_manifest` (`check.rs:294-300`). Keep each site's
  existing rendering and exit code; only the parser and the nouns change. `--manifest` keeps its refusal
  at all three parse sites (`parse_dev`, `parse_check`, `parse_fmt`) and is reworded to name `Blastfile`
  plus `BLASTIT_MANIFEST`.
- **Sentences and exit codes are §15.1's, verbatim** (the design wins over any other wording):
  * missing file, every entry point: `./Blastfile: no such file or directory (put a Blastfile in the working directory, or set BLASTIT_MANIFEST)`
    — `dev` on fd 2 and production stage 0 inside the `app.output` event on fd 1 (`LESSONS.md`, the two
    streams);
  * `BLASTIT_MANIFEST` set and empty: `BLASTIT_MANIFEST must not be empty: put ./Blastfile in the working directory or name a file`;
  * exit codes: `1` for boot/`dev`, `2` for `check` **and `fmt`** (R19; `fmt`'s contract is `0`/`1`/`2`,
    `1` being `--check` not canonical). `check::read_manifest`'s own second missing-file sentence goes.
- **`fmt` re-parses its own output (R25, the `MB.21.3.4` MINOR).** `blastfile::fmt`
  (`crates/blastit-manifest/src/blastfile/print.rs`) parses, prints and refuses a result above
  `MAX_SOURCE_BYTES`, but never re-parses the formatted text, so a printer regression writes a file its
  own reader refuses. Add `parse(&formatted)?` before returning (same budgets, so `MAX_TOKENS`/
  `MAX_TRIVIA`/depth apply to the re-parse) and keep the size refusal. Idempotence (P1/P2) is `MB.21.3.4`'s;
  this is only the re-parse.
- **`cli.rs`'s two duplicated helpers.** `cli.rs:594 one_line_text` re-implements
  `error::{escape_control, clipped}` (the 120-`char` cut and the control-byte escaping), and `cli.rs:590`
  runs it over a path. `crates/blastit-manifest/src/lib.rs` declares `mod error;` privately and both
  helpers are `pub(crate)`, so `cli.rs` cannot name them today: make them `pub` in `error.rs` and
  re-export them from `lib.rs`, then call them and delete `one_line_text`, so §18's "one line, cut at
  120 `char`s, control bytes escaped" has one implementation. (`ManifestError::position()` is
  `MB.21.4.3`'s; do not add it here.)
- **Every fixture `make check` reaches becomes a `Blastfile`** — this is the half the old brief left to a
  sibling and it is why attempt 1 could not land:
  * the nine `crates/blastit/tests/` suites named above: each fixture is created in a temp directory; write
    it as `Blastfile` and keep every assertion, including the `BLASTIT_MANIFEST` ones (`smoke.rs`,
    `dev_signal_reap.rs`), whose value now names a `Blastfile`;
  * the six `python/tests/runnable_docs/fixtures/` directories: rename the file in place and translate its
    body (`docs/reference/blastfile.md` is the truth; `MB.21`'s mapping table is the checklist);
  * `python/tests/runnable_docs/harness.py`: `prepare` (line ~714) stages `Blastfile` (and `_init_git`'s
    `git add`), `READER_FILES` (line ~2768) names `Blastfile`, `mode_page_manifest`/`_page_manifest`
    (line ~1222) stage the page's block as a `Blastfile` and **assert it textually** through the Blastfile
    parser instead of `tomllib.loads` (the `products` view still grants nothing, so the `sql.denied`
    `42501` promise stands), and `mode_dev_learn` (line ~1136) asserts the learned `view` and its
    `query '…'` line textually instead of `tomllib.loads`. `python/tests/runnable_docs/test_harness.py`
    follows where it pins those names. No reason string, mode or assertion is weakened.
- **`docs/local.md`'s one executed block.** Its complete `toml title=…` manifest block (lines 46-60) is the only
  page text `make docs-test` executes, so it becomes a `text title="Blastfile"` block holding an
  equivalent `Blastfile`, which moves that page's row in `crates/blastit-manifest/tests/docs_fences.rs`
  (`PAGES`: `[0, 0, 1]` → `[1, 0, 0]`) — that one row is this leaf's, the rest of the page (its prose, the
  `[[route]]` fragment at line 125, and the other eight pages) is `MB.21.5.1`'s. `THE_RETIRED_OPENING_IS_GONE`
  stays `false`.
- Discovery and the fmt re-parse are pinned by `crates/blastit/tests/{blastfile_discovery, fmt_command,
  critical_blastfile_fmt}.rs`, `crates/blastit/tests/critical_config_and_presets.rs` and
  `crates/blastit-manifest/tests/{blastfile_fmt, critical_blastfile_print}.rs`.

# Acceptance

```console
$ make test-critical
$ make rs-test
$ make docs-test
$ make check
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.1
$ grep -rnE 'blastit[.]toml|TomlManifest|looks_like_toml|is_toml|strip_suffix\("\.toml"\)' crates/blastit/src crates/blastit/tests python/tests/runnable_docs --include='*.rs' --include='*.py' --include='*.toml'
$ grep -c 'BLASTIT_MANIFEST' crates/blastit/src/cli.rs crates/blastit/src/config.rs
$ grep -n 'fn fmt_path\|one_line_text' crates/blastit/src/cli.rs
$ grep -n 'parse(&formatted)' crates/blastit-manifest/src/blastfile/print.rs
```

1. `BLASTIT_MANIFEST=other-blastfile blastit check` reads that file; with no variable, `./Blastfile` is
   read; a fixture with a `Blastfile` in a parent directory proves nothing above the working directory is
   read; a set-but-empty variable prints §15.1's sentence and exits `1` (`dev`/boot), `2` (`check`, `fmt`).
2. In an empty directory every entry point prints exactly
   `./Blastfile: no such file or directory (put a Blastfile in the working directory, or set BLASTIT_MANIFEST)`
   — `dev` on fd 2 and the production boot inside the `app.output` event on fd 1 — and no other sentence
   and no second file name.
3. A body that is not a Blastfile — a `version = 1` line followed by a `[[route]]` table included — fails
   at boot, `dev`, `check` and `fmt` with the ordinary `Blastfile:<line>:<col>` syntax error naming the
   file and the position: boot/`dev` exit `1`, `check`/`fmt` exit `2`. No detection, no format name, no
   second sentence.
4. The first `grep` prints nothing: no retired file-name default, fixture, harness stage or reader path
   in `crates/blastit/src`, `crates/blastit/tests` or `python/tests/runnable_docs`, and no
   `TomlManifest`/`is_toml`/`.toml` suffix test.
5. `blastit fmt` obeys the same discovery: no `PATH` and no `BLASTIT_MANIFEST` formats `./Blastfile`,
   `BLASTIT_MANIFEST` formats that path, and a directory with no file gets the one missing-file sentence
   with exit `2`. `cli.rs` holds no discovery copy (`grep -c 'BLASTIT_MANIFEST' crates/blastit/src/cli.rs`
   is `0`, `.../config.rs` is `≥ 1`) and the `grep -n 'fn fmt_path\|one_line_text'` prints nothing.
6. `blastfile::fmt` re-parses before returning: `blastfile_fmt.rs::output_reparses` formats a messy
   fixture, parses the result with the same budgets and gets the same document, a printer-regression
   fixture is refused instead of written, and `grep -n 'parse(&formatted)'` prints a line.
7. `make test-critical`, `make rs-test`, `make docs-test` and `make check` exit 0 — `docs-test` included,
   with `docs/local.md`'s migrated block executing through the real binary and the six translated fixtures
   green — and `critical_guard.py --node MB.21.4.1` is clean (no assertion weakened, every changed
   fixture listed in `RESULT.md`).
8. The nine crate suites and the six runnable-docs fixtures boot from a `Blastfile`; `RESULT.md` lists each
   rewritten fixture, each changed sentence and any retired-name hit it did not own with the sibling that
   does.

# Notes

- `docs/local.md`'s block is migrated here **only because the harness executes it**: every other mode runs
  a fixture, so no other page breaks. The rest of that page — its prose, its `[[route]]` fragment,
  its `git diff` line — stays `MB.21.5.1`'s, and no other page is edited.
- `MB.21.4.2` no longer translates fixtures: parity proof and §4.3's removals are its work. The retired
  reader still exists in the tree until `MB.21.4.5` (`MB.21.3.5`/`MB.21.3.6` are ancestors, so it must
  compile); this leaf only stops the product from ever reaching it.
- The repository-wide emptiness of the retired name across `e2e/`, `scripts/`, `Makefile` and the last
  Rust paths is `MB.21.4.4`'s and `MB.21.4.5`'s acceptance; `MB.21.5.2` makes it permanent. A hit outside
  this leaf's `paths` goes in `RESULT.md` with the sibling that owns it.
- Do not create a root `Blastfile` for the repository: it is not an app, and nothing boots it.
- Never add a sentence that explains the change to a user. A file that does not parse is a file that does
  not parse.
