# MB.21 — epic gate record (prepared evidence, not a verdict)

Prepared by `doc-writer` for the epic lead. Every command below was run in
`/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0` with
`PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"`. **No "done" verdict is written here**: the epic lead
runs the gate and owns `plan.py set MB.21 done`.

**Shared-tree caveat.** The worktree is shared and moving: `HEAD` advanced four times while this record
was prepared — `8f538ae` → `9c56183` (`MB.21.5.7`) → `2c2c9b3` (`MB.21.5.11`) → `4b41522`
(`MB.21.5.5`) → `bc0b492` (`MB.24.1`). Each row was measured at the moment shown; the shape-dependent
rows were re-checked at `bc0b492` (the retired-name grep is still empty and the two page lengths are
unchanged). `git status` carries **other epics' uncommitted docs** — `MB.23.1`'s `docs/local.md`,
`docs/reference/cli.md`, `docs/reference/log-events.md`, `docs/troubleshooting.md` plus
`docs/blastfile.md` and `docs/frameworks/python/index.md` — and `MB.23.1`'s `--no-reload` fence is what
makes `make check`/`docs-test` red (§E3).

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
Counts as found on disk: `MB.21.3` has **nine** leaves (`3.0`–`3.8`); `MB.21.4` has **ten** direct
children (`4.1`–`4.10`, of which `4.5` is itself an epic) plus `4.5`'s **six** leaves — sixteen nodes,
which is what the brief's "eleven" cannot be reconciled with; `MB.21.5` has fourteen (`5.1`–`5.14`).

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
| `MB.21.5` | Docs migration and release proof: live fences, retired-TOML guard, mutation audit, visual check (epic) | **ready** |
| `MB.21.5.1` | Purge the retired manifest from the site, README, ARCHITECTURE and live briefs, and retire the v1 reference page | done |
| `MB.21.5.2` | Execute Blastfile doc fences live, flip docs_fences strict (retired opening gone) and guard the retired manifest name | done |
| `MB.21.5.3` | Audit and mutation-test the Blastfile parser, CST and editor (test-curator) | done |
| `MB.21.5.4` | Browser proof of the Blastfile pages, nav and both colour schemes (visual-checker) | done |
| `MB.21.5.5` | Close the purity gate's nested-`use` evasion (R24) and prove it in both directions | done (pre-gate; landed `4b41522`) |
| `MB.21.5.6` | Post-0.2.0 coverage: pin `blastfile::read`'s accepted path so the read bound's mutants die in-crate | ready (post-0.2.0) |
| `MB.21.5.7` | Land the design's ≥ 20-document round-trip corpus floor and the hoisted-comment owner case | done (was `review` when this record started; committed as `9c56183`) |
| `MB.21.5.8` | Give P4's postcondition oracle a direct test and a truthful section for unresolved grants | ready (post-0.2.0) |
| `MB.21.5.9` | Pin `EditError::source` so the editor's error chain is walked by a test | proposed (post-0.2.0) |
| `MB.21.5.10` | Remove the uid-0 silent skip from `critical_blastfile_fmt`'s write-failure case | proposed (post-0.2.0) |
| `MB.21.5.11` | Pin the named-store grant guard (coverage; a regression would accept a typo'd grant) | done (landed `2c2c9b3`, though the audit ruled it coverage-only post-0.2.0) |
| `MB.21.5.12` | Close the four message-only refusal-matrix survivors (lexer `name_run`, three parser guards) | proposed (post-0.2.0) |
| `MB.21.5.13` | Close the printer and pattern-name survivors (six mutants, four corpus cases) | ready (post-0.2.0) |
| `MB.21.5.14` | Close the surgical editor's sixteen testable survivors (names, cross-view patches, one-line empty blocks, `transactions` bounding a directive) | ready (post-0.2.0) |

## Evidence

One subsection per epic acceptance item. `make check` and `make e2e` are the only two rows that are not
green, for reasons stated there.

### E1 — every child `done` with a `RESULT.md`; tree has no ready/active/blocked/failed node

```console
$ uv run --no-project tools/plan.py tree --under MB.21 | head -4
○ MB.21 [epic/ready] Replace the v1 manifest with the purpose-built Blastfile DSL (docs first, then parser, fmt and cutover)
  ● MB.21.1 [task/done] @doc-writer a1 Write the Blastfile basic guide and complete language reference, register them, pass dx + ai-ergonomics critics
    ● MB.21.1.1 [task/done] @doc-writer a1 Purge the retired manifest format from the Blastfile pages
  ● MB.21.2 [task/done] @designer a3 ⇠MB.21.1 Design the Blastfile lexer, parser, CST and cutover (ADR-0012) with security/implementability/testability/consistency critics

$ uv run --no-project tools/plan.py tree --under MB.21 | tail -13
    ● MB.21.5.2 [task/done] @implementer a2 ⇠MB.21.5.1 Execute Blastfile doc fences live, flip docs_fences strict (retired opening gone) and guard the retired manifest name
    ● MB.21.5.3 [task/done] @test-curator a3 ⇠MB.21.4.4,MB.21.5.2 Audit and mutation-test the Blastfile parser, CST and editor (test-curator)
    ● MB.21.5.4 [task/done] @visual-checker a1 ⇠MB.21.5.2 Browser proof of the Blastfile pages, nav and both colour schemes (visual-checker)
    ● MB.21.5.5 [task/done] @implementer a4 Close the purity gate's nested-use evasion (R24) and prove it in both directions
    ○ MB.21.5.6 [task/ready] @implementer Post-0.2.0 coverage: pin blastfile::read's accepted path so the read bound's mutants die in-crate
    ● MB.21.5.7 [task/done] @implementer a1 Land the design's >= 20-document round-trip corpus floor and the hoisted-comment owner case
    ○ MB.21.5.8 [task/ready] @implementer Give P4's postcondition oracle a direct test and a truthful section for unresolved grants
    · MB.21.5.9 [task/proposed] @implementer Pin EditError::source so the editor's error chain is walked by a test
    · MB.21.5.10 [task/proposed] @implementer Remove the uid-0 silent skip from critical_blastfile_fmt's write-failure case
    ● MB.21.5.11 [task/done] @implementer a1 Pin the named-store grant guard (coverage; a regression would accept a typo'd grant)
    · MB.21.5.12 [task/proposed] @implementer Close the four message-only refusal-matrix survivors (lexer name_run, three parser guards)
    ○ MB.21.5.13 [task/ready] @implementer Close the printer and pattern-name survivors (six mutants, four corpus cases)
    ○ MB.21.5.14 [task/ready] @implementer Close the surgical editor's sixteen testable survivors (names, cross-view patches, one-line empty blocks, transactions bounding a directive)
```

`plan.py tree` hides done containers by default, so `3`, `4` and `4.5` only appear with `--all`
(`MB.21.5` stays `ready`: its fourteen children are not all closed):

```console
$ uv run --no-project tools/plan.py tree --under MB.21 --all | grep -E 'epic/'
○ MB.21 [epic/ready] Replace the v1 manifest with the purpose-built Blastfile DSL (docs first, then parser, fmt and cutover)
  ● MB.21.3 [epic/done] ⇠MB.21.2 Blastfile toolchain: bounded lexer, LL(1) parser, path compiler, fmt, surgical editor, docs_fences
  ● MB.21.4 [epic/done] ⇠MB.21.3 Cut the product over to Blastfile: discovery, runtime parity, check/--learn, Python, Makefile, e2e, and the erasure of the retired reader
    ● MB.21.4.5 [epic/done] ⇠MB.21.4.4 Erase the retired manifest format from Rust: delete the reader, the TOML synthesis in stage1, the toml dependency and every remaining mention
  ○ MB.21.5 [epic/ready] ⇠MB.21.4 Docs migration and release proof: live fences, retired-TOML guard, mutation audit, visual check
```

Every closed node has its result on disk (the three closed epics included):

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
MB.21.5/MB.21.5.2/RESULT.md
MB.21.5/MB.21.5.3/RESULT.md
MB.21.5/MB.21.5.4/RESULT.md
MB.21.5/MB.21.5.5/RESULT.md
MB.21.5/MB.21.5.7/RESULT.md
MB.21.5/MB.21.5.11/RESULT.md
RESULT.md
```

(`5.5`, `5.7` and `5.11` all closed while this record was being written, which is why their lines above
were re-run at `bc0b492`.)

`MB.21.5.6`, `5.8`–`5.10` and `5.12`–`5.14` have no `RESULT.md` because they are the post-0.2.0
follow-ups (§Rulings).
**Item 1 is therefore open as written** (seven children are still `ready`/`proposed`). The ruling to
record: `5.5` and `5.7` were the pre-gate pair and both landed (`4b41522`, `9c56183`); `5.11` also
landed (`2c2c9b3`) although the audit ruled it coverage-only post-0.2.0; `5.6`, `5.8`–`5.10` and
`5.12`–`5.14` are explicitly post-0.2.0.

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

### E3 — `make check` and `make e2e` exit 0

**`make check` — `BLOCKED BY EXTERNAL (MB.23.1)`, not a pass and not an MB.21 failure.** Exit `2`, at
`docs-test`, on `MB.23.1`'s uncommitted docs. Everything before `docs-test` in the target list ran green
(163 `test result: ok` lines), and every target after it was run separately and is green (below).

```console
$ make check > /tmp/opencode/mb21-make-check.log 2>&1; echo exit=$?
exit=2
$ grep -cE '^test result: ok' /tmp/opencode/mb21-make-check.log
163
$ grep -E '^FAIL|fences:|Error 1' /tmp/opencode/mb21-make-check.log | tail -3
FAIL  docs/reference/cli.md  fence 1  dev-flags             0.2s  blastit dev exited before ready; stdout:
48 fences: 30 executed, 18 accounted, 1 failed · 20.7s
make: *** [Makefile:331: docs-test] Error 1
$ grep -B1 'unknown argument' /tmp/opencode/mb21-make-check.log
stderr:
blastit dev: unknown argument `--no-reload`
```

The failing fence is `docs/reference/cli.md`'s first console block, whose added line is `MB.23.1`'s:

```console
$ git diff docs/reference/cli.md | grep -n 'no-reload'
21:+$ blastit dev --no-reload
29:+| `--no-reload` | reload on | serve without live reload: nothing is watched, and the boot is silent about it |
$ grep -rn 'no_reload\|no-reload' crates/ python/ --include='*.rs' --include='*.py'
(no output)
$ uv run --no-project tools/plan.py show MB.23.1 | sed -n '5,6p'
kind: task
status: active
```

`cargo fmt`, clippy (`-D warnings`), `cargo doc -D warnings`, the Rust suites, `test-critical`,
`py-lint`, `docs-unit` and `docs-check` all ran before that failure without an error line in the log
(`grep -E 'test result: FAILED|^error'` is empty).

The eight targets after `docs-test` in `check`, run on their own:

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

**`make e2e` — FAIL, and the cause is MB.21's own docs sweep, not the environment.** `e2e/run.py`
asserts the two deploy pages' pooler wiring *before* Docker starts and returns `1` on the first
failure, so this run never reached a container scenario: one docs promise still pins the retired
spelling that `MB.21.5.1` removed.

```console
$ make e2e > /tmp/opencode/mb21-e2e.log 2>&1; echo exit=$?
exit=2
$ tail -6 /tmp/opencode/mb21-e2e.log
ok    digitalocean: the page says what Django does about prepared statements
ok    digitalocean: no command needs an id the page never shows how to get
ok    digitalocean: the tag placeholder is the `$GIT_SHA` the build pushed
ok    digitalocean: the spec still attaches the cluster
docs_deploy: 44/45 documented pooler promises hold
make: *** [Makefile:501: e2e] Error 1
$ grep -n '^FAIL' /tmp/opencode/mb21-e2e.log
504:FAIL  digitalocean: the proxy's `ca_cert` input and the service env agree
```

The checker still requires `ca_cert = "DATABASE_CA_CERT"`, the v1 spelling that `809143d`
(`MB.21.5.1`) replaced on the page:

```console
$ sed -n '191,193p' e2e/docs_deploy.py
    checks.that(
        'ca_cert = "DATABASE_CA_CERT"' in page and "key: DATABASE_CA_CERT" in services,
        "digitalocean: the proxy's `ca_cert` input and the service env agree",
$ git show 809143d -- docs/deploy/digitalocean-paas.md | grep -nE '^[-+].*CA_CERT'
36:-change two things: `input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }`,
40:+change two things: the split `postgres` block with `$DATABASE_CA_CERT` beside
$ sed -n '156,161p' e2e/run.py
    # regression there is a docs defect, not a container one, so it is asserted
    # before Docker starts (no Docker, no network).
    if docs_deploy.main() != 0:
        return 1
    env = {
```

**This is the one epic acceptance item that is genuinely red on this tree**: `e2e/docs_deploy.py` is not
in any `MB.21` child's `paths`, so the fix belongs to a small leaf that owns it (or to `MB.21.5`'s
lead), not to a re-run. `MB.21.4.4`'s "take `make e2e` green" predates `MB.21.5.1`'s page rewrite.

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
$ grep 'docs/local.md  fence 6' /tmp/opencode/mb21-docs-test.log
PASS  docs/local.md  fence 6  denial                2.1s  the printed denial matches the documented fields
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
$ timeout 300 cargo test -p blastit-manifest --locked > /tmp/opencode/mb21-manifest-test2.log 2>&1; echo exit=$?
exit=0
$ grep -c '^test result: ok' /tmp/opencode/mb21-manifest-test2.log
33
$ grep -E '^test result: ok|^all doctests' /tmp/opencode/mb21-manifest-test2.log | tail -2
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
all doctests ran in 0.71s; merged doctests compilation took 0.69s
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
9c56183 [MB.21.5.7] Land the >= 20-document round-trip corpus floor
8f538ae [MB.22.1] Document the editor setup: Zed extension, live diagnostics, format-on-save
06874ae [MB.21.5.3] Audit and mutation-test the Blastfile toolchain at 92.0 %
7062ded [MB.20] Rework /frameworks: connector first, framework considerations inside
48d322c [MB.20] Outline the /frameworks rework, both goldfish verdicts PASS
6d2bd41 [MB.21.5.4] Prove the Blastfile pages render in both schemes and on mobile
6bfd1cc [MB.21.5.2] Execute the Blastfile fences live and guard the retired name forever
3452a7a [MB.21.5.1] Sweep the ME brief's manifest reference
809143d [MB.21.5.1] Purge the retired manifest from the site, README and ARCHITECTURE
1f3ffc8 [MB.21.4] Log the closed gate
37eace8 [MB.21.4] Gate passed: the product reads one format
335f564 [MB.21.4.10] Speak Blastfile in every remaining user-facing sentence
```

Docs gates, run individually. (There is no `docs-component` target in this Makefile; `docs-unit`,
`docs-check` and `docs-test` are the three that exist, and all three were run.)

```console
$ make docs-unit > /tmp/opencode/mb21-docs-unit.log 2>&1; echo exit=$?; tail -2 /tmp/opencode/mb21-docs-unit.log
exit=0
107 passed, 65 subtests passed in 1.44s
$ timeout 1500 make docs-test > /tmp/opencode/mb21-docs-test.log 2>&1; echo exit=$?
exit=2
$ tail -3 /tmp/opencode/mb21-docs-test.log
48 fences: 30 executed, 18 accounted, 1 failed · 50.9s
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
- **`--no-reload` is an external blocker, not an MB.21 defect.** `docs/reference/cli.md` and
  `docs/local.md` are `MB.23.1`'s uncommitted working-tree edits; the flag does not exist in
  `crates/`/`python/`, so the fence refuses it. Row E3 is `BLOCKED BY EXTERNAL (MB.23.1)`; re-run
  `make check` once `MB.23.1` lands or its docs are reverted.
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

- **`e2e/docs_deploy.py:192` is stale and blocks `make e2e`** (see E3): it requires the v1
  `ca_cert = "DATABASE_CA_CERT"` on `docs/deploy/digitalocean-paas.md`, which `MB.21.5.1` re-spelled
  as the `postgres` block with `$DATABASE_CA_CERT`. The page is right; the checker needs a leaf that
  owns `e2e/docs_deploy.py`. Until then no container scenario runs at all.
- **Post-0.2.0 nodes:** `MB.21.5.6`, `MB.21.5.8`–`MB.21.5.10` and `MB.21.5.12`–`MB.21.5.14`, per
  `docs/testing/AUDIT-MB.21.md` §5.5. The pre-gate pair landed (`5.7` as `9c56183`, `5.5` as
  `4b41522`), and `5.11` landed too (`2c2c9b3`) although that table calls it coverage-only after 0.2.0
  — the epic lead may want to record that as a deviation from the ruling.
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
- **Human requests for the conductor:** `plan.py human open` prints **`H-013`** only (answered by
  `MB.21.3.3`'s log — `error.rs` widened into 3.3 and 3.8 — so it needs `plan.py human close H-013`).
  `H-011` is already `[closed]` in `plan/HUMAN.md:116`; the brief's "open H-011" is stale, nothing to
  close. `H-010`–`H-012` are likewise closed there.
- **`MB.21.5.5` and `MB.21.5.11` committed after the rows above were measured** (`4b41522`,
  `2c2c9b3`). Their bytes were already in the tree when `make manifest-purity`, `cargo test -p
  blastit-manifest` and `make check` ran, so those rows describe them; a final re-run at the closing
  `HEAD` is still the lead's call.
