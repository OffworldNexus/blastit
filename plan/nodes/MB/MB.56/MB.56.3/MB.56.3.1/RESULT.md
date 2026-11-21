# RESULT — MB.56.3.1

Created `crates/blastit/tests/e2e_cli.rs` (§3 "Installation & Project Workflow" —
CLI surface & safe handoff) and absorbed the suites it owns. 21 tests.

## Which old file → which new test

| Deleted / edited | Was | Now |
|---|---|---|
| `critical_handoff.rs` (deleted) | `the_four_commands_hand_off_with_argv_and_environment` | same name in `e2e_cli.rs` |
| | `every_auto_discovered_location_hands_off` | same name |
| | `an_activated_venv_replaces_the_project_venv` | same name |
| | `a_candidate_that_is_this_image_does_not_hand_off` | same name — now **observable** (below) |
| | `a_group_or_world_writable_binary_is_skipped_and_the_next_one_runs` | same name |
| | `an_unsafe_activated_venv_is_skipped_and_the_next_candidate_runs` | same name |
| | `an_unsafe_blastit_bin_is_refused_with_the_fix` | same name |
| | `a_binary_owned_by_another_uid_is_refused` | same name (root-only leg, loud skip + `BLASTIT_REQUIRE_ROOT_TESTS`) |
| | `a_symlink_inside_the_project_hands_off_and_one_into_an_unsafe_tree_does_not` | same name |
| | `an_elf_candidate_runs_from_the_verified_descriptor` | same name |
| | `a_file_that_is_neither_an_elf_image_nor_a_script_is_not_run` | same name |
| | `the_message_names_the_command_the_user_typed` | same name |
| | `the_production_shapes_never_hand_off` | same name |
| | `the_two_switches_control_the_handoff` | same name |
| | `an_explicit_blastit_bin_that_is_this_image_is_not_refused` | same name, `/proc/<pid>/exe` now asserted unconditionally |
| | `the_exec_never_reopens_the_verified_path` (source grep) | replaced by the adversarial churn test below |
| `critical_config_and_presets.rs` (CLI-half removed) | `cli_matrix_rejects_each_bad_command_line` | `a_bad_command_line_exits_with_its_documented_status` (+ `--port`/`-p`) |
| | `top_level_help_lists_the_three_commands_and_the_presets` | `the_version_is_one_line_on_stdout_and_nothing_on_stderr`, `help_lists_the_commands_and_presets_but_never_the_internal_flags`, `an_unknown_preset_names_the_valid_presets_and_exits_two` |
| `smoke.rs` (CLI-facing part removed) | `an_unknown_preset_is_refused_with_the_list_of_presets` | `an_unknown_preset_names_the_valid_presets_and_exits_two` |
| `dx_capture.rs` + `dx-mask.txt`, `dx-mask-host.txt`, `dx-parity.txt`, `dx-parity.expected` (deleted) | CLI-parity mask self-tests | exact `--version`/`help`/refusal assertions |
| `dx_invariance.rs` (deleted) | row 1/row 3 `blastit dev` serving + banner | serving legs belong to `e2e_dev.rs` (`MB.56.3.2`) |

## Consolidated review fix

1. **Per-spelling preset oracle.** The refusal paragraph (everything before the
   first `usage:`) is asserted separately from the usage block: `--preset` and
   `--preset=nope` must name `--preset` and must not mention `BLASTIT_PRESET`;
   `BLASTIT_PRESET=nope` must do the reverse. `--preset=digitalocean-paas` is a
   separate accepted case: exit `1` (stage 0), never the parser's `2`.
2. **Production port flag.** `--port 8080` and `-p 8080` are rejected cases: exit
   `2`, stderr names `--port`.
3. **TOCTOU proof.** `a_swapped_path_never_runs_a_program_that_verification_skipped`
   is a real adversarial E2E: a thread `rename(2)`s the candidate path between a
   trusted ELF (`/bin/true`, `0755`) and a marker-writing script that the mode
   check must **skip** (`0777`), while 60 `blastit dev` runs execute. Every run
   either hands off to the verified ELF (`0`) or skips the script and lets the
   global `dev` answer (`1`); the marker must never appear, and
   `handed_off > 0` proves the churn really reached the ELF branch.
   **Precisely what cannot be tested:** no test can inject into the
   `verify` → `execveat` window itself, so this is a one-sided security oracle
   (it can catch a regression, never prove its absence). The deterministic
   guarantee lives in `cli/handoff.rs`: an ELF is started with
   `execveat(verified.bin, "", AT_EMPTY_PATH)`, so the inode `fstat`ed is the
   inode run; a `#!` script must be `execve`d by path (the kernel resolves the
   interpreter), and its protection is the tree rule — no directory on the way to
   it may be writable by another uid — which the world-writable-parent,
   foreign-uid and symlink cases pin behaviourally. No production code changed.
4. **Self-image test was vacuous, now observable.** The candidate's parent is
   `chmod 0777` before the run: a candidate that was *not* recognised as this
   image would fail the tree rule and print `skipping local binary … (… 0777 …)`,
   so "no skip line, `dev` answers itself with exit `1`" distinguishes the
   identity answer from an attempted handoff.
5. **Portability.** `script(1)` is resolved through this process's `PATH`
   (never hardcoded); its absence prints `SKIP …` and is fatal only when
   `BLASTIT_REQUIRE_PTY_TESTS=1` is set. The pty half asserts only that an escape
   appeared, while the redirected-file half stays exact. `/proc/<pid>/exe` is now
   `expect`ed rather than silently skipped. `Project::new` returns `Option`, prints
   a loud `SKIP` with every root it tried, and panics only under
   `BLASTIT_REQUIRE_HANDOFF_TESTS=1`.

## Docs

- `docs/reference/cli.md`: `blastit --port 8080` now exits **2** naming `PORT`
  (was `1`); dev-vs-check CLI exit contract after the `dev` flag table; added the
  `help [COMMAND]` table row; `--version` is one line and answers only an
  otherwise-empty command line; the per-stream colour rule; `-- ARGS…` refuses
  `--blastit-manifest`/`--blastit-live-server` (`2`); the handoff section now
  states that an activated `$VIRTUAL_ENV` names the first candidate, that an
  ambient candidate's failure is a **skip**, and that `BLASTIT_BIN` fails closed.
  The unproven "the `boot` event names the build that actually ran" clause was
  **dropped** (this slice has no E2E for it).
- `docs/reference/settings.md`: added the `BLASTIT_BIN` row.
- `docs/troubleshooting.md`: new "The wrong `blastit` answers" rows for
  `skipping local binary …` and `refusing local handoff to …`, with their fixes.
- `docs/install.md` unchanged: its handoff/`--version` text already matches.

## Evidence

```
$ cargo test -p blastit --test e2e_cli
running 21 tests
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ make fast-check          # green: guard, fmt, clippy, doc, rs-test, ruff, plan, ELF, e2e_cli, install-test
$ make docs-check          # green: zensical --strict
```

## Follow-ups

- `Makefile`'s `universal-smoke` named the deleted `--test critical_handoff`; it
  now runs `--test e2e_cli` (one line, required to keep `fast-check` green).
  `MB.56.3.5` owns the final Makefile leg sweep.
- `e2e/dx_replay.py` still names `crates/blastit/tests/dx_capture.rs` and
  `dx-mask.txt`, which this slice deletes because the node scopes the `dx-*`
  fixtures here. No `Makefile`/CI target runs that driver; `MB.56.3.5` should
  retire it with the rest of the DX-invariance residue.
- `crates/blastit/src/cli.rs` / `help.rs` / `parse.rs` doc comments still cite
  line numbers in `critical_config_and_presets.rs` that no longer exist;
  harmless prose, outside this node's `paths`.
- `smoke.rs` keeps its serving and §6 halves; `MB.56.3.2`/`MB.56.3.4`/
  `MB.56.3.5` delete them.
