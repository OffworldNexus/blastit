# MB.41.1 — RESULT (onboarding spine)

## What changed, per file

| File | Change |
|---|---|
| `zensical.toml` | `site_description` now names Linux + Python WSGI + embedded PHP + static sites. `content.tabs.link` and `pymdownx.tabbed.alternate_style` untouched. |
| `README.md` | Linux in the first paragraph; "Linux application server behind your cloud ingress" (Traefik / DO App Platform / Cloudflare / nginx terminate TLS + HTTP/2); 3 zero-sudo commands; ingress → blastit → app mermaid; "Where it sits" table. GitHub-style `> [!NOTE]` (README is not part of the Zensical build). |
| `docs/index.md` | Deleted the old `86–93` "Yes for … No for HTTP/2, WebSockets …" apology. Linux + cloud-app-server framing, 3 runnable commands, one tab group with the 10-line Blastfile + CLI per runtime, trust-boundary mermaid, `Where next`. |
| `docs/getting-started.md` | Linux stated first with the three runtime bullets (Python 3.13+; PHP 8.3/8.4/8.5 NTS embed + archive; static = archive only). Install / write two files / run are tabbed; `??? note` keeps the platform + archive matrix; no numeric counts (platform-matrix safe). |
| `docs/blastfile.md` | `## The file`, `## Local development`, `## CI/CD` tabbed per runtime (own `entrypoint`, grants, commands); `!!! note "Canonical layout"`; grant-flow mermaid; deployments folded into `Where next`. |
| `docs/local.md` | Linux first, `!!! warning "One uid, not a sandbox"`, `??? note "Developing on macOS or Windows?"` (Docker/Compose or VM + native `blastit-lsp`); Start / PostgreSQL+`DATABASE_URL` / `--learn` tabbed; denial section kept; Zed `settings.json` fence byte-identical. |
| `docs/ci.md` | Python-only opening replaced by a per-runtime GitHub Actions workflow tab (Python `uv run blastit check --json`; PHP `fmt --check` + `check` preflight + HTTP smoke on `blastit dev`; Static `check` root audit), finding table, `--json`/exit codes, `Where next` → Deployments. |

## Tab titles used (exactly, everywhere)

`=== "Python (WSGI)"` · `=== "PHP"` · `=== "Static & SPA"` — 11 groups each, so `content.tabs.link` never crosses a language. Bodies indented 4 spaces.

## Verification of the fences

- Every `text title="Blastfile"` fence was extracted (dedented) and parsed with `target/debug/blastit fmt --check`: all 10 **parse**; the only non-zero exits are "not in canonical layout" (inline `postgres { … }` / empty `{}` blocks, the same style `docs/frameworks/php/index.md` uses).
- No `php-fpm`, `FastCGI`, `listen =`, `workers =`, `version = 2`, `blastit.toml`, "not implemented" or "No for HTTP/2" anywhere in the seven paths.
- Line counts: README 126, index 128, getting-started 128, blastfile 127, local 129, ci 126 (all ≤ 130).

## Follow-ups

- `make docs-test` (`crates/blastit-manifest/tests/docs_fences.rs` `PAGES` / `STATEMENT_LISTS` and the runnable-console harness) is expected to be red until **MB.41.3**: documents went from 1 to 3 on `docs/index.md`, `docs/getting-started.md` and `docs/blastfile.md`, and `docs/ci.md`'s statement list went 1 → 0.
- `docs/frameworks/*` still uses its own (non-tabbed) style; MB.41.2 owns those pages so linked tabs match site-wide.
- Tab bodies are indented 4 spaces by `pymdownx.tabbed`; MB.41.3's fence extractor must dedent before running console fences.

## Verification command

```
make docs-check platform-matrix-check zed-check
```

```
No issues found
Build finished in 0.19s
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; reaches: served 6 of 12 shims, 0 probed, 0 refused, 6 unbuilt; 12 served once the aarch64 legs run; 0 leg(s) dropped
zed-check: OK (59 properties checked)
EXIT:0
```

Line counts after the gate: `README.md` 126, `docs/index.md` 128,
`docs/getting-started.md` 128, `docs/blastfile.md` 127, `docs/local.md` 129,
`docs/ci.md` 126.

Note: restoring the `#for-agents-json` and `#bootstrap-an-existing-project` anchors
(and dropping the `permissions:` line from the sample workflows) was required —
`docs/reference/cli.md` and `docs/frameworks/index.md` link into `ci.md` and
`docs-check` is `--strict`.

## Fix round (review-ux + review-arch)

Only the confirmed findings were changed; all three tab manifests keep
`text title="Blastfile"` and page structure is otherwise untouched.

| # | Finding | Fix |
|---|---|---|
| 1 | PHP install tab had no download | `getting-started.md` PHP tab now `curl -fsSLO …/blastit-x86_64-unknown-linux-gnu.tar.gz` then `tar -xzf blastit-*.tar.gz && export PATH=…` |
| 2 | Static used `uvx` in five pages | every `uvx blastit …` removed (`grep` clean); the Static install tab, `index.md`, `local.md`, `blastfile.md` and the `ci.md` Static job all use the release archive + `blastit …`; only the Python tab keeps `uv run blastit …`; "no language runtime" wording fixed |
| 3 | Postgres one-liner exited immediately | `local.md` now `docker run --rm -d -p 127.0.0.1:5432:5432 -e POSTGRES_USER=shop -e POSTGRES_PASSWORD=secret -e POSTGRES_DB=shop postgres:18`, matching `DATABASE_URL=postgresql://shop:secret@127.0.0.1:5432/shop` |
| 4 | `check \|\| test $? -eq 2` greened refusals | line deleted; `fmt --check` stays the gate, `check` prose states it exits `2` without pytest and that the smoke run proves the app; fixed 2 s wait replaced by `blastit dev &` + bounded `for i in $(seq 30)` loop with `exit 1` |
| 5 | `--env-file` is not a flag | `uv run --env-file .env blastit dev` (Python) and `set -a; . ./.env; set +a` (archive) |
| 6 | Golden-path fences failed `fmt --check` | all 10 `Blastfile` fences rewritten to the canonical form (empty `view` bodies expanded, `postgres` grants block form) |
| 7 | Banner showed `/.*` | `README.md` and `getting-started.md` print `view  hello  GET, HEAD  /*`; getting-started gains `http · scheduler · dbproxy · 1 route`; `local.md` says one components line plus a row per view and per `static` mount |
| 8 | Python-only `pytest` prose in a shared paragraph | moved inside `=== "Python (WSGI)"`; shared line is "one command per runtime below" |
| 9 | `raw_ingress` row missing | row restored in the findings table and the production boot-refusal sentence restored in the note |
| 10 | `blastfile.md#formatting` dead | added `## Formatting { #formatting }` (with the `!!! note "What \`fmt\` rewrites"` expansion rule) |
| 11 | Bare `blastit --preset` / dead archive comment | `index.md` uses `uv run blastit --preset digitalocean-paas`; `README.md` shows the archive `curl`/`tar`/`blastit dev` commands |
| 12 | Missing laptop path and ETag clause | `index.md` note states Docker/Compose or a Linux VM plus native `blastit-lsp` on Linux/macOS/Windows; the static prose states every served file carries an `ETag` |

### Fence verification

Extracted all ten fences and ran the tree binary:

```
README.md fence1: exit=0
docs/index.md fence1..3: exit=0
docs/getting-started.md fence1..3: exit=0
docs/blastfile.md fence1..3: exit=0
ALL OK
```

### Gate

```
uvx --offline zensical build --strict
No issues found
Build finished in 0.19s
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; … 0 leg(s) dropped
zed-check: OK (59 properties checked)
EXIT=0
```

Line counts: `README.md` 128, `docs/index.md` 130, `docs/getting-started.md` 130,
`docs/blastfile.md` 130, `docs/local.md` 130, `docs/ci.md` 129 (all ≤ 130).

Out of scope, untouched: `crates/blastit-manifest/tests/docs_fences.rs` and
`python/tests/runnable_docs/*` (MB.41.3); `make docs-test` stays red as expected.
