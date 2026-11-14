# Visual & QA Record: Static assets and SPAs (MB.2.7)

- **Date:** 2026-11-14
- **Auditor:** `implementer` acting as visual-checker
- **Scope:** the built page `docs/frameworks/static-and-spa.md` (`site/frameworks/static-and-spa/`) and the page's own `$ ` commands replayed in `e2e/static_assets.py`.
- **Base commit:** `69504ed` (`release/0.2.0`)
- **Build:** `CARGO_NET_OFFLINE=true UV_OFFLINE=1 make docs-check` → `No issues found` in 0.19 s.
- **Viewports:** desktop 1568 px wide (full page, light and dark) and mobile 390×844 (light and dark).

## 1. Render audit

| View | Artifact | Result | Observation |
|---|---|---|---|
| Desktop, light | `.playwright-mcp/visual-mb27-light.png` (1568×3642) | PASS | Title, lede, the 10-line manifest block with syntax highlighting, the four-key table, all eight command/output fences, the five-row "What is served" table, and the right-hand "On this page" list all render; no clipping. |
| Desktop, dark | `.playwright-mcp/visual-mb27-dark.png` (1568×3642) | PASS | Slate palette, code fences and inline code readable, table rules visible, no inverted or missing text. |
| Mobile, light | `.playwright-mcp/visual-mb27-mobile-light.png` (390×full) | PASS | Single column; wide tables and the curl output scroll inside their containers; `scrollWidth == clientWidth` (375 px), so there is no page-level horizontal scroll. |
| Mobile, dark | `.playwright-mcp/visual-mb27-mobile-dark.png` (390×full) | PASS | Same layout with the dark palette; header collapses to the drawer button, breadcrumb and title intact. |

- **Navigation:** the Frameworks sub-navigation now lists *Django*, *Flask, SQLAlchemy and raw WSGI* and *Static assets and SPAs*, in that order; the current page carries the active pill. The page is in the top `Frameworks` menu and in the Previous/Next sequence (Flask ← static-and-spa → Command line).
- **Links:** every in-page target answers `200` on the built site: `/how-it-works/`, `/security/`, `/deploy/`, `/frameworks/django/`, `/frameworks/flask/`, `/reference/manifest/`, `/ci/`.
- **Console:** 0 errors, 0 warnings. No horizontal overflow at 390 px.
- The screenshots are run artifacts under the git-ignored `.playwright-mcp/`; this node's `paths` admit only this report, so the images are not committed with it.

## 2. The page's `$ ` commands, replayed

`e2e/static_assets.py` reads the page, extracts every `$ ` line from its ```` ```console ```` fences, and either replays it against a real containerised `blastit dev` or records why it cannot run here. An unclassified command fails the driver, so a page edit cannot silently stop being tested.

| Page command | Handling | Evidence |
|---|---|---|
| `npx vite build --outDir public` | skipped: needs Node and a Vite project | the fixture is already a build output |
| `uv init --app` / `uv add blastit` | skipped: project creation + PyPI | fence 2 of the runnable-docs inventory installs the built wheel instead |
| `uv run blastit dev` | replayed | the service's banner is the page's three lines verbatim, including `127.0.0.1:8080` and `1 route` |
| `curl -i …/assets/app.4f1c2a9b7d3e.js` | replayed at the page's path | `200 OK`, `text/javascript`, `public, max-age=31536000, immutable` |
| `curl -i …/dashboard/settings` | replayed at the page's path | `200 OK`, `text/html; charset=utf-8`, `no-cache` |
| `uv run blastit check` (×2) | replayed | exit `0`, empty stdout on the all-static manifest |
| `uv run blastit check --json` | replayed | one JSON document, `ok: true`, `routes.total: 1`, `requests: 0` |
| `blastit --preset compose-traefik` | skipped: production preset | the page says it exits `1` locally; `e2e/rootless` (E1) proves the preset and its refusal |

Real output from the container: every line the page's two "Local" fences document appears, in order (the extra `Content-Length`/`ETag` headers are the responder's, not part of the promise):

```console
$ curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.js
HTTP/1.1 200 OK
Content-Type: text/javascript
Content-Length: 28
ETag: "1c-18e7bd6d0dabd632"
Cache-Control: public, max-age=31536000, immutable

$ curl -i http://127.0.0.1:8080/dashboard/settings
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 250
ETag: "fa-18e7bd6d0dabd632"
Cache-Control: no-cache
```

## 3. What was proven where

| Promise | Proof | Result |
|---|---|---|
| hashed asset is `200`, `text/javascript`, `public, max-age=31536000, immutable` | `make e2e` → `static_assets.py` | PASS |
| deep SPA URL is `200` `text/html` and the bytes of `public/index.html` | same | PASS |
| `..`, `%2e%2e`, NUL and backslash are `400`, never file bytes | same, raw request targets so no client normalises them | PASS |
| `.env`, `.git/config`, `.git/` and `public/link -> /etc/passwd` are `404` | same | PASS |
| a refusal is never masked by the SPA fallback | same (`body != index.html` on every denial) | PASS |
| `/.well-known/` stays public; `/docs` and `/docs/` are the same directory index | same | PASS |
| `HEAD` is the `200` head with no body; `POST` is `405` with `Allow: GET, HEAD`; a `GET` with a body is `413` | same | PASS |
| a `spa = false` route answers the deep URL with `404` | same, second service | PASS |
| the page's own commands still match the product | `uv run --no-project python e2e/static_assets.py --self-test` | 17 checks PASS |
| the full Docker e2e (Django, Flask, rootless, static) | `CARGO_NET_OFFLINE=true UV_OFFLINE=1 make e2e` | exit 0; 72 static checks PASS |

The Docker leg runs the driver *inside* the `static` container, so the page's `127.0.0.1:8080` targets and its banner are verbatim; the fixture is copied to a writable `/site` by `e2e/static_site/run-dev.sh` so the `.env`, `.git/config` and out-of-root symlink decoys exist without being committed (git refuses a nested `.git`, and a committed `.env` would be ignored).

## 4. Defects & observations

- No defects on this page at the tested viewports. The mobile clipping is contained scrolling of wide tables and `curl` output, the same behaviour as the other reference pages; the document itself has no horizontal overflow.
- This node does not edit the page: the only visible difference in the Frameworks navigation is the page's own entry, added by MB.2.1.

## 5. Follow-ups

- **MB.2.1 (owner of the inventory):** with this leg green, the page's fences 3–8 can move from *static/accounted* to executed modes in `inventory.toml`; this node does not edit that file.
- **Lead:** design §1.2 keeps the `blastit dev` banner at its three documented lines; MB.2.6's note that the banner reports static roots is superseded by the page and should be dropped from that node's wording.

## 6. Verdict

VERDICT: PASS
