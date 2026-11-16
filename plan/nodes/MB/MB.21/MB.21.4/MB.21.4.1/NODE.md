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
  - crates/blastit/tests/fmt_command.rs
  - crates/blastit/tests/critical_blastfile_fmt.rs
  - "crates/blastit-manifest/src/blastfile/print.rs"
  - "crates/blastit-manifest/tests/blastfile_fmt.rs"
  - "crates/blastit-manifest/tests/critical_blastfile_print.rs"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T08:04"
---

# Goal

Discover `BLASTIT_MANIFEST` else `./Blastfile` at every entry point — `fmt` included — through **one**
function (`config::manifest_path`), name `./Blastfile` when nothing is there, and treat every other
body as an ordinary `Blastfile:<line>:<col>` syntax error. There is no detector, no sniffer, no suffix
test, no "migration" classifier and no second file name anywhere in the product. `blastit fmt`
re-parses the text it is about to write and refuses rather than emitting a file its own reader rejects
(R25), and `cli.rs` stops carrying private copies of `error.rs`'s two folding helpers.

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
- **One discovery function (this leaf's first new law).** `config::manifest_path`
  (`config.rs:2363`) is private, returns `Result<PathBuf, ConfigError>` and is the only place that
  reads `BLASTIT_MANIFEST`, refuses a set-but-empty value and defaults to the working directory —
  but `cli::fmt_path` (`cli.rs:471-509`) re-implements that whole rule a second time. Make
  `manifest_path` `pub(crate)`, have `Config::from_env` (`config.rs:1141`), `stage0`, `check`'s
  `read_manifest` and `fmt_path` all call it, and delete `fmt_path`'s copy. The set-but-empty refusal,
  the `./Blastfile` default and the missing-file sentence then exist exactly once, and `fmt` can never
  disagree with `dev`/`check` about which file it read. Nothing inspects the body to pick a path.
- **`fmt` re-parses its own output (a `MB.21.3.4` MINOR, `R25`).** `blastfile::fmt`
  (`crates/blastit-manifest/src/blastfile/print.rs`) parses, writes and refuses a formatted result
  above `MAX_SOURCE_BYTES`, but never re-parses the formatted text, so a printer regression writes a
  file its own reader refuses — the S11 defect the design accepted against R25. Add
  `parse(&formatted)?` before returning (same budgets, so `MAX_TOKENS`/`MAX_TRIVIA`/depth apply to the
  re-parse) and keep the size refusal. Prove it with `blastfile_fmt.rs::output_reparses` plus a
  printer-regression fixture: the formatted text must equal what `fmt` would print again (idempotence
  is P1/P2's, the re-parse is this one).
- **`cli.rs`'s two duplicated helpers.** `cli.rs:594 one_line_text` re-implements
  `error::{escape_control, clipped}` (both `pub(crate)`: the 120-`char` cut and the control-byte
  escaping), and `cli.rs:590` runs it over a path. Call the `error.rs` helpers instead and delete the
  local function, so §18's "one line, cut at 120 `char`s, control bytes escaped" rule has one
  implementation. (`ManifestError::position()` — which removes the string-splitting around it — is
  `MB.21.4.3`'s; do not add it here.)
- Tests: extend `crates/blastit/tests/critical_config_and_presets.rs` (path precedence + defaults) and
  create `crates/blastit/tests/critical_blastfile_discovery.rs` for the missing-file sentence and the
  syntax error at each entry point (boot, `dev`, `check`, `fmt`). The production stage-0 sentence is
  asserted on **stdout** as a JSON `app.output` event (`LESSONS.md`, the two streams). `fmt`'s
  discovery and its re-parse are pinned in `crates/blastit/tests/fmt_command.rs`,
  `crates/blastit/tests/critical_blastfile_fmt.rs` and
  `crates/blastit-manifest/tests/blastfile_fmt.rs`.

# Acceptance

```console
$ make test-critical
$ make rs-test
$ uv run --no-project tools/critical_guard.py --base HEAD~1 --node MB.21.4.1
$ grep -rnE 'blastit[.]toml|TomlManifest|looks_like_toml|is_toml|strip_suffix\(".toml"\)' crates/blastit/src
$ cargo test -p blastit-manifest --test blastfile_fmt
$ grep -c 'BLASTIT_MANIFEST' crates/blastit/src/cli.rs crates/blastit/src/config.rs
$ grep -n 'parse(&formatted)\|one_line_text' crates/blastit-manifest/src/blastfile/print.rs crates/blastit/src/cli.rs
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
6. `blastit fmt` obeys the same discovery as every other entry point: with no `PATH` and no
   `BLASTIT_MANIFEST` it formats `./Blastfile`, with `BLASTIT_MANIFEST` set it formats that file, a
   set-but-empty variable is the same refusal `dev`/`check` give, and a directory with no file gets the
   one missing-file sentence. `cli.rs` holds no discovery copy (`grep -c 'BLASTIT_MANIFEST'
   crates/blastit/src/cli.rs` is `0`, `.../config.rs` is `≥ 1`) and `grep -n 'fn fmt_path'
   crates/blastit/src/cli.rs` prints nothing.
7. `blastfile::fmt` re-parses before returning: `blastfile_fmt.rs::output_reparses` formats a messy
   fixture, parses the result with the same budgets and gets the same text back, and a
   printer-regression fixture is refused instead of written. `grep -n 'parse(&formatted)'
   crates/blastit-manifest/src/blastfile/print.rs` prints a line, and `grep -n 'one_line_text'
   crates/blastit/src/cli.rs` prints none.

# Notes

- The repository-wide emptiness of the retired name across every crate, `python/`, `e2e/`, `scripts/`
  and `Makefile` is `MB.21.4.4`'s and `MB.21.4.5`'s acceptance, and `MB.21.5.2` makes it permanent. This
  node leaves the discovery and messaging surface clean and lists, in `RESULT.md`, every remaining hit
  it did not own and which sibling owns it.
- `MB.21.3.5`/`MB.21.3.6` are ancestors, so the retired reader still exists in the tree — that is fine;
  it is deleted by `MB.21.4.5`. This leaf only stops the product from ever reaching it.
- The `fmt` work here is the `MB.21.3.4` MINOR, not a new feature: R25's size half already landed
  (`fmt` refuses a result above `MAX_SOURCE_BYTES`), the re-parse half did not, and `fmt`'s discovery
  was a second copy of `config::manifest_path`. Both are refusals and one-function rules, so no page
  sentence changes and `MB.21.5.1` has nothing to rewrite.
- Do not create a root `Blastfile` for the repository: it is not an app, and nothing boots it.
- Never add a sentence that explains the change to a user. A file that does not parse is a file that
  does not parse.
