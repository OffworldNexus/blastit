# Static sites and SPAs

A built front end is one `static` mount and no app: no `entrypoint`, no worker, no database.

## The Blastfile

```text title="Blastfile"
static "/" {
    root "public"
    fallback "index.html"
    immutable "/assets/*"
}
```

`uvx blastit dev` serves it on `127.0.0.1:8080`; `uvx` is `npx` for Python tools, fetching blastit
into a throwaway cache with no project and no venv.

```console
$ uvx blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

## The four directives

| Directive | What it does |
|---|---|
| `root` | the directory served, relative to the `Blastfile`; required |
| `index` | the file a directory answers with, `index.html` by default |
| `fallback` | the file a deep path that is no file answers with |
| `immutable` | globs whose files never change, so a match gets the year-long cache |

The reference has [the defaults and the matching rules](../reference/blastfile.md#static-mounts).

## Build into the root

```console
$ npx vite build --outDir public
```

Vite writes `dist/`; one flag puts it where `root` looks. Vite's own `publicDir` is emptied by the
build, so move what you keep into `static/` and set `publicDir: "static"` in `vite.config.ts`.

## Deep URLs and caching

```console
$ curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.js
HTTP/1.1 200 OK
Content-Type: text/javascript
Cache-Control: public, max-age=31536000, immutable
```

The hashed asset's name changes when its bytes do, so `immutable` keeps it for a year. A deep path
is no file, so `fallback` answers `public/index.html` with `no-cache`:

```console
$ curl -i http://127.0.0.1:8080/dashboard/settings
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Cache-Control: no-cache
```

Everything else is revalidated. A path outside `root` is refused, never masked by the app or by
`index.html`.

## Check the roots

```console
$ uvx blastit check
$ uvx blastit check --json
```

`blastit check` audits every `root` for existence and readability, exits `0`, or `2` naming a root
it cannot read. `--json` prints the same document on success; exit `2` leaves stdout empty.

## Deploy it

```console
$ blastit --preset compose-traefik
```

`blastit --preset compose-traefik` on the deploy host starts this file with the Compose preset's
defaults; [deployments](../deploy/index.md) own the stack.
