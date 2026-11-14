# Static assets and SPAs

Serve a built front end and clean URLs from `blastit.toml`. No nginx, no regex soup.

```toml title="blastit.toml"
version = 1

[[route]]
name = "site"
path = ['.*']

[route.static]
root = "public"
spa = true
immutable = ["/assets/*"]
```

An all-static manifest needs no `app`: every route is a `[route.static]`.

| Key | Default | What it does |
|---|---|---|
| `root` | required | directory served, relative to the manifest |
| `index` | `index.html` | the file a directory or the SPA fallback answers with |
| `spa` | `false` | a path that is no file answers `index` instead of the app |
| `immutable` | none | globs matched whole against the answering file's path (`/…`); a match gets the year-long cache; `*` stops at `/` |

## Run it

A `blastit.toml` beside `package.json` is the whole configuration: one `[[route]]`
matching every path, and its `[route.static]` naming `root = "public"`. The manifest needs nothing else.

`uvx` is `npx` for Python tools: [install `uv`](https://docs.astral.sh/uv/getting-started/installation/)
once, and it fetches blastit and the Python it needs into a throwaway cache — no
`pyproject.toml`, no venv, no Python project. Blastit is a Python package today; a
standalone binary and an npm package are planned, so that install will go away.

From `package.json` and `blastit.toml`'s directory, a Vite build writes `dist/`; one flag puts it
where `root` looks. Vite's own `public/` is its `publicDir`, and the build empties it first: move
what you keep there to `static/` and set `publicDir: "static"` in `vite.config.ts`.

```console
$ npx vite build --outDir public
```

```console
$ uvx blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

`blastit dev` serves the files it finds, re-reading them per request; keep `vite dev` for hot reload.

## Local

A file is served as it is. A fingerprinted one is cached for a year, because its
name changes when its bytes do:

```console
$ curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.js
HTTP/1.1 200 OK
Content-Type: text/javascript
Cache-Control: public, max-age=31536000, immutable
```

A deep URL is no file, so `spa = true` answers with `public/index.html`:

```console
$ curl -i http://127.0.0.1:8080/dashboard/settings
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Cache-Control: no-cache
```

## What is served

In this order:

| Request | Answer |
|---|---|
| an existing file | the file, `200` |
| a directory | its `index` file, `200` (`/docs` or `/docs/`) |
| no file, `spa = true` | `index`, `200` |
| no file, no `spa` | the manifest's `app` |
| no file, no `spa`, no `app` | `404` |

A directory without its `index` file falls through like any missing path, and a
fallback whose file is missing is a `404`. Only a file from `root` whose path
matches `immutable` gets `Cache-Control: public, max-age=31536000, immutable`;
every other answer, `index` included, is `no-cache`.

The responder answers `GET` and `HEAD` only. Any other method, on a path it would
answer, is `405` with `Allow: GET, HEAD`; a `GET` or `HEAD` with a body is `413`.
A path it does not answer reaches the app untouched, with the route's `methods`
and `[route.ingress]`. `HEAD` is `GET` without a body.

Only files under `root` are reachable. `..` (plain or percent-encoded), NUL and
other control bytes, absolute and backslash paths are refused with `400`; a
symlink that leaves `root` and a dotfile such as `.env` or `.git` are `404`,
except `/.well-known/`, which is public. A refusal is final: it is never the app
and never `index.html`. Mechanism in [How it works](../how-it-works.md), threat
list in the [security model](../security.md).

## CI/CD

An all-static manifest has no app: `check` audits every `root` (exists, a
directory, readable) instead of running pytest, and exits `0`. A root that is
missing or not a directory stops it before anything runs (`2`), so build first.
Run them where `blastit.toml` is, with `uv` in the job (`astral-sh/setup-uv`); no lockfile or
`uv sync` is involved. Every manifest with an `app` runs the suite as [CI/CD](../ci.md) describes.

```console
$ uvx blastit check
$ uvx blastit check --json
```

With an `app`, a path no route matched is a finding (`1`) and the suite runs as usual. `--json`
is the same verdict as one document; [CI/CD](../ci.md#what-it-fails-on) owns the list and the
[exit codes](../ci.md#exit-codes).

## Front controller

A front-controller route omits `spa`: with `spa = true` the fallback answers
first, so the app is never reached. Without it, a request that is no file is the
manifest's `app`: `/checkout/42` reaches it with `PATH_INFO=/checkout/42`, and the
manifest keeps one `.*`. A PHP app is this row with `public/index.php` as its
entry script.

`0.2.0` ships the rewrite and this responder; the FastCGI worker that runs `public/index.php`,
and refuses to serve it as a file, is `0.2.1`. One `path = ['.*']` route also answers `GET /api/…`
with `index`: give an SPA's API routes their own `[[route]]` above it, because the first match wins.

## Deployments

In the container image, under an existing preset:

```console
$ blastit --preset compose-traefik
```

`uvx blastit --preset compose-traefik` on a laptop exits `1` on purpose: production refuses to
start without its capabilities, and the recipe's `db`, `pgbouncer` and `migrate` services are the
parts an all-static site does not need. See [Deployments](../deploy/index.md).

## Next

[Django](django.md) · [Flask, SQLAlchemy and raw WSGI](flask.md) ·
[the manifest](../reference/manifest.md).
