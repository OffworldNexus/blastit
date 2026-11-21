# MB.21 — epic gate record (measured evidence, not a verdict)

Prepared by `doc-writer` for the epic lead and re-measured at the closing HEAD. Every command below was
run in `/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` with
`PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"`. **The verdict is the lead's** — the plan says
`MB.21 [epic/done]` and the gate commit is `ce7ed71` — and the rows below are the measurement it rests
on, re-run at `ee8bfb7`; nothing between `ee8bfb7` and `ce7ed71` touches code (`ce7ed71` is `plan/` only).

**Shared-tree caveat — why the timestamps matter.** The worktree is shared and moving: `HEAD` advanced
`8f538ae` → … → `ee8bfb7` → `ce7ed71` while this record was written, and **another session's in-flight
editor work sits in it as uncommitted files**. Between 06:12 and 06:22 that churn moved the first red
`make check` target twice (§E3): `rs-fmt` and `rs-lint` failed at 06:12/06:13 and passed again at 06:14
when their owner reformatted and fixed the lints, while `rs-doc` went red on a different uncommitted
file. `docs/reference/cli.md` was committed by `MB.23.1` (`055607b`) and keeps `docs-test` red because
the flag it documents is not in `crates/` yet.

## Summary

The retired manifest format is gone from the product and from the docs. `blastit-manifest` has one
reader, `Blastfile`; discovery is `./Blastfile` plus `BLASTIT_MANIFEST` and nothing else — no detector,
no sniffer, no suffix test, no migration sentence, no key map. `cargo tree -p blastit-manifest | grep -ci
toml` is `0`, the retired reader (`src/parse.rs`) and its suite are deleted, every `e2e/` and
`python/tests/` fixture is a `Blastfile` (15 files, 0 `*.toml`), and ten retired-name probes
(`-k retired`) — `test_the_real_tree_carries_no_retired_toml_manifest` among them — refuse the name in
both directions with no allowlist.

The Blastfile language is documented, designed and mutation-tested: `docs/blastfile.md` (98 lines) and
`docs/reference/blastfile.md` (622 lines) were written before any lexer and reviewed by `dx` +
`ai-ergonomics`; `docs/design/blastfile-dsl.md` and `ADR-0012-blastfile-dsl.md` were reviewed by
`security`/`implementability`/`testability`/`consistency`; `blastit fmt [--check] [PATH]` is a canonical
printer over a lossless CST, `--learn` splices through the same tree (the tutorial's learn fence adds one
route and one query in one pass), and `test-curator` measured **92.0 %** over 827 accounted mutants of
828 in nine in-scope modules, with every module ≥ 84.6 %.

## Children

Ids and titles are copied from `uv run --no-project tools/plan.py tree --under MB.21 --all` (run below).
Counts as found on disk: **49 nodes** (`45` shown by default; the four done epics `MB.21.3`, `4`, `4.5`
and `5` are hidden), of which **5 `epic/done` + 36 `task/done` + 8 `task/superseded`**, zero
`ready`/`proposed`/`active`/`blocked`/`failed`. `MB.21.3` has **nine** leaves (`3.0`–`3.8`); `MB.21.4`
has **ten** direct children (`4.1`–`4.10`, of which `4.5` is itself an epic) plus `4.5`'s **six** leaves
— sixteen nodes, which is what the brief's "eleven" cannot be reconciled with; `MB.21.5` has fourteen
(`5.1`–`5.14`); `MB.21.6`–`MB.21.8` are three further single leaves.

| Node | Title | Status |
|---|---|---|
| `MB.21.1` | Write the Blastfile basic guide and complete language reference, register them, pass dx + ai-ergonomics critics | done |
| `MB.21.1.1` | Purge the retired manifest format from the Blastfile pages | done |
| `MB.21.2` | Design the Blastfile lexer, parser, CST and cutover (ADR-0012) with security/implementability/testability/consistency critics | done |
| `MB.21.3` | Blastfile toolchain: bounded lexer, LL(1) parser, path compiler, fmt, surgical editor, docs_fences (epic) | done |
| `MB.21.3.0` | Model parity (a): Manifest name/mounts/php/equivalent, StaticMount, Route.sandbox, MountTable, uid by sandbox, StaticPolicy::fallback | done |
| `MB.21.3.1` | Bounded Blastfile lexer: tokens, `$IDENT`, raw and escaped strings, trivia, line:col errors | done |
| `MB.21.3.2` | LL(1) Blastfile parser: the lossless CST and the recursive-descent parser that builds it | done |
| `MB.21.3.3` | Compile Blastfile path patterns and normalise HEAD from GET for lowering | done |
| `MB.21.3.4` | Lossless Blastfile CST, canonical printer and `blastit fmt [--check] [PATH]` | done |
| `MB.21.3.5` | Surgical comment-preserving Blastfile editor for `--learn` (replaces the TOML patcher) | done |
| `MB.21.3.6` | Parse blastfile doc fences in docs_fences, reporting both openings until the strict flip | done |
| `MB.21.3.7` | Model parity (b): `StaticPolicy.fallback` replaces spa, mount-prefix shape rules, and sandbox-keyed uniqueness | done |
| `MB.21.3.8` | Lower the Blastfile CST into Manifest against the expected model, with the parity oracle, the nine suites and the fixtures | done |
| `MB.21.4` | Cut the product over to Blastfile: discovery, runtime parity, check/--learn, Python, Makefile, e2e, and the erasure of the retired reader (epic) | done |
| `MB.21.4.1` | Cut every entry point over to `./Blastfile` in one atomic commit: one discovery function, Blastfile parsing, the §15.1 sentences and exit codes, fmt re-parse, and every fixture `make check` reaches | done |
| `MB.21.4.2` | Prove runtime parity on Blastfile: the capability-map parity suite, the mount/entrypoint sentence re-spellings, and 4.1's five MINORs | done |
| `MB.21.4.3` | Map check, `dev --learn` and the `--json` fix actions onto Blastfile nodes, re-spell §13.2/§10.2/§11.3, close §18 and grow the editor's grant scope | done |
| `MB.21.4.4` | Rewrite every `e2e/` fixture, the remaining `python/tests` fixtures, the scripts and the Makefile's docs-serve stage to Blastfile, and take `make e2e` green | done |
| `MB.21.4.5` | Erase the retired manifest format from Rust: delete the reader, the TOML synthesis in stage1, the toml dependency and every remaining mention (epic) | done |
| `MB.21.4.5.1` | Retire the v1 parser suite and translate the 0.1.1 schema suite to Blastfile, re-homing every refusal | done |
| `MB.21.4.5.2` | Prove the static mount's and the php entrypoint's grammar in the two protected suites | done |
| `MB.21.4.5.3` | Translate the last manifest suites and keep the docs-fence harness honest without the retired opening | done |
| `MB.21.4.5.4` | Build the `--learn` routing table from the model and re-point the boot helpers and env/sqlpolicy examples at Blastfile | done |
| `MB.21.4.5.5` | Translate the scheduler's manifest fixtures and delete its second reader | done |
| `MB.21.4.5.6` | Delete the reader, the retired patcher and the model surface they filled, and drop the toml dependency — alone | done |
| `MB.21.4.6` | Raise the six Blastfile budgets (16 MiB source, 4M tokens/trivia, 4 KiB patterns, 4K list items, 256-char app name) and fix the token-cap sentence | done |
| `MB.21.4.7` | Cut `python/blastit` over to Blastfile: fold and the pytest plugin through `blastfile_apply_edits`, `_blastit.audit` off the retired reader, and the four Python suites translated | done |
| `MB.21.4.8` | Bound the dev test client's response pairing: parse a response against its own request, close after a body, and deadline every wait with a small diagnostic | done |
| `MB.21.4.9` | Refuse an `AllowEmptyOnly` request from its head and never dispatch one before `REQ_END`: close the 413-vs-200 race behind the dev suite's flake | done |
| `MB.21.4.10` | Delete the last retired spellings from user-facing sentences and give the report's table scope a `blastfile` key | done |
| `MB.21.5` | Docs migration and release proof: live fences, retired-TOML guard, mutation audit, visual check (epic) | done |
| `MB.21.5.1` | Purge the retired manifest from the site, README, ARCHITECTURE and live briefs, and retire the v1 reference page | done |
| `MB.21.5.2` | Execute Blastfile doc fences live, flip docs_fences strict (retired opening gone) and guard the retired manifest name | done |
| `MB.21.5.3` | Audit and mutation-test the Blastfile parser, CST and editor (test-curator) | done |
| `MB.21.5.4` | Browser proof of the Blastfile pages, nav and both colour schemes (visual-checker) | done |
| `MB.21.5.5` | Close the purity gate's nested-`use` evasion (R24) and prove it in both directions | done (pre-gate; landed `4b41522`) |
| `MB.21.5.6` | Post-0.2.0 coverage: pin `blastfile::read`'s accepted path so the read bound's mutants die in-crate | superseded → `MB.26.1` |
| `MB.21.5.7` | Land the design's ≥ 20-document round-trip corpus floor and the hoisted-comment owner case | done (was `review` when this record started; committed as `9c56183`) |
| `MB.21.5.8` | Give P4's postcondition oracle a direct test and a truthful section for unresolved grants | superseded → `MB.26.2` |
| `MB.21.5.9` | Pin `EditError::source` so the editor's error chain is walked by a test | superseded → `MB.26.3` |
| `MB.21.5.10` | Remove the uid-0 silent skip from `critical_blastfile_fmt`'s write-failure case | superseded → `MB.26.4` |
| `MB.21.5.11` | Pin the named-store grant guard (coverage; a regression would accept a typo'd grant) | done (landed `2c2c9b3`, though the audit ruled it coverage-only post-0.2.0) |
| `MB.21.5.12` | Close the four message-only refusal-matrix survivors (lexer `name_run`, three parser guards) | superseded → `MB.26.5` |
| `MB.21.5.13` | Close the printer and pattern-name survivors (six mutants, four corpus cases) | superseded → `MB.26.6` |
| `MB.21.5.14` | Close the surgical editor's sixteen testable survivors (names, cross-view patches, one-line empty blocks, `transactions` bounding a directive) | superseded → `MB.26.7` |
| `MB.21.6` | Make the DigitalOcean deploy page and its docs check agree on the Blastfile CA spelling | done (46 checks) |
| `MB.21.7` | Repoint the e2e harness at the moved PHP page and replay its two commands | done |
| `MB.21.8` | A queued request must never be handed to a retired PHP view (502 `view_died` under 8-way concurrency) | superseded → `MB.26.8` (filed, never ran) |

The audit's remaining coverage nodes (`5.6`, `5.8`–`5.10`, `5.12`–`5.14`) and `MB.21.8`'s PHP
view-retire race are **re-homed to `MB.26` as post-0.2.0 work** (`MB.26.1`–`MB.26.8`); the originals are
`superseded`, not open children of this epic.

## Evidence

One subsection per epic acceptance item. `make check` is the only row that is not green, for the external
reason stated there; `make e2e` is green.

### E1 — every child `done` with a `RESULT.md`; tree has no ready/active/blocked/failed node

```console
$ uv run --no-project tools/plan.py tree --under MB.21 | head -5
● MB.21 [epic/done] Replace the v1 manifest with the purpose-built Blastfile DSL (docs first, then parser, fmt and cutover)
  ● MB.21.1 [task/done] @doc-writer a1 Write the Blastfile basic guide and complete language reference, register them, pass dx + ai-ergonomics critics
    ● MB.21.1.1 [task/done] @doc-writer a1 Purge the retired manifest format from the Blastfile pages
  ● MB.21.2 [task/done] @designer a3 ⇠MB.21.1 Design the Blastfile lexer, parser, CST and cutover (ADR-0012) with security/implementability/testability/consistency critics
    ● MB.21.3.0 [task/done] @implementer a1 ⇠MB.21.2 Model parity (a): Manifest name/mounts/php/equivalent, StaticMount, Route.sandbox, MountTable, uid by sandbox, StaticPolicy::fallback

$ uv run --no-project tools/plan.py tree --under MB.21 | tail -11
    ● MB.21.5.7 [task/done] @implementer a1 Land the design's >= 20-document round-trip corpus floor and the hoisted-comment owner case
    ~ MB.21.5.8 [task/superseded] @implementer Give P4's postcondition oracle a direct test and a truthful section for unresolved grants
    ~ MB.21.5.9 [task/superseded] @implementer Pin EditError::source so the editor's error chain is walked by a test
    ~ MB.21.5.10 [task/superseded] @implementer Remove the uid-0 silent skip from critical_blastfile_fmt's write-failure case
    ● MB.21.5.11 [task/done] @implementer a1 Pin the named-store grant guard (coverage; a regression would accept a typo'd grant)
    ~ MB.21.5.12 [task/superseded] @implementer Close the four message-only refusal-matrix survivors (lexer name_run, three parser guards)
    ~ MB.21.5.13 [task/superseded] @implementer Close the printer and pattern-name survivors (six mutants, four corpus cases)
    ~ MB.21.5.14 [task/superseded] @implementer Close the surgical editor's sixteen testable survivors (names, cross-view patches, one-line empty blocks, transactions bounding a directive)
  ● MB.21.6 [task/done] @implementer a1 Make the DigitalOcean deploy page and its docs check agree on the Blastfile CA spelling
  ● MB.21.7 [task/done] @implementer Repoint the e2e harness at the moved PHP page and replay its two commands
  ~ MB.21.8 [task/superseded] @implementer A queued request must never be handed to a retired PHP view (502 view_died under 8-way concurrency)
```

`plan.py tree` hides done containers by default, so the four done epics only appear with `--all`:

```console
$ uv run --no-project tools/plan.py tree --under MB.21 --all | grep -E 'epic/'
● MB.21 [epic/done] Replace the v1 manifest with the purpose-built Blastfile DSL (docs first, then parser, fmt and cutover)
  ● MB.21.3 [epic/done] ⇠MB.21.2 Blastfile toolchain: bounded lexer, LL(1) parser, path compiler, fmt, surgical editor, docs_fences
  ● MB.21.4 [epic/done] ⇠MB.21.3 Cut the product over to Blastfile: discovery, runtime parity, check/--learn, Python, Makefile, e2e, and the erasure of the retired reader
    ● MB.21.4.5 [epic/done] ⇠MB.21.4.4 Erase the retired manifest format from Rust: delete the reader, the TOML synthesis in stage1, the toml dependency and every remaining mention
  ● MB.21.5 [epic/done] ⇠MB.21.4 Docs migration and release proof: live fences, retired-TOML guard, mutation audit, visual check
```

**Item 1 holds, with one paperwork gap:** no `ready`/`active`/`blocked`/`failed` node remains under
`MB.21` (the seven audit survivors and the PHP race are `superseded`, not open — `MB.26.1`–`MB.26.8`).
Every closed node has its result on disk:

```console
$ find plan/nodes/MB/MB.21 -name RESULT.md | sort | sed 's|plan/nodes/MB/MB.21/||'
MB.21.1.1/RESULT.md
MB.21.1/RESULT.md
MB.21.2/RESULT.md
MB.21.3/MB.21.3.0/RESULT.md
MB.21.3/MB.21.3.1/RESULT.md
MB.21.3/MB.21.3.2/RESULT.md
MB.21.3/MB.21.3.3/RESULT.md
MB.21.3/MB.21.3.4/RESULT.md
MB.21.3/MB.21.3.5/RESULT.md
MB.21.3/MB.21.3.6/RESULT.md
MB.21.3/MB.21.3.7/RESULT.md
MB.21.3/MB.21.3.8/RESULT.md
MB.21.3/RESULT.md
MB.21.4/MB.21.4.1/RESULT.md
MB.21.4/MB.21.4.10/RESULT.md
MB.21.4/MB.21.4.2/RESULT.md
MB.21.4/MB.21.4.3/RESULT.md
MB.21.4/MB.21.4.4/RESULT.md
MB.21.4/MB.21.4.5/MB.21.4.5.1/RESULT.md
MB.21.4/MB.21.4.5/MB.21.4.5.2/RESULT.md
MB.21.4/MB.21.4.5/MB.21.4.5.3/RESULT.md
MB.21.4/MB.21.4.5/MB.21.4.5.4/RESULT.md
MB.21.4/MB.21.4.5/MB.21.4.5.5/RESULT.md
MB.21.4/MB.21.4.5/MB.21.4.5.6/RESULT.md
MB.21.4/MB.21.4.5/RESULT.md
MB.21.4/MB.21.4.6/RESULT.md
MB.21.4/MB.21.4.7/RESULT.md
MB.21.4/MB.21.4.8/RESULT.md
MB.21.4/MB.21.4.9/RESULT.md
MB.21.4/RESULT.md
MB.21.5/MB.21.5.1/RESULT.md
MB.21.5/MB.21.5.11/RESULT.md
MB.21.5/MB.21.5.2/RESULT.md
MB.21.5/MB.21.5.3/RESULT.md
MB.21.5/MB.21.5.4/RESULT.md
MB.21.5/MB.21.5.5/RESULT.md
MB.21.5/MB.21.5.7/RESULT.md
MB.21.6/RESULT.md
RESULT.md
```

(`5.5`, `5.7`, `5.11`, `6` and `7` all closed while this record was being written; `21.8` was re-homed
to `MB.26.8`.)

`MB.21.5.6`, `5.8`–`5.10`, `5.12`–`5.14` and `MB.21.8` have no `RESULT.md` because they never ran: they
are `superseded` into `MB.26.1`–`MB.26.8` (§Rulings). `MB.21.7` has only a `LOG.md`, whose gate line is
`ONE clean make e2e on an idle host -> EXIT=0, 311 PASS/ok, 0 FAIL` (§E3) — the one `done` node without
a `RESULT.md`.

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

### E2 — the repository runs on Blastfile (fixtures ≥ 15 `Blastfile`, 0 `*.toml`)

```console
$ find e2e python/tests -name 'Blastfile' | wc -l
15
$ find e2e python/tests -name '*.toml' ! -name 'Cargo.toml' ! -name 'inventory.toml' | wc -l
0
$ find e2e python/tests -name 'Blastfile' | sort
e2e/demo/Blastfile
e2e/php/fixture/Blastfile
e2e/sqlalchemy/Blastfile
e2e/static_site/Blastfile
e2e/static_site/nospa/Blastfile
e2e/venv-smoke/Blastfile
python/tests/live_project/Blastfile
python/tests/packaged_project/Blastfile
python/tests/project/Blastfile
python/tests/runnable_docs/fixtures/denial/Blastfile
python/tests/runnable_docs/fixtures/findings/Blastfile
python/tests/runnable_docs/fixtures/hello/Blastfile
python/tests/runnable_docs/fixtures/learn/Blastfile
python/tests/runnable_docs/fixtures/shop/Blastfile
python/tests/runnable_docs/fixtures/static-site/Blastfile
```

The Makefile's docs-serve stage writes one:

```console
$ grep -n 'Blastfile' Makefile | head -2
85:DOCS_SERVE_MANIFEST := $(DOCS_SERVE_STAGE)/Blastfile
160:# `Blastfile` names variables and the value is resolved by `blastit-env` at
```

### E3 — `make e2e` exits 0; `make check` is blocked by files outside this epic

**`make e2e` — PASS, one invocation on an idle host.** `MB.21.6` fixed the DigitalOcean leg (46/46
pooler promises: the page's `postgres` block with `$DATABASE_CA_CERT`, not v1's `ca_cert =`), and
`MB.21.7` repointed the harness at `docs/frameworks/php/index.md` after `MB.20`'s move and replayed the
page's two commands. I did not re-run the six-minute Docker suite; the exit code is the one `MB.21.7`
recorded, and the counts below were re-read from the log it left:

```console
$ grep -n 'ONE clean make e2e' plan/nodes/MB/MB.21/MB.21.7/LOG.md
5:verified: grep empty, only docs/frameworks/php/ dir, ONE clean make e2e on an idle host -> EXIT=0, 311 PASS/ok, 0 FAIL; make doc-tone ok; make docs-check No issues found. Commit 33661e7
$ pgrep -af '[e]2e/run.py|[m]ake e2e'; echo pgrep-exit=$?   # bracket form cannot match itself
pgrep-exit=1
$ docker ps --format '{{.Names}}' | grep -i blastit; echo containers-exit=$?
containers-exit=1
$ grep -cE '^(PASS|ok)' /tmp/opencode/mb21-e2e-clean.log
311
$ grep -cE '^  ok' /tmp/opencode/mb21-e2e-clean.log     # indented sub-assertions
411
$ grep -c '^FAIL' /tmp/opencode/mb21-e2e-clean.log
0
$ grep -cE 'Traceback|make: \*\*\*' /tmp/opencode/mb21-e2e-clean.log
0
$ tail -2 /tmp/opencode/mb21-e2e-clean.log
  ok   and the restarted boot serves it (200)
  ok   still 0 decoy opens (0)
```

Both legs this epic's sweep had broken are green inside that run:

```console
$ sed -n '573p;763p' /tmp/opencode/mb21-e2e-clean.log
PASS: the php2shell matrix and docs/frameworks/php/index.md replayed against a real blastit dev (140 checks)
docs_deploy: 46/46 documented pooler promises hold
```

**The earlier failures were overlap, not the pages.** This checkout's other full-suite attempts ran
concurrently with a second `make e2e` and share **fixed container names/ports**: `e2e-mb21.log` and
`e2e-mb21-final2.log` died at `compose up failed`, `e2e-mb21-run5.log` on the single step
`FAIL a restart serves the page as it is now`, and `run2`/`run4` on `502`/`bad gateway` in the
php2shell matrix (`14 of 140`, `3 of 140`). `mb21-e2e-clean.log` is the only complete `0 FAIL` run;
`MB.21.7`'s brief now says one run per checkout and `pgrep` first.

**`make check` — `BLOCKED BY EXTERNAL`, exit `2` at `rs-doc` (06:14, `ee8bfb7`).** Not an MB.21 defect:
every red target is another session's uncommitted or committed work on this shared tree.

```console
$ make check > /tmp/opencode/mb21-check-now.log 2>&1; echo MAKE_CHECK_EXIT=$?
MAKE_CHECK_EXIT=2
$ tail -2 /tmp/opencode/mb21-check-now.log
warning: build failed, waiting for other jobs to finish...
make: *** [Makefile:177: rs-doc] Error 101
```

The first red target moved twice in ten minutes:

| Target | Measured | Cause | Owner |
|---|---|---|---|
| `rs-fmt` | **pass** (exit `0`) at 06:14; failed 06:12 with 30 `Diff in` lines | `crates/blastit/src/lsp.rs` + `tests/critical_lsp.rs` (**untracked**) | other session; reformatted 06:13 |
| `rs-lint` | **pass** (exit `0`) at 06:14; failed 06:13 (`Error 101`) | `blastfile/sites.rs:78` `'a` elidable, `:176` empty line after doc comment (**untracked**) | other session; fixed 06:13 |
| `rs-doc` | **FAIL** (`Error 101`) — the first red target (`make check` 06:14; re-run alone 06:19) | `error.rs:1523` links private `prefix` and an out-of-scope `Display`; file is `M` (uncommitted) | other session |
| `docs-test` | **FAIL** (`Error 1`) when reached at 06:18 | `cli.md` fence 1: `--no-reload` documented at HEAD, zero occurrences in `cli.rs` | `MB.23.5` |

```console
$ make rs-doc > /tmp/opencode/mb21-rsdoc-now.log 2>&1; echo exit=$?
exit=2
$ grep -E '^error|--> crates' /tmp/opencode/mb21-rsdoc-now.log
error: public documentation for `message` links to private item `prefix`
    --> crates/blastit-manifest/src/error.rs:1523:11
error: unresolved link to `Display`
    --> crates/blastit-manifest/src/error.rs:1523:60
error: could not document `blastit-manifest`
$ git status --short crates/blastit-manifest/src/error.rs
 M crates/blastit-manifest/src/error.rs
```

The untracked editor work is real, not a rumour (`git status --short crates/`):

```console
$ git status --short crates/
 M crates/blastit-manifest/src/blastfile/mod.rs
 M crates/blastit-manifest/src/error.rs
 M crates/blastit/src/cli.rs
 M crates/blastit/src/lib.rs
?? crates/blastit-manifest/src/blastfile/sites.rs
?? crates/blastit-manifest/tests/warning_sites.rs
?? crates/blastit/src/lsp.rs
?? crates/blastit/tests/critical_lsp.rs
```

The live `docs-test` failure, measured on its own:

```console
$ timeout 900 make docs-test > /tmp/opencode/mb21-docstest-now2.log 2>&1; echo DOCS_TEST_EXIT=$?
DOCS_TEST_EXIT=2
$ sed -n '82p;85p;91,92p' /tmp/opencode/mb21-docstest-now2.log
FAIL  docs/reference/cli.md  fence 1  dev-flags             0.2s  blastit dev exited before ready; stdout:
blastit dev: unknown argument `--no-reload`
48 fences: 30 executed, 18 accounted, 1 failed · 29.9s
make: *** [Makefile:331: docs-test] Error 1
```

(One earlier `make docs-test` at 06:17 died before the fences on a stub wheel — `BadZipFile` from
`build-wheel.sh`'s sibling writing the same stage — which is the same overlap class as `make e2e`.)

The page is committed at `HEAD` and the flag is absent from the binary:

```console
$ git log --oneline -1 -- docs/reference/cli.md
055607b [MB.23.1] Document live reload: edits reload every view together
$ sed -n '20p;28p' docs/reference/cli.md
$ blastit dev --no-reload
| `--no-reload` | reload on | turn live reload off: nothing is watched, and no `dev.reload.unavailable` is printed |
$ grep -c -- '--no-reload' docs/reference/cli.md
2
$ git show HEAD:crates/blastit/src/cli.rs | grep -c 'no.reload'; echo grep-exit=$?
0
grep-exit=1
$ cd /tmp/opencode/mb21-now-probe && /…/target/debug/blastit dev --no-reload; echo exit=$?
blastit dev: unknown argument `--no-reload`
exit=1
```

(`/…/` is the worktree's `target/debug/blastit`, as in §E4.)

**The epic's own legs, measured green at `ee8bfb7`** (`make docs-test`'s `docs-unit` half passed `107`
before that fence failed):

```console
$ uv run --no-project tools/plan.py validate
plan OK
$ uv run --no-project tools/guard.py
guard: OK (27 staged/untracked file(s) checked)
$ timeout 600 cargo test -p blastit-manifest --locked; echo exit=$?
exit=0            # 34 × `test result: ok`, 0 × FAILED
$ make docs-unit > /tmp/opencode/mb21-docsunit-now.log 2>&1; tail -1 /tmp/opencode/mb21-docsunit-now.log
107 passed, 65 subtests passed in 7.14s
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.47s
$ make doc-tone
doc-tone: ok — 30 user-facing page(s), no banned phrase
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q -k 'retired' | tail -1
10 passed, 25 deselected in 1.38s
```

The eight targets after `docs-test` in `check`, run on their own (measured 05:28, before the shared tree
moved; `doc-tone`, `plan-validate` and `docs-check` re-run green at 06:19):

```console
$ make doc-tone manifest-purity plan-validate seam-check workflow-check packaging-gates-test \
    platform-matrix-check rs-no-python > /tmp/opencode/mb21-postcheck.log 2>&1; echo exit=$?
exit=0
$ grep -E 'doc-tone|manifest-purity|plan OK|workflow-check: ok|packaging-gates-test: ok|default build' \
    /tmp/opencode/mb21-postcheck.log
doc-tone: ok — 30 user-facing page(s), no banned phrase
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 55 assertions, 22 fixtures proved, 16 real source file(s)
plan OK
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=12, test-seam=7)
default build: 0 pyo3 dependencies (no libpython linked)
```

### E4 — no migration path exists (retired body refused; one missing-file sentence)

Run live against the built binary. An empty directory prints one sentence and exits `2`; a v1 body
named `Blastfile` is a single `line:col` refusal at `check`, `fmt` and `dev`, with no second sentence
and no "did you mean":

```console
$ cd /tmp/opencode/mb21-empty && /…/target/debug/blastit check; echo exit=$?
blastit check: ./Blastfile: no such file or directory (put a Blastfile in the working directory, or set BLASTIT_MANIFEST)
exit=2
$ D=$(mktemp -d /tmp/opencode/mb21-v1.XXXXXX); printf 'version = 1\napp = "demo.app:main"\n' > "$D/Blastfile"
$ cd "$D" && /…/target/debug/blastit check; echo exit=$?
blastit check: manifest ./Blastfile: 1:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, `trait`, `view`, `static`
exit=2
$ /…/target/debug/blastit fmt --check; echo exit=$?
./Blastfile:1:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, `trait`, `view`, `static`
exit=2
$ /…/target/debug/blastit dev 2>&1 | head -3
blastit dev: manifest ./Blastfile: 1:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`, `trait`, `view`, `static`
```

(`/…/` stands for
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/debug/blastit`; the `dev` exit code
is not quoted because the pipe through `head` masks it.)

Two exact-wording notes for the lead: the missing-file sentence is `./Blastfile: no such file or
directory (put a Blastfile in the working directory, or set BLASTIT_MANIFEST)`
(`crates/blastit/src/config.rs:2411`), **not** the epic's shorthand `missing ./Blastfile`; and the
parse refusal is prefixed `manifest ` at `check`/`dev` while `fmt` prints the bare
`./Blastfile:1:1: …`. Same single sentence, two renderings.

The guard is green and the negative probes are green:

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q -k 'the_real_tree_carries_no_retired_toml_manifest' | tail -2
.                                                                        [100%]
1 passed, 34 deselected in 0.47s
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q -k 'retired' | tail -2
..........                                                               [100%]
10 passed, 25 deselected in 1.48s
```

The guard has no allowlist and scans the live tree only (`python/tests/test_docs_assets.py:24-35,1057`):

```console
$ sed -n '30,35p;1057,1060p' python/tests/test_docs_assets.py
`test_the_real_tree_carries_no_retired_toml_manifest`; there is no allowlist,
because a list a lazy edit can widen is the defect the guard exists to prevent.
The name is composed (`"blastit" + ".toml"`) so this module needs no exemption of
itself, and the dated record (`docs/decisions|design|research|knowledge|
testing/`, `docs/changelog.md`, `LOG.md`/`RESULT.md`, finished briefs) is outside
the scanned set by construction — it is why the name can be recognised at all.
def test_the_real_tree_carries_no_retired_toml_manifest():
    """The name `MB.21.4.5`/`MB.21.5.1` erased cannot come back unnoticed."""
    findings = retired_manifest_findings(ROOT)
    assert not findings, "\n".join(str(finding) for finding in findings)
```

The retired body is a syntax error and `dev`/`check`/`fmt` share the reader; `docs-test` executes the
denial fence that proves it, and it passed in both runs:

```console
$ grep 'docs/local.md  fence 6' /tmp/opencode/mb21-docstest-now2.log
PASS  docs/local.md  fence 6  denial                0.1s  the printed denial matches the documented fields
```

### E5 — the retired name is nowhere outside records; both pages are lean and true

```console
$ grep -rnE 'blastit[.]toml' README.md docs ARCHITECTURE.md crates python scripts e2e Makefile \
    --exclude-dir=__pycache__ --exclude='*.so' --exclude-dir=target \
    | grep -vE 'docs/changelog\.md|docs/(decisions|design|research|knowledge|testing|archive)/'
(no output)   # exit 1
$ for f in $(grep -rlE 'blastit[.]toml' plan/nodes --include=NODE.md); do \
    printf '%s %s\n' "$(grep -m1 -oE 'status: [a-z]+' "$f" | cut -d' ' -f2)" "$f"; done \
    | grep -vE '^(done|superseded) '
(no output)   # exit 1
```

Both pages are public and registered, the retired reference page is gone, and the guide is under the
150-line limit:

```console
$ ls docs/blastfile.md docs/reference/blastfile.md docs/design/blastfile-dsl.md \
    docs/decisions/ADR-0012-blastfile-dsl.md
docs/blastfile.md
docs/decisions/ADR-0012-blastfile-dsl.md
docs/design/blastfile-dsl.md
docs/reference/blastfile.md
$ test ! -e docs/reference/manifest.md; echo absent-exit=$?
absent-exit=0
$ wc -l docs/blastfile.md docs/reference/blastfile.md
   98 docs/blastfile.md
  622 docs/reference/blastfile.md
```

Critic state, as the RESULTs record it (the review files still start from their round-1 `VERDICT: FAIL`
and carry a `## Resolution check` per finding):

```console
$ sed -n '25,28p' plan/nodes/MB/MB.21/MB.21.1/RESULT.md
**Critic state.** The final `dx` review reported 1 BLOCKER, 4 MAJOR and 15 MINOR;
the final `ai-ergonomics` review reported 0 BLOCKER, 5 MAJOR and 11 MINOR. Both
files in the worktree are the pre-fix revisions; every finding is resolved below
and the lead runs the final verification round.
$ sed -n '70,72p' plan/nodes/MB/MB.21/MB.21.2/RESULT.md
BLOCKER/MAJOR those files raise is resolved in design §22 (accepted with the
section that changed, or rejected with the owner decision as the reason), and the
**resolution ledger is accepted by the lead**, verified by a fresh goldfish
```

### E6 — mutation audit ≥ 75 % and the visual proof

`test-curator`'s report (`docs/testing/AUDIT-MB.21.md`) is the evidence:

```console
$ sed -n '17,18p' docs/testing/AUDIT-MB.21.md
nine modules in scope clears the 75 % kill bar** (84.6–100 %, 92.0 % over 827 accounted mutants of 828; §2.5). No case in
scope is fluff and no protected suite lost an assertion.
$ sed -n '226p' docs/testing/AUDIT-MB.21.md
| `src/blastfile/syntax.rs` | 110 | 74 | 1 | 5 | 30 | **92.5 %** |
```

Visual proof is `MB.21.5.4`'s RESULT, quoted from its summary:

```console
$ sed -n '5p' plan/nodes/MB/MB.21/MB.21.5/MB.21.5.4/RESULT.md
Visual inspection and browser verification of the new Blastfile documentation pages (`/blastfile/` and `/reference/blastfile/`) confirmed clean rendering, responsive layout, proper navigation structure, and high contrast in both light and dark colour schemes.
```

The site also builds strict today:

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.27s
```

### E7 — the crate, the dependency and the commit trail

```console
$ timeout 600 cargo test -p blastit-manifest --locked > /tmp/opencode/mb21-manifest-now.log 2>&1; echo exit=$?
exit=0
$ grep -c '^test result: ok' /tmp/opencode/mb21-manifest-now.log
34
$ grep -c '^test result: FAILED' /tmp/opencode/mb21-manifest-now.log
0
$ grep -E '^test result: ok|^all doctests' /tmp/opencode/mb21-manifest-now.log | tail -2
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
all doctests ran in 0.32s; merged doctests compilation took 0.32s
$ ls crates/blastit-manifest/tests/ | grep blastfile
blastfile_body.rs
blastfile_entrypoint.rs
blastfile_errors.rs
blastfile_fmt.rs
blastfile_grants.rs
blastfile_patterns.rs
blastfile_traits.rs
critical_blastfile_body.rs
critical_blastfile_entrypoint.rs
critical_blastfile_errors.rs
critical_blastfile_lexer.rs
critical_blastfile_parity.rs
critical_blastfile_parser.rs
critical_blastfile_patterns.rs
critical_blastfile_print.rs
critical_blastfile_security.rs
critical_blastfile_traits.rs
$ test ! -e crates/blastit-manifest/src/parse.rs; echo absent-exit=$?
absent-exit=0
$ test ! -e crates/blastit-manifest/tests/parse.rs; echo suite-absent-exit=$?
suite-absent-exit=0
$ cargo tree -p blastit-manifest | grep -ci toml
0
$ git log --oneline | head -12
ce7ed71 [MB.21] Gate passed: the product and the docs read one manifest format
ee8bfb7 [MB.25.1] Document the INFO `runtime` event: PHP version + php.ini, Python version + environment
0c2c117 [MB.23.2] Design the unprivileged live-reload watcher, the one all-views reload, and ADR-0014
33661e7 [MB.21.7] Repoint the e2e harness at the moved PHP page and its two commands
053b56e [MB.22.4] Ship the Zed extension and the in-repo tree-sitter-blastfile grammar
7c2e9ca [MB.22.2] Design blastit lsp and the Zed extension, and record ADR-0013
055607b [MB.23.1] Document live reload: edits reload every view together
e7b9581 [MB.21.6] Make the DigitalOcean page and its check agree on the Blastfile CA
bc0b492 [MB.24.1] State the real per-route concurrency rule in the user docs
4b41522 [MB.21.5.5] Make the purity gate fail closed instead of enumerating spellings
2c2c9b3 [MB.21.5.11] Pin the named-store grant guard the audit found unpinned
9c56183 [MB.21.5.7] Land the >= 20-document round-trip corpus floor
```

(`34` suites is `33` + `tests/warning_sites.rs`, the shared tree's untracked suite; the crate's committed
33 are unchanged. `MB.21.7`'s `33661e7` and `MB.21.6`'s `e7b9581` are the two children added at the
gate.)

Docs gates, run individually. (There is no `docs-component` target in this Makefile; `docs-unit`,
`docs-check` and `docs-test` are the three that exist, and all three were run.)

```console
$ make docs-unit > /tmp/opencode/mb21-docsunit-now.log 2>&1; echo exit=$?; tail -1 /tmp/opencode/mb21-docsunit-now.log
exit=0
107 passed, 65 subtests passed in 7.14s
$ timeout 900 make docs-test > /tmp/opencode/mb21-docstest-now2.log 2>&1; echo exit=$?
exit=2
$ tail -2 /tmp/opencode/mb21-docstest-now2.log
48 fences: 30 executed, 18 accounted, 1 failed · 29.9s
make: *** [Makefile:331: docs-test] Error 1
```

`docs-test`'s only failure is the `MB.23.1` fence above; the Blastfile pages' fences all executed green
(`docs/blastfile.md` fences 1–3: `dev` served, `check` clean, presets fail closed; `docs/local.md`
fence 5 `dev-learn` wrote the added route and query in one pass).

## Rulings and deviations

- **Total-erasure law (owner directive).** The v1 manifest is erased, not migrated: no detector,
  no sniffer, no suffix test, no migration sentence/page/key map/apology. Enforced in two places that
  both exist on this tree: the live-tree guard
  `test_the_real_tree_carries_no_retired_toml_manifest` (`python/tests/test_docs_assets.py:1057`, run by
  `docs-unit`, therefore by `make check`) with **no allowlist** and ten retired-name probes beside it
  (four of them crafted negatives: a published page, a live brief, a crate/script, and the retired
  reader/reference),
  and the **residual retired-spelling ledger** in `MB.21.4.5`'s RESULT (§"The residual retired-spelling
  ledger", `:73`), whose user-facing rows `MB.21.4.10` then closed. Records stay by design: `LOG.md`,
  `RESULT.md`, finished briefs, `docs/{decisions,design,research,knowledge,testing,archive}/**` and the
  changelog.
- **`body raw` / `body raw unvalidated` and the `workers` removal are settled owner decisions**, not
  open design questions (`ADR-0012` decisions 6 and 7). `body raw` lowers to `(Raw, allow_raw = false)`:
  `dev` warns, `check` reports `raw_ingress`, production refuses through
  `Manifest::raw_ingress_refusals`; `body raw unvalidated` acknowledges and boots with the warning;
  `--learn` can write neither. `workers` is not a keyword — `parser.rs:779` returns
  `ManifestError::WorkersRemoved`, and no replacement knob exists. Both are pinned in the protected
  suites (`critical_blastfile_body.rs:35` carries the `("raw unvalidated", Raw, true)` pair).
- **Six raised budgets (`MB.21.4.6`)**, verified in the constants rather than in prose:
  `MAX_SOURCE_BYTES = 16_777_216`, `MAX_TOKENS = 4_194_304`, `MAX_TRIVIA = 4_194_304`,
  `MAX_LIST_ITEMS = 4_096`, `MAX_APP_NAME_CHARS = 256`, and `MAX_PATTERN_BYTES = 4_096` with
  `REGEX_SIZE_LIMIT = 1 << 20` still bounding the compiled program. The "split the file" refusal was
  replaced by the real remedies (table grants or a shared `trait`); `grep -rn 'split the file' crates/
  docs/` prints nothing (`MB.21.4.6` RESULT §2).
- **`table_grants.blastfile` rename (deliberate, owner rule).** The report's table scope is
  `table_grants.blastfile` with a pasteable Blastfile fragment — `TableGrants::to_blastfile` emits
  `select`/`insert`/`update`/`delete` directives plus a `# <locator>` comment, and
  `crates/blastit-py/src/audit.rs:370,1566` write it under the `"blastfile"` key. It supersedes that one
  field name in `ADR-0003`; `MB.21.4.10`'s RESULT is the authority, and `MB.21.4`'s LOG records the
  amendment.
- **`--no-reload` is an external blocker, not an MB.21 defect.** `docs/reference/cli.md` was committed by
  `MB.23.1` (`055607b`) and documents the flag twice; `git show HEAD:crates/blastit/src/cli.rs | grep -c
  no.reload` is `0`. The implementation is `MB.23.5` (`ready`; designed in `MB.23.2`). Until it lands,
  `docs-test` fails that fence; `make check` never reaches it because `rs-doc` is red first (§E3).
- **The two long-standing flakes.**
  1. `blastit-view`'s 1 s frame budget — `wait_for` is still `for _ in 0..200 { … sleep(5ms) }`
     (`crates/blastit-view/src/php.rs:1235-1243`). **Open and out of scope** for `MB.21` by the epic's
     own note; it needs its own leaf in the milestone that owns that crate.
  2. The dev-client pairing / `413`-vs-`200` race behind
     `dev_boots_five_unprivileged_roles_and_strips_secrets` — **closed** by `MB.21.4.9` (refuse an
     `AllowEmptyOnly` head synchronously and never dispatch before `REQ_END`, enforced in `begin_raw`,
     `request_end` and `forward_static`); its client half was fixed and pinned by `MB.21.4.8`, whose
     RESULT records `STATUS: blocked` precisely because the server half was not in its `paths`.
- **`MB.21.4.5`'s acceptance `git grep 'mod parse'` over-matches.** The unanchored form matches
  `mod parser;` and is therefore unusable as written; the anchored form is the real check:

  ```console
  $ git grep -n 'mod parse' -- crates
  crates/blastit-manifest/src/blastfile/mod.rs:47:mod parser;
  $ git grep -nE '^\s*mod parse\s*;' -- crates; echo anchored-exit=$?
  anchored-exit=1
  ```

  The deletion the item asks for is real; the command in its acceptance text should be the anchored
  one.

## Follow-ups

- **`e2e/docs_deploy.py` no longer blocks `make e2e` — the harness is green.** `MB.21.6` re-spelled the
  `ca_cert` check to the `postgres` block with `$DATABASE_CA_CERT` that `MB.21.5.1` put on
  `docs/deploy/digitalocean-paas.md` (46/46 pooler promises), and `MB.21.7` repointed the harness at
  `docs/frameworks/php/index.md` (MB.20's move) and replayed that page's two commands. One single clean
  invocation of `make e2e` on an idle host: `E2E_EXIT=0`, **311 `PASS`/`ok`, 0 `FAIL`**
  (`/tmp/opencode/mb21-e2e-clean.log`). Earlier full-suite attempts in this same checkout were poisoned
  by *overlapping* `make e2e` runs (fixed container names): one run per checkout, and
  `pgrep -af 'make e2e|e2e/run.py'` first.
- **Post-0.2.0 work moved out of this epic:** the audit's remaining coverage nodes (`5.6`, `5.8`–`5.10`,
  `5.12`–`5.14`, per `docs/testing/AUDIT-MB.21.md` §5.5) and `MB.21.8`'s PHP view-retire race are
  **re-homed to `MB.26`** (`MB.26.1`–`MB.26.8`), and the originals are `superseded`. The pre-gate pair
  landed (`5.7` as `9c56183`, `5.5` as `4b41522`), and `5.11` landed too (`2c2c9b3`) although that table
  calls it coverage-only after 0.2.0 — the epic lead may want to record that as a deviation from the
  ruling. `MB.21.8` is **filed, not done**: eight concurrent GETs at one PHP view on a **one-slot** view
  gave `1 × 200` and `7 × 502`, each `{"outcome":"view_died","queued_ms":15.3,…,"status":502}` — the
  queued waiters are dispatched to the view that has already retired. Same class as `MB.3.45`,
  reproducible at will; `MB.26.8` carries the brief, paths and acceptance verbatim.
- **`docs-site-plan.md` (repository root) still names the retired page** — the file exists (260 lines)
  and its page table and FAQ carry it; it is a working plan, not a published page, and the guard does
  not scan it:

  ```console
  $ grep -n 'reference/manifest' docs-site-plan.md
  40:| `reference/manifest.md` | the manifest format, key by key |
  60:| 7 | What is a proxy, and where does `DATABASE_URL` go? | `reference/manifest.md#proxies` |
  61:| 8 | Exact `queries`, or `tables`? | `reference/manifest.md#grants` |
  ```
- **Rust doc-comment survivors of the retired vocabulary** are real in the two crates the brief names,
  but they are `[route.ingress]`/`body = "json"`/`allow_raw`, not the file name — 13 lines, comment-only,
  never rendered:

  ```console
  $ grep -rcE '\[route\.|\[proxy\.|allow_raw' crates/blastit-ingress/src/*.rs \
      crates/blastit-dbproxy/src/*.rs | grep -v ':0'
  crates/blastit-ingress/src/lib.rs:11
  crates/blastit-dbproxy/src/config.rs:1
  crates/blastit-dbproxy/src/server.rs:1
  ```
- **`ARCHITECTURE.md` §6 is not in `PAGES` on this tree** — the brief's item does not hold as written.
  `PAGES` (`docs_fences.rs:72`) pins thirteen pages and `DOCUMENTED_PAGES` (`common/mod.rs:413`) twelve;
  neither lists `ARCHITECTURE.md`, whose §6 is already titled "The `Blastfile`". The only
  `ARCHITECTURE.md` references in the harness are `FRONT_DOOR`
  (`python/tests/test_docs_assets.py:853`) and a file-existence assert (`:1197`). The lead should
  confirm what the item meant before filing anything.
- **No open human request at 06:20:** `plan.py human open` prints `(none)` and `plan.py state` reports
  `open human requests: 0`. `H-013` (the previous record's only item) is closed; `H-010`–`H-012` are
  closed in `plan/HUMAN.md`.
- **`MB.21.5.5` and `MB.21.5.11` committed after the rows above were measured** (`4b41522`, `2c2c9b3`),
  and the gate commit `ce7ed71` touches `plan/` only. Every row marked `06:14`/`06:19` was re-run at
  `ee8bfb7`; the mutation audit (§E6) and the live refusals (§E4) are from their own runs, as labelled.

## Final gate status (post-close)

Last measured state, after the gate commit `ce7ed71` and unchanged at `f6ec3cb` (three later commits touch
only `plan/`, `docs/design/` and `docs/decisions/`): one `make check` with no other run in the worktree
(`/tmp/opencode/mb21-final-check-alone.log`). This supersedes the per-cause detail above.

- `make rs-fmt` and `make rs-lint`: **green** — the editor and warning-site sessions repaired their
  untracked files in place. `make rs-doc` is green at this measurement too: the earlier rustdoc refusal
  ("public documentation for `message` links to private item `prefix`", `error.rs:1523`) no longer
  reports, and `crates/blastit-manifest/src/error.rs` is still modified-but-uncommitted by another
  session (last commit `335f564`, this epic's own `MB.21.4.10`) — their in-flight edit, never this
  epic's.
- `make check`: **still exit 2**, now at `docs-test`, one failed fence — `docs/reference/cli.md` fence 1,
  `blastit dev: unknown argument --no-reload`, while `crates/blastit/src/cli.rs` has no such flag. That
  is the tree's only *committed* defect, and it is `MB.23`'s (the doc landed before the flag; `MB.23.2`
  is the design node).
- Every leg that measures this epic is green: `plan.py validate` (`plan OK`), `guard.py`, the manifest
  crate's 34 suites, `docs-unit` (107 passed), `docs-check` (`No issues found`), `doc-tone`, the
  retired-name guard and all epic acceptance greps — and `make e2e` passes end to end (311 checks).
