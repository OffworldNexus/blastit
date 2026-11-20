# Result MB.42.12

## Summary

`blastit-lsp` now answers its own argv instead of ignoring it, without touching the path
`blastit lsp` takes.

* **New entry point** `blastit_lsp::run_binary(&args)`, called by the binary's `main` (which is
  the only place a process argv exists): it answers argv through `answer_argv` first and only
  then calls `run()`. `run()` itself is **byte-identical** — it is shared with `blastit lsp`,
  whose argv belongs to the CLI, so the session code stays argv-blind and a bare run still
  prints nothing on either stream. Keeping `std::env` out of `lib.rs` preserves the shared
  module's invariant that its critical suite asserts (`crates/blastit/tests/critical_lsp.rs`,
  R3: the text tool names no environment).
* **`--version` / `-V`, alone** → one line `blastit-lsp <CARGO_PKG_VERSION>` on **stdout**
  (`version_line()`, the same `env!("CARGO_PKG_VERSION")` the capability document's `serverInfo`
  carries), exit `0`.
* **Any other argument** (`--help` included) → **one** sentence on **stderr** and exit `2`:
  `blastit-lsp is a language server: it takes no arguments and speaks LSP on stdin and stdout; point your editor at it.`
  The node allowed either this or a usage block; this is the *sentence* choice — there is no
  LSP option to list, so a usage block would be an invented surface. `--version --bogus` is also
  a misuse (`2`), not a version answer.
* Testability: the decision lives in `fn answer_argv(&[OsString], &mut impl Write, &mut impl Write) -> Option<u8>`
  (`None` = no argument = serve), unit-tested without a process; the exit codes and the real
  streams are covered by the spawned-binary critical suite.
* **Protected test updated (authorized by `paths: ["crates/blastit-lsp/**"]`)**:
  `tests/critical_lsp_stdio.rs::an_argument_is_ignored_and_end_of_input_exits_one` asserted the
  old bug (`--bogus` → exit `1`, silent). It is replaced by three cases covering the new contract
  — `version_is_one_line_on_stdout_and_exit_zero`, `any_other_argument_is_one_sentence_on_stderr_and_exit_two`,
  `no_argument_is_the_session_and_end_of_input_exits_one_silently` — plus a `run_argv` helper and
  an updated module doc. No other critical assertion was relaxed: the two environment/session
  cases are untouched and still pass.

## Evidence

### Before

```console
BEFORE blastit-lsp --help -> exit=1 stdout=0B stderr=0B
BEFORE blastit-lsp --version -> exit=1 stdout=0B stderr=0B
BEFORE blastit lsp --help -> exit=0 stdout=454B stderr=0B
==> usage: blastit lsp
```

### After

```console
AFTER blastit-lsp --help -> exit=2 stdout=0B stderr=117B
  stderr: blastit-lsp is a language server: it takes no arguments and speaks LSP on stdin and stdout; point your editor at it.
AFTER blastit-lsp --version -> exit=0 stdout=18B stderr=0B
  stdout: blastit-lsp 0.2.0
AFTER blastit-lsp (no args) -> exit=1 stdout=0B stderr=0B
AFTER blastit lsp --help -> exit=0 stdout=454B stderr=0B
==> usage: blastit lsp
```

### `cargo test -p blastit-lsp --locked`

```console
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/critical_lsp_stdio.rs
test no_argument_is_the_session_and_end_of_input_exits_one_silently ... ok
test a_poisoned_environment_changes_nothing ... ok
test the_standalone_binary_speaks_the_same_session ... ok
test version_is_one_line_on_stdout_and_exit_zero ... ok
test any_other_argument_is_one_sentence_on_stderr_and_exit_two ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### `cargo clippy -p blastit-lsp --all-targets --locked -- -D warnings`

```console
    Checking blastit-lsp v0.2.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.41s
```

### Gates

```console
$ make zed-check      # PASS
$ make fast-check     # PASS (guard, critical-guard, rs-fmt, rs-lint, rs-doc, rs-test,
                      #       py-lint, workflow-check, plan-validate, universal-smoke)
```

## Follow-ups

* `docs/reference/cli.md` documents `blastit lsp`; the standalone binary's `--version`/misuse
  stderr sentence is a packaging surface worth one line there (outside this node's `paths`).
