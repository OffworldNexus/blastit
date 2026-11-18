# RESULT MB.35.2 — the portable `blastit-lsp` user contract, written before the crate

Owner: doc-writer. Worktree `.worktrees/release-0.2.0` (`release/0.2.0`), base `d5e43e9`,
attempt 1. The lead closed iteration after five goldfish rounds; this file records all five
verdicts, including the findings not acted on.

## 1. Files changed

| File | Lines | What |
|---|---|---|
| `docs/reference/cli.md` | +73/−6 | `blastit-lsp` row in the command index; `## blastit lsp` (Linux) keeps its contract and gains the delegation sentence and the `--help`/`2` facts it lacked; new `## blastit-lsp`; two `json` settings shapes |
| `docs/local.md` | +43/−18 | opens with the Linux/editor split; `## Editors` step 1 is an executable checkout path (clone → `rustup target add wasm32-wasip2` → dev extension); step 2 is the one resolution order; closing paragraph states what macOS/Windows lack and where the boundary is explained |
| `docs/getting-started.md` | +8/−2 | one sentence in `## 1. Install`: no `uv add blastit` build on macOS/Windows, the editor comes from the Zed extension, serving is Linux; the sdist is scoped to other Linux targets and names the C compiler |
| `editors/zed/README.md` | +34/−13 | registry-then-checkout install; the resolution table; the "never `blastit` without `arguments`" warning; one repo-relative link to the CLI page |
| `docs/troubleshooting.md` | +7/−0 | new `## The editor` with two rows (cannot find the server; no asset / download fails) |
| `python/tests/test_docs_assets.py` | 0 | byte-identical to `HEAD` — no `text` block was added (see §3) |
| `docs/design/portable-lsp.review-dx.md` | new | critic report, `dx` lens |
| `docs/design/portable-lsp.review-ai-ergonomics.md` | new | critic report, `ai-ergonomics` lens |
| `plan/nodes/MB/MB.35/MB.35.2/RESULT.md` | new | this file |

`docs/research/INDEX.md` (modified in the tree) is MB.35.1's entry, not this node's.

## 2. The brief's list, item by item

| Requirement | State |
|---|---|
| new `## blastit-lsp` right after `## blastit lsp` | yes, `docs/reference/cli.md:88` |
| what it is (same server, its own binary for Linux/macOS/Windows) | yes, first sentence |
| how to run (path, no arguments, stdin/stdout, extensionless `Blastfile`) | yes |
| what it never does (`./Blastfile`, `BLASTIT_*`, socket, app, privilege) | yes, one sentence, with "`blastit` itself stays the Linux-only server" |
| exit codes `0`/`1` | yes (`0` after `shutdown`; `1` on exit-without-`shutdown`, unframeable stream, I/O) |
| where to get it | the extension download, the five assets, `/releases/latest/download/<asset>`, and "on Linux `blastit lsp` needs no download" |
| `binary.path` | file not folder, macOS JSON, doubled-backslash Windows value, Linux `blastit` + `"arguments": ["lsp"]` JSON, "without `arguments` it starts the serving server and fails closed" |
| `## blastit lsp` says it delegates | "This command delegates to the same implementation as the portable `blastit-lsp`." |
| macOS/Windows = editor only | `cli.md` ("serving an app needs Linux, as root or with the capability set"), `local.md` (plus `dev`/`check`/`fmt`/`lsp` absent, "run the audit in CI or a Linux container"), `getting-started.md` |
| Zed README's two lines, no asset URLs | table with the four resolution rows and the install line; `grep -n 'blastit-lsp-\|releases' editors/zed/README.md` → no hits; the CLI page is the single source |
| no new navigation entry | none touched |

Anchors: `docs/reference/cli.md` has two sections whose text would slugify to
`blastit-lsp`, so the Linux command's heading reads ``## `blastit lsp` (Linux)``
(`#blastit-lsp-linux`) and the portable binary owns `#blastit-lsp`. That name is identical
on the built site and on GitHub, unlike an `attr_list` id (which GitHub renders literally
and then slugs into a different anchor). Verified: `site/reference/cli/index.html` has
`id="blastit-lsp"` and `id="blastit-lsp-linux"`; `site/local/index.html` links both.

## 3. Fence counts (all four)

| Count | Before | After | Note |
|---|---|---|---|
| `text` blocks on public pages | 33 | **33** | no `text` block added; `EXPECTED_FENCES` still pins 29 highlighted + 4 plain, so nothing new to pin |
| `console` fences (executed by `docs-test`) | 48 | **48** | 30 executed, 18 accounted, 0 failed |
| `json` fences | 6 | **8** | the two settings shapes on `docs/reference/cli.md`; the text inventory does not track them, and `zed-check`'s settings check still parses `docs/local.md`'s |
| `text`/`json` blocks on `docs/reference/cli.md` | 0/0 | **0/2** | measured with `fences.scan_page` |

No block needed pinning. The first draft's `text` block (`tar`/`Expand-Archive` onto
`PATH`) was removed rather than pinned: both lenses showed it could not be followed (no
URL, no arch choice, a directory that is not on a stock macOS GUI `PATH`, two shells in
one block), and the section now names the tools inline and the five assets in a table.

## 4. Acceptance — full output

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
exit=0
```

```console
$ make doc-tone docs-check
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.21s
exit=0
```

```console
$ make docs-test
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py \
	python/tests/test_docs_assets.py \
	python/tests/test_release_scripts.py \
	scripts/test_doc_tone.py -q
............................................................................... [ 63%]
.............................................                            [100%]
124 passed, 65 subtests passed in 3.34s
scripts/build-wheel.sh -o "/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/docs-wheel"
README.md: already absolute
    Finished `release` profile [optimized] target(s) in 0.10s
⚠️  Warning: `build-backend` in pyproject.toml is not set to `maturin`, packaging tools such as pip will not use maturin to build this project.
🍹 Building a mixed python/rust project
🐍 Found CPython 3.14 at /home/remy/.cache/uv/archive-v0/A78_05IgIV57nQaD/bin/python3
🔗 Found pyo3 bindings
    Finished `release` profile [optimized] target(s) in 0.11s
📦 Including files matching "python/blastit/_bin/blastit-dbproxy"
📦 Built wheel for CPython 3.14 to /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/docs-wheel/blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl: Tag: cp314-cp314-manylinux_2_39_x86_64
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok   /var/tmp/blastit-wheel.6gloeN/_blastit0.so: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   /var/tmp/blastit-wheel.6gloeN/_blastit0.so: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   /var/tmp/blastit-wheel.6gloeN/_blastit0.so: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
ok   /var/tmp/blastit-wheel.6gloeN/_blastit0.so: no BLASTIT_TEST_RELOAD_PAUSE (absent, as shipped)
.                                                                        [100%]
1 passed in 0.02s
harness: flag tables checked against --help: psql
harness: wheel blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 5234c8f8988d · per-run uv cache /tmp/opencode/brd-gbfdpazf/uv-cache · installed package verified against the built wheel
PASS  README.md  fence 1  install               0.5s  installed blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl and verified it against the built wheel
PASS  README.md  fence 2  dev                   0.1s  served on 127.0.0.1:8080
PASS  README.md  fence 3  dev-curl              0.1s  served and answered
PASS  docs/index.md  fence 1  summary               1.4s  dev, check and preset agree
PASS  docs/getting-started.md  fence 1  install               0.3s  installed blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl and verified it against the built wheel
PASS  docs/getting-started.md  fence 2  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/getting-started.md  fence 3  dev-curl              0.1s  served and answered
PASS  docs/blastfile.md  fence 1  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/blastfile.md  fence 2  check                 1.1s  check reported a clean suite
PASS  docs/blastfile.md  fence 3  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/local.md  fence 1  dev                   0.0s  served on 127.0.0.1:8080
PASS  docs/local.md  fence 2  pgstub                0.0s  substituted: `docker` is not run; the harness's PostgreSQL wire stub answered StartupMessage and SELECT 1 instead
PASS  docs/local.md  fence 3  page-manifest         0.1s  the page's `postgres {}` view answered 500 with a `sql.denied` line and SQLSTATE 42501
PASS  docs/local.md  fence 4  learn-git             2.2s  learn wrote the manifest, then check was clean
PASS  docs/local.md  fence 5  dev-learn             0.1s  one pass added 1 route(s) and 1 query(ies), written to the printed manifest
PASS  docs/local.md  fence 6  denial                0.1s  the printed denial matches the documented fields
PASS  docs/ci.md  fence 1  check                 1.2s  check reported a clean suite
PASS  docs/ci.md  fence 2  findings              1.1s  six documented findings, one line each
PASS  docs/ci.md  fence 3  json                  0.9s  one JSON document with 6 findings
PASS  docs/ci.md  fence 4  check                 1.0s  check reported a clean suite
PASS  docs/ci.md  fence 5  learn-sequence        2.8s  learn, dry-run and check agree
PASS  docs/deploy/digitalocean-paas.md  fence 1  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/deploy/digitalocean-paas.md  fence 2  static                0.1s  statically checked every command, flag and snippet (Deployment recipe: doctl and Docker run against a registry and a cloud account, so the commands are statically checked instead of executed.)
PASS  docs/deploy/digitalocean-paas.md  fence 3  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: `doctl databases create` needs a DigitalOcean account, so the command is statically checked instead of executed.)
PASS  docs/deploy/digitalocean-paas.md  fence 4  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: `doctl databases user create` needs the cluster the fence above creates in a DigitalOcean account, and psql runs the owner's role and grant SQL against it.)
PASS  docs/deploy/digitalocean-paas.md  fence 5  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: `doctl databases pool create` needs the cluster and the app user the fences above create.)
PASS  docs/deploy/digitalocean-paas.md  fence 6  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: `doctl apps create` needs a DigitalOcean account, the pool from the fence above, and the image fence 2 builds.)
PASS  docs/deploy/compose-traefik.md  fence 1  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/deploy/compose-traefik.md  fence 2  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: an image build, a push and a Compose stack on a host with a registry; the boot line it shows is checked as JSON.)
PASS  docs/deploy/anything-else.md  fence 1  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: setpriv with capability flags needs the production uid layout, which this unprivileged gate deliberately cannot create.)
PASS  docs/deploy/anything-else.md  fence 2  example               0.0s  accounted: Measures X-Forwarded-For through a real edge the harness does not have; the trusted-proxy decision itself is a settings, unit and e2e concern.
PASS  docs/deploy/anything-else.md  fence 3  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: `psql` runs against a managed database's direct port, which this gate does not have, so the flags and the `SHOW TimeZone` read-back are checked against the installed binary.)
PASS  docs/deploy/anything-else.md  fence 4  static                0.0s  statically checked every command, flag and snippet (Deployment recipe: Docker runs the published image with capabilities and a memory limit.)
PASS  docs/frameworks/python/django.md  fence 1  check                 1.2s  check reported a clean suite
PASS  docs/frameworks/python/flask.md  fence 1  install-external      0.0s  accounted: Installs flask, psycopg and pytest from the package index; only blastit comes from this workspace. blastit's own install runs in README.md and Getting started.
PASS  docs/frameworks/php/index.md  fence 1  static                0.0s  statically checked every command, flag and snippet (The gate builds no PHP project fixture: an `entrypoint php` file needs a `public/index.php` app and a host NTS `libphp`, and this offline gate installs neither. It checks the documented `blastit dev --learn` command and its flags against the installed CLI, and parses the page's Blastfile block.)
PASS  docs/frameworks/php/index.md  fence 2  example               0.0s  accounted: No PHP project fixture or host NTS `libphp` exists in this offline gate, so the `fmt --check` gate cannot run; the command is documented in docs/blastfile.md and the page's Blastfile block is parsed by the docs_fences gate.
PASS  docs/frameworks/static-and-spa.md  fence 1  example               0.0s  accounted: `uvx` installs blastit from the package index, which this offline gate does not have; e2e/static_assets.py compares this banner with a container's own `blastit dev`, line for line.
PASS  docs/frameworks/static-and-spa.md  fence 2  example               0.0s  accounted: Vite writes `dist/` and this page serves `public/`; e2e/static_assets.py names this command as its one Node skip, and its fixture is that build output.
PASS  docs/frameworks/static-and-spa.md  fence 3  dev-curl              2.1s  served and answered
PASS  docs/frameworks/static-and-spa.md  fence 4  dev-curl              2.1s  served and answered
PASS  docs/frameworks/static-and-spa.md  fence 5  example               0.0s  accounted: `uvx` reaches the package index, which the offline gate does not have; e2e/static_assets.py runs the same `blastit check` and `--json` in its container and asserts the exit code and the document.
PASS  docs/frameworks/static-and-spa.md  fence 6  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/reference/cli.md  fence 1  dev-flags             0.2s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 2  check-flags           4.9s  each check flag runs
PASS  docs/reference/cli.md  fence 3  serve-flags           0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                 0.1s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked every command, flag and snippet (The e2e harnesses need Docker and root; the runnable-docs gate is unprivileged and Docker-free, so only the command names and the scripts they name are checked.)

48 fences: 30 executed, 18 accounted, 0 failed · 24.9s
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 2.64s
$ uvx --offline ruff format --check python/tests/test_docs_assets.py
1 file already formatted
```

```console
$ git diff --stat -- docs editors/zed/README.md python/tests/test_docs_assets.py
 docs/getting-started.md |  8 ++++--
 docs/local.md           | 43 ++++++++++++++++++-----------
 docs/reference/cli.md   | 73 ++++++++++++++++++++++++++++++++++++++++++++-----
 docs/research/INDEX.md  |  1 +
 docs/troubleshooting.md |  7 +++++
 editors/zed/README.md   | 34 +++++++++++++++---------
 6 files changed, 128 insertions(+), 38 deletions(-)
```

One earlier `docs-test` run (04:33, while the worktree was under load) failed
`docs/local.md` fence 5 (`dev-learn`) with `exited -9 after SIGINT (expected 0)`; the
fence passed on every later run, including the full run above. No file in this node
touches that fence.

`plan validate` was briefly red on `MB.34.7` (a concurrent node in this shared worktree:
`task has children`, then `task needs owner`) and green again afterwards; no MB.35.2 file
was involved.

## 5. Goldfish reviews — all five rounds, including what was not acted on

Each round was a fresh `critic` session with only the five page paths, one lens and an
output path: `docs/design/portable-lsp.review-dx.md` and
`docs/design/portable-lsp.review-ai-ergonomics.md` (round 5 overwrote round 4).

| Round | dx | ai-ergonomics | What changed |
|---|---|---|---|
| 1 | FAIL 4B/8M | FAIL 1B/7M | one install story; no invented PATH; `2`; JSON; platform scope |
| 2 | FAIL 2B/4M | FAIL 0B/10M | remove `uv` from `## blastit lsp`; executable Mac path; scope the manual download; `--help`; the `arguments` trap |
| 3 | FAIL 1B/4M | FAIL 0B/6M | `cargo` contradiction; `local.md` opening; five asset names; exit `1` causes |
| 4 | FAIL 0B/5M | FAIL 1B/4M | portable `--help`/`2` removed (no `main` parses argv); two troubleshooting rows; corrected parser oracle; quarantine; `/releases/latest/download/<asset>` |
| 5 | FAIL 0B/5M | FAIL 1B/4M | closing pass, written against a pre-edit snapshot (see below) |

**Round 5 was stale.** Both reports quote strings that the pages no longer contain —
`"`2` for an argument it does not take"` and `"`blastit-lsp --help` prints the usage"`
in the portable section (ai B1), `"an ungranted `query`"` (ai M4), `"reads the shell `PATH`
Zed captured"` (ai M1), the one-row troubleshooting cell (dx M4), and
`"the only step needing `cargo`"` without the wasm target (dx M1). Each was already fixed
before those reports were written. Checked with `grep` against the current pages; the one
surviving `2 for an argument it does not take` is in `## blastit lsp`, where it is true.

Round-5 findings and what happened to them:

| Finding | Resolution |
|---|---|
| dx M1 — the install path omits `rustup target add wasm32-wasip2` | **applied**: both `docs/local.md` step 1 and `editors/zed/README.md` now name the Rust toolchain *and* the target, before `zed: install dev extension` |
| dx M2 — no runnable command on macOS/Windows; `--help` is POSIX-only | **partly applied**: the `--help` sentence is gone, and the unpack names its tools (`tar -xzf` / `Expand-Archive`). Not implemented as a `text` block: that would add an untested, network-dependent block and re-open the 33-block inventory for a command the table already makes unambiguous |
| dx M3 — the README promises an `arguments` shape the CLI page never shows | **applied**: a second `json` fence on the CLI page with `"arguments": ["lsp"]` |
| dx M4 — one troubleshooting cell holds four failures | **applied**: two rows, "cannot find" and "no asset / download fails"; not split into four, so the section stays one screen and the brief's one-row intent survives in shape |
| dx M5 — macOS quarantine makes "proves the file runs here" false | **applied**: the proof sentence is gone, and the manual path carries the `xattr -d com.apple.quarantine` remedy |
| ai B1 — `--help` and exit `2` are a CLI surface no task builds | **applied** via the alternative: the portable section promises only no arguments and `0`/`1`; `blastit lsp` keeps `--help`/`2` because it has them |
| ai M1 — the `PATH` sentence is inverted | **applied**: "reads your shell's `PATH`, which may not be the one Zed captured at launch, so restart Zed after changing it"; the reliable answer (`binary.path`) leads the row. Not added: the Dock-launch sentence (see §6) |
| ai M2 — quarantine unhandled | **applied**: the `xattr` remedy |
| ai M3 — asset names without a URL | **applied**: "Each asset is `https://github.com/OffworldNexus/blastit/releases/latest/download/<asset>`, or the same name under a tag" |
| ai M4 — "ungranted `query`" is not an editor diagnostic | **applied**: "a `query` that does not parse gets one diagnostic at its first token — the sentence `blastit check` prints on Linux" |

**No BLOCKER or MAJOR from round 5 is left unaddressed in substance.** The two
partials are stated above (dx M2's form; ai M1's macOS Dock sentence). Verdicts as
returned: round 5 `VERDICT: FAIL` for both lenses; rounds 1–4 likewise. The pages were
revised after every round; the reports in `docs/design/` are the record of each pass.

### MINORs recorded, not fixed

| Lens | Finding | Why not fixed |
|---|---|---|
| dx | `blastit lsp --help` needs `uv run` under the `uv add blastit` install | the line documents the installed CLI; the venv case is `uv run blastit lsp --help`, one word a reader already knows from `uv run blastit dev` |
| dx | a `uv add` install leaves `blastit` in `.venv/bin`, off the `PATH` Zed captured, so the extension downloads instead | it is MB.33's resolution consequence and belongs in its page sentence; also a hidden-variable fix under "one canonical way per task" |
| dx | "with no arguments" in `docs/local.md` reads as if it applies to `blastit lsp` | the command name contains its argument, and the Zed README table spells the pair out |
| dx, ai | the aarch64 Linux row is an asset the research calls "declared, unbuilt" | the row *is* the contract MB.35.5 builds and MB.33 codes against; "Nothing else is built" states the closed set, and dropping the row would desynchronise the extension's table |
| dx | the squiggle's `line:col` is compared to `blastit check`, which macOS lacks | the comparison is positional (the same diagnostics), and the sentence already says "on Linux" where it matters |
| ai | another LSP client also needs a `Blastfile` selector and `languageId` | the page's audience for that sentence is "not Zed"; `editors/zed/extension.toml` is the selector's source of truth, and the page stays lean |
| ai | `lsp` → `blastit-lsp` (arrow) vs the dotted key | a settings generator reads the JSON two paragraphs above; one spelling change is cosmetic |
| ai | `docs/local.md` says "else `blastit lsp`" where the README says "`blastit` on your `PATH`" | same fact, two registers; the README's table is the exact one |
| ai | cache wording ("once per extension version" vs "downloaded once") | three short phrases for one fact; the CLI page carries the precise one |
| ai | "one directory" does not name the archive's inner path | the settings paragraph says "not its folder" and `binary.path` takes the file, so the inner name never has to be known |
| ai | `Blastfile:<line>:<col>:` vs "`Blastfile:` prefix" | pre-existing wording in `## blastit lsp`; a path/position detail a reader sees in the message itself |
| ai | the version is only in `initialize`'s `serverInfo.version`; no `--version` | true, but documenting a protocol field as the version check is a new contract `MB.35.4` does not own; recorded for it in §7 |

## 6. Deviations from the brief, with reasons

1. **`--help` and exit `2` are not promised for `blastit-lsp`.** The brief says "no
   arguments" and "exit codes `0`/`1`", and `MB.35.4` ships `bin blastit-lsp` as a `main`
   around `blastit_lsp::run()`, which parses no argv; promising `--help` would document a
   hang. `blastit lsp` keeps both because it has them today.
2. **The `text` block was removed** (see §3): it could not be followed and contradicted the
   extension's own download. The brief's pinning rule is satisfied with zero new blocks.
3. **The Linux command's heading gains `(Linux)`**, no `attr_list` id. It explains the
   command's platform, keeps one anchor name for the site and GitHub, and gives the
   portable section the guessable `#blastit-lsp` that the brief's links expect.
4. **`uv run blastit lsp` removed** from `## blastit lsp`, which the brief assigned to
   MB.33: both lenses blocked on it and MB.33's acceptance rejects it.
5. **Two troubleshooting rows, not one**: one cell with four failures was graded MAJOR
   twice.
6. **Five asset names on the CLI page**: "the asset for your platform" was undecidable for
   an agent without a list, and `MB.33`/`MB.35` already fix exactly five names.
7. **`--version` not documented**: no such flag exists anywhere.

## 7. Follow-ups (outside this node's paths)

- `docs/deploy/index.md` should open with one sentence: the server is Linux only (root or
  capabilities + seccomp); macOS/Windows get `blastit-lsp` and never serve. Both
  "serving is Linux" sentences here link there and carry the claim themselves.
- `MB.35.4`: if the portable binary gains argv handling, `blastit-lsp --help` (exit `0`,
  usage line `usage: blastit-lsp`) is the launch probe this page would document; it would
  also expose the `build` value and make "pin a version" checkable. Today the version is
  only `initialize`'s `serverInfo.version`.
- `MB.33`: the Linux `uv add blastit` install leaves `blastit` in `.venv/bin`, which is not
  on the `PATH` the extension searches, so the extension downloads instead. That is the
  extension's resolution consequence and its page sentence.
- `docs/local.md` step 3 compares the squiggle position to `blastit check`; a macOS reader
  cannot run it. Wording only, recorded above.
- `blastit fmt` has no row in the CLI command index although `docs/reference/blastfile.md`
  documents it; a pre-existing one-row gap.
