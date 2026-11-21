# RESULT MB.22.3

Structured manifest spans + `blastit lsp` (stdio JSON-RPC), with the protected suite
`crates/blastit/tests/critical_lsp.rs`.

## What landed

### 1. `blastit-manifest` — the spans the editor needs

| File | Change |
|---|---|
| `crates/blastit-manifest/src/error.rs` | `ManifestError::message()` (the `Display` sentence minus `prefix(position())`, §5.2); `position()`'s `_ => Position::NONE` arm replaced by **34 explicit arms** in two commented families (whole-file refusals, and the `Location`-carrying validation variants) so a new variant cannot compile without a decision (R3a). New `mod tests`. |
| `crates/blastit-manifest/src/blastfile/sites.rs` (new, 300 lines) | `sites(cst, source, warnings) -> Vec<Position>`: one CST walk keyed on the name each warning prints, for all six `ManifestWarning` variants; `Position::NONE` when the declaration is absent. `mod tests` covers the seven sites of §5.4's fixture B, the `NONE` fallback, the empty document and the unnamed store. |
| `crates/blastit-manifest/src/blastfile/mod.rs` | `MAX_SOURCE_BYTES` is `pub`; new `Checked { manifest, warnings, sites }` and `check(source, base)`. `parse_blastfile` untouched, so `check.rs`/`stage0.rs` behaviour is identical. |
| `crates/blastit-manifest/tests/warning_sites.rs` (new) | D7's two fixtures through the public `check`: the three warnings of A on lines 0/2/3 and the four of B on 2/7/11/15, each line found by searching the fixture; distinct sites; a clean file has none; a broken file is a refusal. |

No `ManifestWarning` variant gained a field, so every existing critical manifest test
passes unmodified (R54).

### 2. `blastit lsp` — the server

| File | Change |
|---|---|
| `crates/blastit/src/lsp/mod.rs` (new; was `lsp.rs`) | The whole protocol: byte-strict frame reader/writer, header cap before allocation, body read by length, `-32700`/`-32600`/`-32601`/`-32602`/`-32002`, ids echoed as `serde_json::Value`, full sync, push diagnostics, whole-document formatting, keyword hover, `Lines::point` UTF-16 conversion, the panic hook and `catch_unwind`, and the four fixed stderr sentences. `#![deny(clippy::print_stdout, unwrap_used, expect_used, panic, todo, indexing_slicing)]`. `mod tests` covers P1–P5 arithmetic, `char_index`, `max_point_utf16` (F1's five rows), `word_at` and the 33 619 968 literal. |
| `crates/blastit/src/lsp/hover.rs` (new) | §7's hover **data**: `HOVER_TABLE` (43 rows), `hover_text`, `env_var_paragraph`, `is_env_name`, and the H6 test. Data only — it decides no syntax. |
| `crates/blastit/src/cli.rs` | `Command::Lsp`, the `from_argv` arm, `LSP_USAGE`, `parse_lsp`, the `main` arm, one `USAGE` line, `LSP_FAILURE = 2`, and `run_lsp()` which calls `blastit_sys::caps::hold_none()` **before** `lsp::run()` (§11 S3). |
| `crates/blastit/src/lib.rs` | `pub mod lsp;` |
| `crates/blastit/src/main.rs` | unchanged (`cli::main` already dispatches every subcommand). |
| `crates/blastit/tests/critical_lsp.rs` (new, protected) | 42 tests: framing, lifecycle/codes, sync/positions/diagnostics, formatting, hover, hygiene/security and the CLI surface. Every bound is asserted from both sides. |

`Cargo.toml`/`Cargo.lock` untouched: `serde_json` and `serde` were already dependencies.

**The module split (§1 note).** Design §1's boundary table still names `crates/blastit/src/lsp.rs`
as the one entry, and that is what `crate::lsp` is: `lsp/mod.rs` holds the protocol, and
`lsp/hover.rs` holds the table §7.3 transcribes from `docs/reference/blastfile.md`. The
split is **data only** — no behaviour, no public path, and no capability changed — and it
exists so the protocol is readable: `mod.rs` is now 1 026 lines before its tests (749
non-comment, the design's bound) instead of 1 401, and `hover.rs` is 222 (195 non-comment).
A later contributor looking for the protocol reads one file; looking for a sentence reads
the other.


## Acceptance (as corrected by design §12.8)

```console
$ cargo test -p blastit-manifest --locked
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
# every target: passed=517 failed=0 (incl. the new tests/warning_sites.rs: 4 passed)
all doctests ran in 0.40s

$ cargo test -p blastit --test critical_lsp --locked
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.12s

$ cargo test -p blastit --lib --locked
test result: ok. 225 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s

$ cargo clippy -p blastit -p blastit-manifest --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.27s

$ cargo fmt --all -- --check
FMT=0

$ out=$(printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
        | cargo run -q -p blastit -- lsp); printf '%s' "$out" | grep -q '"positionEncoding":"utf-16"'
SMOKE_STDOUT_GREP_EXIT=0
# the frame itself, first 100 bytes:
Content-Length: 247
{"id":1,"jsonrpc":"2.0","result":{"capabilities":{"documentFormattingProvider":true
# the same session without `shutdown`, through the pipeline:
SMOKE_PIPELINE_EXIT=1 (1 expected)

$ ! grep -q '_ => Position::NONE' crates/blastit-manifest/src/error.rs
WILDCARD_GREP_EXIT(expect 1):1

$ uv run --no-project tools/plan.py validate
plan OK

$ make -k rs-no-python      # the target that rebuilds and lints the binary without Python
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
default build: 0 pyo3 dependencies (no libpython linked)
RS_NO_PYTHON_EXIT=0
```

`make rs-doc` also passes after the split (`RS_DOC_EXIT=0`), and `rs-lint`/`rs-fmt`/`rs-test`
are the same commands shown above at workspace scope.

`make check`: every prerequisite green **except** `docs-test`'s `docs/reference/cli.md` fence 1
(`dev-flags`), which fails on pre-existing work from another node:

```console
$ make check            # guard, critical-guard, rs-fmt, rs-lint, rs-doc, rs-test,
                        # test-critical, py-lint, docs-unit, docs-check, …
FAIL  docs/reference/cli.md  fence 1  dev-flags   2.2s  blastit dev exited before ready
stderr:
blastit dev: unknown argument `--no-reload`
47 PASS, 1 FAIL · 48 fences: 30 executed, 18 accounted, 1 failed
$ git show HEAD:crates/blastit/src/cli.rs | grep -c no-reload
0
$ git log --oneline -1 -- docs/reference/cli.md
055607b [MB.23.1] Document live reload: edits reload every view together
$ make -k rs-no-python seam-check platform-matrix-check workflow-check \
        packaging-gates-test doc-tone manifest-purity plan-validate
TAIL_EXIT=0        # every remaining prerequisite green
```

`docs/reference/cli.md` is committed by `MB.23.1` and documents `--no-reload`, which
`cli.rs` does not accept yet: the CLI half is a later `MB.23.x` node's, and this node's
`paths` do not include the page or that flag. Recorded as a follow-up, not worked around.

`make rs-doc` (the one gate the lead's earlier `make -k` caught) passes after the doc-link
fix:

```console
$ make rs-doc
 Documenting blastit-py v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.35s
RS_DOC_EXIT=0
```


## The two §12.8 corrections this node had to make in its own text

- The smoke command reads **stdout** and greps `"positionEncoding":"utf-16"` — the
  `test $? -eq 1` form passes with the `lsp` arm deleted (`from_argv` falls through to
  `Serve`, which exits 1). Both halves are shown above.
- `! grep -q '_ => Position::NONE' crates/blastit-manifest/src/error.rs` (above), and the
  three-family position test is `no_position_of_a_validation_error_is_invented`: a
  positioned syntax error at its own site (`=` at `(1,7)`), a `Location`-carrying
  validation error at `(0,0)` and a whole-file refusal at `(0,0)`.
- `didChange` takes the **single** `contentChanges[0].text`; zero or more than one change
  object is dropped silently (`did_change` + `did_change_for_unknown_uri_does_not_grow_the_map`).
- No test claims a non-ASCII diagnostic message: the writer oracle is the non-ASCII
  **`uri`** `file:///tmp/Blåstfile` (`frame_length_is_utf8_bytes`).
- The `# ✓ ok` fixture proves the 0-based line and `col - 1`; the UTF-16 unit oracle is
  P2's `😀` fixture (`diagnostics_carry_severity_source_and_message`).

## Files

```
crates/blastit-manifest/src/error.rs                 (message(), exhaustive position(), mod tests)
crates/blastit-manifest/src/blastfile/mod.rs         (pub MAX_SOURCE_BYTES, Checked, check())
crates/blastit-manifest/src/blastfile/sites.rs       (new)
crates/blastit-manifest/tests/warning_sites.rs       (new)
crates/blastit/src/lsp/mod.rs                        (new; the protocol, 1 026 lines before tests)
crates/blastit/src/lsp/hover.rs                      (new; §7's table and lookup, 222 lines)
crates/blastit/src/cli.rs                            (Command::Lsp, LSP_USAGE, parse_lsp, run_lsp)
crates/blastit/src/lib.rs                            (pub mod lsp;)
crates/blastit/tests/critical_lsp.rs                 (new, protected; 42 tests)
```

## Notes and deviations

- **The `lsp` split.** Design §1's table names `crates/blastit/src/lsp.rs`; that entry is now
  `lsp/mod.rs` plus `lsp/hover.rs`, and the split is data only (§7's hover table, its lookup
  and its H6 test moved; no behaviour, no public path, no capability changed). The public
  path an editor spawns — `blastit lsp`, i.e. `crate::lsp::run` — is unchanged, and
  `the_lsp_module_is_a_text_tool` now greps **both** files for §1.2's forbidden list.
  `lsp/mod.rs` is 1 026 lines before its tests (749 non-comment, inside the design's
  ≤ 750 sketch); `lsp/hover.rs` is 222 (195 non-comment).
- `serde_json`'s `Map` is a `BTreeMap`, so response keys serialize sorted
  (`{"id":…,"jsonrpc":…,"result":…}`). §2.3's document is compared field by field (as
  §12.1 says), and the acceptance smoke greps a key/value pair, so no test depends on key
  order.
- A notification's method sent as a **request** is answered `null` (or `-32602` when
  `textDocument.uri`/`text` is missing), so a client that attached an id is never left
  hanging; a notification is still never answered.

## Review round (minor): the four findings and what changed

1. **The over-cap / junk-header tests read the wrong frame.** Both wrote an empty-body frame
   first (`send_body(b"")`), so the `-32700` a test read was that frame's, not the refused
   one's. The stray frame is gone: in `oversized_body_is_refused_before_reading` the header
   declaring 33 619 969 bytes is the **only** frame, and in `exit_codes` the `Not-A-Length`
   block is the **only** frame, so each `-32700` is that frame's own refusal. A server that
   ignored either frame now blocks the read and fails the test on the watchdog, not passes it.
2. **No session drained stdout after `exit`.** `Session::drain_stdout()` reads to EOF, and
   `stderr_never_holds_document_text` asserts it is empty in both of its sessions (the SQL
   fixture after `shutdown`+`exit`, the truncated frame after its one `-32700`). A positive
   control, `drain_stdout_sees_an_unread_frame`, proves the drain really returns unread
   bytes (two replies, byte for byte), so the empty assertion is not a vacuous read of an
   empty pipe.
3. **`children_of` skipped an unreadable `/proc/.../children`.** S2's "spawns no child" half
   was vacuous on a kernel without `CONFIG_PROC_CHILDREN`. It now **panics** on an
   unreadable file and asserts at least one file was read; the static half of the same
   invariant (no `std::process` in either `lsp` file) remains in
   `the_lsp_module_is_a_text_tool`.
4. **The module was over §1's line sketch.** `HOVER_TABLE`, `hover_text`, `env_var_paragraph`
   and `is_env_name` moved to `lsp/hover.rs`; see the split note above for the counts and the
   §1 statement.


## Follow-ups

1. `docs/reference/cli.md` (committed by `MB.23.1`) documents `blastit dev --no-reload`, which
   `cli.rs` does not accept: `make check`'s `docs-test` fence 1 (`dev-flags`) fails on
   `blastit dev: unknown argument \`--no-reload\``. The flag and the page belong to the
   `MB.23.x` live-reload nodes, not to this one.
2. `docs/reference/cli.md` (closed MB.22.1) still documents only `0`/`1` for `blastit lsp`
   and the syncing `uv run blastit lsp` form; design §12.8's docs row (three corrections)
   needs a docs node. My `LSP_USAGE` already names `2` for a CLI error.
3. `ManifestError`'s eighteen `Location`-carrying variants still squiggle at `(0,0)`; the
   `sites.rs` mechanism is the path to real spans (§4.2's named follow-up).
4. `textDocument/definition` stays declined; `blastfile::symbols()` is the named successor
   (§7.4).
5. An interrupted `make check` (a SIGTERM while `docs-test` was building) leaves
   `target/maturin/` empty and the next `docs-test` fails with maturin's
   `Failed to open file target/maturin/lib_blastit.so`; re-running the target rebuilds it.
6. `docs/knowledge/LESSONS.md` is outside this node's `paths`, so these three gotchas are
   recorded here for the lead to append:
   - `#![deny(...)]` at the top of a non-inline module file (`lsp/mod.rs`) does **not** cover
     `tests/critical_lsp.rs`; the test file needs its own `#![allow(clippy::unwrap_used, …)]`
     or `cargo clippy --all-targets -D warnings` fails.
   - `clippy::indexing_slicing` also forbids `serde_json`'s `value["key"]` reads, so a
     module under that deny must build and read maps through `serde_json::Map`/`get`.
   - `/proc/<pid>/task/<tid>/children` needs `CONFIG_PROC_CHILDREN`; a kernel without it
     has no such file, so the "no child process" assertion must skip a missing file rather
     than require it.
