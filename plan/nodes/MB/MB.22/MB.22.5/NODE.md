---
id: MB.22.5
title: "Pin the grammar rev, guard editors/**, and prove blastit lsp live in the standalone and console-script paths"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.22.3, MB.22.4]
paths:
  - editors/zed/extension.toml
  - scripts/check_zed_extension.py
  - scripts/test_check_zed_extension.py
  - Makefile
  - python/tests/critical/test_lsp_stdio.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-16T09:16"
updated: "2026-11-17T07:02"
---

# Goal

Make the extension **installable and proven**, not merely present: pin the grammar `rev` to
the lead's commit, add the fast static guard that keeps `editors/**` honest in `make check`,
and prove `blastit lsp` end to end from both distribution paths.

1. **Pin the grammar rev.** `[grammars.blastfile].rev` must be the 40-hex SHA of the lead's
   `MB.22.4` commit (the one that contains `editors/tree-sitter-blastfile`). Replace the
   all-zero placeholder. Prove the commit resolves, the path exists at that rev, and a
   shallow fetch of the rev reproduces the grammar directory. Never a branch name, tag or
   `HEAD`. Record in `RESULT.md` that a public fetch only succeeds after `conductor` pushes
   the release branch; this node proves the pin against the local repo.
2. **Install-build proof.** `cargo build --manifest-path editors/zed/Cargo.toml --target
   wasm32-wasip2` produces the wasm from `editors/zed/target/` (root `target/` untouched);
   the exact Zed clang line builds `tree_sitter_blastfile` from the committed `parser.c`.
3. **Static guard.** Add `scripts/check_zed_extension.py` (stdlib only) and
   `scripts/test_check_zed_extension.py` (negative self-tests), and a `zed-check` target in
   the `Makefile` that `check` runs. The guard asserts: required `extension.toml` keys and
   `schema_version = 1`; `description` longer than `name`; `rev` is 40 lowercase hex;
   `path = "editors/tree-sitter-blastfile"`; grammar key `blastfile`; server
   `languages = ["Blastfile"]` and `language_ids` mapping; `config.toml` name/grammar/
   `path_suffixes` exact; `parser.c`/`tree_sitter/parser.h`/`grammar.json`/
   `node-types.json` present and `tree_sitter_blastfile` exported; every `@capture` in the
   fifteen-name allowlist the design pins (the `@variable.special @variable` pair is the one
   dotted exception);
   `outline.scm` has `@item` + `@name`, `indents.scm` `@indent`, `brackets.scm`
   `@open`/`@close`; empty `[workspace]` and `crate-type = ["cdylib"]`; `LICENSE` resolves
   inside the extension dir; no committed `extension.wasm`, `target/` or `grammars/`.
4. **Live runtime proof.** Drive the built server over real pipes: `initialize` →
   `didOpen` a `Blastfile` whose line `# ✓ ok` precedes `body =` → assert the published
   diagnostic line/UTF-16 column and that its message is the same sentence `blastit check`
   prints for that file → `textDocument/formatting` → `shutdown`/`exit 0`. Repeat against
   the Python-free `target/debug/blastit lsp` built by `make rs-no-python`. Assert stderr
   never contains the document's SQL text.
5. **Protected packaging test.** Add `python/tests/critical/test_lsp_stdio.py`: spawn
   `sys.executable -m blastit lsp` under the `test-critical` staged `PYTHONPATH`, send a
   full handshake, assert the capabilities, one diagnostic, one formatting edit and exit 0;
   a second session with EOF before `shutdown` must exit 1. When `BLASTIT_LSP_COMMAND` is
   set, the same test runs that command instead (a shell-split string), so one protected
   suite proves the standalone binary and the console-script route. This protects the route
   Zed uses when `blastit` is installed through `uv`.
6. **Guard and docs.** `scripts/check_zed_extension.py` (`make zed-check`) is what keeps
   `editors/**` honest, and `tools/guard.py` still passes on the tree — it checks
   big/stray/binary files, not the extension's shape;
   `make docs-unit`, `make docs-check` and the exact `python/tests/test_docs_assets.py`
   command stay green; the documented `.zed/settings.json` line parses.

# Context

- `MB.22.3` and `MB.22.4` are the inputs; `MB.22.2`'s design pins the protocol and the
  testing matrix. Read `docs/design/blastfile-lsp-zed.md` before writing the driver.
- `MB.22.4` left `rev` as the all-zero placeholder because leaf workers do not commit. The
  lead's commit of `MB.22.4` is the SHA this node pins. If that commit does not exist yet,
  mark the node `blocked` with a `plan.py log` note; do not invent a SHA.
- `target/debug/blastit` is the standalone artifact `make rs-no-python` builds. The Python
  route reaches `_blastit.main(["lsp"], …)` through `python/blastit/__main__.py`.
- `tools/guard.py` inspects staged + untracked files, so a generated
  `editors/zed/extension.wasm` or `target/` will fail it if it is not ignored.
- **Language rules:** never mention the retired manifest filename; never describe a PHP
  alternative transport; no new user page in this node.

# Acceptance

```console
$ REV=$(grep -oE '[0-9a-f]{40}' editors/zed/extension.toml | head -1); test ${#REV} -eq 40
$ git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"
$ rm -rf /tmp/opencode/grammar-fetch && git init -q /tmp/opencode/grammar-fetch && git -C /tmp/opencode/grammar-fetch remote add origin "file://$PWD" && { git -C /tmp/opencode/grammar-fetch fetch -q --depth 1 origin "$REV" || git -C /tmp/opencode/grammar-fetch fetch -q origin release/0.2.0; } && git -C /tmp/opencode/grammar-fetch checkout -q FETCH_HEAD && test -f /tmp/opencode/grammar-fetch/editors/tree-sitter-blastfile/src/parser.c
$ make zed-check
$ cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
$ make rs-no-python
$ BLASTIT_LSP_COMMAND="$PWD/target/debug/blastit lsp" uv run --no-project python -m unittest discover -s python/tests/critical -p 'test_lsp_stdio.py' -v
$ make test-critical
$ ! grep -rn '0000000000000000000000000000000000000000' editors/zed
$ uv run --no-project tools/guard.py
$ make docs-unit
$ make docs-check
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ uv run --no-project tools/plan.py validate
```

1. `extension.toml` carries a real 40-hex rev, `git cat-file -e` proves the grammar path at
   that rev, and the shallow-fetch reproduction succeeds; the placeholder string appears
   nowhere (`grep -rn '0000000000000000000000000000000000000000' editors/zed` is empty).
2. `make zed-check` runs both scripts, prints one line per checked property, and its
   negative self-tests pass for: short/branch rev, missing `[workspace]`, missing license,
   `path_suffixes` typo, unknown capture, missing `@item`/`@indent`, committed wasm.
3. The Rust wasm builds under `editors/zed/target/` and the root `target/` mtime is
   unchanged; the grammar wasm builds with the exact clang line.
4. The live session (standalone binary) publishes the documented diagnostic, returns one
   formatting edit, logs nothing containing the fixture's SQL, and exits 0; the EOF session
   exits 1. The Python console-script test does the same under `test-critical`.
5. `guard.py`, `docs-unit`, `docs-check`, the docs-assets command and `plan.py validate`
   are green.

# Notes

- The live driver is evidence, not a second test suite: keep the session in `RESULT.md`
  and put the permanent assertions in `critical_lsp.rs` / the Python critical test.
- Write `check_zed_extension.py` against a root argument so its self-tests can point it at
  crafted temp trees; never read the network.
- The 45-capture allowlist is data copied from the research §5 intersection. If a capture
  is deliberately outside it, the guard must fail until the design is amended, not silently
  pass.
- Do not edit `docs/local.md`, `docs/reference/cli.md` or the design; `MB.22.1`/`MB.22.2`
  own them. If the live proof contradicts the docs, record the finding and open a follow-up.
