# Result MB.2.7

STATUS: done

## Summary

The static-and-spa page is now proved end to end, not only accounted. A new
stdlib leg (`e2e/static_assets.py`) reads `docs/frameworks/static-and-spa.md`,
extracts every `$ ` command from its ```` ```console ```` fences, and either
replays it against a real containerised `blastit dev` or records why it cannot
run here (an unknown command is a failure, so page drift cannot pass silently).

Two compose services run the same image: `static` serves the page-exact
`[route.static]` fixture on the loopback default the page documents, and
`static_nospa` serves the same route without `spa`. The driver runs **inside**
the `static` container, so the page's `127.0.0.1:8080` URLs and its banner are
verbatim. `e2e/static_site/run-dev.sh` copies the fixture to a writable `/site`
and installs the decoys the threat list names (`public/.env`,
`public/.git/config`, `public/link -> /etc/passwd`) without committing a nested
`.git` or an ignored `.env`.

72 assertions per Docker run: the hashed asset's `200` + immutable cache, the
deep SPA URL's `200` + `text/html` + `index.html` bytes, the refusals (`400` for
`..`/`%2e%2e`/NUL/backslash, `404` for dotfiles and the symlink, never file
bytes and never the SPA fallback), `HEAD`, `POST` + `Allow`, `413`, `/.well-known/`,
directory indexes, and the `spa = false` deep-URL `404`. The visual record is
`docs/testing/VISUAL-MB.2.md` (`VERDICT: PASS`).

## Evidence

### 1. `uv run --no-project python e2e/static_assets.py --self-test`

```console
$ uv run --no-project python e2e/static_assets.py --self-test
PASS: the deep URL is documented `text/html`
PASS: the deep URL is documented no-cache
PASS: the driver still asserts every refusal the page names
PASS: self-test, 17 checks
EXIT=0
```

The self-test pins the page's shape: two `curl -i` fences, one banner fence, two
`check` fences, `check --json`, and exactly four build/deploy commands. A new or
removed fence fails it before Docker starts.

### 2. `CARGO_NET_OFFLINE=true UV_OFFLINE=1 make e2e`

`dist/` held exactly **one** wheel (`blastit-0.1.2-cp314-…whl`) before and after;
the prebuilt image's `ls /tmp/dist/*.whl | grep -c .` check passed.

```console
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make e2e
...
PASS: `uv run blastit dev` prints the page's banner verbatim
PASS: `curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.js` is 200 OK
PASS: `/assets/app.4f1c2a9b7d3e.js` answers `Content-Type: text/javascript`
PASS: `/assets/app.4f1c2a9b7d3e.js` answers `Cache-Control: public, max-age=31536000, immutable`
PASS: `/assets/app.4f1c2a9b7d3e.js` is the hashed file's bytes
PASS: `curl -i http://127.0.0.1:8080/dashboard/settings` is 200 OK
PASS: `/dashboard/settings` answers `Content-Type: text/html; charset=utf-8`
PASS: `/dashboard/settings` answers `Cache-Control: no-cache`
PASS: `/dashboard/settings` is `public/index.html`
PASS: `uv run blastit check` exits 0 on an all-static manifest
PASS: `uv run blastit check` prints nothing on success
PASS: `uv run blastit check --json` exits 0
PASS: `uv run blastit check --json` prints exactly one JSON document
PASS: `uv run blastit check --json` says ok
PASS: `uv run blastit check --json` counts the one static route
PASS: `uv run blastit check --json` reports no app requests
PASS: plain `..` is 400 (`/../etc/passwd`)
PASS: `/../etc/passwd` is not masked by the SPA fallback
PASS: `/../etc/passwd` carries no file bytes
PASS: percent-encoded `..` is 400 (`/%2e%2e/%2e%2e/etc/passwd`)
PASS: a NUL byte is 400 (`/%00.js`)
PASS: a backslash path is 400 (`/foo%5c..%5cbar`)
PASS: a dotfile is 404 (`/.env`)
PASS: a dotfile directory is 404 (`/.git/config`)
PASS: the dotfile directory itself is 404 (`/.git/`)
PASS: a symlink that leaves root is 404 (`/link`)
PASS: `/.well-known/` stays public (200)
PASS: `/docs` is the directory index (200)
PASS: `HEAD /assets/…` is 200 OK
PASS: `HEAD /assets/…` has no body
PASS: `POST /assets/…` is 405
PASS: `POST /assets/…` carries `Allow: GET, HEAD`
PASS: `POST /assets/…` is `no-store`
PASS: a `GET` that declares a body is 413
PASS: a `spa = false` route answers a deep URL with 404
PASS: a `spa = false` route still answers its own index
PASS: docs/frameworks/static-and-spa.md replayed against a real blastit dev (72 checks)
...
EXIT=0
```

(Every denial row also asserted `body != index.html` and "carries no file bytes";
the omitted lines are in the run log. The Django, Flask/SQLAlchemy and rootless
legs still print their own `PASS`/`ok` lines and the whole target exited 0.
The target was re-run on the final tree after a formatting-only `ruff` change to
the driver and `run.py`: exit 0 again, with the same 72-check line.)

### 3. `CARGO_NET_OFFLINE=true UV_OFFLINE=1 make docs-test`

```console
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make docs-test
...
PASS  docs/frameworks/static-and-spa.md  fence 4  example  ... MB.2.7's e2e/static_assets.py replays this `curl -i` command and asserts the 200 with the immutable cache header.
PASS  docs/frameworks/static-and-spa.md  fence 5  example  ... MB.2.7's e2e/static_assets.py replays this `curl -i` command and asserts the 200 with `text/html` and `no-cache`.
PASS  docs/frameworks/static-and-spa.md  fence 6  static   ... MB.2.7 replays them on the fixture in the container e2e.
PASS  docs/frameworks/static-and-spa.md  fence 7  static   ... MB.2.7 replays the check command on the static fixture, and 0.2.1's e2e adds the PHP worker replay.
PASS  docs/frameworks/static-and-spa.md  fence 8  presets  ... each preset exists and fails closed without capabilities
47 fences: 28 executed, 19 accounted, 0 failed · 21.2s
EXIT=0
```

Note: the first attempt of this command failed because an ad-hoc `blastit dev`
smoke server from this session still held `127.0.0.1:8080` (the flask fence binds
it). After stopping it, the gate was green; that is a local-tooling artefact, not
a repository defect.

### 4. `CARGO_NET_OFFLINE=true UV_OFFLINE=1 make docs-check`

```console
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
EXIT=0
```

### 5. The visual/QA record

```console
$ grep -q 'VERDICT: PASS' docs/testing/VISUAL-MB.2.md
$ echo "EXIT=$?"
EXIT=0
```

`docs/testing/VISUAL-MB.2.md` records desktop (1568 px) and mobile (390×844)
light/dark renderings, the Frameworks nav entry, 0 console errors, no horizontal
overflow, the page's commands against real container output, and a
proven-where table. Screenshots are at `.playwright-mcp/visual-mb27-{light,dark,
mobile-light,mobile-dark}.png` (git-ignored run artefacts; this node's `paths`
admit only the report, so they are not committed).

## Files

- `e2e/static_assets.py` (new) — page reader, raw-socket HTTP client, replay and
  `--self-test`; stdlib only.
- `e2e/static_site/` (new) — the page-exact manifest, `public/` tree, `nospa/`
  manifest, the decoy sources (`dotenv`, `git-config`, `secret`) and
  `run-dev.sh`.
- `e2e/compose.yaml` — `static` and `static_nospa` services, both with
  healthchecks; titles/mounts documented in place.
- `e2e/run.py` — starts the two services, waits for health (no sleep), runs the
  self-test, and execs the driver inside `static` with `env -u BLASTIT_APP
  -u BLASTIT_PRELOAD`.
- `e2e/README.md` — one paragraph naming the new leg and `E2E_STATIC_PORT`.
- `docs/testing/VISUAL-MB.2.md` (new) — the visual/QA record.

No file under `crates/`, `docs/frameworks/`, `docs/deploy/`, `docs/research/` or
`python/tests/runnable_docs/` was edited.

## Follow-ups

1. **`inventory.toml` (owner MB.2.1):** with this leg green, the page's fences
   3–8 can move from *static/accounted* to executed modes; this node did not
   edit that file, as instructed.
2. **Lead (design §1.2):** `MB.2.6`'s note that the `blastit dev` banner
   "reports static routes and their roots" is superseded by the page; the banner
   is the three documented lines and this leg now asserts them verbatim.
3. **Optional:** publish `E2E_STATIC_PORT` on the host as well. It is currently
   the container's own port by design (a published port would force `0.0.0.0`
   and change the documented banner); if a host-side debugger is ever wanted,
   add a second `0.0.0.0` service rather than changing this one.
