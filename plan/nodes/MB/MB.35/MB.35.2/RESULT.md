# RESULT MB.35.2 — the portable `blastit-lsp` user contract (attempt 3)

Owner: doc-writer. Worktree `.worktrees/release-0.2.0` (`release/0.2.0`), base `d5e43e9`.
Attempt 1 was accepted except for two MAJOR and eight MINOR findings (fixed in attempt 2);
the reviewer then returned `VERDICT: PASS` with four last MINORs (fixed here). No critic round
was re-run. The fence inventory is still 34 `text` blocks.

## 1. Files changed (vs `HEAD`)

| File | +/− | What |
|---|---|---|
| `docs/reference/cli.md` | +77/−7 | the new `## blastit-lsp` section (attempt 1) plus: "once per Blastit release"; "Nothing else is built **for the editor**"; the "shipped from the release that carries the portable server; earlier tags have no `blastit-lsp-*` asset" line; the Windows value as `C:/Users/you/bin/blastit-lsp.exe` |
| `docs/local.md` | +28/−16 | the Linux/editor opening and the executable Editors path; step 2 collapsed to one sentence plus the CLI link, and now scoped by "Once the release ships `blastit-lsp`" |
| `editors/zed/README.md` | +24/−13 | install line without `rustup target add` (Zed builds it and its target); the resolution table; one line saying what the extension does **until** the release ships `blastit-lsp` |
| `docs/getting-started.md` | +8/−4 | the macOS/Windows sentence, and "On Linux," on the standalone-binary paragraph |
| `docs/troubleshooting.md` | +7/−0 | `## The editor`: dotted key + `where.exe`; the `cargo build -p blastit-lsp` command, scoped by "after the release that ships `blastit-lsp`" |
| `python/tests/test_docs_assets.py` | +9/−6 | pins the one new `text` block as plain (33 → 34), and its docstring now says "29 + 5" |
| `plan/nodes/MB/MB.35/MB.35.2/RESULT.md` | modified | this file |

The two critic reports (`docs/design/portable-lsp.review-dx.md`,
`docs/design/portable-lsp.review-ai-ergonomics.md`) are **evidence, not deliverables**:
`NODE.md`'s `paths` exclude `docs/design/**`, and both files are untracked working notes
(§5.3). `zensical.toml` excludes `docs/design/`, so nothing in them is published.

No other path is mine; `git status` also shows other nodes' files (`ARCHITECTURE.md`,
`plan/**`, `docs/decisions/**`, `docs/design/blastfile-lsp-zed.md`).

## 2. The brief's list, item by item

| Requirement | State |
|---|---|
| new `## blastit-lsp` right after `## blastit lsp` | yes, `docs/reference/cli.md:88` |
| what it is (same server, its own binary for Linux/macOS/Windows) | first sentence |
| how to run (path, no arguments, stdin/stdout, extensionless `Blastfile`) | plus the argv contract: no `--help`, no `--version`, an argument is ignored, and by hand it exits `1` at end of input |
| what it never does (`./Blastfile`, `BLASTIT_*`, socket, app, privilege) | one sentence; "`blastit` itself stays the Linux-only server" |
| exit codes `0`/`1` | `0` after `shutdown`; `1` on exit-without-`shutdown`, unframeable stream, I/O |
| where to get it | the extension download (once per **Blastit release**, once the release ships it), the five assets, `/releases/latest/download/<asset>`, "earlier tags have no `blastit-lsp-*` asset", and the Linux `blastit lsp` path |
| `binary.path` | file not folder; macOS JSON; Windows `C:/Users/you/bin/blastit-lsp.exe` (forward slashes); Linux `blastit` + `"arguments": ["lsp"]` JSON; "`"arguments"` belongs only beside a `blastit` path" |
| `## blastit lsp` says it delegates | "This command delegates to the same implementation as the portable `blastit-lsp`." |
| macOS/Windows = editor only | `cli.md` ("serving an app needs Linux, as root or with the capability set"), `local.md`, `getting-started.md` |
| Zed README's two lines, no asset URLs | resolution rows + install line + the "until the release" line; no asset names or URLs (`grep` clean); the CLI page is the single source |
| no new navigation entry | none touched |

Anchors: `## blastit lsp` (Linux) → `#blastit-lsp-linux`, `## blastit-lsp` → `#blastit-lsp`,
identical on the built site and on GitHub (no `attr_list`). Verified in
`site/reference/cli/index.html` (`id="blastit-lsp"`, `id="blastit-lsp-linux"`).

## 3. Fence counts

| Count | Before | After | Note |
|---|---|---|---|
| `text` blocks on public pages | 33 | **34** | one block, still at `docs/reference/cli.md:113` |
| pinned split | 29 + 4 | **29 + 5** | `test_every_text_block_classifies_as_measured` asserts it |
| `console` fences | 48 | **48** | 30 executed, 18 accounted, 0 failed |
| `json` fences | 6 | **8** | two settings shapes on the CLI page |

The block kept its key across attempts (no re-pin needed):

```text
docs/reference/cli.md:113  key: tar -xzf blastit-lsp-aarch64-apple-darwin.tar.gz  plain
```

Plain because the first non-comment line starts with `tar`, which is not a `Blastfile`
section, directive or grant; it is a `text` block (no `$ ` prompt) because it pairs a macOS
`tar` with a PowerShell `Expand-Archive`.

## 4. Acceptance — full output (attempt 3)

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
```

```console
$ make doc-tone docs-check
uv run --no-project scripts/check_doc_tone.py
doc-tone: ok — 30 user-facing page(s), no banned phrase
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.44s
```

```console
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
    python/tests/test_docs_assets.py -q
....................................................                     [100%]
52 passed in 2.69s
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
124 passed, 65 subtests passed in 4.06s
scripts/build-wheel.sh -o "/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/docs-wheel"
README.md: already absolute
    Finished `release` profile [optimized] target(s) in 0.11s
⚠️  Warning: `build-backend` in pyproject.toml is not set to `maturin`, packaging tools such as pip will not use maturin to build this project.
🍹 Building a mixed python/rust project
🐍 Found CPython 3.14 at /home/remy/.cache/uv/archive-v0/A78_05IgIV57nQaD/bin/python3
🔗 Found pyo3 bindings
    Finished `release` profile [optimized] target(s) in 0.17s
📦 Including files matching "python/blastit/_bin/blastit-dbproxy"
📦 Built wheel for CPython 3.14 to /home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/target/docs-wheel/blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl: Tag: cp314-cp314-manylinux_2_39_x86_64
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok   /var/tmp/blastit-wheel.v8jNIg/_blastit0.so: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   /var/tmp/blastit-wheel.v8jNIg/_blastit0.so: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
ok   /var/tmp/blastit-wheel.v8jNIg/_blastit0.so: no BLASTIT_TEST_RELOAD_FAULT (absent, as shipped)
ok   /var/tmp/blastit-wheel.v8jNIg/_blastit0.so: no BLASTIT_TEST_RELOAD_PAUSE (absent, as shipped)
.                                                                        [100%]
1 passed in 0.02s
harness: flag tables checked against --help: psql
harness: wheel blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl sha256 5ac0218b3233 · per-run uv cache /tmp/opencode/brd-7sqaw9bb/uv-cache · installed package verified against the built wheel
PASS  README.md  fence 1  install               0.7s  installed blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl and verified it against the built wheel
PASS  README.md  fence 2  dev                   0.1s  served on 127.0.0.1:8080
PASS  README.md  fence 3  dev-curl              0.1s  served and answered
PASS  docs/index.md  fence 1  summary               1.6s  dev, check and preset agree
PASS  docs/getting-started.md  fence 1  install               0.3s  installed blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl and verified it against the built wheel
PASS  docs/getting-started.md  fence 2  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/getting-started.md  fence 3  dev-curl              2.1s  served and answered
PASS  docs/blastfile.md  fence 1  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/blastfile.md  fence 2  check                 1.1s  check reported a clean suite
PASS  docs/blastfile.md  fence 3  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/local.md  fence 1  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/local.md  fence 2  pgstub                0.0s  substituted: `docker` is not run; the harness's PostgreSQL wire stub answered StartupMessage and SELECT 1 instead
PASS  docs/local.md  fence 3  page-manifest         0.1s  the page's `postgres {}` view answered 500 with a `sql.denied` line and SQLSTATE 42501
PASS  docs/local.md  fence 4  learn-git             2.4s  learn wrote the manifest, then check was clean
PASS  docs/local.md  fence 5  dev-learn             0.1s  one pass added 1 route(s) and 1 query(ies), written to the printed manifest
PASS  docs/local.md  fence 6  denial                2.1s  the printed denial matches the documented fields
PASS  docs/ci.md  fence 1  check                 1.3s  check reported a clean suite
PASS  docs/ci.md  fence 2  findings              1.2s  six documented findings, one line each
PASS  docs/ci.md  fence 3  json                  1.2s  one JSON document with 6 findings
PASS  docs/ci.md  fence 4  check                 1.4s  check reported a clean suite
PASS  docs/ci.md  fence 5  learn-sequence        3.7s  learn, dry-run and check agree
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
PASS  docs/frameworks/python/django.md  fence 1  check                 1.3s  check reported a clean suite
PASS  docs/frameworks/python/flask.md  fence 1  install-external      0.0s  accounted: Installs flask, psycopg and pytest from the package index; only blastit comes from this workspace. blastit's own install runs in README.md and Getting started.
PASS  docs/frameworks/php/index.md  fence 1  static                0.0s  statically checked every command, flag and snippet (The gate builds no PHP project fixture: an `entrypoint php` file needs a `public/index.php` app and a host NTS `libphp`, and this offline gate installs neither. It checks the documented `blastit dev --learn` command and its flags against the installed CLI, and parses the page's Blastfile block.)
PASS  docs/frameworks/php/index.md  fence 2  example               0.0s  accounted: No PHP project fixture or host NTS `libphp` exists in this offline gate, so the `fmt --check` gate cannot run; the command is documented in docs/blastfile.md and the page's Blastfile block is parsed by the docs_fences gate.
PASS  docs/frameworks/static-and-spa.md  fence 1  example               0.0s  accounted: `uvx` installs blastit from the package index, which this offline gate does not have; e2e/static_assets.py compares this banner with a container's own `blastit dev`, line for line.
PASS  docs/frameworks/static-and-spa.md  fence 2  example               0.0s  accounted: Vite writes `dist/` and this page serves `public/`; e2e/static_assets.py names this command as its one Node skip, and its fixture is that build output.
PASS  docs/frameworks/static-and-spa.md  fence 3  dev-curl              0.1s  served and answered
PASS  docs/frameworks/static-and-spa.md  fence 4  dev-curl              2.1s  served and answered
PASS  docs/frameworks/static-and-spa.md  fence 5  example               0.0s  accounted: `uvx` reaches the package index, which the offline gate does not have; e2e/static_assets.py runs the same `blastit check` and `--json` in its container and asserts the exit code and the document.
PASS  docs/frameworks/static-and-spa.md  fence 6  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/reference/cli.md  fence 1  dev-flags             0.3s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 2  check-flags           6.9s  each check flag runs
PASS  docs/reference/cli.md  fence 3  serve-flags           0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                 0.1s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked every command, flag and snippet (The e2e harnesses need Docker and root; the runnable-docs gate is unprivileged and Docker-free, so only the command names and the scripts they name are checked.)

48 fences: 30 executed, 18 accounted, 0 failed · 31.9s
```

One `docs-test` run in this worktree (under load, other nodes building) failed
`docs/local.md` fence 5 (`dev-learn`) with `exited 1 after SIGINT (expected 0)` — the same
fence that failed once in attempt 1 with `exited -9`; the immediate re-run above is green
(48/0), `crates/` and `python/blastit/` are clean of other edits, and no file in this node
touches that fence.

## 5. Reviews

### 5.1 Attempt 2 — the reviewer's ten findings

| # | Finding | Fix |
|---|---|---|
| 1 | MAJOR — `editors/zed/README.md:10` kept `rustup target add wasm32-wasip2`, a phantom step | "install Rust, `git clone …`, then `zed: install dev extension` and pick `editors/zed` (Zed builds it — and its `wasm32-wasip2` target — with `cargo`)"; the maintainer build section at `:93` is untouched |
| 2 | MAJOR — "your own build" named no command, and `cargo build -p blastit` yields the production server | "run `cargo build -p blastit-lsp --release --locked` … then point `binary.path` at `target/release/blastit-lsp` (needs `cargo`, `clang`, `libclang`; `LIBCLANG_PATH` on Windows)" |
| 3 | MINOR — the post-release resolution stated as current | "Until the release that ships `blastit-lsp`, this extension runs a `blastit`/`uv` already on `PATH`." |
| 4 | MINOR — the standalone-binary paragraph lacked a platform | "On Linux, without Python at all, build the standalone binary…" |
| 5 | MINOR — `where` is a PowerShell alias; `lsp` → `blastit-lsp` is not a key | "`where.exe blastit-lsp`" and "`lsp.blastit-lsp.binary.path`" |
| 6 | MINOR — "once per extension version" | "once per Blastit release" |
| 7 | MINOR — "no musl" reads as all of Blastit | "Nothing else is built for the editor: …" |
| 8 | MINOR — no statement that earlier tags have no asset | "Shipped from the release that carries the portable server; earlier tags have no `blastit-lsp-*` asset." |
| 9 | MINOR — doubled backslashes are un-pasteable | "`C:/Users/you/bin/blastit-lsp.exe` — forward slashes are valid JSON and paste anywhere" |
| 10 | MINOR — `docs/local.md` repeated the README's resolution order | step 2 is one sentence plus the CLI link |

### 5.2 Attempt 3 — the reviewer's four closing MINORs (verdict `PASS`)

| # | Finding | Fix |
|---|---|---|
| 1 | `docs/troubleshooting.md:107` presented the build as the fix today, though `-p blastit-lsp` errors until the release | the command now reads "run `cargo build -p blastit-lsp --release --locked` **after the release that ships `blastit-lsp`**, then point …" |
| 2 | `docs/local.md:156-158` said "nothing to configure", false today on macOS/Windows (`editors/zed/src/lib.rs:41-45` errors when neither `blastit` nor `uv` exists) | step 2 now opens "Once the release ships `blastit-lsp`, the extension finds the server itself…" |
| 3 | `python/tests/test_docs_assets.py:1730` docstring said "29 + 4" while the assertion is 29 + 5 | docstring now reads "The 29 + 5 split, matched by first line so a prose edit cannot break it." |
| 4 | `RESULT.md` listed the two `docs/design/` reports as deliverables, though `NODE.md` excludes `docs/design/**` | they are now cited as evidence only and explicitly untracked (§1) |

### 5.3 Correction to the attempt-1 record

Attempt 1's §5 claimed the `rustup target add wasm32-wasip2` phantom step had been applied to
"both pages". That was wrong: it was removed from `docs/local.md` only, and
`editors/zed/README.md` still carried it until finding 1 of attempt 2. Likewise the ai
MAJOR-4 row claimed the troubleshooting row "names `cargo`, `clang`, `libclang` and
`LIBCLANG_PATH`" — it did, but without a command that produces `blastit-lsp`; attempt 2
supplies it.

### 5.4 The five critic rounds, and the recorded acceptance

| Round | dx | ai-ergonomics | What it forced |
|---|---|---|---|
| 1 | FAIL 4B/8M | FAIL 1B/7M | one install story; no invented PATH; JSON; platform scope |
| 2 | FAIL 2B/4M | FAIL 0B/10M | remove `uv` from `## blastit lsp`; executable Mac path; the `arguments` trap |
| 3 | FAIL 1B/4M | FAIL 0B/6M | `cargo` contradiction; `local.md` opening; five asset names; exit `1` causes |
| 4 | FAIL 0B/5M | FAIL 1B/4M | portable `--help`/`2` removed; two troubleshooting rows; the parser oracle; quarantine |
| 5 (closing) | FAIL 0B/2M | FAIL 0B/4M | the argv contract, the build command, `PATH` wording, the phantom step |

Every BLOCKER/MAJOR from rounds 1–5 was applied before the reviewer's pass, and the reviewer
then returned `VERDICT: PASS` with the four MINORs fixed in attempt 3. **The two reports in
`docs/design/` keep their `VERDICT: FAIL` lines**: they reviewed snapshots, the lead's rule
for scoped doc updates is that the node is accepted through the reviewer rather than a sixth
goldfish round, and the reports are left untouched. They are untracked, and `docs/design/` is
in `zensical.toml`'s `exclude_docs`, so nothing in them is published.

### 5.5 MINORs still standing (not in either reviewer list)

| Lens | Finding | Why it stands |
|---|---|---|
| dx | `blastit lsp --help` needs `uv run` under a `uv add` install | it documents the installed CLI; the venv case is one word a reader knows from `uv run blastit dev` |
| dx | no troubleshooting row for the manual path's own failures | two rows is the cap for one screen; row 1 routes to the setting |
| ai | one row merges "no asset" and "download fails" | both are "supply the file yourself", with the same two fixes |
| ai | the extracted path is not spelled out | "not its folder" plus `binary.path` taking the file means the inner name never matters |
| ai | no integrity check for a downloaded executable | the release page lists each asset's `sha256`; a checksum table is release-job scope |
| ai | platform facts are stated per-row inconsistently | the table is the one place; prose stays lean |
| ai | the install step assumes a Rust toolchain can be installed | "install Rust" is the instruction; Zed's builder handles the target |
| ai | the Linux aarch64 asset is promised unconditionally | the row is the contract MB.35.5 builds and MB.33 codes against; "nothing else is built for the editor" states the set |
| ai | the only copyable artefacts are the unparsed ones | `text` is the only fence kind that may carry a non-Linux command |

## 6. Deviations from the brief, with reasons

1. **`--help` and exit `2` are not promised for `blastit-lsp`**, only their absence: the brief
   says "no arguments" and "exit codes `0`/`1`", and `MB.35.4` ships `bin blastit-lsp` as a
   `main` around `blastit_lsp::run()`, which parses no argv.
2. **The Linux heading gains `(Linux)`**, no `attr_list` id: one anchor name for the site and
   GitHub, and `#blastit-lsp` stays with the portable section.
3. **One `text` block was added** (33 → 34) and pinned as plain; it has no `$ ` prompt.
4. **`uv run blastit lsp` removed** from `## blastit lsp` (MB.33's acceptance rejects it).
5. **Two troubleshooting rows, not one**: a four-failure cell was graded MAJOR twice.
6. **Five asset names on the CLI page**: "the asset for your platform" was undecidable.
7. **`--version` not offered**: no such flag exists anywhere.

## 7. Follow-ups (outside this node's paths)

- `docs/deploy/index.md` should carry the Linux-only sentence; the boundary sentences here link
  there and state the claim themselves.
- `MB.35.4`: if the portable binary gains argv handling, `--help` becomes the launch probe and
  the only checkable version (`initialize`'s `serverInfo.version` is the only one today).
- `MB.33`: `uv add blastit` leaves `blastit` in `.venv/bin`, off the `PATH` the extension
  searches, so it downloads instead; that is the extension's own sentence to write.
- `blastit fmt` has no row in the CLI command index although `docs/reference/blastfile.md`
  documents it.
