# MB.40.5 — CLI visual & UX craft overhaul

Base commit `1fde3df`. Groups 1 and 2 delivered; group 3 is **blocked** by protected
critical tests that pin the exact sentences the node asks to change (see
[Blocked](#blocked-group-3)); group 4 was not started, so the node is not finished.

Status: `needs_split` (groups 1–2 done, group 3 blocked, group 4 untouched).

---

## Group 1 — `--help`, `--version`, `help`, preset validation, exit codes

**What changed**

- `blastit --version` / `blastit -V` print `blastit 0.2.0` on stdout and exit `0` —
  **only when the command line carries nothing else**, so `blastit --preset nope -V`
  still names the typo (`2`) instead of printing a version and exiting `0`.
- `blastit help [dev|check|fmt|lsp]` prints the same text `--help` prints. `blastit`
  and `blastit help blastit` are the top-level text; an unknown word — including
  `serve`, which is not a command — is `unknown subcommand 'x' — run 'blastit --help'`,
  exit `2`.
- `USAGE` no longer lists the re-exec grammar (`--stage=1`, `--role=dbproxy`,
  `--config-fd`, `--secrets-fd`, …). Every one of those tokens is still parsed — the
  unit tests for the stage-1 and dbproxy command lines still pass.
- The `usage:` / `presets` headers are bold/cyan **for the stream the text is written
  to**: stdout for `--help`, stderr for a refusal. `usage(coloured: bool)` is the one
  renderer and `refuse(...)` builds its own copy, so `blastit >out.txt` can no longer
  put escape codes into a redirected `err.txt`.
- `--preset` and `BLASTIT_PRESET` are resolved by **one** function
  (`config::preset_unknown`, re-exported; `cli/preset.rs` calls it) and validated by
  `parse` before stage 0 reads capabilities. `preset_flags_are_known` and its second
  scanner are gone; `--preset nope -V`, `--preset=nope` and `--preset -x` all follow
  the same arm.
- Exit statuses are typed: `Refusal { message, exit }`. A value the grammar cannot
  read (`--stage=x`, `--seccomp=maybe`, `--dev-root=…` on a stage-0 line) is a usage
  error (`2`), like an unknown flag; only the genuine re-exec lines (`--stage=1 …`,
  `--role=…`) keep `1`, which a protected test pins.
- The Python wheel CLI is `blastit.__main__.main` → `blastit._blastit.main`, i.e. the
  same Rust `cli::main`, so it inherits every line above with no Python-side parser to
  drift (`python/blastit/__main__.py` unchanged by design).
- `crates/blastit/src/cli.rs` is now the dispatcher only (424 prod lines; it keeps
  `run_lsp` because a protected test reads this file and asserts the capability
  check precedes the session); the words live in `cli/help.rs` (187), the grammar in
  `cli/parse.rs` (494), the preset resolver in `cli/preset.rs` (60) and the
  delegations in `cli/run.rs` (141).

**Before** (base commit, `audit-ux.md` §A3.1/§A3.3/§A10.4 and the old test)

```
$ blastit --version
blastit: `--version` is not `--key=value` … exit=1      # plus 9 capability JSON lines
$ blastit --help                                        # internals sold as features
  blastit --stage=1 --config-fd=N \
          [--dbproxy-pid=P …]
  blastit --role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0|1]
$ blastit --preset nope                                 # silently ignored, exit=1
```

**After (captured from `target/debug/blastit`)**

```
$ blastit --version
blastit 0.2.0
exit=0
$ blastit --help
usage:
  blastit [--preset NAME]                    production: stage 0, the real environment
  blastit dev     [--port N] [--host ADDR] [--learn] [--no-reload]
                                             unprivileged single-UID simulation (no sudo)
  blastit check   [--json] [--learn [--dry-run]] [-- ARGS...]
                                             audit a pytest suite (alias: test)
  blastit fmt     [--check] [PATH]           rewrite a Blastfile in the canonical layout
  blastit lsp                                speak LSP on stdin/stdout (editors)
  blastit help [dev|check|fmt|lsp]           print one command's help

  -h, --help      print this help
  -V, --version   print the version
$ blastit help wat
blastit: unknown subcommand 'wat' — run 'blastit --help'
exit=2
$ blastit help serve
blastit: unknown subcommand 'serve' — run 'blastit --help'
exit=2
$ blastit dev --wat
blastit dev: unknown argument `--wat` — run 'blastit dev --help'
exit=2
$ blastit check --wat
blastit check: unknown argument `--wat` — run 'blastit check --help'
$ blastit fmt --write
blastit fmt: unknown argument `--write` — run 'blastit fmt --help'
$ blastit lsp --bogus
blastit lsp: `--bogus` is not an argument: `blastit lsp` speaks LSP on stdin/stdout and reads no file — run 'blastit lsp --help'
```

Presets — one sentence, two spellings, checked before the capability gate:

```
$ blastit --preset nope
blastit: --preset: unknown preset `nope`: the known presets are digitalocean-paas: …; compose-traefik: …
usage:
…
exit=2
$ blastit --preset nope -V
blastit: --preset: unknown preset `nope`: the known presets are …compose-traefik: …
usage:
…
exit=2                              # the typo is named; `-V` is not answered
$ BLASTIT_PRESET=nope blastit
blastit: BLASTIT_PRESET: unknown preset `nope`: the known presets are …
exit=2
```

Exit statuses, captured one by one:

```
--preset nope            exit=2
--preset nope -V         exit=2   (names the typo, not `-V`)
--preset=no-pe           exit=2
--wat=1                  exit=2
wat                      exit=2
--stage=x                exit=2   (`--stage` wants a whole number, not `x`)
--seccomp=maybe          exit=2   (`--seccomp` wants 0 or 1, not `maybe`)
--dev-root=/tmp/x        exit=2   (`--dev-*` arguments are only for `--stage=1`)
--stage=1 --preset x     exit=1   (protected matrix: internal re-exec grammar)
dev --manifest x         exit=1   (protected matrix: `dev`'s own grammar)
dev --port 70000         exit=1
PORT=70000 dev --learn   exit=1
BLASTIT_PRESET=nope      exit=2
```

Colour, on a real pty (`script -qec`) versus piped, and a refusal with a redirected
stderr:

```
$ script -qec 'blastit --help' /dev/null | head -1 | cat -v
^[[1m^[[36musage:^[[0m
$ blastit --help | head -1 | cat -v
usage:
$ NO_COLOR=1 script -qec 'blastit --help' /dev/null | head -1 | cat -v
usage:
$ script -qec 'blastit wat 2>/tmp/opencode/err.txt' /dev/null >/dev/null
$ grep -c $'\033' /tmp/opencode/err.txt
0                                   # stderr was a file, so no escapes in it
```

**Tests touched**

- `crates/blastit/tests/critical_config_and_presets.rs::top_level_help_lists_the_three_commands_and_the_presets`
  — authorised by the node: `--version`/`-V` = stdout `blastit 0.2.0`, exit `0`; the
  re-exec flags are hidden; `--preset nope -V` exits `2` naming the presets;
  `help dev` / `help wat`; both preset-typo spellings exit `2` naming
  `digitalocean-paas` and `compose-traefik`.
- `crates/blastit/src/cli.rs` unit tests: `the_usage_hides_the_reexec_grammar_and_offers_version_and_help`
  (now also asserts the coloured form strips back to the plain one),
  `help_names_every_command_and_refuses_an_unknown_one` (adds `blastit` and `serve`),
  `a_stage_zero_preset_typo_is_a_usage_error_naming_the_presets` (new shape, through
  `parse` only), `a_value_this_build_cannot_read_is_a_usage_error` (new),
  `the_dev_only_port_flag_is_answered_with_the_two_places_it_belongs` (exit + message).

## Group 2 — per-stream `IsTerminal` + `NO_COLOR` / `TERM=dumb`, one DEBUG palette

**What changed**

- `crates/blastit/src/logs.rs` no longer reads the environment or a descriptor. It
  owns the typed decision: `Colour { stdout, stderr }` plus the pure
  `Colour::new(stdout_terminal, stderr_terminal, no_color, term)`; `Emitter::stdout`
  takes the `Colour`.
- `crates/blastit/src/term.rs` is the single reader: `term::colour()` asks
  `std::io::IsTerminal` on stdout and stderr separately and reads `NO_COLOR` /
  `TERM` once. It is called at each image's boot — `cli::main` for the command line
  (and threaded into stage 0, stage 1 and every emitter it builds), never again.
- `crates/blastit-events/src/pretty.rs`: DEBUG is dim (`ESC[2m`), the code the
  logger's field-dump fallback already used, instead of cyan `ESC[36m`.

**Before**

```rust
let color = unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1;   // stdout only
// and, after the first pass:
pub fn stdout_colour() -> bool { colour_on(std::io::stdout().is_terminal()) }  // every call
```

`NO_COLOR=1 blastit dev | tee log` kept its escape codes; a redirected `err.txt` got
whatever stdout decided; DEBUG was cyan in one renderer and dim in the other.

**After (captured)** — the pty/pipe/`NO_COLOR`/redirected-stderr transcript above.

**Tests**

- `crates/blastit/src/logs.rs::colour_is_asked_per_stream_and_plain_when_the_operator_says_so`
  (rewritten): drives `Colour::new` with explicit descriptors and variables — a pipe,
  one-of-two redirected, `NO_COLOR=1`, `NO_COLOR=""`, `TERM=dumb`, an ordinary TERM.
  No `unsafe { set_var }` scaffolding remains, and the coverage is wider.
- `crates/blastit-events/src/pretty.rs::colour_wraps_only_the_level_word` updated to
  the dim DEBUG code.

## Blocked (`dev` / re-exec exit status)

`A-12`'s "one exit status for one argv mistake" cannot reach `2` everywhere: the
protected matrix pins two families at `1`, and only `:556-583` is authorised.

- `crates/blastit/tests/critical_config_and_presets.rs:426-445` — `dev --manifest
  x`, `dev --preset x`, `dev --port 70000`, `dev --host 999.9.9.9` must exit `1`.
  `dev` therefore keeps `1` and gains only the better sentences.
- `crates/blastit/tests/critical_config_and_presets.rs:466-470` — `--stage=1
  --preset x` must exit `1`, so a malformed re-exec keeps `1`.
  `Refusal::internal` is exactly that case; every value error and unknown flag is
  `Refusal::usage` (`2`).

## Blocked (group 3)

Three protected critical tests pin the exact sentences group 3 must change, and the
node authorises **only** `critical_config_and_presets.rs:556-583`:

1. `crates/blastit/tests/critical_dev_and_fail_closed.rs:1224-1238` —
   `assert_eq!(banner.len(), 3)` plus exact equality on banner lines 1 and 2
   (`"  blastit dev · single-uid simulation · 127.0.0.1:{port}"`,
   `"  Not a sandbox. …"`). A clickable `http://127.0.0.1:PORT`, a `[LEARN MODE]`
   badge and a declared-view list all change those lines.
2. `crates/blastit/tests/critical_static_cli.rs:290-298` — the same three lines by
   exact string, including `"  http · scheduler · dbproxy · 0 routes"`.
3. `crates/blastit/tests/critical_dev_reload.rs:1619-1652` — the boot line is
   exactly `reload: on (N directories)` in pretty output **and** must never appear
   as an event (the JSON arm asserts its absence); the node asks for a structured
   `reload` event instead.

Group 3's remaining items (phase lines before blocking dbproxy readiness and stage-1
import; stderr-TTY human pre-boot refusals) can land with the banner/reload work
once those three assertions are re-scoped by a node that lists them in `paths` +
`# Goal`.

## Not started (group 4)

`check.rs` dbproxy-event routing, the human `check` report fields (`sql`, `proxy`,
observed MIME, exact `fix.to`, `view`/`trait` nouns and plurals), `fmt --check`
unified diff and `check --learn --dry-run` proposed block — untouched.

## Follow-ups

- `blastit dev`'s own argv errors exit `1`, not `2`
  (`crates/blastit/tests/critical_config_and_presets.rs:426-445` pins it). Re-scope
  that matrix in a node that lists the file, then `refuse("blastit dev", …)` returns
  `USAGE_FAILURE` like every other command.
- A bare `--stage` / `--config-fd` in a re-exec line still exits `1` when the value
  is missing entirely (`--stage` with no `=`); the value-bearing forms are `2`.

## Verification

Targeted, run while implementing:

```
$ cargo test -p blastit --lib cli
running 18 tests … test result: ok. 18 passed; 0 failed
$ cargo test -p blastit --lib logs::
running 17 tests … test result: ok. 17 passed; 0 failed
$ cargo test -p blastit --locked --test critical_config_and_presets
running 11 tests … test result: ok. 11 passed; 0 failed
$ cargo test -p blastit-events --locked --lib
running 97 tests … test result: ok. 97 passed; 0 failed
```

Prod file sizes after the split: `cli.rs` 424 (dispatcher + the `lsp` session a
protected test reads here), `cli/parse.rs` 494, `cli/help.rs` 187, `cli/run.rs` 141,
`cli/preset.rs` 60, `term.rs` 27; `logs.rs` 567 including the new `Colour`.

Final gate: `make fast-check` and `make test-critical` (tails below).
