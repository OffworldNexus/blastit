# review-arch — MB.41.1 (technical truth)

Base `dc808a9d`..working tree. Verified against `crates/blastit/src/**`, `docs/reference/cli.md`,
`scripts/platform_matrix.py`, `pyproject.toml`, and the built `target/debug/blastit`
(`fmt --check`, `check`, `dev` boots, `--version`). Gates run: `make docs-check` → OK,
`make zed-check` → OK (59 props), `scripts/platform_matrix.py --check` → ok (4/8 wheels,
2/4 archives, 6/12 shims proved).

VERDICT: FAIL

## BLOCKER

### 1. `docs/local.md:55` — the documented PostgreSQL one-liner cannot start
Text: "a throwaway one is `docker run --rm -d -p 127.0.0.1:5432:5432 postgres:18` away".
Measured (`docker run --rm postgres:18`, image present locally):

```
You must specify POSTGRES_PASSWORD to a non-empty value for the
superuser. For example, "-e POSTGRES_PASSWORD=password" on "docker run".
```

The container exits before init, so the next two commands (`docs/local.md:61`, `:67`
`export DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop`) fail with
`FATAL: role "shop" does not exist` — even if it started, the bare image creates only the
`postgres` role/db. The old page carried the three `-e` flags; the rewrite dropped them
while keeping the URL that depends on them.
Fix: restore `-e POSTGRES_USER=shop -e POSTGRES_PASSWORD=secret -e POSTGRES_DB=shop` (or
change the URL to the `postgres`/`postgres` superuser — the first is what the rest of the
page assumes).

### 2. `docs/ci.md:65` — `blastit check || test $? -eq 2` is a gate that can never fail, and it masks real refusals
Measured on the exact `getting-started` PHP golden path (PHP quickstart + `check`, no pytest):

```
$ target/debug/blastit check
blastit check: /usr/bin/python3: pytest is not installed ... install pytest: uv add --dev pytest
exit=2
```

Exit `2` is the *normal* outcome for every PHP app with no pytest suite, so the step always
passes; worse, `2` is also what an unparseable `Blastfile`, an unreadable `static` root and a
missing/refused `libphp` return (`docs/ci.md:88-90` itself says "the file does not load, so
`check` exits `2`"), so the recipe cannot distinguish "no suite" from "refused".
`docs/ci.md:68-70` then tells the reader that step proves "the manifest, every `root` and the
libphp preflight" — a guarantee the command does not make. It also contradicts
`docs/frameworks/php/index.md:105-110`: "`blastit check` … exits `2` without a pytest suite
and prints no document, so nothing proves a PHP grant is complete in 0.2.0. The gate is
`blastit fmt --check`".
Fix: delete the line. Keep `blastit fmt --check` (canonical + parses) and the smoke run as the
PHP gate, exactly as `frameworks/php/index.md` states; if the preflight output is wanted,
capture stderr and fail when it names a refusal instead of normalising exit `2` away.

### 3. `docs/ci.md:82` — the Static job calls `uvx` without installing `uv`
The job's step list is `actions/checkout` → `npx vite build` → `uvx blastit check`. The Python
tab adds `astral-sh/setup-uv@v10.3.2` (`docs/ci.md:45`), and every workflow in this repo pins
it (`ci.yml:40`, `ci.yml:555`, `docs.yml:56`) precisely because the runner image has no `uv`.
On a clean `ubuntu-24.04` runner the step dies `uvx: command not found`.
Fix: add `- uses: astral-sh/setup-uv@v10.3.2` (and `uvx --python 3.13 blastit check` for
determinism: `pyproject.toml:26` is `requires-python = ">=3.13"` and `platform_matrix.PYTHONS`
is `3.13`/`3.14`, so a 3.12 default either downloads a managed 3.13 or falls back to an sdist
build that needs Rust + libclang).

## MAJOR

### 4. `docs/ci.md:92-98` — the "What it fails on" table lost `raw_ingress`, a real finding kind
`raw_ingress` exists (`crates/blastit/src/audit_v1.rs:461`, `fix_raw_ingress` at `:331`,
emitted by `raw_ingress_findings` at `:1143`) and is documented in
`docs/reference/blastfile.md:482`. The old ci.md row (`a view declares body raw without
unvalidated`) and its production sentence ("an unacknowledged `raw` view refuses the boot
before it binds a listener") are both gone; the surviving note (`docs/ci.md:108-112`) keeps
only the 413 rule. This is the page a CI reader uses to decide what a failing run means, and
the one finding that gates a deliberate opt-out is not in it.
Fix: re-add the row (`| `raw_ingress` | a `body raw` view with no `unvalidated` | add
`unvalidated`, or use a codec — the production boot refuses the file |`) and the boot-refusal
sentence.

### 5. `docs/blastfile.md` — the `#formatting` anchor that two reference links use no longer exists
`docs/reference/cli.md:75` and `:90` both link `[canonical layout](blastfile.md#formatting)`.
The rewritten page has only `!!! note "Canonical layout"` (`docs/blastfile.md:63`); its
headings are `The file`, `Local development`, `CI/CD`, `Where next`. `make docs-check`
(`zensical build --strict`) passes because anchor validation is not on, so this is invisible
to the gate and dead for the reader.
Fix: make it a heading — `## Formatting { #formatting }` — or drop the explicit id and point
the two links at `#canonical-layout`.

### 6. Every golden-path `Blastfile` fence fails the gate the same pages advertise
Measured with the tree's own binary, extracting each `text title="Blastfile"` fence and
de-indenting it exactly as written:

| fence | `fmt --check` |
|---|---|
| `docs/index.md` Python / PHP | 1 / 1 |
| `docs/getting-started.md` Python / PHP | 1 / 1 |
| `docs/blastfile.md` Python / PHP | 1 / 1 |
| `README.md` | 1 |
| `docs/local.md`, `docs/ci.md` (static) | 0 |

`blastit fmt` rewrites `view hello GET "/*" {}` → `view hello GET "/*" {\n}` and
`postgres { select … }` → three lines. `docs/blastfile.md:106` and `docs/ci.md:64` make
`blastit fmt --check` the PHP gate, so a reader who copies the three-step quickstart and runs
the documented gate gets exit `1` on a file that was pasted verbatim from the docs.
(`docs/frameworks/php/index.md` and `.../python/flask.md` are non-canonical too — this is
repo-wide, but MB.41.1 is what makes it load-bearing by promoting `fmt --check`.)
Fix: run `blastit fmt` over these fences and commit the canonical form (expanded empty view
body and multi-line `postgres` blocks), so "copy the file, run the gate" is green.

### 7. The same command list is spelled three ways across tabs (per-runtime drift)
- static = `uvx blastit …`: `docs/index.md:88-89`, `docs/local.md:39,73,94`, `docs/ci.md:82`
- static = bare `blastit …`: `docs/blastfile.md:112` (`$ blastit check`) under
  `=== "Static & SPA"`, while `docs/blastfile.md:86` in the same page's *Local development*
  static tab says `uvx blastit dev`
- opening block `docs/blastfile.md:8-10` (`blastit dev` / `blastit check` / `blastit fmt
  --check`) is presented for all three runtimes, yet the page's own Python tab
  (`:76`, `:101`) needs `uv run blastit …` — after `uv add blastit` the command lives in
  `.venv/bin`, not on `PATH`, so the opener fails for the first runtime on the page.

Fix: one spelling per runtime everywhere — `uv run blastit …` (Python), `blastit …` (archive:
PHP), `uvx blastit …` (static) — and tab the opener or write both forms in it.

### 8. `docs/getting-started.md:28` — the Static install tab contradicts its own requirement line
`docs/getting-started.md:10` says static apps need "the standalone `blastit` binary, no
runtime at all", but the Static install tab is `uvx blastit --version`. `uvx` needs `uv` plus
a managed CPython (`pyproject.toml:26` `requires-python = ">=3.13"`), and `--version` is a
version print (`crates/blastit/src/cli.rs:249-252` — it answers only `argv.len() == 1`), not
an install; nothing is installed for the next step to use.
Fix: give the Static tab the same `tar -xzf blastit-x86_64-unknown-linux-gnu.tar.gz` +
`PATH` steps as the PHP tab (the archive name and its top-level directory both match
`scripts/build-standalone.sh:255` `name="blastit-$target"`), and keep `uvx blastit dev` as
the documented alternative in `docs/frameworks/static-and-spa.md`.

## MINOR

1. `docs/index.md:14-22` — the home page runs `blastit dev` under the promise "one `view` per
   route, with its own process, uid and SQL allowlist", with no "not a sandbox" caveat;
   `README.md:38-41` has one and `docs/getting-started.md:82-88` shows the banner. Add one
   sentence under the console block ("`dev` shares your uid; production splits it").
2. `docs/getting-started.md:8` — `dnf install php-embedded` resolves to PHP 8.0 on RHEL/Alma 9,
   which stage 0 refuses (ADR-0010's 8.3–8.5 gate) and `platform_matrix.py` has no RHEL leg
   (`PHP_BUILDS` is Ubuntu/Debian/Alpine only). Scope it to Fedora or drop the spelling.
3. `docs/getting-started.md:123` — "Wheels cover CPython 3.13 and 3.14 on Linux
   x86_64/aarch64 (glibc, musl)"; `platform_matrix.py --check` reports "8 wheels … proved 4",
   i.e. no aarch64 wheel is built yet. Say "built for" (matching ADR-0010's declaration) or
   name what is proved.
4. `docs/ci.md:66` — `blastit dev & sleep 2 && curl …` races the boot (`LESSONS.md` MB.3.40: a
   port accepts before stage 1's `ready`). Use a bounded retry (`for i in {1..20}; do curl -fs
   && break; sleep 1; done`).
5. `docs/ci.md:3` — "the gate is one unprivileged command per runtime", while the PHP job
   (`docs/ci.md:63`) needs `sudo apt-get` and, per this review, `fmt --check` *and* the smoke
   run. Reword to "a gate you can run without root, once the host libphp is installed".
6. `docs/local.md:52` — "prints one `runtime` and one `view` line per view" omits the `static`
   row a mount adds (`crates/blastit/src/dev_learn.rs:110-132`), which is exactly the static
   tab's case.
7. `docs/ci.md:130-133` — `## Bootstrap an existing project` no longer shows any command or the
   `{"proposed": {…}, "wrote": …}` document, yet `docs/reference/cli.md:64` links here for
   "details" of `--learn`/`--dry-run`. Either show the three commands again or point the
   reference at itself.

## Verified clean (no action)
- `zensical.toml:10` description update; `content.tabs.link` (`:100`) and
  `pymdownx.tabbed.alternate_style = true` (`:176`) intact.
- Every `Blastfile` fence **parses** (no `fmt` refusal): `static "/" { root, fallback,
  immutable }`, `body json`, `postgres $DATABASE_URL`, `entrypoint php`, and `static "/"` +
  `view … "/*"` coexist — confirmed both by `fmt --check` (parse) and by booting the exact
  getting-started PHP file (`GET /` → 200 `hello from blastit`, `GET /whatever` → 200) and the
  static file (`GET /` 200, `/deep/route` 200 via `fallback`, `/assets/app.css` 200 with
  `Cache-Control: public, max-age=31536000, immutable`), and `check` on the static file exits 0.
- `blastit --version` exists (`blastit 0.2.0`), `--learn --dry-run`, `--json`, `--port`,
  `--host` (+`BLASTIT_DEV_EXPOSE`), `--no-reload`, `check --learn` are all real
  (`cli/parse.rs:41-77,138-164,184-201`).
- Preset names `digitalocean-paas` / `compose-traefik` are real (`config/preset.rs:49,60`).
- `libphp-embed` is a real package (Debian trixie `2:8.4+96`, `apt-cache policy`), so the three
  install spellings are not invented; `apk add php84-embed` matches
  `platform_matrix.PHP_BUILDS[("8.4","musl")]["embed"]`.
- Archive asset name `blastit-x86_64-unknown-linux-gnu.tar.gz` and its top-level directory
  match `scripts/build-standalone.sh:255`.
- Security honesty: `blastit dev` is labelled "single-uid simulation / Not a sandbox" in
  `getting-started.md:82-88`, `local.md:11-17` (warning) and `README.md:38-41`; ingress
  (TLS/HTTP/2/edge cache/DDoS) is attributed to the ingress, never to blastit; no secret is
  printed beyond the documented throwaway `shop:secret`; `docs/frameworks/php/index.md:105`
  remains the honest "nothing proves a PHP grant" statement.
- Remaining anchors other pages use (`ci.md#what-it-fails-on`, `ci.md#for-agents-json`,
  `ci.md#bootstrap-an-existing-project`, `local.md#a-denial`, `local.md#editors`) all exist;
  frameworks pages stay reachable through `zensical.toml` nav.
- `docs_fences.rs` PAGES/STATEMENT_LISTS pins were not reviewed as a defect (MB.41.3 owns them).
