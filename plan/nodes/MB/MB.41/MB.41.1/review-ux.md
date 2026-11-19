# review-ux — MB.41.1 (onboarding spine rewrite)

Base `dc808a9d`, paths: `zensical.toml`, `README.md`, `docs/{index,getting-started,blastfile,local,ci}.md`.
Everything below was checked against the tree, not the prose: I ran `make docs-check`,
`make zed-check`, `make platform-matrix-check` (all green), `cargo test -p
blastit-manifest --test docs_fences`, and drove `target/debug/blastit` (`--version`,
`dev --env-file`, `dev --help`, `fmt --check`) over every Blastfile the new pages show.

---

## [BLOCKER]

### 1. `make fast-check` / `make check` is red: the doc-fence census drifted on five pages
`crates/blastit-manifest/tests/docs_fences.rs` pins fences per page and is part of
`rs-test` (so both gates run it). Measured census of the new pages:

| page | pinned (documents, illustrations) | carries |
|---|---|---|
| `docs/index.md` | (1, 2) | (3, 5) |
| `docs/getting-started.md` | (1, 5) | (3, 10) |
| `docs/blastfile.md` | (1, 3) | (3, 8) |
| `docs/local.md` | (1, 9) | (0, 12) |
| `docs/ci.md` | 1 statement list | 0 |

```
thread 'every_documented_document_parses_and_validates' panicked at
crates/blastit-manifest/tests/docs_fences.rs:515: docs/index.md: fence counts drifted:
pinned 1 `text title="Blastfile"` document(s) + 2 illustration(s); the page carries
3 … + 5 ….
```

Three `text title="Blastfile"` fences per page (`docs/index.md:35,54,79`,
`docs/getting-started.md:48,63,82`, `docs/blastfile.md:16,34,45`) cannot be reconciled
by bumping counts alone: the same test pins **one `Shape` per page**
(`docs_fences.rs:519-548`, `Shape::Application` for these five), and the PHP/static
tab documents are `ViewsAndMount` / `StaticSite`. `docs/local.md` also *lost* its only
document (old line 81) and `docs/ci.md` lost its only statement list (old line 119).

**Fix (needs a path widening — `crates/` is not in this node's `paths`):** keep exactly
one ```` ```text title="Blastfile" ```` per page (the Python tab, `Shape::Application`)
and open the other two tabs' files with a distinct caption, which `class_of` reads as an
illustration rather than a second manifest — e.g.
`    ```text title="Blastfile — PHP front controller"` /
`    ```text title="Blastfile — static & SPA"`; restore one `text title="Blastfile"`
document on `docs/local.md`; re-pin `PAGES` (`docs/index.md` (1,7), `docs/getting-started.md`
(1,12), `docs/blastfile.md` (1,10), `docs/local.md` (1,12)), `STATEMENT_LISTS`
(`docs/ci.md: 0` or restore the fence) and `TOTAL`, then re-run
`cargo test -p blastit-manifest --test docs_fences`.

### 2. The PHP install tab's first command cannot run — there is no tarball
`docs/getting-started.md:20-24`

```console
$ tar -xzf blastit-x86_64-unknown-linux-gnu.tar.gz
$ export PATH="$PWD/blastit-x86_64-unknown-linux-gnu:$PATH"
```

Nothing downloaded that file: a PHP reader's very first command dies with
`tar: blastit-x86_64-unknown-linux-gnu.tar.gz: No such file or directory`. `docs/ci.md:62`
has the missing step. The `libphp` package the tab needs is also only in a warning below
(line 34-37), which is fine, but the archive fetch is not.

**Fix:** make the tab mirror CI:

```console
$ curl -fsSLO https://github.com/OffworldNexus/blastit/releases/latest/download/blastit-x86_64-unknown-linux-gnu.tar.gz
$ tar -xzf blastit-*.tar.gz && export PATH="$PWD/blastit-x86_64-unknown-linux-gnu:$PATH"
```

### 3. Static & SPA has two mutually exclusive install stories across the spine
`docs/getting-started.md:10` promises "the standalone `blastit` binary, no runtime at
all"; `docs/getting-started.md:28,108`, `docs/index.md:88-89`, `docs/blastfile.md:86`,
`docs/local.md:39,73,94`, `docs/ci.md:82` then install/run it with `uvx blastit …`,
while `README.md:22` and `docs/index.md:20` say "PHP and static use the archive".
`uvx blastit` is functional (the wheel ships the console script, `--version` → `blastit
0.2.0`) but it fetches a managed CPython plus a PyPI wheel, so "no runtime at all" and
"static needs no language runtime" are false as written — and a static-site user with no
Python follows a tab that contradicts the prerequisite three lines above it.

**Fix:** one story, the archive, everywhere the contract names it:

```console
$ curl -fsSLO https://github.com/OffworldNexus/blastit/releases/latest/download/blastit-x86_64-unknown-linux-gnu.tar.gz
$ tar -xzf blastit-*.tar.gz && export PATH="$PWD/blastit-x86_64-unknown-linux-gnu:$PATH"
$ blastit dev
```

and drop `uvx …` from the Static tabs (keep `uv run blastit …` for Python only).

---

## [MAJOR]

### 4. `--env-file` is not a flag; the documented command is refused
`docs/local.md:56` — "Blastit reads the process environment only (`--env-file` loads a
file)". The binary has no such argument, and the site's own reference says so
(`docs/reference/settings.md:20` → `uv run --env-file .env blastit dev`):

```
$ blastit dev --env-file .env
blastit dev: unknown argument `--env-file` — run 'blastit dev --help'
```

**Fix:** "Blastit reads the process environment only (`uv run --env-file .env blastit
dev` loads a file; `set -a; . ./.env; set +a` for the archive)."

### 5. Every Blastfile on the spine fails the `fmt --check` gate the same pages teach
`blastit fmt` expands one-line blocks and one-line grant blocks; the new examples keep
both compact, and `docs/blastfile.md:115` makes `fmt --check` *the* front-controller gate
(and `:65-66` states the expansion rule). Measured with `target/debug/blastit fmt --check`:

```
--- getting-started:53 / :72, README:57, index.md:70, blastfile.md:41
-view hello GET "/*" {}
+view hello GET "/*" {
+}
--- index.md:43, blastfile.md:24 / :29
-    postgres { select products(id, name, price) }
+    postgres {
+        select products(id, name, price)
+    }
```
(all exit `1`). A reader who copies the golden path and runs the gate the same page
documents gets red on their own file.

**Fix:** write the canonical form in all seven places — `view hello GET "/*" {` / `}` and
`postgres {` + indented `select …` — or run `blastit fmt` over each snippet before
committing.

### 6. Fabricated banner transcript: the route pattern is printed as written, not as a regex
`README.md:65` and `docs/getting-started.md:98` show `  view  hello  GET, HEAD  /.*` for a
`view hello GET "/*"`. The banner joins `Route::path()`, documented "exactly as written"
(`crates/blastit-manifest/src/model.rs:908`; `dev_learn.rs:113-142`, and the unit tests
read `  view  home  GET, HEAD  /`), so the real row is `  view  hello  GET, HEAD  /*`.

**Fix:** `  view  hello  GET, HEAD  /*` in both places.

### 7. The PHP CI snippet converts every "could not start" into a green step
`docs/ci.md:65` — `- run: blastit check || test $? -eq 2   # preflight: 2 means no pytest suite`.
Exit `2` is "it could not start" (`docs/ci.md:120`, `docs/blastfile.md:117`,
`docs/reference/cli.md`): an unparseable `Blastfile`, an unreadable `root`, a missing
`libphp` shim and a bad `DATABASE_URL` all exit `2` and the step then passes. It also
contradicts the exit table two sections below.

**Fix:** accept only the pytest sentence, e.g.

```yaml
- run: |
    set -e
    out=$(blastit check 2>&1) && { echo "$out"; exit 0; }
    rc=$?
    if [ "$rc" -eq 2 ] && printf '%s' "$out" | grep -q 'pytest is not installed'; then
      echo "$out"; exit 0
    fi
    echo "$out"; exit "$rc"
```

### 8. Python-only prose in a shared, non-tabbed paragraph contradicts the PHP/static tabs
`docs/ci.md:23-25` — "With an `app`, `check` puts Blastit in front of your store and runs
`pytest -q` itself (`uv add --dev pytest` if the project has none)". A PHP or static
project has no Python suite: `check` exits `2` on the missing `pytest` (or audits roots and
runs nothing), exactly as the PHP tab at `:69-70` says. A reader on the PHP tab reads two
different contracts on one page.

**Fix:** move that sentence into the `=== "Python (WSGI)"` tab and let the shared line say
only "one command per runtime, below".

---

## [MINOR]

9. `docs/index.md:50` — `$ blastit --preset digitalocean-paas` inside a `uv` project where
   the same tab's other commands are `uv run blastit …`; bare `blastit` is not on `PATH`.
   Use `uv run blastit --preset digitalocean-paas`.
10. `docs/local.md:42-43` — "prints one `runtime` and one `view` line per view". The banner
    prints `http · scheduler · dbproxy · 1 route` plus one row per view **and** per `static`
    mount (`dev_learn.rs:76-142`). Say "one components line, a row per view and per mount".
11. `docs/getting-started.md:96-98` shows the banner without the components line that
    `README.md:64` includes, so two "exact" transcripts of the same command disagree.
12. `docs/getting-started.md:121-124` (`??? note "Linux targets, wheels and the archive"`)
    has no laptop path, and `docs/index.md:25-30` has no `blastit-lsp` sentence: only
    `README.md:17-19` states "run it in Docker/Compose or a Linux VM, `blastit-lsp` native
    on all three". Add the same one line to both.
13. `README.md:22` `# Python; PHP and static use the archive` sits in a block with no PHP or
    static command, so that reader has nothing to type; once #3 lands, point at the archive
    one-liner (or drop the comment).
14. `docs/ci.md:66` `blastit dev & sleep 2 && curl … && kill %1` — a fixed 2 s wait is the
    classic flake (and `kill %1` in a non-interactive shell is fragile). Use a bounded
    readiness loop: `for i in $(seq 30); do curl -fsS localhost:8080/ && break; sleep 0.5; done`.
15. `docs/index.md:78-85` / `docs/blastfile.md:44-51` — the static tab shows `root`,
    `immutable` and `fallback` but never the revalidation default the contract names; add
    "every file gets an `ETag`, so a deploy invalidates cached copies" (one clause).

---

## What is right (checked, not assumed)
- Linux is the first sentence of `README.md:11,17`, `docs/index.md:11`, `docs/getting-started.md:3`,
  `docs/local.md:3`; `ZERO` edge-proxy apologies remain (no HTTP/2, WebSocket, `501` or
  upload-codec hedging anywhere in the seven files).
- Tab titles are exactly `=== "Python (WSGI)"`, `=== "PHP"`, `=== "Static & SPA"`, same
  order in all ten groups; `content.tabs.link` and `pymdownx.tabbed.alternate_style` intact.
- Every page ≤ 130 lines (126-129), a Mermaid diagram on all six, admonitions on five,
  forward navigation on all six, no `sudo` before command 1, no diary/roadmap headings.
- Blastfile grants are real and parse (`fmt` reaches "not canonical", never a `line:col`
  parse error), `postgres $DATABASE_URL` is the documented store form, no `version =`,
  no `php-fpm`, no `workers =`, no 6-glob staircase, `immutable "/assets/*"` is legal.
- `learn: 1 route, 1 query added → …` (`check.rs:1319`), the `check` header/`fix:` lines and
  the `--json` footer in `README.md:84-93` match `audit_v1.rs:1200-1332` verbatim, and
  `blastit --version` → `blastit 0.2.0` is real.
