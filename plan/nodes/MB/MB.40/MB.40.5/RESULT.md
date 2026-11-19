# Gate: MB.40.5 — CLI visual & UX craft overhaul

## Evidence

| Check | Command | Result |
|---|---|---|
| Rust + Python leaf gate | `make fast-check` | exit 0 (all workspace targets, ruff clean, `plan OK`) |
| Critical suite | `make test-critical` | `Ran 53 tests — OK` |
| Protected critical tests | `critical_guard.py --base <slice base> --node <slice>` | OK per slice; every protected change authorized by its node |
| Invariants | `guard.py` / `plan.py validate` | OK / `plan OK` |
| `e2e/dx_replay.py` | regex updated for the widened event column | imports and matches the real 23-column line |

Full `make check` runs **once** at the `MB.40` gate.

## What a developer now sees

```
$ blastit --version
blastit 0.2.0

$ blastit wat
unknown subcommand 'wat' — run 'blastit --help'      (exit 2)

$ blastit --preset nope
unknown preset 'nope' — valid presets: digitalocean-paas, compose-traefik   (exit 2)

$ blastit dev
  blastit dev · single-uid simulation · http://127.0.0.1:18099
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 2 routes
  view  GET, HEAD  /
  view  GET, HEAD, POST  /products/.*
03:37:47.569 INFO  boot                   stage0  seccomp=on  privilege=unprivileged-dev
  app        importing app:app
03:37:47.633 INFO  reload                 1 directory
03:37:47.633 INFO  ready                  serving  routes=2  workers=16
03:37:53.562 INFO  request                GET / → home  200  3.3ms

$ blastit check
blastit check: 4 requests, 2 statements, 3/4 views, 2 proxies
  view catalog: 1 statement not granted on proxy db
    SELECT 2
      fix: add it to view catalog postgres
  view upload: takes a body and declares no ingress validation; observed application/x-www-form-urlencoded
      fix: write `body form`
blastit check: 6 problems · `--json` for the machine-readable list

$ blastit fmt --check
./Blastfile: not in canonical layout
--- ./Blastfile
+++ ./Blastfile
@@ -1,1 +1,1 @@
-app "shop"
\ No newline at end of file
+app "shop"
```

## Review outcomes

- **[BLOCKER]** usage text was coloured from *stdout* but printed to *stderr* (escapes leaked into `2>ci.log`), and `NO_COLOR`/`TERM` were read inside `logs.rs` with `IsTerminal` re-probed per call. Fixed with one typed `logs::Colour` resolved at the composition root (`term.rs`, 27 lines) and threaded through `Emitter`, `usage` and `refuse`.
- **[BLOCKER]** `check`'s new `EventDrain` was stopped only on the error path (and wrapped a pipe in a `UnixStream`, so it read zero bytes): `pg.refused`/`sql.denied` warnings were silently swallowed. Fixed with an idempotent `Drop`, a `PipeReader`, and forwarding through `Emitter::forward` at the run's configured level.
- **[BLOCKER]** `fmt --check` printed no diff for a file whose only deviation was a missing final newline (and for canonical-CRLF files). Fixed with `split_inclusive` + `\ No newline at end of file`, pinned by binary-level tests.
- **[MAJOR]** a second `--preset` grammar and a second hand-rolled `Blastfile` lexer had appeared; both deleted in favour of one owner each (`config::preset_unknown`, the writer's own spliced fragment).
- **[MAJOR]** the `--learn --dry-run` blocks were `fmt(edited)` while the write path stores `edited` — the proposal now prints the exact bytes `--learn` would write.
- **[MAJOR]** `e2e/dx_replay.py`'s request matcher assumed the old 12-column name field.
- **[MINOR]** fixed: `help serve` answering a non-existent command, `--version` matching anywhere in argv, value errors exiting `1` instead of `2`, internal `--role=dbproxy`/`BLASTIT_EXEC` plumbing and a dangling `BLASTIT_EXEC` line in the one user-facing refusal, a vacuous `banner.len() == 3`, missing `fix:` lines, bare machine ids, and `1 requests`/`1 routes` grammar.
- Filtered as non-actionable: `NAME_COLUMN = 23` (11 columns wider; alignment matters more than 80-column wrapping — accepted deliberately).

## Accepted deviations

- Three protected tests were re-pinned to the charter's new shape, each with its intent spelled out in the node's `# Authorization` (banner informational/once/never an event; component list complete; one aligned reload report; `fmt --check` still writes nothing at all; the same findings/counts/order and the `body = "`/`allow_raw` negatives).
- `dev`'s own argv errors still exit `1` (pinned by a protected test that this epic did not authorize); differ from the rest of the CLI's exit `2`.
- `docs/ci.md` still shows the old `route …` block and `ARCHITECTURE.md`'s §8.1 event table lacks the new `reload` row — both land in `MB.40.6`.
