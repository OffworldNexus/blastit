---
id: MB.21.3.4
title: "Lossless Blastfile CST, canonical printer and blastit fmt [--check] [PATH]"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.21.3.3]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/tests/critical_blastfile_print.rs"
  - "crates/blastit-manifest/tests/blastfile_fmt.rs"
  - crates/blastit/src/cli.rs
  - crates/blastit/src/main.rs
  - crates/blastit/tests/critical_blastfile_fmt.rs
  - crates/blastit/tests/fmt_command.rs
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:16"
---

# Goal

Add the lossless CST and the canonical printer, and ship `blastit fmt [--check] [PATH]` — the
formatter a developer runs without thinking, idempotent and comment-preserving.

# Context

- Design: `docs/design/blastfile-dsl.md` §14 (the one tree, the canonical layout, P1–P3). The CST
  keeps every byte: tokens, comments, blank lines, the author's choice of `;` vs newline terminator, and the exact spacing the printer normalizes away.
- Two invariants, both tested:
  1. `fmt(fmt(text)) == fmt(text)` (idempotent);
  2. `parse(fmt(text))` equals `parse(text)` (no semantic change) — the model is compared, not the text.
  A file that is already canonical must come back **byte identical** (comments in place, one blank
  line between top-level sections, `;` only where the printer's rule puts it).
- CLI: `blastit fmt [--check] [PATH]`. `PATH` defaults to `./Blastfile`. Without `--check` it rewrites
  in place (atomic: temp file + rename, `0600`/existing mode preserved, same-directory rename only).
  With `--check` it writes nothing, prints one line per file that would change, exits `1`; a parse
  error exits `2` with `file:line:col`. `--help` text and the exit codes are part of the contract.
- Do not print or accept the retired file name here: a body that is not a Blastfile is an ordinary
  `Blastfile:<line>:<col>` syntax error, discovered by `MB.21.4.1`.
- Tests: `crates/blastit-manifest/tests/critical_blastfile_print.rs` (printer invariants + comment
  survival) and `crates/blastit/tests/critical_blastfile_fmt.rs` (CLI exit codes, in-place rewrite,
  `--check` changing nothing on disk).

# Acceptance

```console
$ cargo test -p blastit-manifest --locked --test critical_blastfile_print
$ make check
$ ./target/debug/blastit fmt --check --help
```

1. Idempotence and model-equality hold for every fixture in `crates/blastit-manifest/tests/fixtures/`
   (loop over all of them in the test, not one hand-picked file).
2. A comment on its own line, a trailing comment after a directive, and a comment as the last line
   without a newline all survive `fmt` exactly once.
3. `--check` on a canonical file exits `0` and touches nothing (compare mtime + inode); on a
   non-canonical file exits `1` and still touches nothing; a broken file exits `2`.
4. In-place rewrite is atomic: the replaced file keeps its permissions and a write that fails mid-way
   leaves the original intact (assert with a read-only directory or an injected failure).
5. `make check` exit 0; the `Makefile`/docs references to the new command are left to `MB.21.5.1`.

# Notes

- Add the subcommand in `crates/blastit/src/cli.rs` next to the existing ones and keep `--help` output
  in the documented style; `docs/reference/cli.md` is rewritten by `MB.21.5.1`.
- Fewer files than it sounds: CST + printer + CLI arm + two test files.
