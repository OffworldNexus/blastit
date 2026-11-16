# Static assets and SPAs

A built front end is one mount and no app: this is the "no executor" page — every other
page fills the same nine slots with a callable or a front controller.

## 1. Executor

```text title="Blastfile"
static "/" {
    root "public"
    fallback "index.html"
    immutable "/assets/*"
}
```

`root` is the directory served, relative to the `Blastfile`. A static-only file needs no
`entrypoint`; with `fallback` a deep path that is no file answers `index.html`, so no
request here reaches an app.

```console
$ uvx blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

`uvx` is `npx` for Python tools: it fetches blastit and a Python into a throwaway cache,
with no `pyproject.toml` and no venv.

## 2. Static assets

```console
$ npx vite build --outDir public
```

Vite writes `dist/`; one flag puts it where `root` looks. Vite's own `public/` is its
`publicDir` and the build empties it: move what you keep there to `static/`, and set
`publicDir: "static"` in `vite.config.ts`.

| Directive | Default | What it does |
|---|---|---|
| `root` | required | the directory served, relative to the `Blastfile` |
| `index` | `index.html` | the file a directory answers with |
| `fallback` | none | the file a path that is no file answers with, instead of a `404` |
| `immutable` | none | globs matched whole against the answering path; `*` stops at `/` |

Only a match gets the year-long cache, because its name changes when its bytes do
(headers trimmed):

```console
$ curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.js
HTTP/1.1 200 OK
Content-Type: text/javascript
Cache-Control: public, max-age=31536000, immutable
```

A deep URL is no file, so `fallback` answers with `public/index.html`:

```console
$ curl -i http://127.0.0.1:8080/dashboard/settings
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Cache-Control: no-cache
```

Everything else is revalidated. Only files under `root` are reachable: `..` (plain or
percent-encoded), NUL, absolute and backslash paths are `400`; any symlink under `root`
(none are followed) and a dotfile are `404` (except `/.well-known/`), and so are `.php`,
`.phar` and `.phtml`. A refusal is never the app or `index`.

## 3. Database

None: a static file declares no `postgres` store, so no listener exists and nothing can
reach a database. A site that needs data puts its API on another file, with an executor.

## 4. Ingress

None to declare: a mount answers `GET` and `HEAD`, another method is a `405` with `Allow:
GET, HEAD`, and a `body`, `prefork`, `use` or grant belongs to a view, not to a mount.

## 5. Environment and secrets

Nothing to strip: no app process runs, so no secret can leak into one. A static deploy
has no credential to rotate.

## 6. Health

`/_blastit/health` is still answered, by the scheduler, so the platform's check has
something to probe — no file, no database.

## 7. Migrations

None: this page owns no schema. If the API beside it has one, its own page covers it.

## 8. Learn and check

An all-static file has no app to learn from: `check` audits every `root` — exists, a
directory, readable — and exits `0` without a suite, so build first:

```console
$ uvx blastit check
$ uvx blastit check --json
```

[CI/CD](../ci.md#exit-codes) has the exit codes.

## 9. Deployment notes

```console
$ blastit --preset compose-traefik
```

The recipe's `db`, `pgbouncer` and `migrate` services are the parts a static site does
not need ([Deployments](../deploy/index.md)).
