# RESULT — MB.34.7

**STATUS: done.** The epic is proved from the outside: `docs/testing/E2E-MB.34.md` holds the
four-defect fixture, the CLI/`--json`/LSP parity table, the Quick Fix wire transcript, the
`tree-sitter parse` tree with its capture list, both gate tails, the `NOT RUN` list and the
hazards. No production file was touched: this leaf's only new file is the evidence page.

Branch `release/0.2.0`, base `d5e43e9`, HEAD `1117a10`, attempt 1/3. Nothing was staged,
committed, pushed or planned; no plan status was changed.

## What the evidence says

| Item | Result |
|---|---|
| CLI / JSON / LSP parity | CLI `5:16`, `6:16`, `8:54`, `9:54`; LSP `(4,15)`, `(5,15)`, `(7,53)`, `(8,53)`, same sentences byte for byte; `--json` prints **no** document (exit 2, empty stdout) because these four are configuration refusals, exactly as `docs/ci.md` now says |
| `--json` finding shape | unchanged: the emitter (`audit_v1.rs`, `check.rs`, `cli.rs`) is byte-identical to `5f293985`, and a live old-style defect prints the same `kind`/`fix` members and the same closed `fix.action` set |
| Quick Fix round trip | **`NOT RUN (no Zed on this host)`** — the wire frames are recorded instead: `Parameterize SQL for Blastfile ($?)` → edit → `publishDiagnostics []` → the edited file passes `blastit check` with exit `0` |
| Highlighting | `sql_keyword`/`sql_parameter`/`sql_ellipsis` inside `sql_string`, lowercase statement too; five captures, all inside the fifteen-name theme allowlist, no new capture name |
| Documented commands | `make doc-tone` (30 pages), `make docs-check` (No issues found), `make docs-test` (48 fences: 30 executed, 18 accounted, 0 failed) — green twice |
| Gates | `make check` exit 0 / 218 s; `make e2e` exit 0 / 181 s (8 groups, 411 `ok`) |

## Acceptance

Every command in `# Acceptance` was run; the tails are pasted in `docs/testing/E2E-MB.34.md`
§6–§7. Summary:

```console
$ uv run --no-project tools/plan.py validate                                             plan OK
$ make doc-tone docs-check docs-test        exit=0 · 0 failed · 48 fences · 21.4 s and 24.0 s
$ UV_OFFLINE=1 … pytest python/tests/test_docs_assets.py -q                              52 passed
$ cargo test -p blastit-sqlpolicy -p blastit-manifest       44 suites, 920 passed, 0 failed
$ cargo test -p blastit --test critical_lsp                          52 passed, 0 failed
$ make zed-check                                        OK (47 properties), 26 tests OK
$ timeout 3000 make check                                             184 ok suites, exit=0
$ timeout 2400 make e2e                                      8 × PASS: 0 failure(s), exit=0
```

`docs/testing/E2E-MB.34.md` contains, verbatim: the fixture, the CLI text, the `--json` excerpt
(empty stdout + the four stderr lines), the LSP publish payload, the four `line:col` values in a
parity table, the Quick Fix before/after with `blastit check` exit `0`, the hover text, and the
`tree-sitter parse` subtree. It ends `VERDICT: PASS`.

## Where the brief and the shipped contract differ (recorded, not hidden)

1. **`--json` has no `line:col` for these four defects.** A `query` that does not load exits `2`
   with an empty stdout (`docs/ci.md`: "an empty stdout is a refusal, never a clean audit";
   "the refusal adds no `kind` and no `fix.action`"). So the JSON column of the parity table is
   "— (no document)" for all four, and the shape comparison uses an old-style observation
   finding instead. Making `--json` print them would have been a new finding kind, which the epic
   forbids.
2. **The multi-line "missing `FROM`" is a `JOIN` without `FROM`.** `SELECT id\nWHERE price > $?`
   — the obvious shape — is *valid* PostgreSQL (`SELECT 1 WHERE true`), so it produced no
   refusal and was replaced by a statement that genuinely fails to parse. Its site is the
   statement's first token (`6:16`), which is the documented `Syntax` contract.
3. **Zed is not installed here** (`which zed` empty; no binary in `~/.local`, `/usr/bin`, `/opt`,
   no flatpak), so the Quick Fix click, the rendered hover and a highlighting screenshot are
   `NOT RUN`, with the LSP wire frames and the parse tree standing in.

## Hazards (also in the page, §8)

1. **`README.md` is dirty on purpose — never `git add` it.** `scripts/readme-absolute-links.py`
   (the release-time transformation, `ref` `develop`) is invoked by `scripts/build-wheel.sh`,
   which copies README aside and *restores it on EXIT* (`build-wheel.sh:119-127`), so a gate
   **preserves** the rewrite and never creates it: this copy was left dirty by a direct or
   interrupted run. A `make check`/`make e2e` run therefore reports "README.md: already
   absolute" and leaves the same bytes (identical sha1 before and after). No plan node owns the
   rewrite; never `git add README.md`.
2. **The worktree is shared.** A sibling node (MB.35) edited `docs/getting-started.md`,
   `docs/local.md`, `docs/reference/cli.md`, `docs/troubleshooting.md`, `editors/zed/README.md`,
   `python/tests/test_docs_assets.py` and `plan/nodes/MB/MB.35/*` at 05:14–05:16, mid-session.
   No `crates/**` and no grammar file changed, so the binary, LSP and tree-sitter evidence still
   describe the committed sources; the docs group was re-run on the dirty tree at 05:18 and
   stayed green with identical fence counts.

## Method (repeatable)

```console
$ uv run --no-project /tmp/opencode/mb347/parity.py  <binary> /tmp/opencode/mb347/four/Blastfile
$ uv run --no-project /tmp/opencode/mb347/compact.py <binary> four/Blastfile     # the publish payload
$ uv run --no-project /tmp/opencode/mb347/quickfix.py <binary> fixproj/Blastfile # the round trip
$ cd /tmp/opencode/mb347/fixproj && BLASTIT_PYTHON=<repo>/.venv/bin/python \
      PYTHONPATH=<repo>/python DATABASE_URL=postgres://…@127.0.0.1:55432/mb347 <binary> check
$ cd editors/tree-sitter-blastfile && tree-sitter parse /tmp/opencode/mb347/highlight.Blastfile
$ cd editors/tree-sitter-blastfile && tree-sitter query highlights.scm /tmp/opencode/mb347/highlight.Blastfile
```

An exit-`0` `blastit check` needs `pytest` in the interpreter it runs on (`BLASTIT_PYTHON`) and a
resolvable `DATABASE_URL`; the proof used `postgres:18` in Docker on `127.0.0.1:55432`, stopped
afterwards. Nothing new enters `scripts/`, `Cargo.toml` or the repo: the three scripts above live
under `/tmp/opencode/mb347`.

## Follow-ups (out of scope)

1. **`docs/knowledge/LESSONS.md` is outside this node's `paths`**, so this gotcha is filed here
   instead of appended: *"PostgreSQL accepts `SELECT 1 WHERE true`, so a "missing `FROM`" fixture
   for the template validator must be a statement that really fails to parse (`JOIN` with no
   `FROM`); and `blastit check` runs pytest on the interpreter `BLASTIT_PYTHON` names, or on the
   first `python3` in `PATH` — export it or `check` stops at "pytest is not installed"."*
2. **The page is 302 lines, not the ~150 the node asks for.** The content is the mandated
   evidence: the fixture, four JSON diagnostics, the parity table, the Quick Fix transcript with
   the hover text, the 32-line parse tree, the five captures, two gate tails, the `NOT RUN` list
   and the hazards. Trimming further would mean dropping a tail the brief requires.
3. **`make e2e` logs one `warning:`** — `blastit-php-sapi`'s documented no-toolchain degrade
   inside the wheel container; it is expected there (`make php-sapi-e2e` owns that shim) and the
   gate exits `0`. No action.

## Post-review correction (2026-11-18, reviewer VERDICT: PASS)

One factual correction to my own hazard text, in this file (§Hazards 1) and in
`docs/testing/E2E-MB.34.md` §8.1. The first version claimed "every gate run reproduces it [the
README rewrite]". That is wrong: `scripts/build-wheel.sh` copies `README.md` aside and restores
it from that copy through `trap restore_readme EXIT` (`build-wheel.sh:119-127`), so a gate
**preserves** the rewrite and never creates it — the dirty copy was left by a direct or
interrupted run of `scripts/readme-absolute-links.py`, not by a gate. Measured here on the
reworded text, with `make docs-test` (the gate that invokes `build-wheel.sh`):

```console
$ sha1sum README.md → 2aa38741d6af35415ff9d155c66b9a439a09fe82
$ make docs-test
48 fences: 30 executed, 18 accounted, 0 failed · 39.8s
$ sha1sum README.md → 2aa38741d6af35415ff9d155c66b9a439a09fe82      # identical
$ git diff --numstat README.md → 5  5                              # the same rewrite, untouched
```

`make doc-tone` is still green on the edited pages (`doc-tone: ok — 30 user-facing page(s), no
banned phrase`); `make docs-check` was not re-run because `docs/testing/` is excluded from the
site and its last run (05:18, with this page present) said "No issues found". The "never
`git add` it" instruction stands in both files. Nothing else was touched.
